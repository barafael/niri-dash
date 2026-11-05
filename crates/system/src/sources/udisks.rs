//! USB drives and SD cards through UDisks2 on the system bus: what is plugged in, mounting,
//! unmounting and ejecting.

use super::perf::human_bytes;
use super::{Ctx, Dedup, Mailbox, Trigger, prop, prop_str, signals_from};
pub use crate::drives::{Drive, State, Volume};
use crate::live::Stateful;
use std::collections::HashMap;
use std::time::Duration;
use zbus::Connection;
use zbus::fdo::ObjectManagerProxy;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// UDisks said no ("target is busy", "not authorized", …).
    #[error("{0}")]
    Refused(String),
    #[error("UDisks isn't running: {0}")]
    NotRunning(#[source] zbus::fdo::Error),
    #[error(transparent)]
    Dbus(#[from] zbus::Error),
    #[error(transparent)]
    Cmd(#[from] super::CmdError),
}

/// UDisks puts the useful part in the D-Bus error's message.
fn refused(e: zbus::Error) -> Error {
    match e {
        zbus::Error::MethodError(_, Some(msg), _) => Error::Refused(msg),
        e => Error::Dbus(e),
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

/// What matters of a UDisks drive object.
#[derive(Debug, Clone, Default)]
struct DriveInfo {
    path: String,
    name: String,
    /// Removable media, or attached over USB or SDIO.
    removable: bool,
    can_power_off: bool,
    ejectable: bool,
    size: u64,
}

/// What matters of a UDisks block object.
#[derive(Debug, Clone, Default)]
struct BlockInfo {
    path: String,
    /// The drive's object path, "/" for none.
    drive: String,
    /// For an unlocked encrypted volume: the encrypted block, else "/".
    backing: String,
    label: String,
    device: String,
    size: u64,
    /// HintIgnore or HintSystem: not for the user to see.
    hidden: bool,
    /// Mount points, when it has a filesystem.
    mounts: Option<Vec<String>>,
    /// The unlocked block, when it is encrypted ("/" while locked).
    cleartext: Option<String>,
}

fn path_prop(props: &HashMap<String, OwnedValue>, key: &str) -> String {
    prop::<ObjectPath>(props, key)
        .map(|p| p.to_string())
        .unwrap_or_else(|| "/".into())
}

/// `ay` and `aay` properties hold NUL-terminated paths.
fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn bytes_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<u8>> {
    let v: &Value = props.get(key)?;
    let v = v.try_clone().ok()?;
    Vec::<u8>::try_from(v).ok()
}

fn mounts_prop(props: &HashMap<String, OwnedValue>) -> Vec<String> {
    let Some(v) = props.get("MountPoints") else {
        return Vec::new();
    };
    let Ok(v) = v.try_clone() else {
        return Vec::new();
    };
    Vec::<Vec<u8>>::try_from(Value::from(v))
        .map(|l| l.iter().map(|b| c_string(b)).collect())
        .unwrap_or_default()
}

/// Put the drives together from the objects: removable ones with something to show, each with
/// its filesystems.
fn assemble(drives: Vec<DriveInfo>, blocks: Vec<BlockInfo>) -> Vec<Drive> {
    let by_path: HashMap<&str, &BlockInfo> = blocks.iter().map(|b| (b.path.as_str(), b)).collect();
    let drive_of = |b: &BlockInfo| -> String {
        if b.drive != "/" {
            return b.drive.clone();
        }
        by_path
            .get(b.backing.as_str())
            .map(|o| o.drive.clone())
            .unwrap_or_else(|| "/".into())
    };
    let mut out: Vec<Drive> = drives
        .into_iter()
        .filter(|d| d.removable)
        .map(|d| {
            let mut volumes: Vec<(String, Volume)> = blocks
                .iter()
                .filter(|b| !b.hidden && drive_of(b) == d.path)
                .filter_map(|b| {
                    let locked = matches!(b.cleartext.as_deref(), Some("/"));
                    // An unlocked encrypted block shows as its cleartext block.
                    if b.mounts.is_none() && !locked {
                        return None;
                    }
                    let name = if !b.label.is_empty() {
                        b.label.clone()
                    } else if b.size > 0 {
                        format!("{} volume", human_bytes(b.size))
                    } else {
                        b.device.rsplit('/').next().unwrap_or("volume").to_string()
                    };
                    let volume = Volume {
                        path: b.path.clone(),
                        name,
                        size: b.size,
                        mount: b.mounts.as_ref().and_then(|m| m.first().cloned()),
                        locked,
                        backing: (b.backing != "/").then(|| b.backing.clone()),
                    };
                    Some((b.device.clone(), volume))
                })
                .collect();
            volumes.sort_by(|a, b| a.0.cmp(&b.0));
            Drive {
                path: d.path,
                name: d.name,
                size: d.size,
                can_power_off: d.can_power_off,
                ejectable: d.ejectable,
                volumes: volumes.into_iter().map(|(_, v)| v).collect(),
            }
        })
        .filter(|d| !d.volumes.is_empty())
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    out
}

async fn fetch(conn: &Connection) -> Result<Vec<Drive>> {
    let om = ObjectManagerProxy::builder(conn)
        .destination(DEST)?
        .path("/org/freedesktop/UDisks2")?
        .build()
        .await?;
    let objects = om.get_managed_objects().await.map_err(Error::NotRunning)?;
    let mut drives = Vec::new();
    let mut blocks = Vec::new();
    for (path, ifaces) in &objects {
        let get = |name: &str| {
            ifaces
                .iter()
                .find(|(i, _)| i.as_str() == name)
                .map(|(_, p)| p)
        };
        if let Some(d) = get(DRIVE) {
            let bus = prop_str(d, "ConnectionBus").unwrap_or_default();
            let name = [prop_str(d, "Vendor"), prop_str(d, "Model")]
                .into_iter()
                .flatten()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            drives.push(DriveInfo {
                path: path.to_string(),
                name: if name.is_empty() {
                    "Drive".into()
                } else {
                    name
                },
                removable: prop(d, "Removable").unwrap_or(false)
                    || prop(d, "MediaRemovable").unwrap_or(false)
                    || matches!(bus.as_str(), "usb" | "sdio"),
                can_power_off: prop(d, "CanPowerOff").unwrap_or(false),
                ejectable: prop(d, "Ejectable").unwrap_or(false),
                size: prop(d, "Size").unwrap_or(0),
            });
        }
        if let Some(b) = get(BLOCK) {
            blocks.push(BlockInfo {
                path: path.to_string(),
                drive: path_prop(b, "Drive"),
                backing: path_prop(b, "CryptoBackingDevice"),
                label: prop_str(b, "IdLabel").unwrap_or_default(),
                device: bytes_prop(b, "PreferredDevice")
                    .or_else(|| bytes_prop(b, "Device"))
                    .map(|d| c_string(&d))
                    .unwrap_or_default(),
                size: prop(b, "Size").unwrap_or(0),
                hidden: prop(b, "HintIgnore").unwrap_or(false)
                    || prop(b, "HintSystem").unwrap_or(false),
                mounts: get(FILESYSTEM).map(mounts_prop),
                cleartext: get(ENCRYPTED).map(|e| path_prop(e, "CleartextDevice")),
            });
        }
    }
    Ok(assemble(drives, blocks))
}

/// No options: UDisks picks the mount point and asks polkit as needed.
fn no_options() -> HashMap<&'static str, Value<'static>> {
    HashMap::new()
}

async fn call(conn: &Connection, path: &str, iface: &str, method: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(conn, DEST, path.to_string(), iface.to_string()).await?;
    proxy
        .call_method(method, &(no_options(),))
        .await
        .map_err(refused)?;
    Ok(())
}

pub struct Drives {
    ctx: Ctx,
    mailbox: Mailbox<State>,
    /// What was found last, which ejecting goes by.
    last: State,
    sent: Dedup<State>,
}

impl Drives {
    pub fn start(ctx: Ctx) -> Self {
        let mut me = Self {
            ctx: ctx.clone(),
            mailbox: Default::default(),
            last: Default::default(),
            sent: Default::default(),
        };
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from(DEST),
            Duration::from_millis(300),
            trigger.clone(),
        );
        let mut ticks = ctx.ticks(trigger);
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            ticks.ready().await;
            loop {
                let state = match async { fetch(&c.bus(true).await?).await }.await {
                    Ok(drives) => State {
                        available: true,
                        drives,
                    },
                    Err(e) => {
                        log::debug!("udisks: {e}");
                        State::default()
                    }
                };
                let _ = found.send(state);
                ticks.next(Duration::from_secs(30)).await;
            }
        });
        me
    }

    fn mount(&self, block: String) {
        let ctx = self.ctx.clone();
        self.ctx.act("Mount", async move {
            call(&ctx.bus(true).await?, &block, FILESYSTEM, "Mount").await
        });
    }

    fn unmount(&self, block: String) {
        let ctx = self.ctx.clone();
        self.ctx.act("Unmount", async move {
            call(&ctx.bus(true).await?, &block, FILESYSTEM, "Unmount").await
        });
    }

    /// Unmount everything on the drive, lock what was unlocked, then power it off (or eject the
    /// medium), so it is safe to pull.
    fn eject(&self, drive: &str) {
        let Some(d) = self.last.drives.iter().find(|d| d.path == drive).cloned() else {
            return;
        };
        let ctx = self.ctx.clone();
        self.ctx.act("Eject", async move {
            let conn = ctx.bus(true).await?;
            for v in &d.volumes {
                if v.mount.is_some() {
                    call(&conn, &v.path, FILESYSTEM, "Unmount").await?;
                }
                if let Some(backing) = &v.backing {
                    call(&conn, backing, ENCRYPTED, "Lock").await?;
                }
            }
            if d.can_power_off {
                call(&conn, &d.path, DRIVE, "PowerOff").await?;
            } else if d.ejectable {
                call(&conn, &d.path, DRIVE, "Eject").await?;
            }
            ctx.toast(format!("{} can be removed", d.name));
            Ok::<_, Error>(())
        });
    }

    fn eject_all(&self) {
        for d in &self.last.drives {
            self.eject(&d.path);
        }
    }

    /// Open a mounted volume in the file manager.
    fn open(&self, mount: String) {
        self.ctx.act("Open", async move {
            super::spawn_detached("xdg-open", &[&mount])?;
            Ok::<_, Error>(())
        });
    }
}

impl Stateful for Drives {
    type Command = crate::drives::Command;
    type Inner = State;

    fn mailbox(&mut self) -> &mut Mailbox<State> {
        &mut self.mailbox
    }

    fn on_command(&mut self, cmd: crate::drives::Command) {
        use crate::drives::Command::*;
        match cmd {
            Mount(block) => self.mount(block),
            Unmount(block) => self.unmount(block),
            Eject(drive) => self.eject(&drive),
            EjectAll => self.eject_all(),
            Open(mount) => self.open(mount),
        }
    }

    fn on_inner(&mut self, state: State) {
        self.last = state.clone();
        if self.sent.changed(&state) {
            self.ctx.send(crate::drives::Event::State(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Actor;
    use crate::sources::testing::{ctx, reported, settle};
    use tokio::sync::mpsc::unbounded_channel;

    fn found(available: bool) -> State {
        State {
            available,
            drives: Vec::new(),
        }
    }

    #[tokio::test]
    async fn what_is_found_is_reported_once_and_remembered() {
        let (ctx, events) = ctx();
        let me = Drives::start(ctx);
        let tx = me.mailbox.sender();
        let _ = tx.send(found(true));
        let _ = tx.send(found(true));
        let (commands, inbox) = unbounded_channel();
        drop(commands);
        let me = me.event_loop(inbox).await;
        assert!(me.last.available);
        assert_eq!(reported(events).len(), 1);
    }

    /// Nothing known about that drive: nothing to do, and no bus to go to.
    #[tokio::test]
    async fn ejecting_an_unknown_drive_does_nothing() {
        let (ctx, events) = ctx();
        let me = Drives::start(ctx);
        let (commands, inbox) = unbounded_channel();
        let _ = commands.send(crate::drives::Command::Eject("/no/such/drive".into()));
        let _ = commands.send(crate::drives::Command::EjectAll);
        drop(commands);
        me.event_loop(inbox).await;
        settle().await;
        assert!(reported(events).is_empty());
    }

    fn block(path: &str, drive: &str, label: &str, device: &str) -> BlockInfo {
        BlockInfo {
            path: path.into(),
            drive: drive.into(),
            backing: "/".into(),
            label: label.into(),
            device: device.into(),
            size: 8 << 30,
            hidden: false,
            mounts: Some(Vec::new()),
            cleartext: None,
        }
    }

    #[test]
    fn removable_drives_and_their_volumes() {
        let drives = vec![
            DriveInfo {
                path: "/d/stick".into(),
                name: "SanDisk Cruzer".into(),
                removable: true,
                can_power_off: true,
                ..Default::default()
            },
            DriveInfo {
                path: "/d/nvme".into(),
                name: "Samsung SSD".into(),
                ..Default::default()
            },
            DriveInfo {
                path: "/d/reader".into(),
                name: "Card reader".into(),
                removable: true,
                ..Default::default()
            },
        ];
        let mut mounted = block("/b/sda1", "/d/stick", "STICK", "/dev/sda1");
        mounted.mounts = Some(vec!["/run/media/me/STICK".into()]);
        let mut whole = block("/b/sda", "/d/stick", "", "/dev/sda");
        whole.mounts = None; // The partition table: no filesystem.
        let mut locked = block("/b/sda2", "/d/stick", "", "/dev/sda2");
        locked.mounts = None;
        locked.cleartext = Some("/".into());
        let mut system = block("/b/nvme0n1p2", "/d/nvme", "root", "/dev/nvme0n1p2");
        system.hidden = true;
        let d = assemble(drives, vec![whole, locked, mounted, system]);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].name, "SanDisk Cruzer");
        let names: Vec<&str> = d[0].volumes.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["STICK", "8.0 GiB volume"]);
        assert!(d[0].volumes[1].locked);
        assert_eq!(
            d[0].volumes[0].mount.as_deref(),
            Some("/run/media/me/STICK")
        );
        assert_eq!(d[0].summary(), "mounted at /run/media/me/STICK");
    }

    #[test]
    fn unlocked_volume_belongs_to_its_drive() {
        let drives = vec![DriveInfo {
            path: "/d/stick".into(),
            name: "Stick".into(),
            removable: true,
            ..Default::default()
        }];
        let mut enc = block("/b/sda1", "/d/stick", "", "/dev/sda1");
        enc.mounts = None;
        enc.cleartext = Some("/b/dm0".into());
        let mut clear = block("/b/dm0", "/", "SECRET", "/dev/dm-0");
        clear.backing = "/b/sda1".into();
        let d = assemble(drives, vec![enc, clear]);
        assert_eq!(d[0].volumes.len(), 1);
        assert_eq!(d[0].volumes[0].name, "SECRET");
        assert_eq!(d[0].volumes[0].backing.as_deref(), Some("/b/sda1"));
    }

    #[test]
    fn c_strings() {
        assert_eq!(c_string(b"/dev/sda1\0"), "/dev/sda1");
        assert_eq!(c_string(b"/mnt"), "/mnt");
    }
}

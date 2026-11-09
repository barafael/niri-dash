//! The removable-drives and phone widgets: what UDisks and KDE Connect report, and their buttons.

use super::{App, model};
use crate::{
    Control, Dash, DriveVolume, Drives as UiDrives, PhoneItem, Phones as UiPhones, RemovableDrive,
};
use niri_dash_system::{Tell, drives, perf::human_bytes, phones};
use slint::ComponentHandle;
use std::rc::Rc;

fn volume_detail(v: &drives::Volume) -> String {
    if v.locked {
        "Locked".into()
    } else if let Some(m) = &v.mount {
        m.clone()
    } else if v.size > 0 {
        format!("{} · not mounted", human_bytes(v.size))
    } else {
        "Not mounted".into()
    }
}

impl App {
    pub(super) fn show_drives(&self, s: drives::State) {
        let ui = self.ui();
        let d = ui.global::<UiDrives>();
        d.set_available(s.available);
        // Each volume also goes into one flat list, for the keyboard cursor.
        let mut all: Vec<DriveVolume> = Vec::new();
        let mut drives = Vec::new();
        for drive in &s.drives {
            let mut volumes = Vec::new();
            for v in &drive.volumes {
                let vol = DriveVolume {
                    path: v.path.as_str().into(),
                    name: v.name.as_str().into(),
                    detail: volume_detail(v).into(),
                    mount: v.mount.as_deref().unwrap_or_default().into(),
                    mounted: v.mount.is_some(),
                    locked: v.locked,
                    index: all.len() as i32,
                    drive: drive.path.as_str().into(),
                };
                all.push(vol.clone());
                volumes.push(vol);
            }
            drives.push(RemovableDrive {
                path: drive.path.as_str().into(),
                name: drive.name.as_str().into(),
                detail: drive.summary().into(),
                volumes: model(volumes),
            });
        }
        d.set_drives(model(drives));
        d.set_volumes(model(all));
    }

    pub(super) fn show_phones(&self, s: phones::State) {
        let ui = self.ui();
        let p = ui.global::<UiPhones>();
        p.set_available(s.available);
        p.set_phones(model(
            s.phones
                .iter()
                .map(|ph| PhoneItem {
                    id: ph.id.as_str().into(),
                    name: ph.name.as_str().into(),
                    status: ph.status().into(),
                    reachable: ph.reachable,
                    charge: ph.charge.unwrap_or(-1),
                    charging: ph.charging,
                    can_ring: ph.can_ring,
                    can_share: ph.can_share,
                })
                .collect(),
        ));
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let d = ui.global::<UiDrives>();
    let a = app.clone();
    d.on_mount(move |block| a.tx.drives.tell(drives::Command::Mount(block.into())));
    let a = app.clone();
    d.on_unmount(move |block| a.tx.drives.tell(drives::Command::Unmount(block.into())));
    let a = app.clone();
    d.on_eject(move |drive| a.tx.drives.tell(drives::Command::Eject(drive.into())));
    let a = app.clone();
    d.on_eject_all(move || a.tx.drives.tell(drives::Command::EjectAll));
    let a = app.clone();
    d.on_open(move |mount| {
        a.tx.drives.tell(drives::Command::Open(mount.into()));
        a.pending.borrow_mut().push(Control::Hide);
    });

    let p = ui.global::<UiPhones>();
    let a = app.clone();
    p.on_ring(move |id| a.tx.phones.tell(phones::Command::Ring(id.into())));
    let a = app.clone();
    p.on_send(move |id| {
        // The file chooser would be behind the dashboard.
        a.pending.borrow_mut().push(Control::Hide);
        a.tx.phones.tell(phones::Command::SendFiles(id.into()));
    });
}

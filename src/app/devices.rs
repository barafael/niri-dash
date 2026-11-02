//! The removable-drives and phone widgets: what UDisks and KDE Connect report, and their buttons.

use super::{App, model};
use crate::sources::{kdeconnect, perf::human_bytes, udisks};
use crate::{
    Control, Dash, DriveVolume, Drives as UiDrives, PhoneItem, Phones as UiPhones, RemovableDrive,
};
use slint::ComponentHandle;
use std::rc::Rc;

fn volume_detail(v: &udisks::Volume) -> String {
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
    pub(super) fn show_drives(&self, s: udisks::State) {
        let ui = self.ui();
        let d = ui.global::<UiDrives>();
        d.set_available(s.available);
        d.set_drives(model(
            s.drives
                .iter()
                .map(|drive| RemovableDrive {
                    path: drive.path.as_str().into(),
                    name: drive.name.as_str().into(),
                    detail: drive.summary().into(),
                    volumes: model(
                        drive
                            .volumes
                            .iter()
                            .map(|v| DriveVolume {
                                path: v.path.as_str().into(),
                                name: v.name.as_str().into(),
                                detail: volume_detail(v).into(),
                                mount: v.mount.as_deref().unwrap_or_default().into(),
                                mounted: v.mount.is_some(),
                                locked: v.locked,
                            })
                            .collect(),
                    ),
                })
                .collect(),
        ));
    }

    pub(super) fn show_phones(&self, s: kdeconnect::State) {
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
    d.on_mount(move |block| a.drives.mount(block.into()));
    let a = app.clone();
    d.on_unmount(move |block| a.drives.unmount(block.into()));
    let a = app.clone();
    d.on_eject(move |drive| a.drives.eject(drive.into()));
    let a = app.clone();
    d.on_eject_all(move || a.drives.eject_all());
    let a = app.clone();
    d.on_open(move |mount| {
        a.drives.open(mount.into());
        a.pending.borrow_mut().push(Control::Hide);
    });

    let p = ui.global::<UiPhones>();
    let a = app.clone();
    p.on_ring(move |id| a.phones.ring(id.into()));
    let a = app.clone();
    p.on_send(move |id| a.phones.send_files(id.into()));
}

//! The clipboard through ext-data-control (the standardised protocol; KWin, recent niri) or,
//! failing that, its predecessor wlr-data-control. The two are request-for-request and
//! event-for-event the same, so each object is a two-variant wrapper with the few requests used,
//! and both protocols' Dispatch impls forward into the same handlers.

use super::Services;
use crate::clipboard as vocab;
use crate::sources::clipboard::{self, Clip};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, backend::ObjectId, globals::GlobalList,
    protocol::wl_seat,
};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

/// The clipboard's part of the Wayland state.
pub struct Clipboard {
    control: Option<DataControl>,
    device: Option<DataDevice>,
    /// Whether to read what gets copied. Setting the clipboard needs the device either way, and
    /// the compositor tells it of every selection, but its contents only come if asked for.
    recording: bool,
    /// The offer behind the current selection; destroyed when the next one arrives.
    current_offer: Option<DataOffer>,
    /// Our own clipboard contents while we own the selection.
    own_source: Option<(DataSource, Clip)>,
    /// The mime types each pending offer announced.
    offers: HashMap<ObjectId, Vec<String>>,
}

/// The data control manager, of whichever protocol the compositor offers.
enum DataControl {
    Ext(ExtDataControlManagerV1),
    Wlr(ZwlrDataControlManagerV1),
}

/// A seat's data control device.
enum DataDevice {
    Ext(ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlDeviceV1),
}

/// An offer of another client's selection.
enum DataOffer {
    Ext(ExtDataControlOfferV1),
    Wlr(ZwlrDataControlOfferV1),
}

/// A selection we offer ourselves.
#[derive(PartialEq)]
enum DataSource {
    Ext(ExtDataControlSourceV1),
    Wlr(ZwlrDataControlSourceV1),
}

impl DataControl {
    /// Bind ext-data-control if the compositor offers it, else wlr-data-control.
    fn bind(globals: &GlobalList, qh: &QueueHandle<Services>) -> Option<Self> {
        let ext = globals.bind::<ExtDataControlManagerV1, _, _>(qh, 1..=1, ());
        let control = match ext {
            Ok(m) => Some(Self::Ext(m)),
            Err(_) => globals
                .bind::<ZwlrDataControlManagerV1, _, _>(qh, 1..=2, ())
                .ok()
                .map(Self::Wlr),
        };
        match &control {
            Some(c) => log::info!("clipboard through {}", c.protocol()),
            None => log::warn!(
                "the compositor offers neither ext- nor wlr-data-control; \
                 clipboard history is unavailable"
            ),
        }
        control
    }

    fn protocol(&self) -> &'static str {
        match self {
            Self::Ext(_) => "ext-data-control",
            Self::Wlr(_) => "wlr-data-control",
        }
    }

    fn get_data_device(&self, seat: &wl_seat::WlSeat, qh: &QueueHandle<Services>) -> DataDevice {
        match self {
            Self::Ext(m) => DataDevice::Ext(m.get_data_device(seat, qh, ())),
            Self::Wlr(m) => DataDevice::Wlr(m.get_data_device(seat, qh, ())),
        }
    }

    fn create_data_source(&self, qh: &QueueHandle<Services>) -> DataSource {
        match self {
            Self::Ext(m) => DataSource::Ext(m.create_data_source(qh, ())),
            Self::Wlr(m) => DataSource::Wlr(m.create_data_source(qh, ())),
        }
    }
}

impl DataDevice {
    /// Make `source` the selection. A source always comes from the same manager as the device.
    fn set_selection(&self, source: &DataSource) {
        match (self, source) {
            (Self::Ext(d), DataSource::Ext(s)) => d.set_selection(Some(s)),
            (Self::Wlr(d), DataSource::Wlr(s)) => d.set_selection(Some(s)),
            _ => log::warn!("clipboard source and device are of different protocols"),
        }
    }
}

impl DataOffer {
    fn id(&self) -> ObjectId {
        match self {
            Self::Ext(o) => o.id(),
            Self::Wlr(o) => o.id(),
        }
    }

    fn receive(&self, mime: String, fd: std::os::fd::BorrowedFd) {
        match self {
            Self::Ext(o) => o.receive(mime, fd),
            Self::Wlr(o) => o.receive(mime, fd),
        }
    }

    fn destroy(&self) {
        match self {
            Self::Ext(o) => o.destroy(),
            Self::Wlr(o) => o.destroy(),
        }
    }
}

impl DataSource {
    fn offer(&self, mime: String) {
        match self {
            Self::Ext(s) => s.offer(mime),
            Self::Wlr(s) => s.offer(mime),
        }
    }

    fn destroy(&self) {
        match self {
            Self::Ext(s) => s.destroy(),
            Self::Wlr(s) => s.destroy(),
        }
    }
}

impl Clipboard {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Services>) -> Self {
        Self {
            control: DataControl::bind(globals, qh),
            device: None,
            recording: false,
            current_offer: None,
            own_source: None,
            offers: HashMap::new(),
        }
    }

    /// Whether the compositor offers clipboard access (ext- or wlr-data-control).
    pub fn available(&self) -> bool {
        self.control.is_some()
    }

    /// A seat (gained a capability): get a device for its clipboard, if there is none yet.
    pub fn seat(&mut self, seat: &wl_seat::WlSeat, qh: &QueueHandle<Services>) {
        if self.device.is_none()
            && let Some(m) = &self.control
        {
            self.device = Some(m.get_data_device(seat, qh));
        }
    }

    /// Forget an offer that is going away.
    fn drop_offer(&mut self, offer: &DataOffer) {
        self.offers.remove(&offer.id());
        offer.destroy();
    }
}

impl Services {
    /// Start or stop reading what gets copied. Starting reads the current selection too, unless it
    /// was read already.
    pub fn record_clipboard(&mut self, on: bool) {
        let c = &mut self.clipboard;
        let starting = on && !c.recording;
        c.recording = on;
        if starting && let Some(offer) = c.current_offer.take() {
            self.receive(offer);
        }
    }

    /// Put something on the clipboard.
    pub fn set_clipboard(&mut self, clip: Clip) {
        let c = &mut self.clipboard;
        let (Some(m), Some(dev)) = (&c.control, &c.device) else {
            return;
        };
        let source = m.create_data_source(&self.qh);
        for mime in clip.kind.mimes() {
            source.offer(mime.to_string());
        }
        dev.set_selection(&source);
        if let Some((old, _)) = c.own_source.replace((source, clip)) {
            old.destroy();
        }
        let _ = self.conn.flush();
    }

    /// The device's `selection` event: `None` when the clipboard was cleared.
    fn selection(&mut self, offer: Option<DataOffer>) {
        match offer {
            Some(offer) if self.clipboard.recording => self.receive(offer),
            // Kept unread: recording may start while this is still the selection.
            Some(offer) => {
                if let Some(old) = self.clipboard.current_offer.replace(offer) {
                    self.clipboard.drop_offer(&old);
                }
            }
            None => {
                if let Some(old) = self.clipboard.current_offer.take() {
                    self.clipboard.drop_offer(&old);
                }
            }
        }
    }

    /// The device's `finished` event: it's gone (e.g. its seat was removed).
    fn data_device_finished(&mut self) {
        self.clipboard.device = None;
    }

    /// A paste from our own selection: write `source`'s contents into `fd`.
    fn source_send(&self, source: &DataSource, fd: std::os::fd::OwnedFd) {
        if let Some((_, clip)) = self
            .clipboard
            .own_source
            .as_ref()
            .filter(|(s, _)| s == source)
        {
            let data = clip.data.clone();
            std::thread::spawn(move || {
                let mut f = std::fs::File::from(fd);
                let _ = f.write_all(&data);
            });
        }
    }

    /// Someone else took the selection from `source`.
    fn source_cancelled(&mut self, source: DataSource) {
        let c = &mut self.clipboard;
        if c.own_source.as_ref().is_some_and(|(s, _)| *s == source) {
            c.own_source = None;
        }
        source.destroy();
    }

    /// Read the new selection on a thread of its own and report it.
    fn receive(&mut self, offer: DataOffer) {
        let mimes = self
            .clipboard
            .offers
            .remove(&offer.id())
            .unwrap_or_default();
        let Some((kind, mime)) = clipboard::pick(&mimes) else {
            offer.destroy();
            return;
        };
        let Ok((mut reader, writer)) = std::io::pipe() else {
            offer.destroy();
            return;
        };
        offer.receive(mime.to_string(), std::os::fd::AsFd::as_fd(&writer));
        let _ = self.conn.flush();
        drop(writer);
        if let Some(old) = self.clipboard.current_offer.replace(offer) {
            self.clipboard.drop_offer(&old);
        }
        let tx = self.events.clone();
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) if data.len() + n <= clipboard::MAX_BYTES => {
                        data.extend_from_slice(&chunk[..n])
                    }
                    Ok(_) => return, // too big to keep
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
            log::debug!("clipboard: received {} bytes of {mime}", data.len());
            if !data.is_empty() {
                let clip = Clip {
                    kind,
                    data: Arc::new(data),
                };
                let _ = tx.send(vocab::Event::Copied(clip).into());
            }
        });
    }

    /// An offer announced one of its mime types.
    fn offered(&mut self, offer: ObjectId, mime: String) {
        self.clipboard.offers.entry(offer).or_default().push(mime);
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for Services {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for Services {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for Services {
    fn event(
        me: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_data_control_device_v1::Event;
        match event {
            Event::Selection { id } => me.selection(id.map(DataOffer::Ext)),
            // Only the clipboard is kept, not the primary selection.
            Event::PrimarySelection { id: Some(offer) } => {
                me.clipboard.drop_offer(&DataOffer::Ext(offer))
            }
            Event::Finished => me.data_device_finished(),
            _ => {}
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(Services, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for Services {
    fn event(
        me: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_data_control_device_v1::Event;
        match event {
            Event::Selection { id } => me.selection(id.map(DataOffer::Wlr)),
            Event::PrimarySelection { id: Some(offer) } => {
                me.clipboard.drop_offer(&DataOffer::Wlr(offer))
            }
            Event::Finished => me.data_device_finished(),
            _ => {}
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(Services, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, ()> for Services {
    fn event(
        me: &mut Self,
        offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            me.offered(offer.id(), mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for Services {
    fn event(
        me: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            me.offered(offer.id(), mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for Services {
    fn event(
        me: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_data_control_source_v1::Event;
        let source = DataSource::Ext(source.clone());
        match event {
            Event::Send { fd, .. } => me.source_send(&source, fd),
            Event::Cancelled => me.source_cancelled(source),
            _ => {}
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for Services {
    fn event(
        me: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use zwlr_data_control_source_v1::Event;
        let source = DataSource::Wlr(source.clone());
        match event {
            Event::Send { fd, .. } => me.source_send(&source, fd),
            Event::Cancelled => me.source_cancelled(source),
            _ => {}
        }
    }
}

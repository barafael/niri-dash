//! The volume widget: the default speaker and microphone, the output to play on and a slider per
//! app (the mixer). Port of `VolumeWidget` (`legacy/ui/widgets/system.slint`) and
//! `legacy/src/app/audio.rs`.
//!
//! A slider the user dragged keeps its value for a moment against what the sound server reports,
//! which lags behind (`touched`, and `stream_touched` for the app sliders).

use super::sliders::{self, Knob, Row, percent, scroll_list, slider_row};
use super::{Fit, NoStops, digit_level, on, setter};
use crate::dash::Dash;
use crate::keys::{Key, Named};
use crate::kit::{self, Tk};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, ElementId, ImageSource, RenderImage, Role, ScrollHandle, div, img, px,
};
use niri_dash_system::audio::{Command, Event, Sink};
use niri_dash_system::{IconSrc, Tell};
use std::sync::Arc;

/// An app's row in the mixer.
pub struct Stream {
    pub id: u32,
    pub app: String,
    src: IconSrc,
    icon: Option<ImageSource>,
    pub volume: f32,
    pub muted: bool,
}

#[derive(Default)]
pub struct State {
    pub scope: Scope,
    /// The list of outputs is open.
    pub picking: bool,
    pub volume: f32,
    pub muted: bool,
    pub mic_volume: f32,
    pub mic_muted: bool,
    pub sink_name: String,
    pub sinks: Vec<Sink>,
    pub streams: Vec<Stream>,
    /// The tick at which the speaker or microphone slider was last dragged.
    pub touched: Option<u64>,
    /// The app slider last dragged, and when.
    pub stream_touched: Option<(u32, u64)>,
    pub scroll: ScrollHandle,
}

pub fn event(d: &mut Dash, event: &Event) {
    let Event::State(s) = event;
    let c = &mut d.widgets.controls;
    let hold_main = c.held(c.volume.touched);
    let hold_stream = c.held(c.volume.stream_touched.map(|(_, t)| t));
    let v = &mut c.volume;
    if !hold_main {
        v.volume = s.volume;
        if let Some((level, _)) = s.mic {
            v.mic_volume = level;
        }
    }
    v.muted = s.muted;
    v.mic_muted = s.mic.is_none_or(|(_, muted)| muted);
    v.sink_name = s.sink_name.clone();
    v.sinks = s.sinks.clone();
    let touched = v.stream_touched.filter(|_| hold_stream).map(|(id, _)| id);
    let old = std::mem::take(&mut v.streams);
    v.streams = s
        .streams
        .iter()
        .map(|st| {
            let before = old.iter().find(|r| r.id == st.id);
            let volume = match before {
                Some(r) if touched == Some(st.id) => r.volume,
                _ => st.volume,
            };
            // A picture that didn't change is not made again.
            let icon = match before {
                Some(r) if r.src == st.icon => r.icon.clone(),
                _ => picture(&st.icon),
            };
            Stream {
                id: st.id,
                app: st.app.clone(),
                src: st.icon.clone(),
                icon,
                volume,
                muted: st.muted,
            }
        })
        .collect();
}

/// What an app's icon is, for GPUI.
fn picture(src: &IconSrc) -> Option<ImageSource> {
    match src {
        IconSrc::None => None,
        IconSrc::Path(p) => Some(ImageSource::from(p.clone())),
        IconSrc::Rgba { width, height, data } => {
            // GPUI wants its pixels as BGRA.
            let mut px = data.as_ref().clone();
            for p in px.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
            }
            let buf = image::RgbaImage::from_raw(*width, *height, px)?;
            Some(ImageSource::from(Arc::new(RenderImage::new([image::Frame::new(buf)]))))
        }
    }
}

/// What the widget has room for, and where the keyboard stops are: the output picker, the
/// outputs while picking, the output and microphone sliders, then the apps.
struct Layout {
    can_pick: bool,
    show_apps: bool,
    picking: bool,
    has_mic: bool,
    k_sinks: usize,
    k_out: usize,
    k_apps: usize,
    k_count: usize,
}

impl Layout {
    fn of(f: &Fit, v: &State) -> Self {
        let can_pick = f.h >= 200.0 && v.sinks.len() > 1;
        let show_apps = f.h >= 280.0;
        let picking = v.picking;
        let k_sinks = usize::from(can_pick);
        let k_out = k_sinks + if picking && can_pick { v.sinks.len() } else { 0 };
        let has_mic = f.h > 130.0;
        let k_apps = k_out + 1 + usize::from(has_mic);
        let k_count = k_apps + if show_apps && !picking { v.streams.len() } else { 0 };
        Self {
            can_pick,
            show_apps,
            picking,
            has_mic,
            k_sinks,
            k_out,
            k_apps,
            k_count,
        }
    }

    fn count(&self, f: &Fit) -> usize {
        if f.round { 0 } else { self.k_count }
    }
}

fn set_volume(d: &mut Dash, v: f32) {
    let c = &mut d.widgets.controls;
    let v = v.clamp(0.0, 1.0);
    c.volume.volume = v;
    c.volume.touched = Some(c.ticks);
    d.tx.audio.tell(Command::SetVolume(v));
}

fn set_mic_volume(d: &mut Dash, v: f32) {
    let c = &mut d.widgets.controls;
    let v = v.clamp(0.0, 1.0);
    c.volume.mic_volume = v;
    c.volume.touched = Some(c.ticks);
    d.tx.audio.tell(Command::SetMicVolume(v));
}

fn set_stream_volume(d: &mut Dash, id: u32, v: f32) {
    let c = &mut d.widgets.controls;
    let v = v.clamp(0.0, 1.0);
    if let Some(s) = c.volume.streams.iter_mut().find(|s| s.id == id) {
        s.volume = v;
    }
    c.volume.stream_touched = Some((id, c.ticks));
    d.tx.audio.tell(Command::SetStreamVolume(id, v));
}

/// Set (`set`, 0 to 1) or step (by `by`) the slider at stop `i`.
fn set_level(d: &mut Dash, lay: &Layout, round: bool, i: usize, set: Option<f32>, by: f32) {
    let v = &d.widgets.controls.volume;
    if round || i == lay.k_out {
        let new = set.unwrap_or(v.volume + by);
        set_volume(d, new);
    } else if i == lay.k_out + 1 && lay.has_mic {
        let new = set.unwrap_or(v.mic_volume + by);
        set_mic_volume(d, new);
    } else if i >= lay.k_apps
        && let Some(st) = v.streams.get(i - lay.k_apps)
    {
        let (id, new) = (st.id, set.unwrap_or(st.volume + by));
        set_stream_volume(d, id, new);
    }
}

fn mute(d: &mut Dash, lay: &Layout, round: bool, i: usize) {
    let v = &d.widgets.controls.volume;
    if round || i == lay.k_out {
        d.tx.audio.tell(Command::ToggleMute);
    } else if i == lay.k_out + 1 && lay.has_mic {
        d.tx.audio.tell(Command::ToggleMicMute);
    } else if i >= lay.k_apps
        && let Some(st) = v.streams.get(i - lay.k_apps)
    {
        let id = st.id;
        d.tx.audio.tell(Command::ToggleStreamMute(id));
    }
}

struct Keys<'a> {
    d: &'a mut Dash,
    lay: &'a Layout,
    round: bool,
    cursor: usize,
    /// Where the cursor goes after the key (the output was picked).
    cursor_to: Option<usize>,
}

impl Stops for Keys<'_> {
    fn filter(&mut self, key: &Key) -> bool {
        let down = key.is('j') || key.is_named(Named::Down);
        let up = key.is('k') || key.is_named(Named::Up);
        if (down || up) && (self.round || self.cursor >= self.lay.k_out) {
            let by = if down { -sliders::STEP } else { sliders::STEP };
            set_level(self.d, self.lay, self.round, self.cursor, None, by);
            return true;
        }
        false
    }

    fn activate(&mut self, i: usize, _: usize) {
        let lay = self.lay;
        if !self.round && lay.can_pick && i == 0 {
            let v = &mut self.d.widgets.controls.volume;
            v.picking = !v.picking;
        } else if !self.round && i < lay.k_out {
            let id = self
                .d
                .widgets
                .controls
                .volume
                .sinks
                .get(i - lay.k_sinks)
                .map(|s| s.id);
            if let Some(id) = id {
                self.d.tx.audio.tell(Command::SetDefaultSink(id));
            }
            self.d.widgets.controls.volume.picking = false;
            self.cursor_to = Some(lay.k_sinks);
        } else {
            mute(self.d, lay, self.round, i);
        }
    }

    fn adjust(&mut self, i: usize, delta: i32) -> bool {
        if !self.round && i < self.lay.k_out {
            return false;
        }
        set_level(self.d, self.lay, self.round, i, None, delta as f32 * sliders::STEP);
        true
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        if key.is('m') {
            mute(self.d, self.lay, self.round, i);
            return true;
        }
        if let Some(level) = digit_level(key)
            && (self.round || i >= self.lay.k_out)
        {
            set_level(self.d, self.lay, self.round, i, Some(level), 0.0);
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        let v = &mut self.d.widgets.controls.volume;
        std::mem::replace(&mut v.picking, false)
    }
}

/// Escape closes the list of outputs first.
pub fn back(d: &mut Dash) -> bool {
    std::mem::replace(&mut d.widgets.controls.volume.picking, false)
}

/// On a slider j and k (h and l too) make it quieter and louder by 5 %, 1-9 and 0 set 10-100 %,
/// m mutes. Tab goes to the next slider or the output picker; there j/k move as anywhere else
/// and Enter opens the picker or picks an output.
pub fn key(d: &mut Dash, fit: &Fit, key: &Key) -> bool {
    let lay = Layout::of(fit, &d.widgets.controls.volume);
    let mut scope = d.widgets.controls.volume.scope;
    scope.set_count(lay.count(fit), &NoStops);
    let mut keys = Keys {
        d,
        lay: &lay,
        round: fit.round,
        cursor: scope.cursor,
        cursor_to: None,
    };
    let taken = scope.press(key, &mut keys);
    let cursor_to = keys.cursor_to;
    if let Some(c) = cursor_to {
        scope.cursor = c;
        scope.sub = 0;
    }
    // The picker opened or closed, or an app's volume changed under the list: new stops.
    let lay = Layout::of(fit, &d.widgets.controls.volume);
    scope.set_count(lay.count(fit), &NoStops);
    if !fit.round && scope.cursor >= lay.k_apps {
        d.widgets.controls.volume.scroll.scroll_to_item(scope.cursor - lay.k_apps);
    }
    d.widgets.controls.volume.scope = scope;
    taken
}

pub fn refit(d: &mut Dash, fit: &Fit) {
    let n = Layout::of(fit, &d.widgets.controls.volume).count(fit);
    d.widgets.controls.volume.scope.set_count(n, &NoStops);
}

pub fn describe(v: &State, fit: &Fit) -> String {
    let lay = Layout::of(fit, v);
    format!(
        "stop {} of {}{}",
        v.scope.cursor,
        lay.count(fit),
        if v.picking { ", picking an output" } else { "" }
    )
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let fit = Fit::of_ctx(ctx);
    let v = &ctx.d.widgets.controls.volume;
    let tk = ctx.tk;
    let id = ctx.tile.id() as usize;
    if fit.round {
        return div()
            .size_full()
            .child(sliders::ring_knob(
                Knob {
                    id: ("volume-knob", id),
                    name: if v.muted { "Volume, muted" } else { "Volume" }.into(),
                    value: v.volume,
                    tint: tk.accent(),
                    icon: if v.muted { "volume-mute" } else { "volume" },
                    label: if v.muted { "muted".into() } else { percent(v.volume) },
                    dimmed: v.muted,
                    kfocus: ctx.entered,
                    side: fit.w,
                },
                tk,
                setter(cx, |d, v, _| set_volume(d, v)),
                on(cx, |d, _| {
                    d.tx.audio.tell(Command::ToggleMute);
                }),
            ))
            .into_any_element();
    }
    let lay = Layout::of(&fit, v);
    let cursor = v.scope.cursor.min(lay.k_count.saturating_sub(1));
    let at = |stop: usize| ctx.entered && cursor == stop;
    let slot = |n: usize| id * 1000 + n;

    let output = if lay.can_pick {
        let (hover, accent) = (tk.hover(), tk.accent());
        let kf = at(0);
        div()
            .id(ElementId::from(("volume-output", id)))
            .role(Role::Button)
            .aria_label(if lay.picking { "Close output list" } else { "Choose output" })
            .aria_value(v.sink_name.clone())
            .aria_selected(kf)
            .debug_selector({
                let name = if lay.picking { "Close output list" } else { "Choose output" };
                move || name.to_string()
            })
            .flex_none()
            .h(px(24.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.0))
            .pl(px(4.0))
            .rounded(px(tk.radius_small().min(6.0)))
            .border(px(2.0))
            .border_color(if kf { accent } else { gpui::transparent_black() })
            .when(kf, |s| s.bg(hover))
            .hover(move |s| s.bg(hover))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, crate::kit::own_press)
            .on_click(on(cx, |d, _| {
                let v = &mut d.widgets.controls.volume;
                v.picking = !v.picking;
            }))
            .child(muted_text(v.sink_name.clone(), tk).flex_1().min_w_0())
            .child(div().pr(px(2.0)).child(kit::icon(
                if lay.picking { "chevron-left" } else { "chevron-right" },
                14.0,
                tk.dim(),
            )))
            .into_any_element()
    } else {
        muted_text(v.sink_name.clone(), tk).flex_none().into_any_element()
    };

    let sinks = (lay.picking && lay.can_pick).then(|| {
        div()
            .flex()
            .flex_col()
            .flex_none()
            .children(v.sinks.iter().enumerate().map(|(i, k)| {
                let kf = at(lay.k_sinks + i);
                let (hover, accent, soft) = (tk.hover(), tk.accent(), tk.accent_soft());
                let sink_id = k.id;
                div()
                    .id(ElementId::from(("volume-sink", slot(i))))
                    .role(Role::ListItem)
                    .aria_label(k.name.clone())
                    .aria_value(if k.default { "default" } else { "" })
                    .aria_selected(kf)
                    .debug_selector({
                        let name = k.name.clone();
                        move || name.clone()
                    })
                    .h(px(30.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.0))
                    .rounded(px(tk.radius_small().min(15.0)))
                    .border(px(2.0))
                    .border_color(if kf { accent } else { gpui::transparent_black() })
                    .bg(if k.default {
                        soft
                    } else if kf {
                        hover
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover))
                    .cursor_pointer()
                    .text_size(px(tk.font_small()))
                    .text_color(if k.default { accent } else { tk.text() })
                    .on_mouse_down(gpui::MouseButton::Left, crate::kit::own_press)
                    .on_click(on(cx, move |d, _| {
                        d.tx.audio.tell(Command::SetDefaultSink(sink_id));
                        d.widgets.controls.volume.picking = false;
                    }))
                    .child(div().truncate().child(k.name.clone()))
            }))
    });

    let main = slider_row(
        Row {
            id: ("volume-slider", slot(0)),
            label: if v.muted { "Volume, muted" } else { "Volume" }.into(),
            icon: if v.muted { "volume-mute" } else { "volume" },
            icon_label: Some(if v.muted { "Unmute" } else { "Mute" }.into()),
            icon_tint: if v.muted { tk.faint() } else { tk.text() },
            icon_box: 32.0,
            tint: if v.muted { tk.faint() } else { tk.accent() },
            enabled: true,
            value: v.volume,
            kfocus: at(lay.k_out),
            kfocus_icon: false,
        },
        tk,
        setter(cx, |d, v, _| set_volume(d, v)),
        on(cx, |d, _| {
            d.tx.audio.tell(Command::ToggleMute);
        }),
    );

    let mic = (fit.h > 130.0).then(|| {
        slider_row(
            Row {
                id: ("volume-slider", slot(2)),
                label: if v.mic_muted { "Microphone, muted" } else { "Microphone" }.into(),
                icon: if v.mic_muted { "mic-off" } else { "mic" },
                icon_label: Some(
                    if v.mic_muted { "Unmute microphone" } else { "Mute microphone" }.into(),
                ),
                icon_tint: if v.mic_muted { tk.faint() } else { tk.text() },
                icon_box: 32.0,
                tint: if v.mic_muted { tk.faint() } else { tk.good() },
                enabled: true,
                value: v.mic_volume,
                kfocus: at(lay.k_out + 1),
                kfocus_icon: false,
            },
            tk,
            setter(cx, |d, v, _| set_mic_volume(d, v)),
            on(cx, |d, _| {
                d.tx.audio.tell(Command::ToggleMicMute);
            }),
        )
    });

    let apps = lay.show_apps && !lay.picking;
    let separator = apps.then(|| div().flex_none().h(px(1.0)).w_full().bg(tk.border()));
    let nothing = (apps && v.streams.is_empty()).then(|| {
        muted_text("No app is playing sound".into(), tk)
            .flex_none()
            .flex()
            .justify_center()
    });
    let list = apps.then(|| {
        let rows = v
            .streams
            .iter()
            .enumerate()
            .map(|(i, st)| {
                let stream = st.id;
                let kf = at(lay.k_apps + i);
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .pb(px(4.0))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.0))
                            .h(px(18.0))
                            .when_some(st.icon.clone(), |r, src| {
                                r.child(img(src).size(px(16.0)).flex_none())
                            })
                            .child(
                                muted_text(st.app.clone(), tk).min_w_0().text_color(tk.dim()),
                            ),
                    )
                    .child(slider_row(
                        Row {
                            id: ("stream-slider", slot(10 + 2 * i)),
                            label: if st.muted {
                                format!("{}, muted", st.app)
                            } else {
                                st.app.clone()
                            }
                            .into(),
                            icon: if st.muted { "volume-mute" } else { "volume" },
                            icon_label: Some(
                                format!("{} {}", if st.muted { "Unmute" } else { "Mute" }, st.app)
                                    .into(),
                            ),
                            icon_tint: if st.muted { tk.faint() } else { tk.text() },
                            icon_box: 28.0,
                            tint: if st.muted { tk.faint() } else { tk.accent() },
                            enabled: true,
                            value: st.volume,
                            kfocus: kf,
                            kfocus_icon: false,
                        },
                        tk,
                        setter(cx, move |d, v, _| set_stream_volume(d, stream, v)),
                        on(cx, move |d, _| {
                            d.tx.audio.tell(Command::ToggleStreamMute(stream));
                        }),
                    ))
                    .into_any_element()
            })
            .collect();
        scroll_list(("volume-apps", id), &v.scroll, rows, tk)
    });

    div()
        .size_full()
        .p(px(fit.pad))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .when(!(lay.show_apps || lay.picking), |c| c.justify_center())
        .child(output)
        .children(sinks)
        .child(main)
        .children(mic)
        .children(separator)
        .children(nothing)
        .children(list)
        .into_any_element()
}

fn muted_text(text: String, tk: &Tk) -> gpui::Div {
    div()
        .truncate()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .child(text)
}

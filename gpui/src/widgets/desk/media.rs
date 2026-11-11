//! The media widget: the player (MPRIS) that is playing, its cover, progress and controls. Port
//! of `MediaWidget` in `ui/widgets/desk.slint` and `show_media` in
//! `src/app/feeds.rs`.

use super::parts::{self, Look};
use super::pics::{Pic, Pics};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Role, div, px};
use niri_dash_system::media::{Command, State as Sys};
use niri_dash_system::{IconSrc, Senders, Tell};

#[derive(Default)]
pub struct State {
    /// A player is there.
    pub available: bool,
    pub title: String,
    pub artist: String,
    pub player: String,
    pub playing: bool,
    pub progress: f32,
    pub art: Option<Pic>,
    /// The cover as it came and what was made of it: the state arrives every second, and the
    /// pixels are only turned into a picture when they change.
    last_art: Option<(IconSrc, Option<Pic>)>,
}

/// The player's state, or none.
pub fn show(d: &mut Dash, m: Option<Sys>) {
    let desk = &mut d.widgets.desk;
    desk.media.take(&mut desk.pics, m);
}

impl State {
    fn take(&mut self, pics: &mut Pics, m: Option<Sys>) {
        self.available = m.is_some();
        let m = m.unwrap_or_default();
        self.title = m.title;
        self.artist = m.artist;
        self.player = m.player;
        self.playing = m.playing;
        self.progress = m.progress;
        self.art = match (m.art, self.last_art.as_ref()) {
            (Some(new), Some((old, pic))) if new == *old => pic.clone(),
            (Some(new), _) => {
                let pic = pics.get(&new);
                self.last_art = Some((new, pic.clone()));
                pic
            }
            (None, _) => {
                self.last_art = None;
                None
            }
        };
    }

    fn headline(&self) -> String {
        if !self.available {
            "Nothing playing".into()
        } else if self.title.is_empty() {
            "Unknown title".into()
        } else {
            self.title.clone()
        }
    }
}

struct Keys<'a>(&'a Senders);

impl Stops for Keys<'_> {
    fn activate(&mut self, _: usize, _: usize) {
        self.0.media.tell(Command::PlayPause);
    }

    fn adjust(&mut self, _: usize, delta: i32) -> bool {
        self.0.media.tell(if delta > 0 {
            Command::Next
        } else {
            Command::Previous
        });
        true
    }
}

/// Enter or Space plays and pauses, h/l skip back and forward (j/k too: one stop).
pub fn key(d: &mut Dash, key: &Key) -> bool {
    Scope::default().press(key, &mut Keys(&d.tx))
}

pub fn describe(d: &Dash) -> String {
    let m = &d.widgets.desk.media;
    if m.available {
        format!(
            "{} {:?}",
            if m.playing { "playing" } else { "paused" },
            m.title
        )
    } else {
        "nothing playing".into()
    }
}

/// A progress bar `value` (0 to 1) full, 4 px thick.
fn progress(key: u64, value: f32, label: &'static str, tk: &crate::kit::Tk) -> impl IntoElement {
    let (track, accent, ms) = (tk.track(), tk.accent(), 300.0 * tk.motion());
    let value = value.clamp(0.0, 1.0);
    div()
        .id("media-progress")
        .role(Role::ProgressIndicator)
        .aria_label(label)
        .aria_numeric_value((value.clamp(0.0, 1.0) * 100.0).round() as f64)
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .w_full()
        .h(px(4.0))
        .flex_none()
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    // New readings slide in rather than jump.
                    let shown = crate::kit::glide(key, value, ms, window);
                    let r = b.size.height / 2.0;
                    window.paint_quad(gpui::fill(b, track).corner_radii(r));
                    let mut part = b;
                    part.size.width = b.size.width * shown.clamp(0.0, 1.0);
                    if part.size.width > px(0.0) {
                        window.paint_quad(gpui::fill(part, accent).corner_radii(r));
                    }
                },
            )
            .size_full(),
        )
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let m = &ctx.d.widgets.desk.media;
    let on = ctx.entered;
    let id = ctx.tile.id() as usize;
    let (avail, playing) = (m.available, m.playing);
    let play_label = if playing { "Pause" } else { "Play" };
    let play_icon = if playing { "pause" } else { "play" };
    let skip = |name: &'static str,
                icon: &'static str,
                size: f32,
                forward: bool,
                cx: &mut Context<Dash>| {
        parts::icon_button(
            (name, id),
            icon,
            name,
            size,
            tk.text(),
            Look {
                enabled: avail,
                ..Look::on()
            },
            tk,
            cx.listener(move |d, _: &ClickEvent, _, _| {
                d.tx.media.tell(if forward {
                    Command::Next
                } else {
                    Command::Previous
                });
            }),
        )
    };
    let play = |size: f32, icon_px: Option<f32>, cx: &mut Context<Dash>| {
        parts::icon_button(
            ("media-play", id),
            play_icon,
            play_label,
            size,
            tk.text(),
            Look {
                enabled: avail,
                kfocus: on,
                fill: Some(tk.card_raised()),
                icon_size: icon_px,
                ..Look::on()
            },
            tk,
            cx.listener(|d, _: &ClickEvent, _, _| {
                d.tx.media.tell(Command::PlayPause);
            }),
        )
    };

    if ctx.round {
        let w = ctx.w;
        return div()
            .size_full()
            .relative()
            .child(
                div()
                    .id(("media-ring", id))
                    .size_full()
                    .absolute()
                    .role(Role::ProgressIndicator)
                    .aria_label("Track progress")
                    .child(parts::ring(
                        0x3ED1A + id as u64,
                        0.0,
                        m.progress,
                        (w * 0.045).max(4.0),
                        tk.accent(),
                        tk.track(),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(play(w * 0.42, Some(w * 0.42 * 0.4), cx)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(ctx.h * 0.76))
                    .left(px(w * 0.15))
                    .w(px(w * 0.7))
                    .truncate()
                    .text_center()
                    .text_size(px((w * 0.06).max(9.0)))
                    .text_color(tk.dim())
                    .child(if avail {
                        m.title.clone()
                    } else {
                        "Nothing playing".into()
                    }),
            )
            .into_any_element();
    }

    let lane = ctx.h < 120.0;
    let headline = m.headline();
    let headline_color = if avail { tk.text() } else { tk.faint() };
    let sub = if m.artist.is_empty() {
        m.player.clone()
    } else {
        format!("{}  ·  {}", m.artist, m.player)
    };
    if lane {
        let pad = tk.pad_tight();
        let side = 48.0f32.min(ctx.h - 2.0 * pad);
        return div()
            .size_full()
            .px(px(pad))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .when_some(m.art.as_ref(), |row, art| row.child(art.square(side, true)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(tk.font_body()))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(headline_color)
                            .child(headline),
                    )
                    .when(avail, |c| {
                        c.child(parts::muted(
                            if m.artist.is_empty() {
                                m.player.clone()
                            } else {
                                m.artist.clone()
                            },
                            tk,
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .gap(px(2.0))
                    .child(skip("Previous track", "prev", 30.0, false, cx))
                    .child(play(36.0, None, cx))
                    .child(skip("Next track", "next", 30.0, true, cx)),
            )
            .into_any_element();
    }

    let pad = tk.pad_tight();
    let art_w = (ctx.h - 2.0 * pad - 46.0).max(0.0);
    div()
        .size_full()
        .p(px(pad))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(6.0))
        .child(
            div()
                // With art the top takes the room (the art is as high as it is); without, it is
                // as high as its text, and the whole is centered, as in Slint.
                .when(m.art.is_some(), |t| t.flex_1().min_h_0())
                .flex()
                .flex_row()
                .gap(px(12.0))
                .when_some(m.art.as_ref(), |row, art| {
                    row.child(art.boxed(art_w, (ctx.h - 2.0 * pad - 46.0).max(0.0), true))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(px(4.0))
                        .child(
                            div()
                                .truncate()
                                .when(m.art.is_none(), |c| c.text_center())
                                .text_size(px(tk.font_title()))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(headline_color)
                                .child(headline),
                        )
                        .when(avail, |c| {
                            c.child(
                                parts::muted(sub, tk).when(m.art.is_none(), |c| c.text_center()),
                            )
                            .child(progress(
                                0x6d65_0000 + id as u64,
                                m.progress,
                                "Track progress",
                                tk,
                            ))
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(skip("Previous track", "prev", 34.0, false, cx))
                .child(play(40.0, None, cx))
                .child(skip("Next track", "next", 34.0, true, cx)),
        )
        .into_any_element()
}

//! Drawing the dashboard: the backdrop, the top bar, a Blender panel per widget at its place on
//! the grid, the hint labels, the widget blown up with `e`, the quick tips and the toast. Edit
//! mode's parts are `edit`'s.

use crate::dash::{Dash, Key, Mode, Op};
use crate::look::CSS;
use crate::nav::Tip;
use crate::widgets::{self, Ctx, act, run};
use blenderui::prelude::*;

/// The window's root view: it only shows the [`Dash`], which outlives it.
pub struct DashView {
    dash: Entity<Dash>,
}

impl DashView {
    pub fn new(dash: Entity<Dash>) -> Self {
        Self { dash }
    }
}

impl Render for DashView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vs = window.viewport_size();
        let (w, h) = (vs.width.0, vs.height.0);
        if self.dash.read(cx).size != (w, h) {
            self.dash.update(cx, |d, _| d.fit(w, h));
        }
        let hd = self.dash.downgrade();
        let d = self.dash.read(cx);
        root(d, hd)
    }
}

/// The whole tree, for the window and for snapshots.
pub fn root(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let keys = move |e: &KeyDownEvent, phase: DispatchPhase, window: &mut Window, app: &mut App| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let typing = window.text_field_focused();
        let k = Key::from(&e.keystroke);
        let used = hd
            .update(app, |d, cx| d.key(&k, typing, hd, cx))
            .unwrap_or(false);
        if used {
            app.prevent_default();
        }
    };
    // A press that moves or lets go is no hold; the click that ends a hold isn't the widget's.
    let moved = move |e: &MouseMoveEvent, phase: DispatchPhase, _: &mut Window, app: &mut App| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let at = (e.position.x.0, e.position.y.0);
        hd.update(app, |d, _| {
            if d.press.is_some_and(|p| {
                (p.at.0 - at.0).abs() + (p.at.1 - at.1).abs() >= crate::dash::HOLD_SLOP
            }) {
                d.press = None;
            }
        });
    };
    let released = move |_: &MouseUpEvent, phase: DispatchPhase, _: &mut Window, app: &mut App| {
        if phase == DispatchPhase::Capture {
            hd.update(app, |d, _| d.press = None);
        }
    };
    // A new press is a new start: a hold's swallowed click was its own release, if any.
    let pressed = move |_: &MouseDownEvent, phase: DispatchPhase, _: &mut Window, app: &mut App| {
        if phase == DispatchPhase::Capture {
            hd.update(app, |d, _| d.swallow_click = false);
        }
    };
    let swallow = move |_: &ClickEvent, _: &mut Window, app: &mut App| {
        if hd
            .update(app, |d, _| std::mem::take(&mut d.swallow_click))
            .unwrap_or(false)
        {
            app.stop_propagation();
        }
    };
    let content = if d.solo.is_some() {
        solo(d, hd)
    } else {
        dashboard(d, hd)
    };
    html()
        .styled()
        .child(head().child(style().child(CSS)))
        .child(
            body()
                .style(d.look.vars())
                .on_key_event(keys)
                .on_mouse_event(moved)
                .on_mouse_event(released)
                .on_mouse_event(pressed)
                .on_click_capture(swallow)
                .child(content),
        )
}

/// A standalone app: its widget, and its keys under it.
fn solo(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let Some(p) = d.solo.as_ref() else {
        return div();
    };
    let (w, h) = d.size;
    let mut c = Ctx::new(d, p, w - 24.0, h - 70.0, Some(d.nav.cursor), true, hd);
    let body = widgets::build(&mut c);
    let mut out = div().class("solo").child(div().class("body").child(body));
    if let Some(m) = crate::widgets::menu(d, hd) {
        out = out.child(m);
    }
    out.child(tips(d, hd)).children(toast(d))
}

fn dashboard(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let g = d.geometry();
    let gap = d.look.style.gap;
    let editing = d.edit.is_some();
    // The backdrop takes the clicks that land on no widget: away with the dashboard (or, in
    // edit mode, let go of the picked widget); a right click opens edit mode.
    let backdrop = div()
        .style("position: absolute; left: 0; top: 0; right: 0; bottom: 0")
        .on_click(run::<ClickEvent>(
            hd,
            act(|d: &mut Dash, _| {
                match &mut d.edit {
                    // A widget opened by a hold is done with.
                    Some(e) if e.adjust => {
                        d.edit = None;
                        d.nav = Default::default();
                    }
                    Some(e) => {
                        e.pick(0);
                        d.nav.active = 0;
                    }
                    None => d.hide(),
                }
            }),
        ))
        .on_context_menu(run::<ContextMenuEvent>(
            hd,
            act(|d: &mut Dash, _| {
                if d.edit.as_ref().is_none_or(|e| e.adjust) {
                    d.edit = Some(crate::edit::Edit::new(0));
                    d.nav = Default::default();
                }
            }),
        ));
    let mut dash = div().class("dash").child(backdrop);
    if editing || d.nav.op.is_some() {
        dash = dash.child(crate::edit::grid(d, &g));
    }
    let tiles = d.tiles();
    for (p, label) in &tiles {
        let (x, y, w, h) = g.place(p.rect(), gap);
        let entered = d.nav.mode == Mode::Widget && d.nav.active == p.id && d.nav.expanded != p.id;
        let mut c = Ctx::new(
            d,
            p,
            w - 2.0 * pad(d),
            h - 22.0 - 2.0 * pad(d),
            entered.then_some(d.nav.cursor),
            false,
            hd,
        );
        let body = widgets::build(&mut c);
        let mut card = card(d, p, body, &c).id(format!("w{}", p.id)).style(format!(
            "left: {x}px; top: {y}px; width: {w}px; height: {h}px"
        ));
        if entered {
            card = card.class("entered");
        }
        let adjusting = d.edit.as_ref().is_some_and(|e| e.adjust);
        let open = match &d.edit {
            Some(e) if e.adjust => e.picked == p.id,
            Some(_) => true,
            None => false,
        };
        if open || (d.nav.op.is_some() && d.nav.active == p.id) {
            card = crate::edit::decorate(d, hd, p, card);
        }
        if !open && !(editing && !adjusting) {
            // A press held still here opens the widget for adjusting (`Dash::held_press`).
            let id = p.id;
            card = card.on_mouse_down_capture(
                MouseButton::Left,
                move |e: &MouseDownEvent, _: &mut Window, app: &mut App| {
                    let at = (e.position.x.0, e.position.y.0);
                    hd.update(app, |d, _| {
                        d.press = Some(crate::dash::Press {
                            id,
                            at,
                            since: web_time::Instant::now(),
                        });
                    });
                },
            );
        }
        dash = dash.child(card);
        if d.nav.mode == Mode::Hints {
            dash = dash.child(hint(label, &d.nav.typed, x + 6.0, y + 4.0));
        }
    }
    if let Some(p) = d.placed(d.nav.expanded).filter(|_| d.nav.expanded != 0) {
        let (w, h) = d.size;
        let (cw, ch) = ((w * 0.6).min(900.0), (h * 0.65).min(700.0));
        let (x, y) = ((w - cw) / 2.0, (h - ch) / 2.0);
        let mut c = Ctx::new(
            d,
            p,
            cw - 2.0 * pad(d),
            ch - 22.0 - 2.0 * pad(d),
            Some(d.nav.cursor),
            true,
            hd,
        );
        let body = widgets::build(&mut c);
        dash = dash
            .child(div().class("expanded-shade").on_click(run::<ClickEvent>(
                hd,
                act(|d: &mut Dash, _| d.nav.expanded = 0),
            )))
            .child(card(d, p, body, &c).class("entered").style(format!(
                "left: {x}px; top: {y}px; width: {cw}px; height: {ch}px; z-index: 31"
            )));
    }
    dash = dash.child(topbar(d, hd, tiles.len()));
    if let Some(bar) = crate::edit::adjust_bar(d, hd) {
        dash = dash.child(bar);
    } else if editing {
        dash = dash.child(crate::edit::sidebar(d, hd));
        if let Some(s) = crate::edit::settings_card(d, hd) {
            dash = dash.child(s);
        }
    }
    if let Some(m) = crate::widgets::menu(d, hd) {
        dash = dash.child(m);
    }
    dash.child(tips(d, hd)).children(toast(d))
}

fn pad(d: &Dash) -> f32 {
    (d.look.style.padding * 0.7).round()
}

/// A widget's Blender panel: the header with its name, and its body.
fn card(d: &Dash, p: &crate::layout::Placed, body: Div, c: &Ctx) -> Div {
    let round = c.round;
    let mut el = div().class(if round { "card round" } else { "card" });
    if p.pinned {
        el = el.class("pinned");
    }
    if !round {
        let status = widgets::status(d, p);
        let mut head = div()
            .class("head")
            .child(span().class("grow ellipsis").child(p.kind.info().title));
        if p.pinned {
            head = head.child(span().child("📌"));
        }
        if !status.is_empty() {
            head = head.child(span().class("ellipsis").child(status));
        }
        el = el.child(head);
    }
    el.child(div().class("body").child(body))
}

fn hint(label: &str, typed: &str, x: f32, y: f32) -> Div {
    let matched = label.starts_with(&typed.to_lowercase());
    let mut el = div()
        .class("hint")
        .style(format!("left: {x}px; top: {y}px"));
    if !matched {
        el = el.style(format!("left: {x}px; top: {y}px; opacity: 0.3"));
    }
    let n = typed.len().min(label.len());
    if matched && n > 0 {
        el = el.child(span().class("typed").child(label[..n].to_string()));
    }
    el.child(label[if matched { n } else { 0 }..].to_string())
}

fn topbar(d: &Dash, hd: WeakEntity<Dash>, shown: usize) -> Div {
    let now = chrono::Local::now();
    let mut bar = div()
        .class("topbar")
        .child(span().class("name").child("niri-dash"))
        .child(
            span().class("dim small").child(
                now.format_localized("%A, %e %B", d.locale.chrono)
                    .to_string(),
            ),
        );
    let hidden = d.layout.widgets.len().saturating_sub(shown);
    if hidden > 0 {
        bar = bar.child(
            span()
                .class("warn small")
                .child(format!("{hidden} hidden: no room on this grid")),
        );
    }
    if d.data.notifications.dnd.active(now) {
        bar = bar.child(span().class("warn small").child("do not disturb"));
    }
    if d.data.inhibited {
        bar = bar.child(span().class("warn small").child("staying awake"));
    }
    bar = bar.child(div().class("grow"));
    if d.config_broken {
        bar = bar.child(span().class("bad small").child("config.kdl doesn't parse"));
    }
    let editing = d.edit.is_some();
    bar.child(
        Button()
            .child(if editing { "Done" } else { "Edit" })
            .on_click(run::<ClickEvent>(
                hd,
                act(|d: &mut Dash, _| {
                    if d.edit.is_some() {
                        d.edit = None;
                    } else {
                        d.edit = Some(crate::edit::Edit::new(0));
                    }
                    d.nav = Default::default();
                }),
            )),
    )
    .child(
        Button()
            .child("✕")
            .on_click(run::<ClickEvent>(hd, act(|d: &mut Dash, _| d.hide()))),
    )
}

/// The quick tips: what the keys do now, the widget's line above the dashboard's.
fn tips(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let (own, general) = d.tips(hd);
    let line = |tips: Vec<Tip>| {
        div().class("line").children(tips.into_iter().map(|t| {
            let mut el = div().class("tip");
            if !t.key.is_empty() {
                el = el.child(span().class("cap").child(t.key));
            }
            el.child(span().child(t.does))
        }))
    };
    let mut out = div().class("tips");
    if !own.is_empty() {
        out = out.child(line(own));
    }
    if d.nav.op == Some(Op::Move) || d.nav.op == Some(Op::Resize) {
        // The mode's line says it all.
    }
    out.child(line(general))
}

fn toast(d: &Dash) -> Option<Div> {
    d.toast
        .as_ref()
        .map(|(t, _)| div().class("toast").child(t.clone()))
}

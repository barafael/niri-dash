//! The mascot widget: Schraubi the screw, Zolli the folding rule and Verschnitti the offcut from
//! osb-verlegeplan, each peeking up from the bottom edge and talking in a speech bubble, or a cow
//! in the manner of `cowsay`. What they say comes from `crate::mascots` (lines sent with
//! `niri-dash say`, a shell command, their own remarks); this is what they look like and do:
//! blink, glance at the bubble, follow the pointer, duck for a new line, and `Next line` on a
//! click or Enter. Port of `MascotWidget` (`legacy/ui/widgets/mascot.slint`) and
//! `legacy/src/app/mascots.rs`.
//!
//! Animation is time computed when drawn (`motion`): a mascot at rest asks for no frames, one
//! blinking or ducking asks for the next one, and with the look's motion at 0 nothing moves.

use super::art::{self, Art, Raster};
use super::motion::{self, Glide, Pop};
use crate::cowfile::{self, Cow};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::Tk;
use crate::layout::Kind;
use crate::mascots::{Board, Character, Outfit, Say, Spec, Todo, View};
use crate::scope::{Scope, Stops};
use crate::widget_settings::{Opt, get_int, get_str};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, MouseMoveEvent, Pixels, Point, Role, SharedString, Styled,
    StyledImage, canvas, div, img, point, px,
};
use niri_dash_system::{Event, Tell, mascot};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

/// The bubble's own colours, the tooltip yellow of the assistant they parody.
const PAPER: u32 = 0xffffe1;
const INK: u32 = 0x1d2733;

/// What one mascot widget remembers between frames. All of it is time, and changes when drawn.
pub struct Fig {
    /// The pointer over the widget, in its own pixels.
    hover: Cell<Option<(f32, f32)>>,
    /// Where the widget is in the window, to make the pointer's position its own.
    origin: Rc<Cell<Point<Pixels>>>,
    glide: Cell<Glide>,
    /// When its line last changed.
    changed: Cell<Option<Instant>>,
}

impl Default for Fig {
    fn default() -> Self {
        Self {
            hover: Cell::new(None),
            origin: Rc::new(Cell::new(point(px(0.0), px(0.0)))),
            glide: Cell::new(Glide::default()),
            changed: Cell::new(None),
        }
    }
}

pub struct State {
    /// The mascot widgets and what each says.
    pub board: Board,
    /// What each shows, by widget id.
    pub lines: HashMap<u32, View>,
    figs: HashMap<u32, Fig>,
    /// The seconds of the animations are counted from here.
    t0: Instant,
    /// The dashboard came up at this time: blinking starts over.
    epoch: Cell<Instant>,
    /// A frame is asked for at this time (so one is asked once).
    wake: Cell<Option<Instant>>,
    cows: RefCell<HashMap<String, Rc<Cow>>>,
    /// Cows that couldn't be loaded, and when that was seen: asked again after a while, so
    /// installing cowsay helps without a restart.
    failed: RefCell<HashMap<String, Instant>>,
    font: OnceCell<String>,
    /// Lays out text to find out how tall a bubble is.
    measure: OnceCell<gpui::WindowTextSystem>,
    raster: Raster,
    pub scope: Scope,
}

impl Default for State {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            board: Board::default(),
            lines: HashMap::new(),
            figs: HashMap::new(),
            t0: now,
            epoch: Cell::new(now),
            wake: Cell::new(None),
            cows: RefCell::default(),
            failed: RefCell::default(),
            font: OnceCell::new(),
            measure: OnceCell::new(),
            raster: Raster::default(),
            scope: Scope::default(),
        }
    }
}

impl State {
    fn secs(&self, now: Instant) -> f32 {
        now.duration_since(self.t0).as_secs_f32()
    }

    /// What `id` says now: nothing until it has been synced.
    fn line(&self, id: u32) -> View {
        self.lines.get(&id).cloned().unwrap_or(View {
            text: String::new(),
            outfit: Outfit::Plain,
            seq: 0,
            queued: 0,
        })
    }

    /// The cow of that name: ours, or a cow file's.
    fn cow(&self, name: &str) -> Rc<Cow> {
        if let Some(cow) = self.cows.borrow().get(name) {
            return cow.clone();
        }
        let now = Instant::now();
        let recently = self
            .failed
            .borrow()
            .get(name)
            .is_some_and(|at| now.duration_since(*at) < Duration::from_secs(5));
        if !recently {
            match cowfile::load(name) {
                Ok(cow) => {
                    let cow = Rc::new(cow);
                    self.cows.borrow_mut().insert(name.to_string(), cow.clone());
                    return cow;
                }
                Err(why) => {
                    log::warn!("cow: {why}");
                    self.failed.borrow_mut().insert(name.to_string(), now);
                }
            }
        }
        Rc::new(Cow::own())
    }

    fn cow_font(&self) -> &str {
        self.font.get_or_init(cowfile::monospace_family)
    }

    /// Ask for a frame in `secs` seconds, unless one is already coming by then.
    fn want_frame(&self, secs: f32, cx: &mut Context<Dash>) {
        let after = Duration::from_secs_f32(secs.clamp(0.016, 60.0));
        let now = Instant::now();
        let due = now + after;
        if let Some(at) = self.wake.get()
            && at > now
            && at <= due
        {
            return;
        }
        self.wake.set(Some(due));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }
}

// ---- the board ---------------------------------------------------------------------------------

/// The mascots the layout has, as the board wants them.
fn specs(d: &Dash) -> Vec<Spec> {
    let us = d.locale.us;
    d.tiles
        .iter()
        .filter(|t| t.kind() == Kind::Mascot)
        .map(|t| {
            let v = &t.placed.settings;
            Spec {
                id: t.id(),
                character: get_str(v, Opt::Character, us)
                    .parse()
                    .expect("the setting only takes character names"),
                wildness: get_int(v, Opt::Wildness, us) as u8,
                interval: Duration::from_secs(get_int(v, Opt::Interval, us) as u64),
                command: get_str(v, Opt::Command, us),
            }
        })
        .collect()
}

/// Run the commands the mascots asked for, then show what changed.
fn todo(d: &mut Dash, todo: Vec<Todo>) {
    for t in todo {
        if let Todo::Run { id, command, name } = t {
            d.tx.mascot.tell(mascot::Command::Run { id, name, command });
        }
    }
    push(d);
}

/// Take what the board shows. A new line makes its mascot duck and come up with it.
fn push(d: &mut Dash) {
    let motion = d.look.style.motion;
    let st = &mut d.widgets.extras.mascot;
    if !st.board.take_dirty() {
        return;
    }
    let now = Instant::now();
    let ids: Vec<u32> = st.board.ids().collect();
    let mut lines = HashMap::new();
    for id in &ids {
        let Some(view) = st.board.view(*id) else {
            continue;
        };
        let fig = st.figs.entry(*id).or_default();
        if motion > 0.0 && st.lines.get(id).is_none_or(|old| old.seq != view.seq) {
            fig.changed.set(Some(now));
        }
        lines.insert(*id, view);
    }
    st.figs.retain(|id, _| ids.contains(id));
    st.lines = lines;
}

/// The tiles changed: the board matches the mascot widgets on the dashboard.
pub fn sync(d: &mut Dash) {
    let specs = specs(d);
    let t = d.widgets.extras.mascot.board.sync(specs, Instant::now());
    todo(d, t);
}

/// The dashboard came up or went away: the mascots pop up with something new (and blink from
/// the start), and only the ticks while it is up keep them talking.
pub fn awake(d: &mut Dash, on: bool) {
    let now = Instant::now();
    d.widgets.extras.mascot.epoch.set(now);
    if on {
        let t = d.widgets.extras.mascot.board.wake(now);
        todo(d, t);
    }
}

/// A second passed while the dashboard is up: lines expire, remarks change.
pub fn tick(d: &mut Dash) {
    let t = d.widgets.extras.mascot.board.tick(Instant::now());
    todo(d, t);
}

/// What a mascot widget's command printed (None: it failed); a command that fails, hangs or
/// prints nothing gets a remark of the mascot's own.
pub fn event(d: &mut Dash, event: &Event) -> bool {
    let Event::Mascot(mascot::Event::Output { id, stdout }) = event else {
        return false;
    };
    let out = stdout
        .as_deref()
        .and_then(crate::mascots::from_command_output);
    d.widgets
        .extras
        .mascot
        .board
        .answer(*id, out, Instant::now());
    push(d);
    true
}

/// `niri-dash say …`: a line for the mascots, or, with none to say it, a toast.
pub fn say(d: &mut Dash, say: Say, cx: &mut Context<Dash>) {
    let result = d.widgets.extras.mascot.board.say(&say, Instant::now());
    match result {
        Ok(_) => push(d),
        Err(e) => {
            log::info!("mascots: {e}; showing it as a toast");
            d.toast(&say.text);
        }
    }
    cx.notify();
}

/// Clicked, or Enter: a sent line goes away; otherwise a new line of its own.
pub fn next(d: &mut Dash, id: u32) {
    let t = d.widgets.extras.mascot.board.next(id, Instant::now());
    todo(d, vec![t]);
}

// ---- keys ---------------------------------------------------------------------------------------

/// Enter or Space: the next line.
struct Keys {
    next: bool,
}

impl Stops for Keys {
    fn activate(&mut self, _cursor: usize, _sub: usize) {
        self.next = true;
    }
}

pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let mut keys = Keys { next: false };
    let mut scope = d.widgets.extras.mascot.scope;
    let taken = scope.press(key, &mut keys);
    d.widgets.extras.mascot.scope = scope;
    if keys.next {
        next(d, id);
    }
    cx.notify();
    taken
}

/// What has to hold: the board shows exactly the mascot widgets there are.
pub fn check(d: &Dash) -> Result<(), String> {
    let st = &d.widgets.extras.mascot;
    let mut want: Vec<u32> = d
        .tiles
        .iter()
        .filter(|t| t.kind() == Kind::Mascot)
        .map(|t| t.id())
        .collect();
    let mut have: Vec<u32> = st.board.ids().collect();
    want.sort_unstable();
    have.sort_unstable();
    if want != have {
        return Err(format!("mascots {have:?} for the mascot widgets {want:?}"));
    }
    for id in &want {
        if !st.lines.contains_key(id) {
            return Err(format!("mascot {id} shows nothing at all"));
        }
        if st.lines[id].text.trim().is_empty() {
            return Err(format!("mascot {id} has an empty line"));
        }
    }
    Ok(())
}

// ---- drawing ------------------------------------------------------------------------------------

/// The text's height in a box `width` wide, as GPUI will lay it out.
#[allow(clippy::too_many_arguments)]
fn text_height(
    st: &State,
    cx: &App,
    tk: &Tk,
    text: &str,
    size: f32,
    line: f32,
    width: f32,
    bold: bool,
) -> f32 {
    let family = if tk.look.style.font_family.is_empty() {
        SharedString::from(".SystemUIFont")
    } else {
        tk.look.style.font_family.clone()
    };
    let mut font = gpui::font(family);
    if bold {
        font.weight = gpui::FontWeight::BOLD;
    }
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    match st
        .measure
        .get_or_init(|| gpui::WindowTextSystem::new(cx.text_system().clone()))
        .shape_text(
            SharedString::from(text.to_string()),
            px(size),
            &[run],
            Some(px(width.max(1.0))),
            None,
        ) {
        Ok(lines) => lines
            .iter()
            .map(|l| f32::from(l.size(px(line)).height))
            .sum(),
        Err(_) => {
            let per_line = (width / (size * 0.52)).floor().max(1.0);
            (text.chars().count() as f32 / per_line).ceil().max(1.0) * line
        }
    }
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let id = ctx.tile.id();
    let st = &ctx.d.widgets.extras.mascot;
    let line = st.line(id);
    let body = if ctx.opts.character == Character::Cow {
        cow(ctx, id, &line, cx)
    } else {
        figure(ctx, id, &line, cx)
    };
    let tracked = st.figs.get(&id).map(|f| f.origin.clone());
    div()
        .id(("mascot", id as usize))
        .role(Role::Button)
        .aria_label("Next line")
        .debug_selector(|| "Next line".into())
        .size_full()
        .relative()
        .overflow_hidden()
        .cursor_pointer()
        .on_click(cx.listener(move |d, _, _, cx| {
            next(d, id);
            cx.notify();
        }))
        // What the pupils follow.
        .on_mouse_move(cx.listener(move |d, ev: &MouseMoveEvent, _, cx| {
            let Some(fig) = d.widgets.extras.mascot.figs.get(&id) else {
                return;
            };
            let at = ev.position - fig.origin.get();
            let at = (f32::from(at.x), f32::from(at.y));
            let moved = fig
                .hover
                .get()
                .is_none_or(|h| (h.0 - at.0).abs() + (h.1 - at.1).abs() >= 3.0);
            if moved {
                fig.hover.set(Some(at));
                cx.notify();
            }
        }))
        .on_hover(cx.listener(move |d, hovered: &bool, _, cx| {
            if !hovered
                && let Some(fig) = d.widgets.extras.mascot.figs.get(&id)
                && fig.hover.take().is_some()
            {
                cx.notify();
            }
        }))
        .children(tracked.map(|o| {
            canvas(
                |_, _, _| (),
                move |b: Bounds<Pixels>, _, _, _| o.set(b.origin),
            )
            .absolute()
            .size_full()
        }))
        .child(body)
        .into_any_element()
}

/// One layer of a figure, filling it, `(dx, dy)` pixels off.
fn layer(
    st: &State,
    art: &Art,
    name: &str,
    fig: (f32, f32),
    shift: (f32, f32),
    cx: &App,
) -> AnyElement {
    img(st
        .raster
        .source(art::path(art, name), fig.1, art.view.1, cx))
    .object_fit(gpui::ObjectFit::Fill)
    .absolute()
    .left(px(shift.0))
    .top(px(shift.1))
    .w(px(fig.0))
    .h(px(fig.1))
    .into_any_element()
}

fn figure(ctx: &Ctx, id: u32, line: &View, cx: &mut Context<Dash>) -> AnyElement {
    let (tk, st) = (ctx.tk, &ctx.d.widgets.extras.mascot);
    let art = art::of(ctx.opts.character);
    let (w, h) = (ctx.w, ctx.h);
    let motion = tk.motion();
    let now = Instant::now();
    let secs = st.secs(now);
    let edge = tk.pad_tight();
    let fig_h = (h * 0.88).min(w * 0.42 / art.aspect());
    let fig_w = fig_h * art.aspect();
    let fig = st.figs.get(&id);
    let mut frame: Option<f32> = None;
    let mut soon = |s: f32| frame = Some(frame.map_or(s, |f| f.min(s)));

    // Blinks, and between blinks glances at its bubble now and then.
    let (shut, glance) = if motion > 0.0 {
        let b = motion::blink(now.duration_since(st.epoch.get()).as_secs_f32(), art.blink);
        soon(b.next_change + 0.005);
        (b.shut, motion::glancing(b.beat))
    } else {
        (false, false)
    };
    // Out from behind the bottom edge. Ducks for a moment on every new line.
    let pop = Pop::figure(motion);
    let (rise, alpha) = match fig.and_then(|f| f.changed.get()) {
        Some(at) => {
            let t = now.duration_since(at).as_secs_f32();
            if t < pop.total() {
                soon(0.016);
            }
            pop.at(t)
        }
        None => (1.0, 1.0),
    };
    let up_y = h - fig_h * 0.94;
    let (fx, fy) = (edge, (h + 2.0) + (up_y - (h + 2.0)) * rise);
    let (eye_x, eye_y) = (fx + art.head.0 * fig_w, fy + art.head.1 * fig_h);
    // The pupils: after the pointer when it is over the widget, else at the bubble now and then.
    let target = match fig.and_then(|f| f.hover.get()) {
        Some((mx, my)) => (
            ((mx - eye_x) / 90.0).clamp(-1.0, 1.0),
            ((my - eye_y) / 90.0).clamp(-1.0, 1.0),
        ),
        None if glance => (0.85, -0.35),
        None => (0.0, 0.0),
    };
    let glide_for = 0.15 * motion;
    let look = match fig {
        Some(f) => {
            let mut g = f.glide.get();
            g.aim(target, secs, glide_for);
            f.glide.set(g);
            if g.moving(secs, glide_for) {
                soon(0.016);
            }
            g.at(secs, glide_for)
        }
        None => target,
    };
    let reach = 2.5 * art.unit() * fig_h;
    let worn = line.outfit;
    let size = (fig_w, fig_h);
    let mut layers = vec![layer(st, &art, "body", size, (0.0, 0.0), cx)];
    for (outfit, name) in [
        (Outfit::Helmet, "helmet"),
        (Outfit::Vest, "vest"),
        (Outfit::Beard, "beard"),
    ] {
        if worn == outfit {
            layers.push(layer(st, &art, name, size, (0.0, 0.0), cx));
        }
    }
    layers.push(layer(st, &art, "brow", size, (0.0, 0.0), cx));
    layers.push(layer(
        st,
        &art,
        if shut { "closed" } else { "eyes" },
        size,
        (0.0, 0.0),
        cx,
    ));
    if !shut {
        layers.push(layer(
            st,
            &art,
            "pupils",
            size,
            (look.0 * reach, look.1 * reach),
            cx,
        ));
    }
    for (outfit, name) in [
        (Outfit::Shades, "shades"),
        (Outfit::Tophat, "tophat"),
        (Outfit::Party, "party"),
    ] {
        if worn == outfit {
            layers.push(layer(st, &art, name, size, (0.0, 0.0), cx));
        }
    }
    layers.push(layer(st, &art, "mouth", size, (0.0, 0.0), cx));

    // The bubble, as tall as its words need, beside the head.
    let bx = fx + fig_w + 14.0;
    let bw = (w - bx - edge).max(0.0);
    let title: SharedString = if line.queued > 0 {
        format!("{}  ·  {} more", art.name, line.queued).into()
    } else {
        art.name.into()
    };
    let text: SharedString = if line.text.is_empty() {
        "…".into()
    } else {
        line.text.clone().into()
    };
    let (title_size, body_size) = ((tk.font_small() * 0.85).max(9.0), tk.font_body());
    // Slint's Text is as high as its font is (about 1.36 em for Noto Sans).
    let (title_line, body_line) = (title_size * 1.36, body_size * 1.36);
    let inner_w = (bw - 22.0).max(1.0);
    let body_h = text_height(st, cx, tk, &text, body_size, body_line, inner_w, false);
    let bh = (17.0 + title_line + 2.0 + body_h).min((h - 2.0 * edge).max(0.0));
    let room = ((bh - 17.0 - title_line - 2.0) / body_line)
        .floor()
        .max(1.0) as usize;
    let by = (eye_y - bh * 0.4).max(edge).min((h - edge - bh).max(edge));
    let ty = (eye_y - 9.0)
        .max(by + 10.0)
        .min((by + bh - 28.0).max(by + 10.0));
    let (paper, ink) = (gpui::rgb(PAPER), gpui::rgb(INK));
    let bubble = (bw > 24.0).then(|| {
        div()
            .absolute()
            .left(px(bx))
            .top(px(by))
            .w(px(bw))
            .h(px(bh))
            .opacity(alpha)
            .child(
                div()
                    .size_full()
                    .overflow_hidden()
                    .rounded(px(12.0))
                    .border_2()
                    .border_color(ink)
                    .bg(paper)
                    .pt(px(6.0))
                    .pb(px(7.0))
                    .px(px(9.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::hsla(0.0, 0.0, 0.0, 0.25),
                        offset: point(px(0.0), px(3.0)),
                        blur_radius: px(10.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }])
                    .child(
                        super::super::devices::parts::elide(div())
                            .flex_none()
                            .text_size(px(title_size))
                            .line_height(px(title_line))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(ink.opacity(0.75))
                            .child(title),
                    )
                    .child(
                        div()
                            .id(("mascot-says", id as usize))
                            .role(Role::Label)
                            .aria_label(text.clone())
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .text_size(px(body_size))
                            .line_height(px(body_line))
                            .text_color(ink)
                            .line_clamp(room)
                            .child(text),
                    ),
            )
    });
    // The tail, pointing at the mascot's head; its fill covers the bubble's border where they meet.
    let tail = (bw > 24.0).then(|| {
        div()
            .absolute()
            .left(px(bx - 12.0))
            .top(px(ty))
            .w(px(14.0))
            .h(px(18.0))
            .opacity(alpha)
            .child(
                canvas(
                    |_, _, _| (),
                    move |b: Bounds<Pixels>, _, window, _| {
                        let at = |x: f32, y: f32| point(b.origin.x + px(x), b.origin.y + px(y));
                        let mut fill = gpui::PathBuilder::fill();
                        fill.add_polygon(&[at(14.0, 0.0), at(0.0, 11.0), at(14.0, 18.0)], true);
                        if let Ok(p) = fill.build() {
                            window.paint_path(p, paper);
                        }
                        let mut line = gpui::PathBuilder::stroke(px(2.0));
                        line.move_to(at(14.0, 0.0));
                        line.line_to(at(0.0, 11.0));
                        line.line_to(at(14.0, 18.0));
                        if let Ok(p) = line.build() {
                            window.paint_path(p, ink);
                        }
                    },
                )
                .size_full(),
            )
    });
    if let Some(s) = frame {
        st.want_frame(s, cx);
    }
    div()
        .size_full()
        .relative()
        .child(
            div()
                .absolute()
                .left(px(fx))
                .top(px(fy))
                .w(px(fig_w))
                .h(px(fig_h))
                .children(layers),
        )
        .children(bubble)
        .children(tail)
        .into_any_element()
}

/// The eyes of the cow follow what it wears, the only part of it that changes.
fn cow_eyes(shut: bool, outfit: Outfit) -> &'static str {
    if shut {
        return "--";
    }
    match outfit {
        Outfit::Helmet => "OO",
        Outfit::Vest => "==",
        Outfit::Beard => "--",
        Outfit::Shades => "$$",
        Outfit::Tophat => "@@",
        Outfit::Party => "**",
        Outfit::Plain => "oo",
    }
}

fn cow(ctx: &Ctx, id: u32, line: &View, cx: &mut Context<Dash>) -> AnyElement {
    let (tk, st) = (ctx.tk, &ctx.d.widgets.extras.mascot);
    let (w, h) = (ctx.w, ctx.h);
    let motion = tk.motion();
    let now = Instant::now();
    let edge = tk.pad_tight().max(w.min(h) * 0.04);
    // The room the cow has: all the card but its margin, and the corner where "+n more" sits.
    let reserve = if line.queued > 0 { 18.0 } else { 0.0 };
    let mut frame: Option<f32> = None;
    let mut soon = |s: f32| frame = Some(frame.map_or(s, |f| f.min(s)));
    let shut = motion > 0.0 && {
        let b = motion::blink(now.duration_since(st.epoch.get()).as_secs_f32(), 5.9);
        soon(b.next_change + 0.005);
        b.shut
    };
    let pop = Pop::cow(motion);
    let (rise, alpha) = match st.figs.get(&id).and_then(|f| f.changed.get()) {
        Some(at) => {
            let t = now.duration_since(at).as_secs_f32();
            if t < pop.total() {
                soon(0.016);
            }
            pop.at(t)
        }
        None => (1.0, 1.0),
    };
    let eyes = cow_eyes(shut, line.outfit);
    let tongue = if line.outfit == Outfit::Party {
        "U "
    } else {
        "  "
    };
    let drawing = st.cow(&ctx.opts.cowfile).draw(eyes, tongue);
    let scene = crate::mascots::cow_scene(
        &drawing,
        &line.text,
        w - 2.0 * edge,
        h - 2.0 * edge - reserve,
    );
    let x = edge.max((w - scene.width) / 2.0);
    let y = edge
        + reserve
        + ((h - 2.0 * edge - reserve - scene.height) / 2.0).max(0.0)
        + 10.0 * (1.0 - rise);
    if let Some(s) = frame {
        st.want_frame(s, cx);
    }
    div()
        .size_full()
        .relative()
        .child(
            div()
                .id(("cow-says", id as usize))
                .role(Role::Label)
                .aria_label(line.text.clone())
                .absolute()
                .left(px(x))
                .top(px(y))
                // Half an em of slack: the size is an estimate, and a letter cut at the edge
                // looks worse than one a little over.
                .w(px(scene.width.min(w - 2.0 * edge) + scene.font / 2.0))
                .h(px(scene.height + scene.font / 2.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .font_family(st.cow_font().to_string())
                .text_size(px(scene.font))
                .line_height(px(scene.font * crate::mascots::COW_LINE_H))
                .text_color(tk.text())
                .opacity(alpha)
                .child(scene.text),
        )
        .when(line.queued > 0, |c| {
            c.child(
                div()
                    .absolute()
                    .top(px(edge))
                    .right(px(edge))
                    .text_size(px(tk.font_small()))
                    .text_color(tk.dim())
                    .child(format!("+{} more", line.queued)),
            )
        })
        .into_any_element()
}

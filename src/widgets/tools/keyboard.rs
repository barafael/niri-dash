//! The keyboard layout widget: the layout in use, and a click, the wheel, h/l or Enter for the next
//! one; with room, the list of layouts to pick from (`KeyboardLayoutWidget` in
//! `legacy/ui/widgets/tools.slint`).

use super::ui;
use super::{Inst, Plain};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, ScrollEvent, ScrollHandle, div, px};
use niri_dash_system::keyboard::{Command, Layout, Layouts, Switch};
use niri_dash_system::{Compositor, Tell};
use std::cell::Cell;

/// How far the wheel has to turn for one step through the layouts, in pixels (touchpads' small
/// steps add up).
const NOTCH: f32 = 60.0;

#[derive(Default)]
pub struct State {
    /// The compositor says which layouts there are.
    pub available: bool,
    pub layouts: Vec<Layout>,
    pub current: usize,
    /// What the wheel has turned since the last step.
    acc: Cell<f32>,
    list: ScrollHandle,
}

impl State {
    /// Room for the list of layouts.
    fn is_list(&self, h: f32, round: bool) -> bool {
        !round && h > 150.0 && self.layouts.len() > 1
    }

    /// How many stops the keyboard has in a widget shaped like `inst`.
    fn count(&self, inst: &Inst) -> usize {
        if self.is_list(inst.h, inst.round) {
            self.layouts.len()
        } else {
            0
        }
    }
}

/// The compositor reports the layouts (`None`: it doesn't say).
pub fn layouts(d: &mut Dash, l: Option<Layouts>) {
    let k = &mut d.widgets.tools.keyboard;
    k.available = l.is_some();
    let l = l.unwrap_or_default();
    k.layouts = l.layouts;
    k.current = l.current;
    // The list may have shrunk under the cursor.
    let ids: Vec<u32> = super::placed_of(d, crate::layout::Kind::KeyboardLayout)
        .iter()
        .map(|p| p.id)
        .collect();
    for id in ids {
        let mut inst = d.widgets.tools.insts.get(id);
        let count = d.widgets.tools.keyboard.count(&inst);
        inst.scope.set_count(count, &Plain);
        d.widgets.tools.insts.set(id, inst);
    }
}

fn switch(d: &Dash, to: Switch) {
    d.tx.keyboard.tell(Command::Switch(to));
}

/// Entered with the keyboard: the cursor starts on the layout in use.
pub fn entered(d: &mut Dash, id: u32) {
    let mut inst = d.widgets.tools.insts.get(id);
    let count = d.widgets.tools.keyboard.count(&inst);
    inst.scope.set_count(count, &Plain);
    inst.scope.move_to(d.widgets.tools.keyboard.current, &Plain);
    d.widgets.tools.insts.set(id, inst);
    d.widgets
        .tools
        .keyboard
        .list
        .scroll_to_item(d.widgets.tools.keyboard.current);
}

struct Keys<'a> {
    d: &'a mut Dash,
    list: bool,
}

impl Stops for Keys<'_> {
    fn activate(&mut self, cursor: usize, _sub: usize) {
        if self.list {
            switch(self.d, Switch::Index(cursor as u32));
        } else {
            switch(self.d, Switch::Next);
        }
    }

    fn adjust(&mut self, _cursor: usize, delta: i32) -> bool {
        switch(
            self.d,
            if delta > 0 {
                Switch::Next
            } else {
                Switch::Prev
            },
        );
        true
    }
}

/// A key for the entered widget: j/k over the layouts and Enter picks one; otherwise h/l (or
/// Enter) step through them.
pub fn key(d: &mut Dash, id: u32, key: &Key) -> bool {
    let mut inst = d.widgets.tools.insts.get(id);
    let list = d.widgets.tools.keyboard.is_list(inst.h, inst.round);
    let count = d.widgets.tools.keyboard.count(&inst);
    let mut scope: Scope = inst.scope;
    scope.set_count(count, &Plain);
    let taken = scope.press(key, &mut Keys { d, list });
    inst.scope = scope;
    d.widgets.tools.insts.set(id, inst);
    if list {
        d.widgets.tools.keyboard.list.scroll_to_item(scope.cursor);
    }
    taken
}

pub fn describe(d: &Dash, id: u32) -> Option<String> {
    let k = &d.widgets.tools.keyboard;
    let inst = d.widgets.tools.insts.get(id);
    if k.count(&inst) == 0 {
        return Some(format!("layout {} of {}", k.current + 1, k.layouts.len()));
    }
    Some(format!(
        "layout {} of {}",
        inst.scope.cursor + 1,
        k.layouts.len()
    ))
}

pub fn check(d: &Dash) -> Result<(), String> {
    let k = &d.widgets.tools.keyboard;
    for p in super::placed_of(d, crate::layout::Kind::KeyboardLayout) {
        let inst = d.widgets.tools.insts.get(p.id);
        super::cursor_ok("keyboard layout", d, p.id, k.count(&inst), 1)?;
    }
    if k.current >= k.layouts.len() && !k.layouts.is_empty() {
        return Err(format!(
            "keyboard layout {} of {}",
            k.current,
            k.layouts.len()
        ));
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let d = ctx.d;
    let tk = ctx.tk;
    let k = &d.widgets.tools.keyboard;
    let id = ctx.tile.id();
    let list = k.is_list(ctx.h, ctx.round);
    let count = if list { k.layouts.len() } else { 0 };
    d.widgets
        .tools
        .insts
        .drawn(id, ctx.w, ctx.h, ctx.round, count, 1);
    let cursor = d.widgets.tools.insts.get(id).scope.cursor;
    if !list {
        return single(ctx, cx);
    }
    let rows: Vec<AnyElement> = k
        .layouts
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let current = i == k.current;
            ui::row(
                ("layout", i),
                l.name.clone(),
                current.then(|| "current".into()),
                current,
                ctx.entered && cursor == i,
                36.0,
                tk,
            )
            .on_click(cx.listener(move |d, _, _, cx| {
                switch(d, Switch::Index(i as u32));
                cx.notify();
            }))
            .px(px(8.0))
            .gap(px(10.0))
            .child(
                div()
                    .flex_none()
                    .w(px(34.0))
                    .h(px(22.0))
                    .rounded(px(6.0_f32.min(tk.radius_small())))
                    .bg(if current {
                        tk.accent()
                    } else {
                        tk.card_raised()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(tk.font_small()))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(if current { tk.on_accent() } else { tk.dim() })
                    .child(l.short.clone()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(px(tk.font_body()))
                    .text_color(tk.text())
                    .truncate()
                    .child(l.name.clone()),
            )
            .into_any_element()
        })
        .collect();
    div()
        .id(("keyboard-layouts", id))
        .role(Role::List)
        .aria_label("Keyboard layout")
        .size_full()
        .flex()
        .flex_col()
        .p(px(tk.pad()))
        .gap(px(6.0))
        .child(ui::header("keyboard", "Keyboard layout", tk))
        .child(ui::scroll_list(
            ("keyboard-layout-list", id),
            &k.list,
            rows,
            tk,
        ))
        .into_any_element()
}

/// Without room for a list: the layout's short name, and its full name under it; a click or the
/// wheel switches.
fn single(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let d = ctx.d;
    let tk = ctx.tk;
    let k = &d.widgets.tools.keyboard;
    let cur = k.layouts.get(k.current);
    let short = match cur {
        Some(c) if k.available => c.short.clone(),
        _ => "–".to_string(),
    };
    let name = match cur {
        Some(c) if k.available => c.name.clone(),
        _ if d.caps.compositor == Compositor::Niri => "niri isn't reachable".to_string(),
        _ if d.caps.compositor == Compositor::Kwin => {
            "Plasma's keyboard layouts aren't reachable".to_string()
        }
        _ => format!("Keyboard layouts {}", d.caps.unavailable()),
    };
    let size = (ctx.h * 0.38).min(ctx.w * 0.3).clamp(18.0, 72.0);
    div()
        .id(("keyboard-layout", ctx.tile.id()))
        .role(Role::Button)
        .aria_label("Next keyboard layout")
        .debug_selector(|| "Next keyboard layout".to_string())
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .cursor_pointer()
        .on_click(cx.listener(|d, _, _, cx| {
            switch(d, Switch::Next);
            cx.notify();
        }))
        .on_scroll(cx.listener(|d, e: &ScrollEvent, _, cx| {
            let delta = e.delta.pixel_delta(px(20.0));
            let k = &d.widgets.tools.keyboard;
            let acc = k.acc.get() + f32::from(delta.y) + f32::from(delta.x);
            if acc <= -NOTCH {
                k.acc.set(0.0);
                switch(d, Switch::Next);
            } else if acc >= NOTCH {
                k.acc.set(0.0);
                switch(d, Switch::Prev);
            } else {
                k.acc.set(acc);
            }
            cx.stop_propagation();
        }))
        .child(
            div()
                .text_size(px(size))
                .font_weight(tk.display_weight())
                .text_color(tk.text())
                .child(short),
        )
        .when(ctx.h > 70.0 && (!ctx.round || ctx.w > 150.0), |col| {
            col.child(
                div()
                    .max_w_full()
                    .text_size(px(tk.font_small()))
                    .text_color(tk.dim())
                    .truncate()
                    .child(name),
            )
        })
        .into_any_element()
}

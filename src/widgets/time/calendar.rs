//! The calendar widget: a month of days with a keyboard cursor over them. h/l a day back and on,
//! j/k a week, n/p (or Page Down/Up) a month; t goes back to today, Ctrl+C copies the day. The
//! month turns as the cursor leaves it. Round, only today is shown and the month can be turned.

use super::parts::{self, Face};
use crate::dash::Dash;
use crate::keys::{Key, Named};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use chrono::{Datelike, Local, Months, NaiveDate};
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Role, div, px};
use niri_dash_system::{Tell, clipboard};
use std::sync::Arc;
use crate::kit::AriaCursor;
use crate::kit::AriaState;

#[derive(Default)]
pub struct State {
    /// The month the calendar shows, from this one.
    pub month_offset: i32,
    /// The day under the keyboard cursor, once it has been moved; until then the cursor is on
    /// today, or on the same day of the month shown.
    pub selected: Option<NaiveDate>,
}

/// `d` moved by `n` months, the day of the month cut down to fit the month (Jan 31 + 1 → Feb 28).
pub fn add_months(d: NaiveDate, n: i32) -> NaiveDate {
    let m = Months::new(n.unsigned_abs());
    if n >= 0 {
        d.checked_add_months(m)
    } else {
        d.checked_sub_months(m)
    }
    .unwrap_or(d)
}

/// How many months `b` lies after `a`.
pub fn months_between(a: NaiveDate, b: NaiveDate) -> i32 {
    (b.year() * 12 + b.month0() as i32) - (a.year() * 12 + a.month0() as i32)
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

impl State {
    /// The dashboard opens: back to the current month.
    pub fn come_up(&mut self) {
        self.month_offset = 0;
        self.selected = None;
    }

    /// Where the keyboard cursor is.
    pub fn cursor(&self) -> NaiveDate {
        self.selected
            .unwrap_or_else(|| add_months(today(), self.month_offset))
    }

    /// Show the month `d` months on, or this one for 0; the cursor goes along.
    pub fn shift(&mut self, d: i32) {
        if d == 0 {
            self.month_offset = 0;
            self.selected = None;
        } else {
            if let Some(s) = self.selected {
                self.selected = Some(add_months(s, d));
            }
            self.month_offset += d;
        }
    }

    /// Move the cursor by `d` days, and the month shown with it.
    pub fn move_day(&mut self, d: i32) {
        let to = self.cursor() + chrono::Duration::days(d.into());
        self.selected = Some(to);
        self.month_offset = months_between(today(), to);
    }

    /// The first day of the month shown.
    pub fn first(&self) -> NaiveDate {
        let t = today();
        let months = t.year() * 12 + t.month0() as i32 + self.month_offset;
        let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
        NaiveDate::from_ymd_opt(y, m, 1).expect("a valid month")
    }
}

/// One day of the grid.
pub struct Day {
    pub date: NaiveDate,
    pub in_month: bool,
    pub today: bool,
    pub selected: bool,
}

/// Six weeks from the Monday (or Sunday) on or before the 1st: the days, the short weekday
/// names and each row's ISO week.
pub struct Grid {
    pub title: String,
    pub weekdays: Vec<String>,
    pub days: Vec<Day>,
    pub weeks: Vec<u32>,
}

pub fn grid(s: &State, sunday_first: bool) -> Grid {
    let t = today();
    let cursor = s.cursor();
    let first = s.first();
    let loc = niri_dash_system::locale().chrono;
    let back = if sunday_first {
        first.weekday().num_days_from_sunday()
    } else {
        first.weekday().num_days_from_monday()
    };
    let start = first - chrono::Days::new(back as u64);
    let days = (0..42)
        .map(|i| {
            let date = start + chrono::Days::new(i);
            Day {
                date,
                in_month: date.month() == first.month(),
                today: date == t,
                selected: date == cursor,
            }
        })
        .collect();
    let weekdays = (0..7)
        .map(|i| {
            let d = start + chrono::Days::new(i);
            d.format_localized("%a", loc)
                .to_string()
                .chars()
                .take(2)
                .collect()
        })
        .collect();
    // The ISO week of each row's Monday (the 2nd column when weeks start on Sunday).
    let weeks = (0..6)
        .map(|r| {
            let monday = start + chrono::Days::new(r * 7 + u64::from(sunday_first));
            monday.iso_week().week()
        })
        .collect();
    Grid {
        title: first.format_localized("%B %Y", loc).to_string(),
        weekdays,
        days,
        weeks,
    }
}

// ---- the keyboard ---------------------------------------------------------------------------

/// The scope's stops: no list, so j/k and h/l and n/p are the calendar's own.
struct Keys<'a> {
    cal: &'a mut State,
    round: bool,
    /// The day Ctrl+C was asked to copy.
    copied: Option<String>,
}

impl Stops for Keys<'_> {
    // Round, there's only the month to turn.
    fn adjust(&mut self, _cursor: usize, delta: i32) -> bool {
        self.cal.shift(delta);
        true
    }

    // Round, there is no cursor to lose.
    fn activate(&mut self, _cursor: usize, _sub: usize) {
        if self.round {
            self.cal.shift(0);
        }
    }

    fn copy(&mut self, _cursor: usize) -> bool {
        if self.round {
            return false;
        }
        self.copied = Some(self.cal.cursor().format("%Y-%m-%d").to_string());
        true
    }

    fn filter(&mut self, key: &Key) -> bool {
        if self.round {
            return false;
        }
        let named = |n| key.is_named(n);
        if key.is('h') || named(Named::Left) || key.is('l') || named(Named::Right) {
            self.cal
                .move_day(if key.is('h') || named(Named::Left) { -1 } else { 1 });
            return true;
        }
        if key.is('j') || named(Named::Down) || key.is('k') || named(Named::Up) {
            self.cal
                .move_day(if key.is('k') || named(Named::Up) { -7 } else { 7 });
            return true;
        }
        if key.is('n') || named(Named::PageDown) || key.is('p') || named(Named::PageUp) {
            self.cal
                .shift(if key.is('p') || named(Named::PageUp) { -1 } else { 1 });
            return true;
        }
        false
    }

    fn letter(&mut self, key: &Key, _cursor: usize) -> bool {
        if key.is('t') {
            self.cal.shift(0);
            return true;
        }
        false
    }
}

/// Put `text` on the clipboard and say so.
pub fn copy_text(d: &mut Dash, text: &str) {
    d.tx.clipboard.tell(clipboard::Command::Set(clipboard::Clip {
        kind: clipboard::ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    }));
    d.toast(&format!("Copied {text}"));
}

pub fn key(d: &mut Dash, id: u32, key: &Key) -> bool {
    let round = super::parts::card(d, id).is_some_and(|c| c.2);
    let mut keys = Keys {
        cal: &mut d.widgets.time.calendar,
        round,
        copied: None,
    };
    let taken = Scope::new().press(key, &mut keys);
    if let Some(text) = keys.copied.take() {
        copy_text(d, &text);
    }
    taken
}

pub fn describe(d: &Dash) -> Option<String> {
    let c = &d.widgets.time.calendar;
    let day = c.cursor();
    Some(format!("{} {}", day.day(), day.format("%B %Y")))
}

pub fn check(d: &Dash) -> Result<(), String> {
    let c = &d.widgets.time.calendar;
    if c.selected.is_some() && months_between(today(), c.cursor()) != c.month_offset {
        return Err(format!(
            "the cursor is on {} but the month shown is {} months from this one",
            c.cursor(),
            c.month_offset
        ));
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let clock = &ctx.d.widgets.time.clock;
    if ctx.round {
        return div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .text_size(px((ctx.w * 0.09).max(11.0)))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(tk.accent())
                    .child(clock.weekday.clone()),
            )
            .child(
                div()
                    .text_size(px(ctx.w * 0.34))
                    .font_weight(tk.display_weight())
                    .text_color(tk.text())
                    .child(clock.day.to_string()),
            )
            .child(
                div()
                    .text_size(px((ctx.w * 0.08).max(11.0)))
                    .text_color(tk.dim())
                    .child(clock.month.clone()),
            )
            .into_any_element();
    }
    let cal = &ctx.d.widgets.time.calendar;
    let g = grid(cal, ctx.opts.sunday_first);
    let showing_current = cal.month_offset == 0;
    let on = ctx.entered;
    let (sat, sun) = if ctx.opts.sunday_first { (6, 0) } else { (5, 6) };
    let pad = tk.pad();
    let (gw, gh) = (ctx.w - 2.0 * pad, ctx.h - 2.0 * pad - 28.0 - 6.0);
    // An extra narrow column on the left for week numbers.
    let wn = if ctx.opts.week_numbers {
        (gw / 8.0).min(28.0)
    } else {
        0.0
    };
    let (cw, ch) = ((gw - wn) / 7.0, gh / 7.0);
    let title = g.title.clone();
    div()
        .size_full()
        .p(px(pad))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .h(px(28.0))
                .child(
                    parts::elided(g.title.clone())
                        .flex_1()
                        .text_size(px(tk.font_title()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(tk.text()),
                )
                .child(parts::icon_button(
                    "previous",
                    "chevron-left",
                    "Previous month",
                    28.0,
                    Face::default(),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| {
                        d.widgets.time.calendar.shift(-1);
                        cx.notify();
                    }),
                ))
                .child(parts::icon_button(
                    "this month",
                    "circle",
                    "This month",
                    28.0,
                    Face::default().enabled(!showing_current).icon_size(10.0),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| {
                        d.widgets.time.calendar.shift(0);
                        cx.notify();
                    }),
                ))
                .child(parts::icon_button(
                    "next",
                    "chevron-right",
                    "Next month",
                    28.0,
                    Face::default(),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| {
                        d.widgets.time.calendar.shift(1);
                        cx.notify();
                    }),
                )),
        )
        .child(
            div()
                .id("calendar grid")
                .role(Role::List)
                .aria_label("Calendar")
                .relative()
                .flex_none()
                .w(px(gw))
                .h(px(gh))
                .children(g.weekdays.iter().enumerate().map(|(i, name)| {
                    div()
                        .absolute()
                        .left(px(wn + i as f32 * cw))
                        .top(px(0.0))
                        .w(px(cw))
                        .h(px(ch))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(tk.font_small()))
                        .text_color(if i == sat || i == sun { tk.faint() } else { tk.dim() })
                        .child(name.clone())
                }))
                .when(ctx.opts.week_numbers, |grid| {
                    grid.child(
                        div()
                            .absolute()
                            .left(px(0.0))
                            .top(px(ch))
                            .w(px(wn - 2.0))
                            .h(px(ch * 6.0))
                            .rounded(px((wn / 2.0).min(tk.radius_small())))
                            .bg(tk.subtle()),
                    )
                    .children(g.weeks.iter().enumerate().map(|(r, w)| {
                        div()
                            .absolute()
                            .left(px(0.0))
                            .top(px((r as f32 + 1.0) * ch))
                            .w(px(wn - 2.0))
                            .h(px(ch))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px((ch * 0.3).clamp(9.0, 13.0)))
                            .text_color(tk.faint())
                            .child(w.to_string())
                    }))
                })
                .children(g.days.iter().enumerate().map(|(i, day)| {
                    let col = i % 7;
                    let weekend = col == sat || col == sun;
                    let diameter = cw.min(ch) - 4.0;
                    let selected = day.selected && on;
                    let color = if day.today {
                        tk.on_accent()
                    } else if day.in_month {
                        if weekend { tk.dim() } else { tk.text() }
                    } else {
                        super::parts::faded(tk.faint(), 0.5)
                    };
                    div()
                        .id(("day", i))
                        .role(Role::ListItem)
                        .aria_label(if day.in_month {
                            format!("{} {title}", day.date.day())
                        } else {
                            day.date.day().to_string()
                        })
                        .aria_state(if day.today { "today" } else { "" })
                        .aria_cursor(selected)
                        .absolute()
                        .left(px(wn + col as f32 * cw))
                        .top(px((i / 7 + 1) as f32 * ch))
                        .w(px(cw))
                        .h(px(ch))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .size(px(diameter.max(0.0)))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(day.today, |c| c.bg(tk.accent()))
                                .when(!day.today && selected, |c| c.bg(tk.hover()))
                                .when(selected, |c| {
                                    c.border_2().border_color(if day.today {
                                        tk.text()
                                    } else {
                                        tk.accent()
                                    })
                                })
                                .text_size(px((ch * 0.42).clamp(11.0, 18.0)))
                                .font_weight(if day.today {
                                    gpui::FontWeight::BOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(color)
                                .child(day.date.day().to_string()),
                        )
                })),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn months_on_and_back() {
        assert_eq!(add_months(day(2026, 1, 31), 1), day(2026, 2, 28));
        assert_eq!(add_months(day(2026, 1, 15), -1), day(2025, 12, 15));
        assert_eq!(months_between(day(2026, 11, 3), day(2027, 1, 1)), 2);
        assert_eq!(months_between(day(2026, 11, 3), day(2026, 10, 31)), -1);
    }

    #[test]
    fn the_cursor_turns_the_month_as_it_leaves_it() {
        let mut c = State::default();
        let t = today();
        c.move_day(1);
        assert_eq!(c.cursor(), t + chrono::Days::new(1));
        c.move_day(40);
        assert_eq!(c.month_offset, months_between(t, c.cursor()));
        assert!(c.month_offset >= 1);
        c.shift(0);
        assert_eq!((c.month_offset, c.selected), (0, None));
    }

    #[test]
    fn a_month_is_six_weeks() {
        let g = grid(&State::default(), false);
        assert_eq!((g.days.len(), g.weeks.len(), g.weekdays.len()), (42, 6, 7));
        assert!(g.days.iter().any(|d| d.today));
        assert_eq!(g.days.iter().filter(|d| d.selected).count(), 1);
        // Weeks start on the day they say.
        assert_eq!(g.days[0].date.weekday(), chrono::Weekday::Mon);
        let g = grid(&State::default(), true);
        assert_eq!(g.days[0].date.weekday(), chrono::Weekday::Sun);
    }
}

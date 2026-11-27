//! Clock, calendar, timer, stopwatch, pomodoro and the sleep timer.

use super::{Ctx, empty, hex, meter, ring};
use crate::dash::Dash;
use crate::timer::{self, Run};
use crate::widget_settings::Opt;
use blenderui::prelude::*;
use chrono::{Datelike, Duration as CDuration, Local, NaiveDate, Timelike, Weekday};
use niri_dash_system::sleep;
use std::time::{Duration, SystemTime};

/// The time as the clock says it: "14:05", "2:05 PM".
fn hm(t: chrono::DateTime<Local>, h12: bool, secs: bool) -> String {
    match (h12, secs) {
        (true, true) => t.format("%-I:%M:%S %p").to_string(),
        (true, false) => t.format("%-I:%M %p").to_string(),
        (false, true) => t.format("%H:%M:%S").to_string(),
        (false, false) => t.format("%H:%M").to_string(),
    }
}

pub fn clock(c: &mut Ctx) -> Div {
    let now = Local::now();
    let h12 = c.opt_int(Opt::Hours) == 12;
    let secs = c.opt_bool(Opt::Seconds);
    let date = c.opt_bool(Opt::Date);
    if c.round {
        return analog(c, now, secs);
    }
    let size = if c.h < 60.0 {
        "display xs"
    } else if c.h < 110.0 || c.w < 200.0 {
        "display s"
    } else {
        "display"
    };
    let mut el = div()
        .class("col center grow")
        .child(div().class(size).child(hm(now, h12, secs)));
    if date {
        el = el.child(
            div().class("dim").child(
                now.format_localized("%A, %e %B %Y", c.d.locale.chrono)
                    .to_string(),
            ),
        );
    }
    let zones = crate::zones::parse(&c.opt_str(Opt::Zones));
    if !zones.is_empty() && c.h > 90.0 {
        let rows = crate::zones::rows(&zones, chrono::Utc::now(), &Local, h12);
        el = el.child(div().class("col wide").style("margin-top: 6px").children(
            rows.into_iter().map(|z| {
                div()
                    .class("row small")
                    .child(div().class("grow ellipsis").child(z.label))
                    .child(div().class("dim").child(z.delta))
                    .child(div().class("mono").child(z.time))
            }),
        ));
    }
    el
}

/// The round clock: a dial with hands, drawn on a canvas.
fn analog(c: &Ctx, now: chrono::DateTime<Local>, secs: bool) -> Div {
    let (face, line, accent, dim) = (
        hex(c.d.look.color("card-raised")),
        hex(c.d.look.color("text")),
        hex(c.d.look.color("accent")),
        hex(c.d.look.color("faint")),
    );
    let (h, m, s) = (now.hour() % 12, now.minute(), now.second());
    canvas(move |b, p| {
        let side = b.size.width.min(b.size.height) - 4.0;
        let (cx, cy) = (
            b.origin.x + b.size.width / 2.0,
            b.origin.y + b.size.height / 2.0,
        );
        let r = side / 2.0;
        p.oval(
            Bounds {
                origin: point(cx - r, cy - r),
                size: size(side, side),
            },
            rgb(face),
            rgb(dim),
            1.0,
        );
        for i in 0..12 {
            let a = i as f32 / 12.0 * std::f32::consts::TAU;
            let (sn, cs) = a.sin_cos();
            let inner = if i % 3 == 0 { 0.78 } else { 0.86 };
            p.line(
                &[
                    (cx + sn * r * inner, cy - cs * r * inner),
                    (cx + sn * r * 0.94, cy - cs * r * 0.94),
                ],
                rgb(dim),
                if i % 3 == 0 { 2.5 } else { 1.0 },
            );
        }
        let hand = |p: &mut Painter, frac: f32, len: f32, w: f32, col: u32| {
            let a = frac * std::f32::consts::TAU;
            let (sn, cs) = a.sin_cos();
            p.line(
                &[
                    (cx - sn * r * 0.1, cy + cs * r * 0.1),
                    (cx + sn * r * len, cy - cs * r * len),
                ],
                rgb(col),
                w,
            );
        };
        hand(
            p,
            (h as f32 + m as f32 / 60.0) / 12.0,
            0.5,
            (side / 30.0).max(2.5),
            line,
        );
        hand(
            p,
            (m as f32 + s as f32 / 60.0) / 60.0,
            0.75,
            (side / 45.0).max(2.0),
            line,
        );
        if secs {
            hand(p, s as f32 / 60.0, 0.82, 1.2, accent);
        }
        let dot = side / 25.0;
        p.oval(
            Bounds {
                origin: point(cx - dot, cy - dot),
                size: size(2.0 * dot, 2.0 * dot),
            },
            rgb(accent),
            rgb(accent),
            0.0,
        );
    })
    .style("width: 100%; height: 100%")
}

/// The calendar's first day of the week.
fn first_day(c: &Ctx) -> Weekday {
    if c.opt_str(Opt::FirstDay) == "sunday" {
        Weekday::Sun
    } else {
        Weekday::Mon
    }
}

fn shift_month(d: NaiveDate, n: i32) -> NaiveDate {
    let m0 = d.year() * 12 + d.month0() as i32 + n;
    let (y, m) = (m0.div_euclid(12), m0.rem_euclid(12) as u32 + 1);
    let last = (28..=31)
        .rev()
        .find(|&day| NaiveDate::from_ymd_opt(y, m, day).is_some())
        .unwrap_or(28);
    NaiveDate::from_ymd_opt(y, m, d.day().min(last)).unwrap_or(d)
}

pub fn calendar(c: &mut Ctx) -> Div {
    let today = Local::now().date_naive();
    let cur = c.d.w.cal.unwrap_or(today);
    let move_by = |days: i64| {
        move |d: &mut Dash, _: &mut Context<Dash>| {
            let at = d.w.cal.unwrap_or(Local::now().date_naive());
            d.w.cal = Some(at + CDuration::days(days));
        }
    };
    let months = |n: i32| {
        move |d: &mut Dash, _: &mut Context<Dash>| {
            let at = d.w.cal.unwrap_or(Local::now().date_naive());
            d.w.cal = Some(shift_month(at, n));
        }
    };
    c.bind("h", "day back", move_by(-1));
    c.bind("l", "day on", move_by(1));
    c.bind("j", "week on", move_by(7));
    c.bind("k", "week back", move_by(-7));
    c.bind("n", "next month", months(1));
    c.bind("p", "previous month", months(-1));
    c.bind("t", "today", |d, _| d.w.cal = None);
    c.bind("y", "copy the date", move |d, _| {
        let at = d.w.cal.unwrap_or(Local::now().date_naive());
        d.tell(niri_dash_system::clipboard::Command::Set(
            niri_dash_system::clipboard::Clip {
                kind: niri_dash_system::clipboard::ClipKind::Text,
                data: std::sync::Arc::new(at.format("%Y-%m-%d").to_string().into_bytes()),
            },
        ));
        d.toast(format!("Copied {}", at.format("%Y-%m-%d")));
    });
    if c.round {
        return div()
            .class("col center")
            .child(
                div()
                    .class("small dim")
                    .child(today.format_localized("%A", c.d.locale.chrono).to_string()),
            )
            .child(div().class("display s").child(today.day().to_string()))
            .child(
                div()
                    .class("small dim")
                    .child(today.format_localized("%B", c.d.locale.chrono).to_string()),
            );
    }
    let first = first_day(c);
    let weeks = c.opt_bool(Opt::WeekNumbers);
    let month_start = cur.with_day(1).unwrap_or(cur);
    let offset =
        (month_start.weekday().num_days_from_monday() + 7 - first.num_days_from_monday()) % 7;
    let start = month_start - CDuration::days(offset as i64);
    let header = div()
        .class("row")
        .child(c.button("‹", "previous month", months(-1)))
        .child(
            div()
                .class("grow center title")
                .child(cur.format_localized("%B %Y", c.d.locale.chrono).to_string()),
        )
        .child(c.button("›", "next month", months(1)));
    let mut grid = div().class(if weeks { "cal weeks" } else { "cal" });
    if weeks {
        grid = grid.child(div().class("d h").child("wk"));
    }
    for i in 0..7 {
        let wd = start + CDuration::days(i);
        grid = grid.child(
            div().class("d h").child(
                wd.format_localized("%a", c.d.locale.chrono)
                    .to_string()
                    .chars()
                    .take(2)
                    .collect::<String>(),
            ),
        );
    }
    for week in 0..6 {
        let row_start = start + CDuration::days(week * 7);
        if week == 5 && row_start.month() != cur.month() {
            break;
        }
        if weeks {
            grid = grid.child(
                div()
                    .class("d wk")
                    .child(row_start.iso_week().week().to_string()),
            );
        }
        for i in 0..7 {
            let day = row_start + CDuration::days(i);
            let mut cls = String::from("d");
            if day.month() != cur.month() {
                cls.push_str(" other");
            }
            if day == today {
                cls.push_str(" today");
            }
            if c.d.w.cal.is_some() && day == cur {
                cls.push_str(" sel");
            }
            grid = grid.child(div().class(cls).child(day.day().to_string()).on_click(
                super::run::<ClickEvent>(
                    c.hd,
                    super::act(move |d: &mut Dash, _| d.w.cal = Some(day)),
                ),
            ));
        }
    }
    let mut el = div().class("col").child(header).child(grid);
    if c.d.w.cal.is_some_and(|d| d != today) {
        el = el.child(c.button("Today", "back to today", |d, _| d.w.cal = None));
    }
    el
}

pub fn timer(c: &mut Ctx) -> Div {
    let now = SystemTime::now();
    let cd = c.d.w.countdown.clone();
    let left = cd.left(now);
    let running = matches!(cd.state, Run::Running { .. });
    let idle = matches!(cd.state, Run::Idle);
    let done = c.d.w.done_until.is_some();
    let text = if done && idle {
        "Done".to_string()
    } else {
        timer::format_left(left)
    };
    let frac = if idle { 1.0 } else { cd.fraction(now) };
    c.bind("space", if running { "pause" } else { "start" }, |d, _| {
        let saved = &mut d.saved;
        d.w.countdown(saved, |c, now| c.toggle(now));
    });
    c.bind("r", "reset", |d, _| {
        let saved = &mut d.saved;
        d.w.countdown(saved, |c, _| c.reset());
    });
    let display = div()
        .class(if c.h < 80.0 { "display s" } else { "display" })
        .child(text);
    if c.round {
        let col = c.d.look.color("accent").to_string();
        let el = ring(
            frac,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            display.class("xs"),
        );
        return c.press(el, "start or pause", |d, _| {
            let saved = &mut d.saved;
            d.w.countdown(saved, |c, now| c.toggle(now));
        });
    }
    let adjust = |m: i64| {
        move |d: &mut Dash, _: &mut Context<Dash>| {
            let saved = &mut d.saved;
            d.w.countdown(saved, |c, now| c.adjust(m, now));
        }
    };
    let mut el = div()
        .class("col center grow")
        .child(display)
        .child(meter(frac, if done { "good" } else { "" }).class("wide"));
    let mut row = div().class("row");
    row = row
        .child(c.button("−5", "5 minutes less", adjust(-5)))
        .child(c.button("−1", "a minute less", adjust(-1)));
    row = row.child(c.button(
        if running { "Pause" } else { "Start" },
        "start or pause",
        |d, _| {
            let saved = &mut d.saved;
            d.w.countdown(saved, |c, now| c.toggle(now));
        },
    ));
    row = row
        .child(c.button("+1", "a minute more", adjust(1)))
        .child(c.button("+5", "5 minutes more", adjust(5)));
    el = el.child(row);
    if !idle {
        el = el.child(c.button("Reset", "reset", |d, _| {
            let saved = &mut d.saved;
            d.w.countdown(saved, |c, _| c.reset());
        }));
    }
    el
}

pub fn stopwatch(c: &mut Ctx) -> Div {
    let now = SystemTime::now();
    let sw = &c.d.w.stopwatch;
    let running = sw.running();
    let text = timer::format(sw.elapsed(now), true);
    let laps: Vec<Duration> = sw.laps.clone();
    c.bind("space", if running { "pause" } else { "start" }, |d, _| {
        d.w.stopwatch.toggle(SystemTime::now())
    });
    c.bind("r", "reset", |d, _| d.w.stopwatch.reset());
    if running {
        c.bind("a", "lap", |d, _| d.w.stopwatch.lap(SystemTime::now()));
    }
    let display = div()
        .class(if c.h < 80.0 || c.round {
            "display s"
        } else {
            "display"
        })
        .child(text);
    if c.round {
        return c.press(
            div().class("col center").child(display),
            "start or pause",
            |d, _| d.w.stopwatch.toggle(SystemTime::now()),
        );
    }
    let mut row = div().class("row");
    row = row.child(c.button(
        if running { "Pause" } else { "Start" },
        "start or pause",
        |d, _| d.w.stopwatch.toggle(SystemTime::now()),
    ));
    if running {
        row = row.child(c.button("Lap", "lap", |d, _| d.w.stopwatch.lap(SystemTime::now())));
    }
    row = row.child(c.button("Reset", "reset", |d, _| d.w.stopwatch.reset()));
    let mut el = div().class("col center grow").child(display).child(row);
    if !laps.is_empty() && c.h > 120.0 {
        let mut list = c.scroll().class("wide");
        for (i, l) in laps.iter().enumerate().rev() {
            let time = timer::format(*l, true);
            let row = div()
                .class("item small")
                .child(div().class("grow dim").child(format!("Lap {}", i + 1)))
                .child(div().class("mono").child(time.clone()));
            list = list.child(c.read_copy(row, time));
        }
        el = el.child(list);
    }
    el
}

pub fn pomodoro(c: &mut Ctx) -> Div {
    let now = SystemTime::now();
    let p = c.d.w.pomodoro.clone();
    let running = p.running();
    let left = p.left(now);
    let frac = p.fraction(now);
    c.bind("space", if running { "pause" } else { "start" }, |d, _| {
        let saved = &mut d.saved;
        d.w.pomo(saved, |p, now| p.toggle(now));
    });
    c.bind("n", "next part", |d, _| {
        let saved = &mut d.saved;
        d.w.pomo(saved, |p, now| p.skip(now));
    });
    c.bind("r", "start over", |d, _| {
        let saved = &mut d.saved;
        d.w.pomo(saved, |p, _| p.reset());
    });
    let tone = if p.phase.is_break() { "good" } else { "" };
    let col =
        c.d.look
            .color(if p.phase.is_break() { "good" } else { "accent" })
            .to_string();
    let dots = div().class("row center").children(
        (0..p.plan.rounds).map(|i| div().class(if i < p.done { "dot on" } else { "dot" })),
    );
    let display = div()
        .class("col center")
        .child(div().class("small dim").child(p.phase.label()))
        .child(div().class("display s").child(timer::format_left(left)));
    if c.round {
        let el = ring(
            frac,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            display,
        );
        return c.press(el, "start or pause", |d, _| {
            let saved = &mut d.saved;
            d.w.pomo(saved, |p, now| p.toggle(now));
        });
    }
    let row = div()
        .class("row")
        .child(c.button(
            if running { "Pause" } else { "Start" },
            "start or pause",
            |d, _| {
                let saved = &mut d.saved;
                d.w.pomo(saved, |p, now| p.toggle(now));
            },
        ))
        .child(c.button("Skip", "next part", |d, _| {
            let saved = &mut d.saved;
            d.w.pomo(saved, |p, now| p.skip(now));
        }))
        .child(c.button("Reset", "start over", |d, _| {
            let saved = &mut d.saved;
            d.w.pomo(saved, |p, _| p.reset());
        }));
    div()
        .class("col center grow")
        .child(display)
        .child(meter(frac, tone).class("wide"))
        .child(dots)
        .child(row)
}

pub fn sleep(c: &mut Ctx) -> Div {
    use sleep::{Act, Status};
    let s = &c.d.w.sleep;
    let status = s.timer.status();
    let counting = s.timer.counting();
    let headline = s.headline(&Local::now());
    let act = |a: Act| {
        move |d: &mut Dash, _: &mut Context<Dash>| {
            let tx = d.tx.clone();
            d.w.sleep_act(a, &tx);
        }
    };
    let modes = s.modes.clone();
    let selected = s.selected;
    let dry = s.dry_run;
    let awake = s.keep_awake;
    let input = s.input.clone();
    let can_arm = s.can_arm();
    let outcome = s.outcome();
    let frac = s.timer.fraction();
    let parsed = s.when();
    c.bind("p", "pause or resume", act(Act::PauseResume));
    c.bind("+", "a minute later", act(Act::Longer));
    c.bind("-", "a minute sooner", act(Act::Shorter));
    c.bind("d", "dry run", act(Act::ToggleDryRun));
    c.bind("a", "keep the screen on", act(Act::ToggleKeepAwake));
    let mut el = div()
        .class("col grow")
        .child(div().class("title ellipsis").child(headline));
    if counting {
        el = el.child(meter(
            frac,
            if status == Status::Paused { "warn" } else { "" },
        ));
        let row = div()
            .class("row")
            .child(c.button(
                if status == Status::Paused {
                    "Resume"
                } else {
                    "Pause"
                },
                "pause or resume",
                act(Act::PauseResume),
            ))
            .child(c.button("−1 min", "sooner", act(Act::Shorter)))
            .child(c.button("+1 min", "later", act(Act::Longer)))
            .child(c.button("Cancel", "give up", act(Act::Abort)));
        el = el.child(row);
    } else {
        if modes.is_empty() {
            el = el.child(empty("Finding out what this machine can do…"));
        }
        let mut list = div().class("col");
        for (i, m) in modes.iter().enumerate() {
            let row = div()
                .class(if i == selected { "item on" } else { "item" })
                .child(div().child(if i == selected { "●" } else { "○" }))
                .child(div().class("grow").child(m.label()));
            list = list.child(c.press(row, "pick", act(Act::Select(i))));
        }
        el = el.child(list);
        let hd = c.hd;
        let field = TextField()
            .value(input)
            .placeholder("30 min, 1h30, 23:15")
            .class("grow")
            .on_input(move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
                let v = e.value.clone();
                hd.update(app, |d, cx| {
                    d.w.sleep.input = v;
                    cx.notify();
                });
            });
        el = el.child(
            div()
                .class("row")
                .child(div().class("dim").child("When"))
                .child(c.field(field)),
        );
        el = el.child(div().class("small dim").child(match parsed {
            Ok(w) => w.echo(),
            Err(e) => format!("{e:?}").to_lowercase(),
        }));
        let arm = c.button("Arm", "arm", act(Act::Arm));
        el = el.child(
            div()
                .class("row")
                .child(if can_arm { arm } else { arm.disabled(true) }),
        );
    }
    let toggles = div()
        .class("row small")
        .child(
            c.press(
                div()
                    .class("row")
                    .child(Checkbox().checked(dry))
                    .child("Dry run"),
                "dry run",
                act(Act::ToggleDryRun),
            ),
        )
        .child(
            c.press(
                div()
                    .class("row")
                    .child(Checkbox().checked(awake))
                    .child("Screen on"),
                "keep the screen on",
                act(Act::ToggleKeepAwake),
            ),
        );
    el = el.child(toggles);
    if let Some(o) = outcome {
        el = el.child(div().class("small dim").child(o));
    }
    el
}

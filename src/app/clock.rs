//! The clock and calendar widgets, and the clock's other time zones.

use super::{App, model, set_if_changed};
use crate::layout::Kind;
use crate::{CalDay, Calendar, Clock, Dash};
use chrono::{Datelike, Local, Timelike};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
pub(super) struct ClockState {
    /// Ticks the clock while the dashboard is up.
    timer: Timer,
    /// The month the calendar shows, from this one.
    month_offset: Cell<i32>,
    /// The day the calendar was last built for.
    last_day: Cell<u32>,
    /// `Clock.zones`, by widget id.
    zones: Rc<VecModel<crate::ZoneList>>,
}

pub(super) fn clock_text(t: chrono::DateTime<chrono::Local>, us: bool) -> String {
    if us {
        t.format("%-I:%M %p").to_string()
    } else {
        t.format("%-H:%M").to_string()
    }
}

impl App {
    /// The dashboard opens: back to the current month and time, ticking every second.
    pub(super) fn start_clock(&self) {
        self.clock.month_offset.set(0);
        self.tick_clock(true);
        self.arm_clock();
    }

    /// Arm the clock's single-shot timer for just after the next full second. It re-arms itself
    /// while the dashboard is up and lapses when hidden: nothing reads the clock then, and `show`
    /// ticks it right away.
    pub(super) fn arm_clock(&self) {
        let next = 1_005 - Local::now().timestamp_subsec_millis().min(999) as u64;
        self.clock.timer.set_interval(Duration::from_millis(next));
        self.clock.timer.restart();
    }

    pub(super) fn tick_clock(&self, force_calendar: bool) {
        let now = Local::now();
        let ui = self.ui();
        let c = ui.global::<Clock>();
        let (loc, us) = self.locale;
        c.set_time(now.format("%H:%M").to_string().into());
        c.set_seconds(now.format("%S").to_string().into());
        c.set_time12(now.format("%-I:%M").to_string().into());
        c.set_ampm(now.format("%p").to_string().into());
        let date_fmt = if us {
            "%A, %B %-d, %Y"
        } else {
            "%A, %-d. %B %Y"
        };
        c.set_date(now.format_localized(date_fmt, loc).to_string().into());
        c.set_weekday(now.format_localized("%a", loc).to_string().into());
        c.set_month(now.format_localized("%B", loc).to_string().into());
        c.set_day(now.day() as i32);
        let (h, m, s) = (now.hour() as f32, now.minute() as f32, now.second() as f32);
        c.set_hour(h % 12.0 + m / 60.0);
        c.set_minute(m + s / 60.0);
        c.set_second(s);
        self.tick_zones();
        if force_calendar || self.clock.last_day.get() != now.day() {
            self.clock.last_day.set(now.day());
            self.build_calendar();
        }
    }

    fn build_calendar(&self) {
        let today = Local::now().date_naive();
        let off = self.clock.month_offset.get();
        let months = today.year() * 12 + today.month0() as i32 + off;
        let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
        let first = chrono::NaiveDate::from_ymd_opt(y, m, 1).expect("valid month");
        let (loc, _) = self.locale;
        // Six weeks from the Monday (or Sunday) on or before the 1st.
        let grid = |sunday: bool| {
            let back = if sunday {
                first.weekday().num_days_from_sunday()
            } else {
                first.weekday().num_days_from_monday()
            };
            let start = first - chrono::Days::new(back as u64);
            let days: Vec<CalDay> = (0..42)
                .map(|i| {
                    let d = start + chrono::Days::new(i);
                    CalDay {
                        day: d.day() as i32,
                        in_month: d.month() == m,
                        today: d == today,
                    }
                })
                .collect();
            let names: Vec<SharedString> = (0..7)
                .map(|i| {
                    let d = start + chrono::Days::new(i);
                    let s = d.format_localized("%a", loc).to_string();
                    s.chars().take(2).collect::<String>().into()
                })
                .collect();
            // ISO week of each row's Monday (the 2nd column when weeks start on Sunday).
            let weeks: Vec<i32> = (0..6)
                .map(|r| {
                    let monday = start + chrono::Days::new(r * 7 + sunday as u64);
                    monday.iso_week().week() as i32
                })
                .collect();
            (days, names, weeks)
        };
        let ui = self.ui();
        let cal = ui.global::<Calendar>();
        cal.set_title(first.format_localized("%B %Y", loc).to_string().into());
        let (days, names, weeks) = grid(false);
        cal.set_weekdays(model(names));
        cal.set_days(model(days));
        cal.set_weeks(model(weeks));
        let (days, names, weeks) = grid(true);
        cal.set_weekdays_sun(model(names));
        cal.set_days_sun(model(days));
        cal.set_weeks_sun(model(weeks));
        cal.set_showing_current(off == 0);
    }

    /// Refresh `Clock.zones` for every clock widget that lists other zones.
    fn tick_zones(&self) {
        use crate::widget_settings::{Opt, get_int, get_str};
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let clocks: Vec<crate::layout::Placed> = match self.solo {
            Some(Kind::Clock) => vec![layout.solo_widget(Kind::Clock)],
            Some(_) => Vec::new(),
            None => layout
                .visible()
                .into_iter()
                .filter(|p| p.kind == Kind::Clock)
                .cloned()
                .collect(),
        };
        drop(layout);
        let now = chrono::Utc::now();
        let mut rows: Vec<crate::ZoneList> = Vec::new();
        for p in clocks {
            let zones = crate::zones::parse(&get_str(&p.settings, Opt::Zones, us));
            if zones.is_empty() {
                continue;
            }
            let hours12 = get_int(&p.settings, Opt::Hours, us) == 12;
            let items: Vec<crate::ZoneTime> =
                crate::zones::rows(&zones, now, &chrono::Local, hours12)
                    .into_iter()
                    .map(|z| crate::ZoneTime {
                        label: z.label.into(),
                        time: z.time.into(),
                        delta: z.delta.into(),
                    })
                    .collect();
            let id = p.id as usize;
            if rows.len() <= id {
                rows.resize(id + 1, crate::ZoneList::default());
            }
            rows[id] = crate::ZoneList {
                items: model(items),
            };
        }
        set_if_changed(&self.clock.zones, rows, |a, b| {
            a.items.row_count() == b.items.row_count()
                && a.items.iter().zip(b.items.iter()).all(|(x, y)| x == y)
        });
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    ui.global::<crate::Clock>()
        .set_zones(ModelRc::from(app.clock.zones.clone()));
    // Calendar.
    let a = app.clone();
    ui.global::<Calendar>().on_shift(move |d| {
        a.clock.month_offset.set(if d == 0 {
            0
        } else {
            a.clock.month_offset.get() + d
        });
        a.build_calendar();
    });
    let a = Rc::downgrade(app);
    app.clock
        .timer
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = a.upgrade()
                && a.ctx.visible()
            {
                a.tick_clock(false);
                a.arm_clock();
            }
        });
}

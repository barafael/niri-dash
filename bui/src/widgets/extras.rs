//! Weather, the mascot, About and the niri-dash info card.

use super::{Ctx, empty, line};
use crate::mascots::{self, Character, Outfit};
use crate::widget_settings::Opt;
use blenderui::prelude::*;
use niri_dash_system::{host, weather};

fn weather_icon(i: weather::Icon) -> &'static str {
    use weather::Icon::*;
    match i {
        Sun => "sun",
        Moon => "moon",
        CloudSun => "cloud-sun",
        CloudMoon => "cloud-moon",
        Cloud => "cloud",
        Fog => "fog",
        Rain => "rain",
        Snow => "snow",
        Storm => "storm",
    }
}

pub fn weather(c: &mut Ctx) -> Div {
    let w = c.d.data.weather.clone();
    if w.temp.is_empty() {
        return if w.error.is_empty() {
            empty("Asking Open-Meteo…")
        } else {
            empty(w.error)
        };
    }
    let tone = match w.icon {
        weather::Icon::Sun => "warn",
        _ => "text",
    };
    if c.round {
        return div()
            .class("col center")
            .child(c.icon_l(weather_icon(w.icon), tone))
            .child(div().class("title").child(w.temp.clone()));
    }
    let mut el = div()
        .class("col grow")
        .child(
            div()
                .class("row")
                .child(c.icon_l(weather_icon(w.icon), tone))
                .child(div().class("display s").child(w.temp.clone()))
                .child(
                    div()
                        .class("col grow")
                        .child(line(w.description.clone()))
                        .child(line(w.feels.clone()).class("small dim")),
                ),
        )
        .child(line(w.place.clone()).class("small dim"));
    if c.h > 100.0 {
        el = el.child(line(w.details.clone()).class("small faint"));
    }
    if c.h > 115.0 && !w.days.is_empty() {
        let n = ((c.w / 60.0) as usize).clamp(1, w.days.len());
        el = el.child(div().class("row spread").style("margin-top: 4px").children(
            w.days.iter().take(n).map(|day| {
                div()
                    .class("col center")
                    .style("gap: 2px")
                    .child(div().class("small dim").child(day.name.clone()))
                    .child(c.icon(weather_icon(day.icon), "text"))
                    .child(div().class("small").child(day.max.clone()))
                    .child(div().class("small faint").child(day.min.clone()))
            }),
        ));
    }
    if !w.error.is_empty() {
        el = el.child(div().class("small bad").child(w.error.clone()));
    }
    el
}

fn outfit_layer(o: Outfit) -> Option<&'static str> {
    match o {
        Outfit::Plain => None,
        Outfit::Helmet => Some("helmet"),
        Outfit::Vest => Some("vest"),
        Outfit::Beard => Some("beard"),
        Outfit::Shades => Some("shades"),
        Outfit::Tophat => Some("tophat"),
        Outfit::Party => Some("party"),
    }
}

/// The cow's eyes and tongue for an outfit, as cowsay's options would have them.
fn cow_face(o: Outfit) -> (&'static str, &'static str) {
    let eyes = match o {
        Outfit::Helmet => "OO",
        Outfit::Vest => "==",
        Outfit::Beard => "--",
        Outfit::Shades => "$$",
        Outfit::Tophat => "@@",
        Outfit::Party => "**",
        Outfit::Plain => "oo",
    };
    (eyes, if o == Outfit::Party { "U " } else { "  " })
}

pub fn mascot(c: &mut Ctx) -> Div {
    let id = c.p.id;
    let who = super::state::character(&c.p.settings);
    let view = c.d.w.board.view(id);
    let (text, outfit, queued) = view
        .map(|v| (v.text, v.outfit, v.queued))
        .unwrap_or_default();
    let next = move |d: &mut crate::dash::Dash, _: &mut Context<crate::dash::Dash>| {
        let todo = d.w.board.next(id, web_time::Instant::now());
        if let mascots::Todo::Run { id, command, name } = todo {
            d.tell(niri_dash_system::mascot::Command::Run { id, name, command });
        }
    };
    c.bind("n", "something else", next);
    if who == Character::Cow {
        let name = c.opt_str(Opt::Cowfile);
        let cow = crate::cowfile::load(if name.is_empty() { "default" } else { &name })
            .unwrap_or_else(|_| crate::cowfile::Cow::own());
        let (eyes, tongue) = cow_face(outfit);
        let scene = mascots::cow_scene(&cow.draw(eyes, tongue), &text, c.w, c.h);
        let el = div()
            .class("grow center")
            .style("display: flex; align-items: center; justify-content: center; overflow: hidden")
            .child(
                div()
                    .class("mono")
                    .style(format!(
                        "white-space: pre; font-size: {:.1}px; line-height: {}",
                        scene.font,
                        mascots::COW_LINE_H
                    ))
                    .child(scene.text),
            );
        return c.press(el, "something else", next);
    }
    let base = who.name();
    let mut figure = div().style(
        "position: relative; height: 100%; aspect-ratio: 0.6; flex-shrink: 0; min-width: 40px",
    );
    let mut layers = vec!["body", "eyes", "pupils", "brow", "mouth"];
    if let Some(o) = outfit_layer(outfit) {
        layers.push(o);
    }
    for l in layers {
        if let Some(pic) = crate::mascot_art::layer(&format!("{base}-{l}")) {
            figure = figure.child(img().style("position: absolute; left: 0; top: 0; width: 100%; height: 100%; object-fit: contain").image(pic));
        }
    }
    let mut bubble = div()
        .class("col grow")
        .style("background: var(--card-raised); border: 1px solid var(--border); border-radius: 8px; padding: 6px 10px; align-self: center; max-height: 100%; overflow: hidden")
        .child(div().child(text));
    if queued > 0 {
        bubble = bubble.child(div().class("small faint").child(format!("{queued} more")));
    }
    if c.round {
        return c.press(
            div()
                .class("center")
                .style("width: 70%; height: 70%; display: flex")
                .child(figure),
            "something else",
            next,
        );
    }
    let el = div()
        .class("row grow")
        .style("align-items: stretch")
        .child(figure)
        .child(bubble);
    c.press(el, "something else", next)
}

/// A line of facts, with a link to open.
fn fact(c: &mut Ctx, label: &str, value: String, url: Option<String>) -> Div {
    let row = div()
        .class("item small")
        .child(
            div()
                .class("dim")
                .style("width: 35%")
                .child(label.to_string()),
        )
        .child(line(value.clone()).class("grow"));
    match url {
        Some(u) => c.press(row.class("accent"), "open it", move |d, _| {
            d.tell(host::Command::OpenUrl(u.clone()));
            d.hide();
        }),
        None => c.read(row),
    }
}

pub fn about(c: &mut Ctx) -> Div {
    if c.round {
        return div()
            .class("col center")
            .child(div().class("title").child("niri-dash"))
            .child(div().class("small dim").child(env!("CARGO_PKG_VERSION")));
    }
    let mut el = div()
        .class("col grow")
        .child(
            div()
                .class("row")
                .child(div().class("display xs").child("niri-dash"))
                .child(div().class("dim").child(env!("CARGO_PKG_VERSION"))),
        )
        .child(div().class("small dim").child(
            "A widget dashboard for niri, drawn with BUI: Blender's UI toolkit, from Rust.",
        ));
    el = el.child(fact(
        c,
        "Made with",
        "BUI (blenderui 0.6)".into(),
        Some("https://bui.preetham.top".into()),
    ));
    el = el.child(fact(c, "License", "GPL-3.0-or-later (BUI's)".into(), None));
    el.child(fact(
        c,
        "Source",
        "github.com/barafael/niri-dash".into(),
        Some("https://github.com/barafael/niri-dash".into()),
    ))
}

pub fn info(c: &mut Ctx) -> Div {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let mut el = div().class("col scroll");
    el = el.child(fact(
        c,
        "Version",
        format!("niri-dash-bui {}", env!("CARGO_PKG_VERSION")),
        None,
    ));
    el = el.child(fact(
        c,
        "Toolkit",
        "BUI (blenderui 0.6), software raster".into(),
        Some("https://bui.preetham.top/docs/rust/overview/".into()),
    ));
    el = el.child(fact(c, "Profile", profile.into(), None));
    el = el.child(fact(c, "Program", exe, None));
    el = el.child(fact(c, "Source", env!("CARGO_MANIFEST_DIR").into(), None));
    el = el.child(fact(c, "Desktop", c.d.caps.compositor.to_string(), None));
    el.child(fact(
        c,
        "Config",
        crate::layout::Layout::path().display().to_string(),
        None,
    ))
}

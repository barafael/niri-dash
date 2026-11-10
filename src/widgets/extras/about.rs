//! The about card: what this is, which version, where it lives. Port of `AboutWidget`
//! (`slint/ui/widgets/about.slint`), without Slint's own `AboutSlint` (which its license asked
//! for next to a Slint app's version): the toolkit now is GPUI, and it is named in a line.

use super::super::devices::parts::{elide, muted};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::AriaCursor;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, div, px};

/// Where the project lives.
pub const REPOSITORY: &str = "github.com/barafael/niri-dash";

/// What the card's one stop, the link, does.
struct Keys {
    open: bool,
}

impl Stops for Keys {
    fn activate(&mut self, _cursor: usize, _sub: usize) {
        self.open = true;
    }
}

/// A key for the entered widget: Enter (or Space) opens the repository's page.
pub fn key(_d: &mut Dash, key: &Key, cx: &mut Context<Dash>) -> bool {
    let mut keys = Keys { open: false };
    let mut scope = Scope {
        count: 1,
        ..Scope::default()
    };
    let taken = scope.press(key, &mut keys);
    if keys.open {
        cx.open_url(&format!("https://{REPOSITORY}"));
    }
    taken
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    div()
        .id("about")
        .role(Role::Group)
        .aria_label("About niri-dash")
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            elide(div())
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(format!("niri-dash {}", env!("CARGO_PKG_VERSION"))),
        )
        .child(muted("MIT or Apache-2.0", tk))
        .child(
            elide(div())
                .id("about-repository")
                .role(Role::Link)
                .aria_label(format!("Open {REPOSITORY}"))
                .aria_cursor(ctx.entered)
                .debug_selector(|| "Open repository".into())
                .rounded(px(tk.radius_small()))
                .border_2()
                .border_color(if ctx.entered {
                    tk.accent()
                } else {
                    gpui::transparent_black()
                })
                .cursor_pointer()
                .text_size(px(tk.font_small()))
                .text_color(tk.accent())
                .on_click(cx.listener(|_, _, _, cx| cx.open_url(&format!("https://{REPOSITORY}"))))
                .child(REPOSITORY),
        )
        .child(muted("Made with GPUI", tk))
        .into_any_element()
}

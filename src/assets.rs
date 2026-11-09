//! What the dashboard loads by name: its icons (`icons/<name>.svg`) and the mascots' drawings
//! (`mascots/<name>.svg`), embedded in the binary.

use gpui::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;
use std::borrow::Cow;

#[derive(RustEmbed)]
#[folder = "assets/"]
#[include = "icons/*"]
#[include = "mascots/*"]
struct Files;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Files::get(path).map(|f| f.data))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Files::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect())
    }
}

/// The path of the icon called `name`.
pub fn icon_path(name: &str) -> SharedString {
    SharedString::from(format!("icons/{name}.svg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icons_are_embedded() {
        assert!(Assets.load("icons/wifi.svg").unwrap().is_some());
        assert!(Assets.load("icons/nonsense.svg").unwrap().is_none());
        assert!(Assets.list("mascots/").unwrap().len() > 10);
    }
}

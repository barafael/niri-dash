//! System tray: a StatusNotifierWatcher/host via the `system-tray` crate, with dbusmenu menus.

use super::icons::Icons;
use super::{Ctx, Dedup, IconSrc};
pub use crate::tray::{Entry, Item, Menu, Toggle};
use std::sync::Arc;
use system_tray::client::{ActivateRequest, Client};
use system_tray::item::{IconPixmap, StatusNotifierItem};
use system_tray::menu::{MenuItem, MenuType, ToggleState, ToggleType};
use tokio::sync::mpsc;

enum Cmd {
    Activate { address: String, x: i32, y: i32 },
    OpenMenu { address: String, x: f32, y: f32 },
    MenuItem { address: String, id: i32 },
}

#[derive(Clone)]
pub struct Tray {
    tx: mpsc::UnboundedSender<Cmd>,
}

/// SNI pixmaps are ARGB32 in network byte order; pick the one closest to 48px.
fn pixmap(p: &[IconPixmap]) -> Option<IconSrc> {
    let best = p
        .iter()
        .filter(|p| p.width > 0 && p.height > 0)
        .min_by_key(|p| (p.width - 48).abs())?;
    let mut rgba = Vec::with_capacity(best.pixels.len());
    for px in best.pixels.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[px[1], px[2], px[3], px[0]]);
    }
    Some(IconSrc::Rgba {
        width: best.width as u32,
        height: best.height as u32,
        data: Arc::new(rgba),
    })
}

fn item_icon(i: &StatusNotifierItem, icons: &mut Icons) -> IconSrc {
    let by_name = i
        .icon_name
        .as_deref()
        .filter(|n| !n.is_empty())
        .and_then(|n| icons.lookup_in(i.icon_theme_path.as_deref(), n, 48));
    by_name
        .map(IconSrc::Path)
        .or_else(|| i.icon_pixmap.as_deref().and_then(pixmap))
        .unwrap_or(IconSrc::None)
}

/// Remove dbusmenu mnemonics: "__" is a literal underscore, a single "_" marks the access key.
pub fn strip_mnemonic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn flatten(items: &[MenuItem], depth: i32, out: &mut Vec<Entry>) {
    for m in items.iter().filter(|m| m.visible) {
        if m.menu_type == MenuType::Separator {
            if out.last().is_some_and(|e| !e.separator) {
                out.push(Entry {
                    id: m.id,
                    label: String::new(),
                    depth,
                    separator: true,
                    enabled: false,
                    toggle: Toggle::None,
                    checked: false,
                    header: false,
                });
            }
            continue;
        }
        let has_children = !m.submenu.is_empty();
        out.push(Entry {
            id: m.id,
            label: strip_mnemonic(m.label.as_deref().unwrap_or("")),
            depth,
            separator: false,
            enabled: m.enabled,
            toggle: match m.toggle_type {
                ToggleType::Checkmark => Toggle::Check,
                ToggleType::Radio => Toggle::Radio,
                _ => Toggle::None,
            },
            checked: m.toggle_state == ToggleState::On,
            header: has_children,
        });
        if has_children {
            flatten(&m.submenu, depth + 1, out);
        }
    }
    while out.last().is_some_and(|e| e.separator) {
        out.pop();
    }
}

fn snapshot(client: &Client, icons: &mut Icons) -> Vec<Item> {
    let map = client.items();
    let Ok(map) = map.lock() else { return vec![] };
    let mut items: Vec<Item> = map
        .iter()
        .map(|(address, (item, menu))| Item {
            address: address.clone(),
            title: item
                .title
                .clone()
                .or_else(|| item.tool_tip.as_ref().map(|t| t.title.clone()))
                .unwrap_or_else(|| item.id.clone()),
            icon: item_icon(item, icons),
            has_menu: item.menu.is_some() || menu.is_some(),
            item_is_menu: item.item_is_menu,
        })
        .collect();
    items.sort_by_key(|a| a.title.to_lowercase());
    items
}

impl Tray {
    pub fn start(ctx: Ctx) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        let c = ctx.clone();
        ctx.spawn(async move {
            let client = match Client::new().await {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    log::warn!("tray: {e}");
                    return;
                }
            };
            let mut events = client.subscribe();
            let cl = client.clone();
            let c2 = c.clone();
            tokio::spawn(async move {
                let mut sent = Dedup::default();
                let mut icons = Icons::default();
                let mut publish = || {
                    let items = snapshot(&cl, &mut icons);
                    if sent.changed(&items) {
                        c2.send(crate::tray::Event::Items(items));
                    }
                };
                publish();
                loop {
                    match events.recv().await {
                        Ok(_) => {
                            // Coalesce bursts (menus and icons arrive as many small updates).
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            while events.try_recv().is_ok() {}
                            publish();
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });
            while let Some(cmd) = rx.recv().await {
                let menu_path = |address: &str| {
                    client
                        .items()
                        .lock()
                        .ok()
                        .and_then(|m| m.get(address).and_then(|(i, _)| i.menu.clone()))
                };
                match cmd {
                    Cmd::Activate { address, x, y } => {
                        if let Err(e) = client
                            .activate(ActivateRequest::Default { address, x, y })
                            .await
                        {
                            log::warn!("tray activate: {e}");
                        }
                    }
                    Cmd::OpenMenu { address, x, y } => {
                        let Some(path) = menu_path(&address) else {
                            continue;
                        };
                        let _ = client
                            .about_to_show_menuitem(address.clone(), path, 0)
                            .await;
                        // Give the item a moment to send an updated layout.
                        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                        let (title, entries) = {
                            let map = client.items();
                            let Ok(map) = map.lock() else { continue };
                            let Some((item, menu)) = map.get(&address) else {
                                continue;
                            };
                            let mut entries = Vec::new();
                            if let Some(menu) = menu {
                                flatten(&menu.submenus, 0, &mut entries);
                            }
                            (item.title.clone().unwrap_or_default(), entries)
                        };
                        c.send(crate::tray::Event::Menu(Menu {
                            address,
                            title,
                            entries,
                            x,
                            y,
                        }));
                    }
                    Cmd::MenuItem { address, id } => {
                        let Some(menu_path) = menu_path(&address) else {
                            continue;
                        };
                        if let Err(e) = client
                            .activate(ActivateRequest::MenuItem {
                                address,
                                menu_path,
                                submenu_id: id,
                            })
                            .await
                        {
                            log::warn!("tray menu: {e}");
                        }
                    }
                }
            }
        });
        Self { tx }
    }

    pub fn activate(&self, address: String, x: i32, y: i32) {
        let _ = self.tx.send(Cmd::Activate { address, x, y });
    }

    pub fn open_menu(&self, address: String, x: f32, y: f32) {
        let _ = self.tx.send(Cmd::OpenMenu { address, x, y });
    }

    pub fn menu_item(&self, address: String, id: i32) {
        let _ = self.tx.send(Cmd::MenuItem { address, id });
    }
}

#[cfg(test)]
mod tests {
    use super::strip_mnemonic;

    #[test]
    fn mnemonics() {
        assert_eq!(strip_mnemonic("_Quit"), "Quit");
        assert_eq!(strip_mnemonic("snake__case"), "snake_case");
    }
}

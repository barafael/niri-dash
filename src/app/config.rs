//! config.kdl: saving edits, and picking up changes made to it (or the theme files) by hand.

use super::App;
use crate::layout::Layout;
use crate::sources::{Ctx, Msg};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

impl App {
    pub(super) fn save_layout(&self) {
        if self.config_broken.get() {
            self.toast("config.kdl has errors, so edits aren't saved. Fix the file first.");
            return;
        }
        if let Some(text) = self.layout.borrow_mut().save() {
            *self.last_written.borrow_mut() = text;
        }
    }

    pub(super) fn apply_settings(&self) {
        let (weather, cell) = {
            let l = self.layout.borrow();
            (l.weather_config(self.locale.1), l.cell_size)
        };
        self.apply_look(self.ctx.visible());
        self.weather.configure(weather);
        self.ui().set_cell_size(cell);
    }

    pub(super) fn reload_config(&self) {
        let path = Layout::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if text == *self.last_written.borrow() {
            // Our own write; only a color scheme or style file may have changed.
            self.apply_look(self.ctx.visible());
            return;
        }
        match Layout::parse(&text) {
            Ok((fresh, warnings)) => {
                self.config_broken.set(false);
                self.layout.borrow_mut().replace(fresh);
                *self.last_written.borrow_mut() = text;
                self.apply_settings();
                self.sync_widgets();
                self.refresh_settings();
                if !warnings.is_empty() {
                    self.toast(&format!("config.kdl: {}", warnings.join("; ")));
                }
            }
            Err(e) => {
                self.config_broken.set(true);
                self.toast(&format!("config.kdl: {e}"));
            }
        }
    }
}

/// Watch config.kdl and the theme files: inotify on the directories holding them (after following
/// symlinks, so a symlinked dotfile counts as its target). The list of files changes with the
/// theme, which goes through config.kdl, so the watches are re-synced after every event.
pub(super) fn watch_config(paths: Arc<Mutex<Vec<PathBuf>>>, ctx: Ctx) {
    use notify::{RecursiveMode, Watcher};
    use std::collections::HashSet;
    fn resolve(p: &std::path::Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap_or_else(|_| {
            match (
                p.parent().and_then(|d| std::fs::canonicalize(d).ok()),
                p.file_name(),
            ) {
                (Some(d), Some(n)) => d.join(n),
                _ => p.to_path_buf(),
            }
        })
    }
    std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
            let mut watcher = match notify::recommended_watcher(tx) {
                Ok(w) => w,
                Err(e) => {
                    log::warn!("not watching the config for changes: {e}");
                    return;
                }
            };
            let mut dirs: HashSet<PathBuf> = HashSet::new();
            loop {
                let list = paths.lock().map(|p| p.clone()).unwrap_or_default();
                let targets: Vec<PathBuf> = list.iter().map(|p| resolve(p)).collect();
                let want: HashSet<PathBuf> = targets
                    .iter()
                    .filter_map(|t| t.parent().map(|d| d.to_path_buf()))
                    .collect();
                for d in dirs.difference(&want) {
                    let _ = watcher.unwatch(d);
                }
                for d in want.difference(&dirs) {
                    if let Err(e) = watcher.watch(d, RecursiveMode::NonRecursive) {
                        log::debug!("watching {}: {e}", d.display());
                    }
                }
                dirs = want;
                let hits = |ev: notify::Result<notify::Event>| {
                    ev.is_ok_and(|e| {
                        !e.kind.is_access() && e.paths.iter().any(|p| targets.contains(p))
                    })
                };
                let Ok(ev) = rx.recv() else { return };
                let mut changed = hits(ev);
                // Editors save in several steps; report the burst once.
                std::thread::sleep(Duration::from_millis(50));
                while let Ok(ev) = rx.try_recv() {
                    changed |= hits(ev);
                }
                if changed {
                    ctx.send(Msg::ConfigChanged);
                }
            }
        })
        .expect("spawning config watcher");
}

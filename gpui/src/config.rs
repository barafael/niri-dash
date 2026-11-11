//! config.kdl: saving what edit mode changed, laying out a changed file, and watching the files.

use crate::dash::Dash;
use crate::layout::Layout;
use crate::window::Input;
use gpui::Context;
use niri_dash_system::{Sink, Tell, weather};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

impl Dash {
    pub fn save_layout(&mut self) {
        if self.config_broken {
            self.toast("config.kdl has errors, so edits aren't saved. Fix the file first.");
            return;
        }
        if let Some(text) = self.layout.save() {
            self.last_written = text;
        }
    }

    /// What the config says that the system has to be told, and the look.
    pub fn apply_settings(&mut self, animate: bool) {
        let weather = self.layout.weather_config(self.locale.us);
        self.apply_look(animate);
        self.tx.weather.tell(weather::Command::Configure(weather));
        self.set_size_forced();
    }

    /// config.kdl or the theme changed on disk: lay it out again.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let path = Layout::path();
        let Ok(text) = crate::files::read(&path) else {
            return;
        };
        if text == self.last_written {
            // Our own write; only a color scheme or style file may have changed.
            self.apply_look(self.visible);
            self.refresh_looks();
            cx.notify();
            return;
        }
        match Layout::parse(&text) {
            Ok((fresh, warnings)) => {
                self.config_broken = false;
                let refit = self.layout.replace(fresh);
                self.last_written = text;
                self.apply_settings(self.visible);
                self.sync_widgets();
                self.refresh_looks();
                self.tidy_edit();
                if refit.changed() {
                    self.save_layout();
                }
                let mut messages = Vec::new();
                if !warnings.is_empty() {
                    messages.push(format!("config.kdl: {}", warnings.join("; ")));
                }
                messages.extend(refit.message());
                if !messages.is_empty() {
                    self.toast(&messages.join("  ·  "));
                }
            }
            Err(e) => {
                self.config_broken = true;
                self.toast(&format!("config.kdl: {e}"));
            }
        }
        cx.notify();
    }
}

/// Watch config.kdl and the theme files: inotify on the directories holding them (after following
/// symlinks, so a symlinked dotfile counts as its target). The list of files changes with the
/// theme, which goes through config.kdl, so the watches are re-synced after every event.
pub fn watch_config(paths: Receiver<Vec<PathBuf>>, input: Sink<Input>) {
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
            let mut list: Vec<PathBuf> = Vec::new();
            loop {
                // The newest list counts.
                while let Ok(newer) = paths.try_recv() {
                    list = newer;
                }
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
                    let _ = input.send(Input::ConfigChanged);
                }
            }
        })
        .expect("spawning config watcher");
}

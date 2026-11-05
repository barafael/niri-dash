//! Running Claude Code sessions, from the files Claude Code keeps in `~/.claude/sessions/<pid>.json`,
//! matched to their window by walking up the process tree.

use super::{Ctx, Dedup, Trigger, strip_spinner};
pub use crate::claude::{Session, State, Status};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    pid: i32,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    status_updated_at: Option<u64>,
    #[serde(default)]
    started_at: Option<u64>,
    #[serde(default)]
    proc_start: Option<String>,
    #[serde(default)]
    entrypoint: String,
}

fn proc_stat(pid: i32) -> Option<(i32, String)> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name can contain spaces and parens; fields restart after the last ')'.
    let rest = &stat[stat.rfind(')')? + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After ')': state(0) ppid(1) ... starttime is field 22 overall, index 19 here.
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.to_string()))
}

fn ago(ms: u64, now_ms: u64) -> String {
    let s = now_ms.saturating_sub(ms) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min", s / 60),
        3600..=86_399 => format!("{} h {} min", s / 3600, s % 3600 / 60),
        _ => format!("{} d", s / 86_400),
    }
}

fn tilde(path: &str) -> String {
    match dirs::home_dir().and_then(|h| path.strip_prefix(h.to_str()?).map(str::to_string)) {
        Some(rest) => format!("~{rest}"),
        None => path.to_string(),
    }
}

/// Window titles from Claude Code start with a status glyph (✳ idle, a spinner while busy).
fn strip_glyph(title: &str) -> String {
    strip_spinner(title.trim()).to_string()
}

/// Where Claude Code keeps one file per running session.
fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".claude")
        .join("sessions")
}

/// Whether there can be sessions at all, before asking the compositor about windows for them.
fn any_session_files() -> bool {
    std::fs::read_dir(sessions_dir()).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "json"))
    })
}

/// The sessions, given each window by the pid of its process: (window id, title).
fn fetch(windows: &HashMap<i32, (u64, String)>) -> State {
    let Ok(rd) = std::fs::read_dir(sessions_dir()) else {
        return State::default();
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut sessions = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(f) = serde_json::from_str::<SessionFile>(&text) else {
            continue;
        };
        // Skip stale files: the process is gone, or the pid now belongs to something else.
        let Some((mut ppid, start)) = proc_stat(f.pid) else {
            continue;
        };
        if f.proc_start.as_deref().is_some_and(|s| s != start) {
            continue;
        }
        let mut window = None;
        for _ in 0..12 {
            if let Some(w) = windows.get(&ppid) {
                window = Some(w.clone());
                break;
            }
            match proc_stat(ppid) {
                Some((pp, _)) if pp > 1 => ppid = pp,
                _ => break,
            }
        }
        let status: Status = f.status.parse().unwrap_or_default();
        let since = f
            .status_updated_at
            .or(f.started_at)
            .map(|t| ago(t, now_ms))
            .unwrap_or_default();
        let status_text = match &status {
            Status::Busy => format!("working · {since}"),
            Status::Idle => format!("waiting for you · {since}"),
            Status::Other(o) if o.is_empty() => since,
            Status::Other(o) => format!("{} · {since}", o.replace('_', " ")),
        };
        // Terminal titles carry the session's topic; editor windows (e.g. Zed's agent) don't.
        let from_title = window
            .as_ref()
            .filter(|(_, t)| {
                t.trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_alphanumeric())
            })
            .map(|(_, t)| strip_glyph(t))
            .filter(|t| !t.is_empty());
        let title = from_title.unwrap_or_else(|| {
            if f.name.is_empty() {
                tilde(&f.cwd)
            } else {
                f.name.clone()
            }
        });
        let via = if f.entrypoint.starts_with("sdk") {
            " · agent SDK"
        } else {
            ""
        };
        sessions.push(Session {
            pid: f.pid,
            title,
            place: format!("{}{via}", tilde(&f.cwd)),
            status,
            status_text,
            window: window.map(|(id, _)| id),
        });
    }
    sessions.sort_by(|a, b| {
        (b.status == Status::Busy)
            .cmp(&(a.status == Status::Busy))
            .then(a.place.cmp(&b.place))
            .then(a.pid.cmp(&b.pid))
    });
    let busy = sessions.iter().filter(|s| s.status == Status::Busy).count();
    State { sessions, busy }
}

pub fn start(ctx: Ctx) {
    let c = ctx.clone();
    // Claude Code rewrites ~/.claude/sessions/<pid>.json on every status change.
    let trigger = Trigger::default();
    ctx.on_dir_changes(sessions_dir(), trigger.clone());
    // The fallback also keeps the "5 min" texts current.
    let sent = Arc::new(Dedup::default());
    ctx.poll_events(trigger, Duration::from_secs(30), move || {
        let (c, sent) = (c.clone(), sent.clone());
        async move {
            let windows = if any_session_files() {
                crate::compositor::windows_by_pid(&c).await
            } else {
                HashMap::new()
            };
            if let Ok(state) = tokio::task::spawn_blocking(move || fetch(&windows)).await
                && sent.changed(&state)
            {
                c.send(crate::claude::Event::Sessions(state));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs() {
        assert_eq!(
            strip_glyph("✳ Niri visual improvements"),
            "Niri visual improvements"
        );
        assert_eq!(strip_glyph("◑ Testbox"), "Testbox");
        assert_eq!(strip_glyph("plain title"), "plain title");
    }

    #[test]
    fn stat_parsing_handles_spaces() {
        let me = std::process::id() as i32;
        let (ppid, start) = proc_stat(me).unwrap();
        assert!(ppid > 0);
        assert!(start.parse::<u64>().is_ok());
    }
}

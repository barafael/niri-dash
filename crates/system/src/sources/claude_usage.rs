//! Claude usage from local data only: the plan from `~/.claude/.credentials.json` (only the
//! plan fields are read; tokens never are), and token usage from the session transcripts in
//! `~/.claude/projects`. Like ccusage, usage is grouped into 5-hour windows (each starts at the
//! hour of its first message), and every message is counted once (by message and request id).
//! The plan's actual limits aren't stored locally, so this shows usage, not a percentage of them.

use super::{Ctx, Dedup};
use chrono::{DateTime, Duration as Span, DurationRound, Local, Utc};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const WINDOW: Span = Span::hours(5);
const KEEP_DAYS: i64 = 8;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub plan: String,
    pub window_active: bool,
    pub window_tokens: String,
    /// Just the number, for small spaces ("2.2M").
    pub window_count: String,
    pub window_cached: String,
    pub window_messages: u32,
    pub window_left: String,
    pub window_progress: f32,
    pub window_resets: String,
    pub burn: String,
    pub today: String,
    pub week: String,
    /// Tokens per hour over the last 24 hours, scaled to the busiest hour.
    pub hourly: Vec<f32>,
    /// Models used in the current window, by share of tokens.
    pub models: Vec<(String, f32)>,
}

#[derive(Debug, Clone)]
struct Entry {
    at: DateTime<Utc>,
    model: String,
    /// Input, output and cache writes: what counts against usage.
    tokens: u64,
    cache_read: u64,
}

#[derive(Deserialize)]
struct Line {
    timestamp: Option<DateTime<Utc>>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Only the plan fields of the credentials file; everything else (the tokens) is skipped.
#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<PlanFields>,
}

#[derive(Deserialize)]
struct PlanFields {
    #[serde(rename = "subscriptionType")]
    subscription: Option<String>,
    #[serde(rename = "rateLimitTier")]
    tier: Option<String>,
}

/// "max" + "default_claude_max_5x" -> "Max 5×".
pub fn plan_name(subscription: &str, tier: &str) -> String {
    let mut name = match subscription {
        "" => "Unknown plan".to_string(),
        s => {
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    };
    if let Some(mult) = tier
        .rsplit('_')
        .next()
        .and_then(|t| t.strip_suffix('x'))
        .filter(|m| m.parse::<u32>().is_ok())
    {
        name.push_str(&format!(" {mult}×"));
    }
    name
}

fn read_plan(home: &Path) -> String {
    let text = std::fs::read_to_string(home.join(".claude/.credentials.json")).unwrap_or_default();
    match serde_json::from_str::<Credentials>(&text)
        .ok()
        .and_then(|c| c.oauth)
    {
        Some(p) => plan_name(
            p.subscription.as_deref().unwrap_or(""),
            p.tier.as_deref().unwrap_or(""),
        ),
        None => "Not signed in".into(),
    }
}

/// Reads transcripts incrementally: each file from where the last scan stopped.
#[derive(Default)]
struct Scanner {
    offsets: HashMap<PathBuf, u64>,
    /// Messages already counted, with their time, so the set ages out with `entries`.
    seen: HashMap<String, DateTime<Utc>>,
    entries: Vec<Entry>,
}

fn jsonl_files(dir: &Path, since: SystemTime, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            jsonl_files(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl")
            && meta.modified().is_ok_and(|m| m >= since)
        {
            out.push(p);
        }
    }
}

impl Scanner {
    fn scan(&mut self, projects: &Path) {
        let since = SystemTime::now() - Duration::from_secs(KEEP_DAYS as u64 * 86_400);
        let mut files = Vec::new();
        jsonl_files(projects, since, &mut files);
        // Files that left the window (or were deleted) aren't read again.
        self.offsets.retain(|p, _| files.contains(p));
        for path in files {
            let offset = self.offsets.get(&path).copied().unwrap_or(0);
            let Ok(mut f) = std::fs::File::open(&path) else {
                continue;
            };
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            let start = if len < offset { 0 } else { offset };
            if start == len || f.seek(SeekFrom::Start(start)).is_err() {
                continue;
            }
            let mut reader = BufReader::new(f.by_ref());
            let mut consumed = start;
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    // A line still being written: read it next time.
                    Ok(_) if !line.ends_with('\n') => break,
                    Ok(n) => consumed += n as u64,
                }
                if line.contains("\"usage\"") {
                    self.take(&line);
                }
            }
            self.offsets.insert(path, consumed);
        }
        let cutoff = Utc::now() - Span::days(KEEP_DAYS);
        self.entries.retain(|e| e.at >= cutoff);
        self.seen.retain(|_, at| *at >= cutoff);
        self.entries.sort_by_key(|e| e.at);
    }

    fn take(&mut self, line: &str) {
        let Ok(l) = serde_json::from_str::<Line>(line) else {
            return;
        };
        let (Some(at), Some(msg)) = (l.timestamp, l.message) else {
            return;
        };
        let Some(u) = msg.usage else { return };
        // The same message is logged once per streamed content block; count it once.
        if let Some(id) = &msg.id
            && self
                .seen
                .insert(
                    format!("{id}:{}", l.request_id.as_deref().unwrap_or("")),
                    at,
                )
                .is_some()
        {
            return;
        }
        let tokens = u.input_tokens + u.output_tokens + u.cache_creation_input_tokens;
        if tokens == 0 && u.cache_read_input_tokens == 0 {
            return;
        }
        self.entries.push(Entry {
            at,
            model: msg.model.unwrap_or_default(),
            tokens,
            cache_read: u.cache_read_input_tokens,
        });
    }
}

pub fn short_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.2}B", n as f64 / 1e9),
    }
}

fn short_model(m: &str) -> String {
    // "claude-opus-5-5" -> "Opus 5.5"
    let parts: Vec<&str> = m.trim_start_matches("claude-").split('-').collect();
    match parts.as_slice() {
        [family, rest @ ..] if !rest.is_empty() => {
            let mut f = family.chars();
            let fam: String = f
                .next()
                .map(|c| c.to_uppercase().chain(f).collect())
                .unwrap_or_default();
            let ver: Vec<&str> = rest
                .iter()
                .take_while(|p| p.chars().all(|c| c.is_ascii_digit()) && p.len() <= 2)
                .copied()
                .collect();
            if ver.is_empty() {
                fam
            } else {
                format!("{fam} {}", ver.join("."))
            }
        }
        _ => m.to_string(),
    }
}

/// The current 5-hour window, if one is running: its start and the entries in it.
fn current_window(entries: &[Entry], now: DateTime<Utc>) -> Option<(DateTime<Utc>, &[Entry])> {
    let mut start: Option<(DateTime<Utc>, usize)> = None;
    let mut last: Option<DateTime<Utc>> = None;
    for (i, e) in entries.iter().enumerate() {
        let new_block = match (start, last) {
            (None, _) => true,
            (Some((s, _)), Some(l)) => e.at >= s + WINDOW || e.at - l >= WINDOW,
            _ => false,
        };
        if new_block {
            let floored = e.at.duration_trunc(Span::hours(1)).unwrap_or(e.at);
            start = Some((floored, i));
        }
        last = Some(e.at);
    }
    let (s, i) = start?;
    (now < s + WINDOW).then(|| (s, &entries[i..]))
}

fn summarize(entries: &[Entry], plan: String, now: DateTime<Utc>) -> State {
    let mut st = State {
        plan,
        ..Default::default()
    };
    let local_today = now.with_timezone(&Local).date_naive();
    let (mut today_tokens, mut today_msgs, mut week_tokens) = (0u64, 0u32, 0u64);
    let mut hourly = vec![0u64; 24];
    for e in entries {
        if e.at.with_timezone(&Local).date_naive() == local_today {
            today_tokens += e.tokens;
            today_msgs += 1;
        }
        if now - e.at < Span::days(7) {
            week_tokens += e.tokens;
        }
        let ago = (now - e.at).num_hours();
        if (0..24).contains(&ago) {
            hourly[23 - ago as usize] += e.tokens;
        }
    }
    let peak = hourly.iter().copied().max().unwrap_or(0).max(1);
    st.hourly = hourly.into_iter().map(|h| h as f32 / peak as f32).collect();
    st.today = format!(
        "Today {} tokens · {today_msgs} messages",
        short_count(today_tokens)
    );
    st.week = format!("7 days {} tokens", short_count(week_tokens));
    if let Some((start, window)) = current_window(entries, now) {
        let end = start + WINDOW;
        let tokens: u64 = window.iter().map(|e| e.tokens).sum();
        let cached: u64 = window.iter().map(|e| e.cache_read).sum();
        let left = end - now;
        st.window_active = true;
        st.window_tokens = format!("{} tokens", short_count(tokens));
        st.window_count = short_count(tokens);
        st.window_cached = format!("{} read from cache", short_count(cached));
        st.window_messages = window.len() as u32;
        st.window_left = format!(
            "{} h {:02} min left",
            left.num_hours(),
            left.num_minutes() % 60
        );
        st.window_progress =
            ((now - start).num_seconds() as f32 / WINDOW.num_seconds() as f32).clamp(0.0, 1.0);
        st.window_resets = format!("resets {}", end.with_timezone(&Local).format("%H:%M"));
        let minutes = ((now - start).num_seconds() as f64 / 60.0).max(1.0);
        st.burn = format!(
            "{} tokens/min",
            short_count((tokens as f64 / minutes) as u64)
        );
        let mut by_model: HashMap<String, u64> = HashMap::new();
        for e in window {
            *by_model.entry(short_model(&e.model)).or_default() += e.tokens;
        }
        let mut models: Vec<(String, u64)> = by_model
            .into_iter()
            .filter(|(m, _)| !m.is_empty())
            .collect();
        models.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
        st.models = models
            .into_iter()
            .take(3)
            .map(|(m, t)| (m, t as f32 / tokens.max(1) as f32))
            .collect();
    } else {
        st.window_left = "No active window".into();
    }
    st
}

pub fn start(ctx: Ctx) {
    let home = dirs::home_dir().unwrap_or_default();
    let scanner = Arc::new(Mutex::new(Scanner::default()));
    let c = ctx.clone();
    let sent = Arc::new(Dedup::default());
    ctx.poll(Duration::from_secs(30), move || {
        let (c, home, scanner, sent) = (c.clone(), home.clone(), scanner.clone(), sent.clone());
        async move {
            let state = tokio::task::spawn_blocking(move || {
                let mut s = scanner.lock().ok()?;
                s.scan(&home.join(".claude/projects"));
                Some(summarize(&s.entries, read_plan(&home), Utc::now()))
            })
            .await;
            if let Ok(Some(state)) = state
                && sent.changed(&state)
            {
                c.send(crate::claude::Event::Usage(Box::new(state)));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn entry(t: &str, tokens: u64) -> Entry {
        Entry {
            at: at(t),
            model: "claude-opus-5-5".into(),
            tokens,
            cache_read: 0,
        }
    }

    #[test]
    fn plans() {
        assert_eq!(plan_name("max", "default_claude_max_5x"), "Max 5×");
        assert_eq!(plan_name("pro", "default_claude_pro"), "Pro");
        assert_eq!(short_model("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(short_model("claude-haiku-5-5-20261001"), "Haiku 5.5");
    }

    #[test]
    fn windows_start_on_the_hour_and_last_five_hours() {
        let e = vec![
            entry("2026-11-01T08:40:00Z", 10),
            entry("2026-11-01T12:30:00Z", 20),
            entry("2026-11-01T13:10:00Z", 40),
        ];
        // 08:00-13:00 is over at 13:30; the 13:10 message started a new window at 13:00.
        let (start, w) = current_window(&e, at("2026-11-01T13:30:00Z")).unwrap();
        assert_eq!(start, at("2026-11-01T13:00:00Z"));
        assert_eq!(w.len(), 1);
        // At 12:45 the first window is still running and holds the first two.
        let (start, w) = current_window(&e[..2], at("2026-11-01T12:45:00Z")).unwrap();
        assert_eq!(start, at("2026-11-01T08:00:00Z"));
        assert_eq!(w.iter().map(|e| e.tokens).sum::<u64>(), 30);
        assert!(current_window(&e, at("2026-11-01T19:00:00Z")).is_none());
    }

    #[test]
    fn messages_count_once() {
        let mut s = Scanner::default();
        let line = r#"{"type":"assistant","timestamp":"2026-11-01T09:00:00Z","requestId":"r1","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":5,"output_tokens":7,"cache_creation_input_tokens":3,"cache_read_input_tokens":100}}}"#;
        s.take(line);
        s.take(line);
        assert_eq!(s.entries.len(), 1);
        assert_eq!((s.entries[0].tokens, s.entries[0].cache_read), (15, 100));
    }

    #[test]
    fn credentials_never_need_tokens() {
        let text = r#"{"claudeAiOauth":{"accessToken":"secret","refreshToken":"secret","subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#;
        let c: Credentials = serde_json::from_str(text).unwrap();
        let p = c.oauth.unwrap();
        assert_eq!(
            plan_name(
                p.subscription.as_deref().unwrap(),
                p.tier.as_deref().unwrap()
            ),
            "Max 20×"
        );
    }
}

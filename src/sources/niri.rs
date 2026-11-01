//! Windows and workspaces from niri's event stream; window actions over the IPC socket.

use super::{Ctx, IconSrc, Msg, icons, strip_spinner};
use niri_ipc::state::{EventStreamState, EventStreamStatePart};
use niri_ipc::{Action, Reply, Request, Response, socket::Socket};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: u64,
    pub header: bool,
    pub title: String,
    pub app: String,
    pub icon: IconSrc,
    pub focused: bool,
    pub urgent: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowsState {
    pub connected: bool,
    pub rows: Vec<Row>,
    pub count: usize,
    pub focused_app: String,
    pub focused_icon: Option<IconSrc>,
}

/// Talking to niri over its IPC socket failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("connecting to niri: {0}")]
    Connect(#[source] std::io::Error),
    #[error("talking to niri: {0}")]
    Io(#[from] std::io::Error),
    /// niri understood the request and refused it.
    #[error("niri: {0}")]
    Refused(String),
    #[error("niri sent an unexpected reply")]
    Unexpected,
    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub fn request(req: Request) -> Result<Response> {
    let mut socket = Socket::connect().map_err(Error::Connect)?;
    let reply: Reply = socket.send(req)?;
    reply.map_err(Error::Refused)
}

/// Run an action on a blocking thread (the socket is synchronous).
pub async fn action_async(a: Action) -> Result<()> {
    tokio::task::spawn_blocking(move || action(a)).await?
}

pub fn action(a: Action) -> Result<()> {
    request(Request::Action(a)).map(|_| ())
}

fn build(state: &EventStreamState) -> WindowsState {
    let mut workspaces: Vec<_> = state.workspaces.workspaces.values().collect();
    workspaces.sort_by(|a, b| a.output.cmp(&b.output).then(a.idx.cmp(&b.idx)));
    let multi_output = {
        let mut outs: Vec<_> = workspaces
            .iter()
            .filter_map(|w| w.output.as_deref())
            .collect();
        outs.dedup();
        outs.len() > 1
    };
    let mut rows = Vec::new();
    let mut count = 0;
    let mut focused = None;
    for ws in workspaces {
        let mut wins: Vec<_> = state
            .windows
            .windows
            .values()
            .filter(|w| w.workspace_id == Some(ws.id))
            .collect();
        if wins.is_empty() {
            continue;
        }
        wins.sort_by_key(|w| {
            (
                w.is_floating,
                w.layout
                    .pos_in_scrolling_layout
                    .unwrap_or((usize::MAX, usize::MAX)),
                w.id,
            )
        });
        let mut title = format!("Workspace {}", ws.idx);
        if let Some(name) = &ws.name {
            title.push_str(&format!(" · {name}"));
        }
        if multi_output && let Some(o) = &ws.output {
            title.push_str(&format!(" · {o}"));
        }
        if ws.is_focused {
            title.push_str("  (current)");
        }
        rows.push(Row {
            id: 0,
            header: true,
            title,
            app: String::new(),
            icon: IconSrc::None,
            focused: false,
            urgent: false,
        });
        for w in wins {
            count += 1;
            let app = w.app_id.clone().unwrap_or_default();
            let icon = icons::app_icon(&app)
                .map(IconSrc::Path)
                .unwrap_or(IconSrc::None);
            if w.is_focused {
                focused = Some((app.clone(), icon.clone()));
            }
            rows.push(Row {
                id: w.id,
                header: false,
                title: w
                    .title
                    .as_deref()
                    .map(strip_spinner)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| app.clone()),
                app: app.rsplit('.').next().unwrap_or(&app).to_string(),
                icon,
                focused: w.is_focused,
                urgent: w.is_urgent,
            });
        }
    }
    let (focused_app, focused_icon) = match focused {
        Some((a, i)) => (a, Some(i)),
        None => (String::new(), None),
    };
    WindowsState {
        connected: true,
        rows,
        count,
        focused_app,
        focused_icon,
    }
}

pub fn start(ctx: Ctx) {
    std::thread::Builder::new()
        .name("niri-events".into())
        .spawn(move || {
            let mut warned = false;
            loop {
                // Warn once; niri may simply not be running (yet).
                if let Err(e) = stream(&ctx) {
                    if warned {
                        log::debug!("niri event stream: {e:#}");
                    } else {
                        log::warn!("niri event stream: {e:#}");
                        warned = true;
                    }
                } else {
                    warned = false;
                }
                ctx.send(Msg::Windows(WindowsState::default()));
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawning niri thread");
}

fn stream(ctx: &Ctx) -> Result<()> {
    let mut socket = Socket::connect().map_err(Error::Connect)?;
    let reply = socket.send(Request::EventStream)?;
    reply.map_err(Error::Refused)?;
    let mut read = socket.read_events();
    let mut state = EventStreamState::default();
    let mut sent_layouts = None;
    let sent_windows = super::Dedup::default();
    let mut sent_casts: Option<Vec<ScreenCast>> = None;
    loop {
        let ev = read()?;
        let config_changed = matches!(ev, niri_ipc::Event::ConfigLoaded { .. });
        state.apply(ev);
        let windows = build(&state);
        if sent_windows.changed(&windows) {
            ctx.send(Msg::Windows(windows));
        }
        let layouts = state.keyboard_layouts.keyboard_layouts.clone();
        if sent_layouts.as_ref() != Some(&layouts) {
            ctx.send(Msg::KeyboardLayouts(layouts.clone()));
            sent_layouts = Some(layouts);
        }
        let casts = casts(&state);
        if sent_casts.as_ref() != Some(&casts) {
            ctx.send(Msg::Casts(casts.clone()));
            sent_casts = Some(casts);
        }
        if config_changed {
            ctx.refresh();
        }
    }
}

/// A screencast as the privacy widget shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenCast {
    pub session_id: u64,
    /// What is shared: a monitor name or a window title.
    pub target: String,
    pub active: bool,
    /// The consumer's process (wlr-screencopy only).
    pub pid: Option<i32>,
    /// The PipeWire node of the stream, to find who reads it.
    pub pw_node: Option<u32>,
}

fn casts(state: &EventStreamState) -> Vec<ScreenCast> {
    let mut v: Vec<ScreenCast> = state
        .casts
        .casts
        .values()
        .map(|c| ScreenCast {
            session_id: c.session_id,
            target: match &c.target {
                niri_ipc::CastTarget::Nothing {} => "nothing yet".into(),
                niri_ipc::CastTarget::Output { name } => format!("screen {name}"),
                niri_ipc::CastTarget::Window { id } => state
                    .windows
                    .windows
                    .get(id)
                    .and_then(|w| w.title.clone())
                    .map_or_else(
                        || "a window".into(),
                        |t| format!("window \u{201c}{t}\u{201d}"),
                    ),
            },
            active: c.is_active,
            pid: c.pid,
            pw_node: c.pw_node_id,
        })
        .collect();
    v.sort_by_key(|c| c.session_id);
    v
}

pub fn switch_layout(ctx: &Ctx, layout: niri_ipc::LayoutSwitchTarget) {
    ctx.act("Switch layout", async move {
        action_async(Action::SwitchLayout { layout }).await
    });
}

pub fn stop_cast(ctx: &Ctx, session_id: u64) {
    ctx.act("Stop screen sharing", async move {
        action_async(Action::StopCast { session_id }).await
    });
}

/// Let the user click a pixel anywhere; `None` if they cancelled. Blocks until then.
pub fn pick_color() -> Result<Option<[f64; 3]>> {
    match request(Request::PickColor)? {
        Response::PickedColor(c) => Ok(c.map(|c| c.rgb)),
        other => {
            log::debug!("pick color: unexpected reply {other:?}");
            Err(Error::Unexpected)
        }
    }
}

pub fn focus(ctx: &Ctx, id: u64) {
    ctx.act("Focus window", async move {
        action_async(Action::FocusWindow { id }).await
    });
}

pub fn close(ctx: &Ctx, id: u64) {
    ctx.act("Close window", async move {
        action_async(Action::CloseWindow { id: Some(id) }).await
    });
}

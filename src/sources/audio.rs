//! Default speaker and microphone through PipeWire's `wpctl`; per-app volume and the outputs
//! from the PipeWire graph (`pw-dump`).

use super::{Ctx, IconSrc, Msg, Trigger, icons, pipewire, run};
use std::time::Duration;
use tokio::sync::watch;

const SINK: &str = "@DEFAULT_AUDIO_SINK@";
const SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";

#[derive(Debug, Clone, Default)]
pub struct State {
    pub available: bool,
    pub volume: f32,
    pub muted: bool,
    pub mic: Option<(f32, bool)>,
    pub sink_name: String,
    /// Output devices, to pick the default from.
    pub sinks: Vec<Sink>,
    /// Apps playing sound.
    pub streams: Vec<Stream>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub id: u32,
    pub app: String,
    pub icon: IconSrc,
    pub volume: f32,
    pub muted: bool,
}

/// Streams nobody wants a slider for.
fn ignored(n: &pipewire::Node) -> bool {
    n.monitor || n.app.starts_with("speech-dispatcher") || n.name.starts_with("speech-dispatcher")
}

/// Outputs and playing apps from the PipeWire graph.
pub fn mixer(g: &pipewire::Graph) -> (Vec<Sink>, Vec<Stream>) {
    let sinks = g
        .of_class("Audio/Sink")
        .map(|n| Sink {
            id: n.id,
            name: n.device_label(),
            default: n.name == g.default_sink,
        })
        .collect();
    let streams = g
        .of_class("Stream/Output/Audio")
        .filter(|n| !ignored(n) && matches!(n.state.as_str(), "running" | "idle"))
        .map(|n| Stream {
            id: n.id,
            app: n.app_label(),
            icon: [&n.icon, &n.binary, &n.app.to_lowercase()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .find_map(|s| icons::lookup(s, 32))
                .map_or(IconSrc::None, IconSrc::Path),
            volume: n.volume.unwrap_or(1.0),
            muted: n.muted,
        })
        .collect();
    (sinks, streams)
}

/// Parse `Volume: 0.40 [MUTED]`.
pub fn parse_volume(s: &str) -> Option<(f32, bool)> {
    let rest = s.trim().strip_prefix("Volume:")?.trim();
    let v: f32 = rest.split_whitespace().next()?.parse().ok()?;
    Some((v, rest.contains("[MUTED]")))
}

async fn fetch() -> State {
    let Some((volume, muted)) = run("wpctl", &["get-volume", SINK])
        .await
        .ok()
        .as_deref()
        .and_then(parse_volume)
    else {
        return State::default();
    };
    let mic = run("wpctl", &["get-volume", SOURCE])
        .await
        .ok()
        .as_deref()
        .and_then(parse_volume);
    let sink_name = run("wpctl", &["inspect", SINK])
        .await
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.contains("node.description"))
                .and_then(|l| {
                    l.split_once('=')
                        .map(|(_, v)| v.trim().trim_matches('"').to_string())
                })
        })
        .unwrap_or_default();
    let (sinks, streams) = match pipewire::dump().await {
        Ok(g) => mixer(&g),
        Err(e) => {
            log::debug!("pw-dump: {e}");
            Default::default()
        }
    };
    State {
        available: true,
        volume,
        muted,
        mic,
        sink_name,
        sinks,
        streams,
    }
}

#[derive(Clone)]
pub struct Audio {
    ctx: Ctx,
    sink: watch::Sender<Option<f32>>,
    source: watch::Sender<Option<f32>>,
    /// Per-app volume: (node id, volume).
    stream: watch::Sender<Option<(u32, f32)>>,
}

/// Set the volume of a node: its id, or a default device like `@DEFAULT_AUDIO_SINK@`.
async fn set_node_volume(node: &str, v: f32) {
    let v = format!("{:.3}", v.clamp(0.0, 1.0));
    if let Err(e) = run("wpctl", &["set-volume", node, &v]).await {
        log::debug!("wpctl set-volume {node}: {e}");
    }
}

impl Audio {
    pub fn start(ctx: Ctx) -> Self {
        let c = ctx.clone();
        // PipeWire's pulse server reports device and default changes; streams (sink-input,
        // source-output) change constantly during playback and are ignored.
        let trigger = Trigger::default();
        ctx.on_command_lines(
            "pactl",
            &["subscribe"],
            |l| {
                l.contains(" on sink #")
                    || l.contains(" on source #")
                    || l.contains(" on server")
                    || l.contains(" on sink-input #")
            },
            trigger.clone(),
        );
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let c = c.clone();
            async move { c.send(Msg::Audio(fetch().await)) }
        });
        // Slider drags only apply the latest value, so they don't queue up processes.
        Self {
            sink: ctx.latest(|v| set_node_volume(SINK, v)),
            source: ctx.latest(|v| set_node_volume(SOURCE, v)),
            stream: ctx.latest(|(id, v): (u32, f32)| async move {
                set_node_volume(&id.to_string(), v).await
            }),
            ctx,
        }
    }

    pub fn set_volume(&self, v: f32) {
        let _ = self.sink.send(Some(v));
    }

    pub fn set_mic_volume(&self, v: f32) {
        let _ = self.source.send(Some(v));
    }

    pub fn toggle_mute(&self) {
        self.ctx.act("Mute", async {
            run("wpctl", &["set-mute", SINK, "toggle"])
                .await
                .map(|_| ())
        });
    }

    pub fn set_stream_volume(&self, id: u32, v: f32) {
        let _ = self.stream.send(Some((id, v)));
    }

    pub fn toggle_stream_mute(&self, id: u32) {
        self.ctx.act("Mute", async move {
            run("wpctl", &["set-mute", &id.to_string(), "toggle"])
                .await
                .map(|_| ())
        });
    }

    pub fn set_default_sink(&self, id: u32) {
        self.ctx.act("Output", async move {
            run("wpctl", &["set-default", &id.to_string()])
                .await
                .map(|_| ())
        });
    }

    pub fn toggle_mic_mute(&self) {
        self.ctx.act("Mute microphone", async {
            run("wpctl", &["set-mute", SOURCE, "toggle"])
                .await
                .map(|_| ())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::parse_volume;

    #[test]
    fn volume() {
        assert_eq!(parse_volume("Volume: 0.72\n"), Some((0.72, false)));
        assert_eq!(parse_volume("Volume: 0.40 [MUTED]"), Some((0.40, true)));
        assert_eq!(parse_volume("Translate ID error"), None);
    }
}

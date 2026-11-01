//! Who uses the microphone and the camera right now, for the privacy widget. Screen sharing comes
//! from niri (`Msg::Casts`); its consumer is looked up here in the PipeWire graph.

use super::{Ctx, Dedup, Msg, Trigger, pipewire};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// Apps recording audio.
    pub mic: Vec<String>,
    /// Apps reading a camera.
    pub camera: Vec<String>,
    /// For each video node that something reads (screencasts among them): who reads it.
    pub video_readers: Vec<(u32, String)>,
}

fn is_camera(n: &pipewire::Node) -> bool {
    n.class == "Video/Source" && (n.name.starts_with("v4l2_") || n.name.starts_with("libcamera"))
}

/// Usage from the PipeWire graph.
pub fn from_graph(g: &pipewire::Graph) -> Usage {
    let mut mic = BTreeSet::new();
    for n in g.of_class("Stream/Input/Audio") {
        if n.state != "running" || n.monitor || n.media_name == "Peak detect" {
            continue;
        }
        // Recording a sink's monitor (a level meter, a screen recorder's audio) isn't the mic.
        let from_monitor = g
            .inputs_of(n.id)
            .filter_map(|i| g.node(i))
            .any(|src| src.class == "Audio/Sink");
        if !from_monitor {
            mic.insert(n.app_label());
        }
    }
    let mut camera = BTreeSet::new();
    let mut video_readers = Vec::new();
    for src in g.nodes.iter().filter(|n| n.class == "Video/Source") {
        for reader in g.outputs_of(src.id).filter_map(|i| g.node(i)) {
            let app = reader.app_label();
            video_readers.push((src.id, app.clone()));
            if is_camera(src) && src.state == "running" {
                camera.insert(app);
            }
        }
        if is_camera(src) && src.state == "running" && g.outputs_of(src.id).next().is_none() {
            camera.insert("An app".to_string());
        }
    }
    Usage {
        mic: mic.into_iter().collect(),
        camera: camera.into_iter().collect(),
        video_readers,
    }
}

/// Processes holding a `/dev/video*` open (apps that use the camera without PipeWire). Only our
/// own processes are readable, which are the ones that matter.
fn direct_camera_users() -> Vec<String> {
    let mut apps = BTreeSet::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    for p in procs.flatten() {
        let name = p.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
        else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else {
            continue;
        };
        let uses = fds.flatten().any(|fd| {
            std::fs::read_link(fd.path())
                .is_ok_and(|t| t.to_str().is_some_and(|t| t.starts_with("/dev/video")))
        });
        if uses {
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
            let comm = comm.trim();
            // PipeWire and WirePlumber hold the camera for their clients; those show up above.
            if !comm.is_empty()
                && !matches!(comm, "pipewire" | "wireplumber" | "pipewire-media-session")
            {
                apps.insert(comm.to_string());
            }
        }
    }
    apps.into_iter().collect()
}

pub fn start(ctx: Ctx) {
    let trigger = Trigger::default();
    // Recording streams come and go as source-output events.
    ctx.on_command_lines(
        "pactl",
        &["subscribe"],
        |l| l.contains(" on source-output #") || l.contains(" on source #"),
        trigger.clone(),
    );
    let c = ctx.clone();
    let sent = Arc::new(Dedup::default());
    ctx.poll_events(trigger, Duration::from_secs(3), move || {
        let (c, sent) = (c.clone(), sent.clone());
        async move {
            let mut usage = match pipewire::dump().await {
                Ok(g) => from_graph(&g),
                Err(e) => {
                    log::debug!("privacy: {e}");
                    Usage::default()
                }
            };
            let direct = tokio::task::spawn_blocking(direct_camera_users)
                .await
                .unwrap_or_default();
            for app in direct {
                if !usage.camera.iter().any(|a| a.eq_ignore_ascii_case(&app)) {
                    usage.camera.push(app);
                }
            }
            if sent.changed(&usage) {
                c.send(Msg::Privacy(usage));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::pipewire::{Graph, Node};

    fn node(id: u32, class: &str, name: &str, app: &str, state: &str) -> Node {
        Node {
            id,
            class: class.into(),
            name: name.into(),
            app: app.into(),
            state: state.into(),
            ..Default::default()
        }
    }

    #[test]
    fn mic_camera_and_readers() {
        let g = Graph {
            nodes: vec![
                node(1, "Audio/Source", "alsa_input", "", "running"),
                node(2, "Stream/Input/Audio", "", "Zoom", "running"),
                node(3, "Audio/Sink", "alsa_output", "", "running"),
                node(4, "Stream/Input/Audio", "", "OBS", "running"),
                node(5, "Video/Source", "v4l2_input.cam", "", "running"),
                node(6, "Stream/Input/Video", "", "Firefox", "running"),
                node(7, "Video/Source", "niri", "niri", "running"),
                node(8, "Stream/Input/Video", "", "Chromium", "running"),
                node(9, "Stream/Input/Audio", "", "Idle app", "idle"),
            ],
            links: vec![(1, 2), (3, 4), (5, 6), (7, 8)],
            ..Default::default()
        };
        let u = from_graph(&g);
        assert_eq!(u.mic, ["Zoom"]);
        assert_eq!(u.camera, ["Firefox"]);
        assert!(u.video_readers.contains(&(7, "Chromium".into())));
    }
}

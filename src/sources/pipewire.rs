//! The PipeWire graph from `pw-dump`: devices, app streams, their links and the default devices.
//! Used for per-app volume and for the privacy indicator (who records, who films).

use super::{CmdError, run};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub id: u32,
    /// "Audio/Sink", "Stream/Output/Audio", "Video/Source", …
    pub class: String,
    /// "running", "idle", "suspended", …
    pub state: String,
    pub name: String,
    pub description: String,
    pub app: String,
    pub binary: String,
    pub icon: String,
    pub media_name: String,
    pub pid: Option<u32>,
    /// Peak meters (pavucontrol and friends) read a monitor of another node.
    pub monitor: bool,
    /// Volume the way wpctl and mixers show it (cube root of the linear channel volume).
    pub volume: Option<f32>,
    pub muted: bool,
}

impl Node {
    /// The best name to show for an app stream.
    pub fn app_label(&self) -> String {
        [&self.app, &self.binary, &self.name, &self.media_name]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("#{}", self.id))
    }

    pub fn device_label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            self.description.clone()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// (output node, input node).
    pub links: Vec<(u32, u32)>,
    /// Default devices by node name.
    pub default_sink: String,
    pub default_source: String,
}

impl Graph {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.nodes.iter().filter(move |n| n.class == class)
    }

    /// Nodes that `id` feeds into.
    pub fn outputs_of(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        self.links
            .iter()
            .filter(move |(o, _)| *o == id)
            .map(|(_, i)| *i)
    }

    /// Nodes feeding into `id`.
    pub fn inputs_of(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        self.links
            .iter()
            .filter(move |(_, i)| *i == id)
            .map(|(o, _)| *o)
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn as_u32(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Parse `pw-dump`'s JSON.
pub fn parse(json: &str) -> Result<Graph, serde_json::Error> {
    let objects: Vec<Value> = serde_json::from_str(json)?;
    let mut g = Graph::default();
    for o in &objects {
        let ty = o.get("type").and_then(Value::as_str).unwrap_or_default();
        let info = o.get("info").unwrap_or(&Value::Null);
        match ty {
            "PipeWire:Interface:Node" => {
                let props = info.get("props").unwrap_or(&Value::Null);
                let class = s(props, "media.class");
                if class.is_empty() {
                    continue;
                }
                let params = info
                    .get("params")
                    .and_then(|p| p.get("Props"))
                    .and_then(Value::as_array)
                    .and_then(|a| a.first());
                let volume = params
                    .and_then(|p| p.get("channelVolumes"))
                    .and_then(Value::as_array)
                    .and_then(|vs| {
                        vs.iter()
                            .filter_map(Value::as_f64)
                            .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |m| m.max(v))))
                    })
                    .map(|v| v.max(0.0).cbrt() as f32);
                let muted = params
                    .and_then(|p| p.get("mute"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                g.nodes.push(Node {
                    id: o.get("id").and_then(as_u32).unwrap_or_default(),
                    class,
                    state: s(info, "state"),
                    name: s(props, "node.name"),
                    description: {
                        let d = s(props, "node.description");
                        if d.is_empty() {
                            s(props, "node.nick")
                        } else {
                            d
                        }
                    },
                    app: s(props, "application.name"),
                    binary: s(props, "application.process.binary"),
                    icon: s(props, "application.icon-name"),
                    media_name: s(props, "media.name"),
                    pid: props.get("application.process.id").and_then(as_u32),
                    monitor: props
                        .get("stream.monitor")
                        .is_some_and(|v| v.as_bool() == Some(true) || v.as_str() == Some("true")),
                    volume,
                    muted,
                });
            }
            "PipeWire:Interface:Link" => {
                let (Some(out), Some(inp)) = (
                    info.get("output-node-id").and_then(as_u32),
                    info.get("input-node-id").and_then(as_u32),
                ) else {
                    continue;
                };
                g.links.push((out, inp));
            }
            "PipeWire:Interface:Metadata" => {
                let name = o
                    .get("props")
                    .map(|p| s(p, "metadata.name"))
                    .unwrap_or_default();
                if name != "default" {
                    continue;
                }
                for m in o
                    .get("metadata")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let key = s(m, "key");
                    let value = m.get("value").map(|v| s(v, "name")).unwrap_or_default();
                    match key.as_str() {
                        "default.audio.sink" => g.default_sink = value,
                        "default.audio.source" => g.default_source = value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(g)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cmd(#[from] CmdError),
    #[error("reading pw-dump: {0}")]
    Json(#[from] serde_json::Error),
}

pub async fn dump() -> Result<Graph, Error> {
    Ok(parse(&run("pw-dump", &[]).await?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 32, "type": "PipeWire:Interface:Node", "info": {"state": "running",
        "props": {"media.class": "Audio/Sink", "node.name": "alsa_output.analog",
                  "node.description": "Speakers"},
        "params": {"Props": [{"channelVolumes": [0.125, 0.064], "mute": false}]}}},
      {"id": 79, "type": "PipeWire:Interface:Node", "info": {"state": "running",
        "props": {"media.class": "Stream/Output/Audio", "application.name": "Firefox",
                  "application.icon-name": "firefox", "application.process.id": 4242},
        "params": {"Props": [{"channelVolumes": [1.0, 1.0], "mute": true}]}}},
      {"id": 90, "type": "PipeWire:Interface:Node", "info": {"state": "running",
        "props": {"media.class": "Stream/Input/Audio", "application.name": "pavucontrol",
                  "stream.monitor": "true"}}},
      {"id": 91, "type": "PipeWire:Interface:Node", "info": {"props": {}}},
      {"id": 77, "type": "PipeWire:Interface:Link",
        "info": {"output-node-id": 79, "input-node-id": 32}},
      {"id": 40, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
        "metadata": [{"subject": 0, "key": "default.audio.sink",
                      "value": {"name": "alsa_output.analog"}}]}
    ]"#;

    #[test]
    fn parses_nodes_links_defaults() {
        let g = parse(DUMP).unwrap();
        assert_eq!(g.nodes.len(), 3);
        let sink = g.node(32).unwrap();
        assert_eq!(sink.device_label(), "Speakers");
        assert!((sink.volume.unwrap() - 0.5).abs() < 1e-6);
        let ff = g.node(79).unwrap();
        assert_eq!(ff.app_label(), "Firefox");
        assert_eq!(ff.pid, Some(4242));
        assert!(ff.muted);
        assert!(g.node(90).unwrap().monitor);
        assert_eq!(g.outputs_of(79).collect::<Vec<_>>(), [32]);
        assert_eq!(g.default_sink, "alsa_output.analog");
        assert_eq!(g.of_class("Audio/Sink").count(), 1);
    }
}

//! The volume widget: the default speaker and microphone, outputs and per-app volume.

use super::{App, model, set_if_changed};
use crate::sources;
use crate::{Audio as UiAudio, Dash};
use slint::{ComponentHandle, Model, ModelRc};
use std::rc::Rc;
use std::time::{Duration, Instant};

impl App {
    pub(super) fn show_audio(&self, s: sources::audio::State) {
        let ui = self.ui();
        let a = ui.global::<UiAudio>();
        let touched = self
            .audio_touched
            .get()
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
        if !touched {
            a.set_volume(s.volume);
            if let Some((v, _)) = s.mic {
                a.set_mic_volume(v);
            }
        }
        a.set_muted(s.muted);
        a.set_mic_muted(s.mic.is_none_or(|(_, m)| m));
        a.set_sink_name(s.sink_name.as_str().into());
        self.sync_mixer(&s);
    }

    fn sync_mixer(&self, s: &crate::sources::audio::State) {
        let ui = self.ui();
        let au = ui.global::<crate::Audio>();
        au.set_sinks(model(
            s.sinks
                .iter()
                .map(|k| crate::AudioSink {
                    id: k.id as i32,
                    name: k.name.as_str().into(),
                    default: k.default,
                })
                .collect(),
        ));
        let touched = self
            .stream_touched
            .get()
            .filter(|(_, t)| t.elapsed() < Duration::from_millis(1500))
            .map(|(id, _)| id);
        let rows: Vec<crate::AudioStream> = s
            .streams
            .iter()
            .map(|st| {
                let icon = self.image(&st.icon);
                let mut volume = st.volume;
                if touched == Some(st.id)
                    && let Some(old) = self.streams.iter().find(|r| r.id as u32 == st.id)
                {
                    volume = old.volume;
                }
                crate::AudioStream {
                    id: st.id as i32,
                    app: st.app.as_str().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                    volume,
                    muted: st.muted,
                }
            })
            .collect();
        set_if_changed(&self.streams, rows, |a, b| a == b);
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let au = ui.global::<UiAudio>();
    let a = app.clone();
    au.on_set_volume(move |v| {
        a.audio_touched.set(Some(Instant::now()));
        a.audio.set_volume(v);
    });
    let a = app.clone();
    au.on_set_mic_volume(move |v| {
        a.audio_touched.set(Some(Instant::now()));
        a.audio.set_mic_volume(v);
    });
    let a = app.clone();
    au.on_toggle_mute(move || a.audio.toggle_mute());
    let a = app.clone();
    au.on_toggle_mic_mute(move || a.audio.toggle_mic_mute());
    let au = ui.global::<crate::Audio>();
    au.set_streams(ModelRc::from(app.streams.clone()));
    let a = app.clone();
    au.on_set_stream_volume(move |id, v| {
        a.stream_touched
            .set(Some((id as u32, std::time::Instant::now())));
        a.audio.set_stream_volume(id as u32, v);
    });
    let a = app.clone();
    au.on_toggle_stream_mute(move |id| a.audio.toggle_stream_mute(id as u32));
    let a = app.clone();
    au.on_set_default_sink(move |id| a.audio.set_default_sink(id as u32));
}

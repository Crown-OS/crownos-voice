//! The persistent enhanced microphone that stands in as the default source for every app.

mod step;

use std::sync::Arc;

use voice_abi::{Model, Workload};
use voice_protocol::StreamId;

use crate::daemon::Daemon;
use crate::enhancement::{from_setting, model_for};
use crate::live::{LiveStream, StreamOwner, StreamPlan, SwitchCause, spawn_backend_switch};
pub use step::AllAppsState;
use step::{AllAppsStep, next_step};

impl Daemon {
    /// The model the settings ask the all-apps microphone to run; `None` asks for no microphone.
    pub fn wanted_all_apps_model(&self) -> Option<Model> {
        let voice = &self.settings;
        voice
            .enhance_all_apps
            .then(|| model_for(from_setting(voice.all_apps_enhancement)))
            .flatten()
    }

    /// Opens, switches or closes the all-apps microphone to match the settings.
    pub fn reconcile_all_apps(&mut self) {
        let running = match self.all_apps {
            AllAppsState::Running(stream) => self.streams.get(&stream).map(|live| live.model),
            _ => None,
        };
        match next_step(self.all_apps, running, self.wanted_all_apps_model()) {
            AllAppsStep::Open(model) => self.open_all_apps(model),
            AllAppsStep::Close(stream) => {
                log::info!("closing the all-apps microphone");
                self.streams.remove(&stream);
                self.all_apps = AllAppsState::Off;
            }
            AllAppsStep::Switch(stream, model) => self.switch_all_apps(stream, model),
            AllAppsStep::Wait => {}
        }
    }

    /// Takes over the all-apps microphone once its backend and nodes exist.
    pub fn adopt_all_apps(&mut self, opened: Result<LiveStream, String>) {
        match opened {
            Ok(live) => {
                log::info!(
                    "all-apps microphone {} on {}",
                    live.node.node_name,
                    live.node.backend
                );
                self.all_apps = AllAppsState::Running(live.node.stream);
                self.streams.insert(live.node.stream, live);
                self.reconcile_all_apps();
            }
            Err(reason) => {
                log::warn!("could not open the all-apps microphone: {reason}");
                self.all_apps = AllAppsState::Failed;
            }
        }
    }

    fn open_all_apps(&mut self, model: Model) {
        let plan = StreamPlan {
            owner: StreamOwner::AllApps,
            stream: StreamId(self.allocate_id()),
            target: None,
            model,
        };
        let started = LiveStream::open_in_background(
            plan,
            Arc::clone(&self.registry),
            self.notifier.clone(),
            None,
        );
        self.all_apps = match started {
            Ok(()) => AllAppsState::Opening,
            Err(error) => {
                log::warn!("could not start the all-apps microphone: {error}");
                AllAppsState::Failed
            }
        };
    }

    fn switch_all_apps(&self, stream: StreamId, model: Model) {
        let started = spawn_backend_switch(
            stream,
            SwitchCause::Settings,
            model,
            Arc::clone(&self.registry),
            self.notifier.clone(),
            move |registry| registry.choose(model, Workload::Realtime),
        );
        if let Err(error) = started {
            log::warn!("could not switch the all-apps microphone: {error}");
        }
    }
}

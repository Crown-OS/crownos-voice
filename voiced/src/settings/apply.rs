use std::sync::Arc;

use crownos_config::schema::Voice;
use voice_abi::Workload;

use crate::all_apps::AllAppsState;
use crate::daemon::Daemon;
use crate::live::{SwitchCause, spawn_backend_switch};

impl Daemon {
    /// Puts edited settings into effect without interrupting running streams.
    pub fn apply_settings(&mut self, settings: Voice) {
        log::info!("voice settings changed: {settings:?}");
        let backend_changed = settings.backend != self.settings.backend;
        self.settings = settings;
        if backend_changed {
            self.registry.restrict(self.settings.backend);
            self.reselect_backends();
        }
        if self.all_apps == AllAppsState::Failed {
            self.all_apps = AllAppsState::Off;
        }
        self.reconcile_all_apps();
    }

    /// Moves every stream to the best backend the settings now allow.
    fn reselect_backends(&self) {
        for (&stream, live) in &self.streams {
            let model = live.model;
            let started = spawn_backend_switch(
                stream,
                SwitchCause::Settings,
                model,
                Arc::clone(&self.registry),
                self.notifier.clone(),
                move |registry| registry.choose(model, Workload::Realtime),
            );
            if let Err(error) = started {
                log::warn!(
                    "could not reselect the backend of stream {}: {error}",
                    stream.0
                );
            }
        }
    }
}

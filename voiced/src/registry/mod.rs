//! The installed backend plugins: probed lazily, loaded on demand, unloaded when idle.
//!
//! Shared between the IPC loop and the threads that create backend instances.

mod preference;
mod probe;
mod probe_cache;
mod residency;
mod selection;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

use crownos_config::schema::VoiceBackend;
use voice_abi::host::{LoadError, LoadedBackend};
use voice_abi::{Model, Workload};
use voice_protocol::BackendInfo;

use crate::processor::FrameProcessor;
use crate::util::env_dir::env_dir;
use preference::Preference;
use probe::ProbedBackend;
use residency::IDLE_TTL;
pub use selection::BackendChoice;
use selection::{Candidate, first_created, rank};

const BACKENDS_DIR_VARIABLE: &str = "CROWN_VOICE_BACKENDS";
const DEFAULT_BACKENDS_DIR: &str = "/usr/lib/crownos/voice/backends";
const MODELS_DIR_VARIABLE: &str = "CROWN_VOICE_MODELS";
const DEFAULT_MODELS_DIR: &str = "/usr/share/crownos/voice/models";

pub struct Registry {
    plugin_dir: PathBuf,
    models_dir: PathBuf,
    probed: OnceLock<Vec<ProbedBackend>>,
    setting: Mutex<VoiceBackend>,
}

impl Registry {
    pub fn from_env() -> Self {
        Self {
            plugin_dir: env_dir(BACKENDS_DIR_VARIABLE, DEFAULT_BACKENDS_DIR),
            models_dir: env_dir(MODELS_DIR_VARIABLE, DEFAULT_MODELS_DIR),
            probed: OnceLock::new(),
            setting: Mutex::default(),
        }
    }

    /// Limits later choices to the backends `setting` permits.
    pub fn restrict(&self, setting: VoiceBackend) {
        *self.setting.lock().unwrap_or_else(PoisonError::into_inner) = setting;
    }

    fn backends(&self) -> &[ProbedBackend] {
        self.probed
            .get_or_init(|| probe::probe_dir(&self.plugin_dir, &Preference::load()))
    }

    /// Creates a processor on the best backend for `workload` that can run `model`.
    ///
    /// Blocks while the plugin is mapped and its model loaded.
    pub fn choose(
        &self,
        model: Model,
        workload: Workload,
    ) -> Option<(BackendChoice, FrameProcessor)> {
        let order = self.ranking(workload);
        let (position, processor) = self.create_first(&order, model, workload)?;
        Some((BackendChoice::new(order, position), processor))
    }

    /// Name of the backend [`Self::choose`] tries first for `workload`.
    pub fn preferred(&self, workload: Workload) -> Option<String> {
        let first = *self.ranking(workload).first()?;
        self.backends()
            .get(first)
            .map(|backend| backend.name().to_owned())
    }

    /// Backends permitted by the setting that can serve `workload`, best first.
    fn ranking(&self, workload: Workload) -> Vec<usize> {
        let setting = *self.setting.lock().unwrap_or_else(PoisonError::into_inner);
        let candidates: Vec<Candidate> = self
            .backends()
            .iter()
            .map(|backend| backend.candidate(workload, setting))
            .collect();
        rank(&candidates)
    }

    /// Creates a processor on the next backend below `choice`'s current one that can run `model`.
    pub fn demote(
        &self,
        choice: &BackendChoice,
        model: Model,
        workload: Workload,
    ) -> Option<(BackendChoice, FrameProcessor)> {
        let (offset, processor) = self.create_first(choice.fallbacks(), model, workload)?;
        Some((choice.demoted(offset), processor))
    }

    fn create_first(
        &self,
        order: &[usize],
        model: Model,
        workload: Workload,
    ) -> Option<(usize, FrameProcessor)> {
        let backends = self.backends();
        first_created(order, |index| {
            instantiate(&backends[index], &self.models_dir, model, workload)
        })
    }

    /// "name: device" of a backend, for clients.
    pub fn label(&self, index: usize, workload: Workload) -> String {
        self.backends()
            .get(index)
            .map_or_else(String::new, |backend| backend.label(workload))
    }

    /// Unloads backends whose last instance ended more than [`IDLE_TTL`] ago.
    pub fn sweep(&self, now: Instant) {
        for backend in self.probed.get().into_iter().flatten() {
            let Ok(mut residency) = backend.residency.try_lock() else {
                continue;
            };
            let was_loaded = residency.is_loaded();
            residency.sweep(now, IDLE_TTL);
            if was_loaded && !residency.is_loaded() {
                log::info!("unloaded idle backend {}", backend.name());
            }
        }
    }

    pub fn status(&self) -> Vec<BackendInfo> {
        self.backends().iter().map(ProbedBackend::info).collect()
    }
}

fn instantiate(
    backend: &ProbedBackend,
    models_dir: &Path,
    model: Model,
    workload: Workload,
) -> Result<FrameProcessor, LoadError> {
    let library = {
        let mut residency = backend
            .residency
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // SAFETY: plugins are installed into the system backend directory by trusted packages.
        let loaded = residency.get_or_try_load(|| unsafe { LoadedBackend::load(&backend.path) })?;
        Arc::clone(loaded)
    };
    let instance = library.create(model, workload, models_dir)?;
    log::info!(
        "{} serves a {workload:?} {model:?} instance",
        backend.name()
    );
    let latency = library.latency_samples(model);
    Ok(FrameProcessor::new(model, instance, latency))
}

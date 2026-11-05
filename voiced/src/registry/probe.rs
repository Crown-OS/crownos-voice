use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crownos_config::schema::VoiceBackend;
use serde::{Deserialize, Serialize};
use voice_abi::host::LoadedBackend;
use voice_abi::{Probe, Workload};
use voice_protocol::BackendInfo;

use super::preference::Preference;
use super::probe_cache::{PluginFingerprint, ProbeCache};
use super::residency::Residency;
use super::selection::{Candidate, permits};

const PLUGIN_EXTENSION: &str = "so";

/// What a plugin reported for one workload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkloadProbe {
    pub score: u32,
    pub device: String,
}

impl From<Probe> for WorkloadProbe {
    fn from(probe: Probe) -> Self {
        Self {
            score: probe.score,
            device: probe.device.as_str().to_owned(),
        }
    }
}

/// Everything probing a plugin tells the daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub name: String,
    pub realtime: WorkloadProbe,
    pub offline: WorkloadProbe,
}

/// A plugin's probe results, kept after its library is unloaded.
pub struct ProbedBackend {
    pub path: PathBuf,
    pub report: ProbeReport,
    preference: Option<usize>,
    pub residency: Mutex<Residency<LoadedBackend>>,
}

impl ProbedBackend {
    pub fn name(&self) -> &str {
        &self.report.name
    }

    fn probe(&self, workload: Workload) -> &WorkloadProbe {
        match workload {
            Workload::Realtime => &self.report.realtime,
            Workload::Offline => &self.report.offline,
        }
    }

    /// Ranking input; a backend the setting does not permit scores zero, so it is never chosen.
    pub fn candidate(&self, workload: Workload, setting: VoiceBackend) -> Candidate {
        Candidate {
            score: if permits(setting, self.name()) {
                self.probe(workload).score
            } else {
                0
            },
            preference: self.preference,
        }
    }

    pub fn label(&self, workload: Workload) -> String {
        match self.probe(workload).device.as_str() {
            "" => self.report.name.clone(),
            device => format!("{}: {device}", self.report.name),
        }
    }

    pub fn info(&self) -> BackendInfo {
        let ProbeReport {
            name,
            realtime,
            offline,
        } = &self.report;
        let device = [realtime, offline]
            .into_iter()
            .map(|probe| probe.device.as_str())
            .find(|device| !device.is_empty());
        BackendInfo {
            name: name.clone(),
            device: device.unwrap_or_default().to_owned(),
            realtime_score: realtime.score,
            offline_score: offline.score,
            loaded: self
                .residency
                .try_lock()
                .map_or(true, |residency| residency.is_loaded()),
        }
    }
}

/// Probes every plugin in `dir`, reusing cached results for plugins that did not change.
pub fn probe_dir(dir: &Path, preference: &Preference) -> Vec<ProbedBackend> {
    let cache_file = ProbeCache::location();
    let cached = cache_file
        .as_deref()
        .map(ProbeCache::load)
        .unwrap_or_default();
    let mut current = ProbeCache::default();
    let backends = plugin_paths(dir)
        .into_iter()
        .filter_map(|path| {
            let fingerprint = PluginFingerprint::of(&path).ok();
            let report = match fingerprint.as_ref().and_then(|print| cached.lookup(print)) {
                Some(report) => report.clone(),
                None => probe_plugin(&path)?,
            };
            log_report(&report);
            if let Some(fingerprint) = fingerprint {
                current.insert(fingerprint, report.clone());
            }
            Some(ProbedBackend {
                preference: preference.rank(&report.name),
                report,
                residency: Mutex::default(),
                path,
            })
        })
        .collect();
    if let Some(file) = cache_file.filter(|_| current != cached)
        && let Err(error) = current.save(&file)
    {
        log::warn!(
            "could not cache backend probes in {}: {error}",
            file.display()
        );
    }
    backends
}

fn plugin_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == PLUGIN_EXTENSION)
            })
            .collect(),
        Err(error) => {
            log::warn!("no voice backends in {}: {error}", dir.display());
            Vec::new()
        }
    };
    paths.sort();
    paths
}

/// Loads `path` once to probe it, then unloads it.
fn probe_plugin(path: &Path) -> Option<ProbeReport> {
    // SAFETY: plugins are installed into the system backend directory by trusted packages.
    let library = unsafe { LoadedBackend::load(path) }
        .inspect_err(|error| log::warn!("skipping backend: {error}"))
        .ok()?;
    Some(ProbeReport {
        name: library.name().to_owned(),
        realtime: library.probe(Workload::Realtime).into(),
        offline: library.probe(Workload::Offline).into(),
    })
}

fn log_report(report: &ProbeReport) {
    log::info!(
        "backend {} scores realtime {} ({}), offline {} ({})",
        report.name,
        report.realtime.score,
        report.realtime.device,
        report.offline.score,
        report.offline.device
    );
}

//! Probe results of unchanged plugins, kept across daemon starts so they need no dlopen.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::probe::ProbeReport;
use crate::util::cache_dir::user_cache_dir;

const CACHE_FILE: &str = "crownos/voice/backends.ron";
const STAGING_EXTENSION: &str = "ron.tmp";

/// Identifies one build of a plugin file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginFingerprint {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

impl PluginFingerprint {
    pub fn of(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            path: path.to_owned(),
            modified: metadata.modified()?,
            size: metadata.len(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CachedProbe {
    fingerprint: PluginFingerprint,
    report: ProbeReport,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbeCache {
    plugins: Vec<CachedProbe>,
}

impl ProbeCache {
    /// `$XDG_CACHE_HOME/crownos/voice/backends.ron`.
    pub fn location() -> Option<PathBuf> {
        user_cache_dir().map(|dir| dir.join(CACHE_FILE))
    }

    /// A missing, unreadable or outdated file is an empty cache.
    pub fn load(file: &Path) -> Self {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> anyhow::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let staging = file.with_extension(STAGING_EXTENSION);
        std::fs::write(
            &staging,
            ron::ser::to_string_pretty(self, Default::default())?,
        )?;
        std::fs::rename(&staging, file)?;
        Ok(())
    }

    pub fn lookup(&self, fingerprint: &PluginFingerprint) -> Option<&ProbeReport> {
        self.plugins
            .iter()
            .find(|cached| cached.fingerprint == *fingerprint)
            .map(|cached| &cached.report)
    }

    pub fn insert(&mut self, fingerprint: PluginFingerprint, report: ProbeReport) {
        self.plugins.push(CachedProbe {
            fingerprint,
            report,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::registry::probe::WorkloadProbe;

    fn fingerprint(size: u64) -> PluginFingerprint {
        PluginFingerprint {
            path: PathBuf::from("/usr/lib/crownos/voice/backends/libvoice_cpu.so"),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            size,
        }
    }

    fn report() -> ProbeReport {
        let probe = |score| WorkloadProbe {
            score,
            device: "DeepFilterNet3".into(),
        };
        ProbeReport {
            name: "cpu".into(),
            realtime: probe(50),
            offline: probe(40),
        }
    }

    #[test]
    fn only_an_unchanged_plugin_hits() {
        let mut cache = ProbeCache::default();
        cache.insert(fingerprint(4096), report());
        assert_eq!(cache.lookup(&fingerprint(4096)), Some(&report()));
        assert_eq!(cache.lookup(&fingerprint(4097)), None);
    }

    #[test]
    fn survives_a_save_and_load() -> anyhow::Result<()> {
        let file = std::env::temp_dir()
            .join(format!("crown-voiced-cache-{}", std::process::id()))
            .join("backends.ron");
        let mut cache = ProbeCache::default();
        cache.insert(fingerprint(4096), report());
        cache.save(&file)?;
        let loaded = ProbeCache::load(&file);
        if let Some(dir) = file.parent() {
            std::fs::remove_dir_all(dir)?;
        }
        assert_eq!(loaded, cache);
        Ok(())
    }
}

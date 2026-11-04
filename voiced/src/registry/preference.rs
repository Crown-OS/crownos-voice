//! The installer's preferred backend order from `/etc/crownos/ai.ron`.

use crownos_config::system::ai::{AiConfig, Backend};

/// Voice plugin names, best first, as the installer set them up.
#[derive(Debug, Default)]
pub struct Preference {
    plugins: Vec<&'static str>,
}

impl Preference {
    /// Reads `/etc/crownos/ai.ron`; a missing, unreadable or invalid file means no preference.
    pub fn load() -> Self {
        match AiConfig::load() {
            Ok(Some(config)) => Self::from_config(&config),
            Ok(None) => Self::default(),
            Err(error) => {
                log::warn!("ignoring the AI backend preference: {error}");
                Self::default()
            }
        }
    }

    fn from_config(config: &AiConfig) -> Self {
        let mut plugins = Vec::with_capacity(config.backends.len());
        for plugin in config
            .backends
            .iter()
            .filter_map(|&backend| voice_plugin(backend))
        {
            if !plugins.contains(&plugin) {
                plugins.push(plugin);
            }
        }
        Self { plugins }
    }

    pub fn rank(&self, plugin: &str) -> Option<usize> {
        self.plugins.iter().position(|&listed| listed == plugin)
    }
}

fn voice_plugin(backend: Backend) -> Option<&'static str> {
    match backend {
        Backend::Cpu => Some("cpu"),
        Backend::Cuda | Backend::MiGraphX | Backend::OpenVino | Backend::VitisAi => Some("onnx"),
        Backend::Maxine => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_inference_stacks_to_voice_plugins() {
        let config = AiConfig {
            backends: vec![
                Backend::Maxine,
                Backend::Cuda,
                Backend::OpenVino,
                Backend::Cpu,
            ],
            ..AiConfig::default()
        };
        let preference = Preference::from_config(&config);
        assert_eq!(preference.rank("onnx"), Some(0));
        assert_eq!(preference.rank("cpu"), Some(1));
        assert_eq!(preference.rank("maxine"), None);
        assert_eq!(preference.rank("passthrough"), None);
    }
}

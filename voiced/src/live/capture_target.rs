//! Which microphone a stream captures from when the client names none.

use super::NODE_PREFIX;

/// An `Audio/Source` node as the PipeWire registry lists it.
pub struct SourceNode {
    pub name: String,
    pub priority: i32,
}

/// The first listed default that is a real, present source, else the highest-priority real source.
pub fn capture_target<'a>(
    defaults: &[Option<&'a str>],
    sources: &'a [SourceNode],
) -> Option<&'a str> {
    let real_sources = || {
        sources
            .iter()
            .filter(|source| !is_enhanced_node(&source.name))
    };
    defaults
        .iter()
        .flatten()
        .copied()
        .find(|&name| real_sources().any(|source| source.name == name))
        .or_else(|| {
            real_sources()
                .max_by_key(|source| source.priority)
                .map(|source| source.name.as_str())
        })
}

/// Whether `name` is a node this service created.
pub fn is_enhanced_node(name: &str) -> bool {
    name.strip_prefix(NODE_PREFIX)
        .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::ALL_APPS_NODE;

    fn sources() -> Vec<SourceNode> {
        [
            ("alsa_input.usb-mic", 1009),
            ("crownos-voice.4242.1", 2000),
            ("alsa_input.pci-webcam", 900),
        ]
        .into_iter()
        .map(|(name, priority)| SourceNode {
            name: name.into(),
            priority,
        })
        .collect()
    }

    #[test]
    fn keeps_a_real_default_source() {
        assert_eq!(
            capture_target(&[Some("alsa_input.pci-webcam")], &sources()),
            Some("alsa_input.pci-webcam")
        );
    }

    #[test]
    fn prefers_the_configured_source_over_the_effective_one() {
        assert_eq!(
            capture_target(
                &[Some("alsa_input.pci-webcam"), Some("alsa_input.usb-mic")],
                &sources()
            ),
            Some("alsa_input.pci-webcam")
        );
    }

    #[test]
    fn skips_a_configured_source_that_is_unplugged() {
        assert_eq!(
            capture_target(
                &[Some("alsa_input.unplugged"), Some("alsa_input.pci-webcam")],
                &sources()
            ),
            Some("alsa_input.pci-webcam")
        );
    }

    #[test]
    fn never_captures_from_an_enhanced_node() {
        let sources = sources();
        assert_eq!(
            capture_target(&[Some("crownos-voice.4242.1")], &sources),
            Some("alsa_input.usb-mic")
        );
        assert_eq!(
            capture_target(&[None], &sources),
            Some("alsa_input.usb-mic")
        );
    }

    #[test]
    fn finds_nothing_when_only_enhanced_nodes_exist() {
        let only_enhanced = [SourceNode {
            name: "crownos-voice.1.1".into(),
            priority: 1,
        }];
        assert_eq!(
            capture_target(&[Some("crownos-voice.1.1")], &only_enhanced),
            None
        );
    }

    #[test]
    fn recognises_only_this_services_nodes() {
        assert!(is_enhanced_node("crownos-voice.4242.1"));
        assert!(is_enhanced_node(ALL_APPS_NODE));
        assert!(!is_enhanced_node("crownos-voice-capture"));
        assert!(!is_enhanced_node("alsa_input.usb-mic"));
    }
}

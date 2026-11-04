//! Which microphone a stream captures from when the client names none.

use super::NODE_PREFIX;

/// An `Audio/Source` node as the PipeWire registry lists it.
pub struct SourceNode {
    pub name: String,
    pub priority: i32,
}

/// The default source, or the highest-priority other source when the default is an enhanced node.
pub fn capture_target<'a>(default: Option<&'a str>, sources: &'a [SourceNode]) -> Option<&'a str> {
    default.filter(|name| !is_enhanced_node(name)).or_else(|| {
        sources
            .iter()
            .filter(|source| !is_enhanced_node(&source.name))
            .max_by_key(|source| source.priority)
            .map(|source| source.name.as_str())
    })
}

/// Whether `name` is a node this service created.
pub fn is_enhanced_node(name: &str) -> bool {
    name.strip_prefix(NODE_PREFIX)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// The node name in a `default.audio.source` metadata value, `{"name":"alsa_input.usb"}`.
pub fn metadata_node_name(value: &str) -> Option<&str> {
    let (_, after_key) = value.split_once("\"name\"")?;
    let quoted = after_key.trim_start().strip_prefix(':')?.trim_start();
    let (name, _) = quoted.strip_prefix('"')?.split_once('"')?;
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            capture_target(Some("alsa_input.pci-webcam"), &sources()),
            Some("alsa_input.pci-webcam")
        );
    }

    #[test]
    fn never_captures_from_an_enhanced_node() {
        let sources = sources();
        assert_eq!(
            capture_target(Some("crownos-voice.4242.1"), &sources),
            Some("alsa_input.usb-mic")
        );
        assert_eq!(capture_target(None, &sources), Some("alsa_input.usb-mic"));
    }

    #[test]
    fn finds_nothing_when_only_enhanced_nodes_exist() {
        let only_enhanced = [SourceNode {
            name: "crownos-voice.1.1".into(),
            priority: 1,
        }];
        assert_eq!(
            capture_target(Some("crownos-voice.1.1"), &only_enhanced),
            None
        );
    }

    #[test]
    fn recognises_only_this_services_nodes() {
        assert!(is_enhanced_node("crownos-voice.4242.1"));
        assert!(!is_enhanced_node("crownos-voice-capture"));
        assert!(!is_enhanced_node("alsa_input.usb-mic"));
    }

    #[test]
    fn reads_the_name_from_default_metadata() {
        assert_eq!(
            metadata_node_name(r#"{"name":"alsa_input.pci-0000_06_00.6.analog-stereo"}"#),
            Some("alsa_input.pci-0000_06_00.6.analog-stereo")
        );
        assert_eq!(metadata_node_name(r#"{ "name" : "mic" }"#), Some("mic"));
        assert_eq!(metadata_node_name("{}"), None);
    }
}

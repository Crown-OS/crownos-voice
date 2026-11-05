//! Keys and values of the PipeWire `default` metadata object.

pub const DEFAULT_METADATA: &str = "default";
/// The source the session manager settled on.
pub const DEFAULT_SOURCE_KEY: &str = "default.audio.source";
/// The source the user picked, as `pactl set-default-source` writes it.
pub const CONFIGURED_SOURCE_KEY: &str = "default.configured.audio.source";
/// Moves a stream to another node, as `pactl move-source-output` does.
pub const TARGET_OBJECT_KEY: &str = "target.object";
pub const JSON_TYPE: &str = "Spa:String:JSON";

/// The node name in a default-node value, `{"name":"alsa_input.usb"}`.
pub fn metadata_node_name(value: &str) -> Option<&str> {
    let (_, after_key) = value.split_once("\"name\"")?;
    let quoted = after_key.trim_start().strip_prefix(':')?.trim_start();
    let (name, _) = quoted.strip_prefix('"')?.split_once('"')?;
    Some(name)
}

/// The default-node value naming `node_name`.
pub fn metadata_value(node_name: &str) -> String {
    format!(r#"{{"name":"{node_name}"}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_name_from_default_metadata() {
        assert_eq!(
            metadata_node_name(r#"{"name":"alsa_input.pci-0000_06_00.6.analog-stereo"}"#),
            Some("alsa_input.pci-0000_06_00.6.analog-stereo")
        );
        assert_eq!(metadata_node_name(r#"{ "name" : "mic" }"#), Some("mic"));
        assert_eq!(metadata_node_name("{}"), None);
    }

    #[test]
    fn writes_values_it_reads_back() {
        let value = metadata_value("crownos-voice.all-apps");
        assert_eq!(value, r#"{"name":"crownos-voice.all-apps"}"#);
        assert_eq!(metadata_node_name(&value), Some("crownos-voice.all-apps"));
    }
}

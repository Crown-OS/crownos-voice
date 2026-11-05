//! Keeping the default source on the all-apps node while following the user's microphone choice.

use super::capture_target::is_enhanced_node;

/// The all-apps node and the real microphone it stands in for.
#[derive(Debug)]
pub struct DefaultClaim {
    own_node: String,
    microphone: Option<String>,
}

/// What to do after the configured default source changed.
#[derive(Debug, PartialEq, Eq)]
pub enum ClaimStep {
    /// The default already is the all-apps node.
    Hold,
    /// Point the default back at the all-apps node.
    Reclaim,
    /// The user picked another microphone: capture it, then reclaim the default.
    Retarget,
}

impl DefaultClaim {
    pub fn new(own_node: &str, microphone: Option<&str>) -> Self {
        Self {
            own_node: own_node.to_owned(),
            microphone: microphone.map(str::to_owned),
        }
    }

    pub fn own_node(&self) -> &str {
        &self.own_node
    }

    /// The microphone the all-apps node captures, and the default to restore on release.
    pub fn microphone(&self) -> Option<&str> {
        self.microphone.as_deref()
    }

    /// Reacts to the configured default source becoming `configured`.
    pub fn on_configured(&mut self, configured: Option<&str>) -> ClaimStep {
        match configured {
            Some(name) if name == self.own_node => ClaimStep::Hold,
            Some(name) if !is_enhanced_node(name) && self.microphone() != Some(name) => {
                self.microphone = Some(name.to_owned());
                ClaimStep::Retarget
            }
            _ => ClaimStep::Reclaim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::ALL_APPS_NODE;

    const BUILT_IN: &str = "alsa_input.pci-analog-stereo";
    const HEADSET: &str = "alsa_input.usb-headset";

    fn claim() -> DefaultClaim {
        DefaultClaim::new(ALL_APPS_NODE, Some(BUILT_IN))
    }

    #[test]
    fn claims_the_default_at_start() {
        let mut claim = claim();
        assert_eq!(claim.on_configured(Some(BUILT_IN)), ClaimStep::Reclaim);
        assert_eq!(claim.microphone(), Some(BUILT_IN));
    }

    #[test]
    fn holds_its_own_claim() {
        assert_eq!(claim().on_configured(Some(ALL_APPS_NODE)), ClaimStep::Hold);
    }

    #[test]
    fn follows_the_user_to_another_microphone() {
        let mut claim = claim();
        assert_eq!(claim.on_configured(Some(HEADSET)), ClaimStep::Retarget);
        assert_eq!(claim.microphone(), Some(HEADSET));
        assert_eq!(claim.on_configured(Some(ALL_APPS_NODE)), ClaimStep::Hold);
    }

    #[test]
    fn never_captures_another_enhanced_node() {
        let mut claim = claim();
        assert_eq!(
            claim.on_configured(Some("crownos-voice.4242.1")),
            ClaimStep::Reclaim
        );
        assert_eq!(claim.microphone(), Some(BUILT_IN));
    }

    #[test]
    fn reclaims_a_cleared_default() {
        let mut claim = claim();
        assert_eq!(claim.on_configured(None), ClaimStep::Reclaim);
        assert_eq!(claim.microphone(), Some(BUILT_IN));
    }

    #[test]
    fn learns_the_microphone_when_none_was_known() {
        let mut claim = DefaultClaim::new(ALL_APPS_NODE, None);
        assert_eq!(claim.on_configured(Some(HEADSET)), ClaimStep::Retarget);
        assert_eq!(claim.microphone(), Some(HEADSET));
    }
}

use voice_abi::Model;
use voice_protocol::Enhancement;

/// The network that serves `enhancement`; `None` for [`Enhancement::Off`].
pub fn model_for(enhancement: Enhancement) -> Option<Model> {
    match enhancement {
        Enhancement::Off => None,
        Enhancement::Light => Some(Model::Light),
        Enhancement::NoiseSuppression => Some(Model::Standard),
        Enhancement::StudioVoice => Some(Model::Studio),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_active_enhancement_has_a_model() {
        assert_eq!(model_for(Enhancement::Off), None);
        assert_eq!(model_for(Enhancement::Light), Some(Model::Light));
        assert_eq!(
            model_for(Enhancement::NoiseSuppression),
            Some(Model::Standard)
        );
        assert_eq!(model_for(Enhancement::StudioVoice), Some(Model::Studio));
    }
}

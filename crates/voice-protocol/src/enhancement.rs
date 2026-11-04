use serde::{Deserialize, Serialize};

/// What an app asks the service to do to its microphone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Enhancement {
    /// Raw microphone; the service is not involved.
    Off,
    /// RNNoise-class suppression: lowest cost, steady noise only.
    Light,
    /// DeepFilterNet3-class AI noise suppression.
    #[default]
    NoiseSuppression,
    /// Noise suppression, dereverberation and level control.
    StudioVoice,
}

impl Enhancement {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::Light,
        Self::NoiseSuppression,
        Self::StudioVoice,
    ];

    pub fn is_active(self) -> bool {
        self != Self::Off
    }
}

use std::f32::consts::FRAC_PI_2;

use super::Complex32;

const EPSILON: f32 = 1e-12;

/// DeepFilterNet's post-filter: attenuates bins further where the network already attenuated them.
pub(crate) fn post_filter(noisy: &[Complex32], enhanced: &mut [Complex32], beta: f32) {
    for (noisy, enhanced) in noisy.iter().zip(enhanced) {
        let gain = (enhanced.norm() / (noisy.norm() + EPSILON)).clamp(EPSILON, 1.0);
        let sine_gain = gain * (gain * FRAC_PI_2).sin();
        *enhanced *= (beta + 1.0) / (1.0 + beta * (gain / sine_gain).powi(2));
    }
}

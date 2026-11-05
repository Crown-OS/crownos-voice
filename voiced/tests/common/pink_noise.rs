/// Paul Kellet's economy pink filter: three one-pole sections plus a direct path.
const POLES: [f32; 3] = [0.997_65, 0.963, 0.57];
const POLE_GAINS: [f32; 3] = [0.099_046, 0.296_516_4, 1.052_691_3];
const DIRECT_GAIN: f32 = 0.1848;

/// Deterministic 1/f noise at `rms`, closer to room and fan noise than white noise.
pub fn pink_noise(len: usize, rms: f32) -> Vec<f32> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut sections = [0f32; 3];
    let noise: Vec<f32> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let white = (state >> 40) as f32 / (1u64 << 23) as f32 - 1.0;
            sections
                .iter_mut()
                .zip(POLES.iter().zip(POLE_GAINS))
                .map(|(section, (pole, gain))| {
                    *section = pole * *section + gain * white;
                    *section
                })
                .sum::<f32>()
                + DIRECT_GAIN * white
        })
        .collect();
    let measured =
        (noise.iter().map(|sample| sample * sample).sum::<f32>() / len.max(1) as f32).sqrt();
    let scale = rms / measured.max(f32::MIN_POSITIVE);
    noise.into_iter().map(|sample| sample * scale).collect()
}

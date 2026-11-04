use nnnoiseless::DenoiseState;
use voice_abi::FRAME_LEN;

/// nnnoiseless expects samples in the `i16` range.
const I16_SCALE: f32 = 32_768.0;

const _: () = assert!(DenoiseState::FRAME_SIZE == FRAME_LEN);

/// RNNoise via nnnoiseless: one 10 ms frame in, one out.
pub(crate) struct Rnnoise {
    state: Box<DenoiseState<'static>>,
    scaled_input: [f32; FRAME_LEN],
}

impl Rnnoise {
    pub(crate) fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            scaled_input: [0.0; FRAME_LEN],
        }
    }

    pub(crate) fn process_frame(
        &mut self,
        input: &[f32; FRAME_LEN],
        output: &mut [f32; FRAME_LEN],
    ) {
        for (scaled, sample) in self.scaled_input.iter_mut().zip(input) {
            *scaled = sample * I16_SCALE;
        }
        self.state.process_frame(output, &self.scaled_input);
        output.iter_mut().for_each(|sample| *sample /= I16_SCALE);
    }
}

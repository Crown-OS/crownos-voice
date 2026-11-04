/// A noise suppressor running on whole [`voice_abi::FRAME_LEN`] frames of mono 48 kHz audio.
pub trait Suppressor {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Enhances `input` into `output`; both are the same whole number of frames long.
    fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), Self::Error>;
}

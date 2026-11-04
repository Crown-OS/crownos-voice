use deep_filter_rt::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use voice_abi::{BackendError, FRAME_LEN};

/// libDF's default post-filter strength, used for the Studio profile.
const STUDIO_POST_FILTER_BETA: f32 = 0.02;
const MONO: usize = 1;

/// How hard DeepFilterNet3 suppresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Strength {
    Standard,
    /// Adds libDF's post-filter, which further attenuates bins the network already treats as noise.
    Studio,
}

/// DeepFilterNet3 (the model embedded in `deep-filter-rt`) streaming one hop per frame on tract.
pub(crate) struct DeepFilter {
    model: Box<DfTract>,
}

impl DeepFilter {
    pub(crate) fn new(strength: Strength) -> Result<Self, BackendError> {
        let params = RuntimeParams::default_with_ch(MONO);
        let params = match strength {
            Strength::Standard => params,
            Strength::Studio => params.with_post_filter(STUDIO_POST_FILTER_BETA),
        };
        let model = DfTract::new(DfParams::default(), &params).map_err(failed)?;
        Ok(Self {
            model: Box::new(model),
        })
    }

    pub(crate) fn process_frame(
        &mut self,
        input: &[f32; FRAME_LEN],
        output: &mut [f32; FRAME_LEN],
    ) -> Result<(), BackendError> {
        let noisy = ArrayView2::from_shape((MONO, FRAME_LEN), input.as_slice()).map_err(failed)?;
        let enhanced =
            ArrayViewMut2::from_shape((MONO, FRAME_LEN), output.as_mut_slice()).map_err(failed)?;
        self.model.process(noisy, enhanced).map_err(failed)?;
        Ok(())
    }
}

fn failed(error: impl std::fmt::Display) -> BackendError {
    BackendError::Failed(format!("DeepFilterNet3: {error}"))
}

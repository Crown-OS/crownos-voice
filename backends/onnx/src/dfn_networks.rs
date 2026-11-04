use std::path::Path;

use ort::session::{Session, SessionOutputs};
use ort::value::{DynValue, TensorRef};
use voice_abi::BackendError;
use voice_engine::dfn::{DF_BINS, DF_COEFS_LEN, ERB_BANDS};

use crate::execution_providers::execution_providers;
use crate::network_spec::{DF_DECODER, ENCODER, ERB_DECODER, NetworkSpec};
use crate::provider::Provider;
use crate::runtime;
use crate::util::backend_error::failed;

/// The three DeepFilterNet3 graphs, all on one execution provider.
pub(crate) struct DfnNetworks {
    encoder: Session,
    erb_decoder: Session,
    df_decoder: Session,
}

/// Per-step outputs of one sequence, indexed by time step.
pub(crate) struct Estimates<'a> {
    pub(crate) lsnr: &'a [f32],
    pub(crate) erb_gains: &'a [[f32; ERB_BANDS]],
    pub(crate) df_coefs: &'a [[f32; DF_COEFS_LEN]],
}

impl DfnNetworks {
    /// Maps ONNX Runtime only once all three graphs are known to exist.
    pub(crate) fn load(dir: &Path, provider: Provider) -> Result<Self, BackendError> {
        if let Some(missing) = [&ENCODER, &ERB_DECODER, &DF_DECODER]
            .iter()
            .map(|network| dir.join(network.file))
            .find(|path| !path.is_file())
        {
            return Err(BackendError::Unavailable(format!(
                "model {}",
                missing.display()
            )));
        }
        runtime::load()?;
        Ok(Self {
            encoder: session(dir, &ENCODER, provider)?,
            erb_decoder: session(dir, &ERB_DECODER, provider)?,
            df_decoder: session(dir, &DF_DECODER, provider)?,
        })
    }

    /// Runs `steps` frames of time-major `feat_erb [S][32]` and planar `feat_spec [re|im][S][96]`,
    /// then hands the estimates of every step to `consume`.
    pub(crate) fn run<R>(
        &mut self,
        steps: usize,
        feat_erb: &[f32],
        feat_spec: &[f32],
        consume: impl FnOnce(&Estimates<'_>) -> R,
    ) -> Result<R, BackendError> {
        let feat_erb =
            TensorRef::from_array_view(([1, 1, steps, ERB_BANDS], feat_erb)).map_err(failed)?;
        let feat_spec =
            TensorRef::from_array_view(([1, 2, steps, DF_BINS], feat_spec)).map_err(failed)?;
        let encoded = self
            .encoder
            .run(ort::inputs!["feat_erb" => feat_erb, "feat_spec" => feat_spec])
            .map_err(failed)?;
        let gains = self
            .erb_decoder
            .run(ort::inputs![
                "emb" => output(&encoded, "emb")?,
                "e3" => output(&encoded, "e3")?,
                "e2" => output(&encoded, "e2")?,
                "e1" => output(&encoded, "e1")?,
                "e0" => output(&encoded, "e0")?,
            ])
            .map_err(failed)?;
        let coefs = self
            .df_decoder
            .run(ort::inputs![
                "emb" => output(&encoded, "emb")?,
                "c0" => output(&encoded, "c0")?,
            ])
            .map_err(failed)?;
        let estimates = Estimates {
            lsnr: values(output(&encoded, "lsnr")?)?,
            erb_gains: values(first_output(&gains)?)?.as_chunks().0,
            df_coefs: values(first_output(&coefs)?)?.as_chunks().0,
        };
        if estimates.lsnr.len() < steps
            || estimates.erb_gains.len() < steps
            || estimates.df_coefs.len() < steps
        {
            return Err(failed(format!(
                "the networks returned fewer than {steps} steps"
            )));
        }
        Ok(consume(&estimates))
    }
}

fn session(dir: &Path, network: &NetworkSpec, provider: Provider) -> Result<Session, BackendError> {
    Session::builder()
        .map_err(failed)?
        .with_execution_providers(execution_providers(provider, network))
        .map_err(failed)?
        .commit_from_file(dir.join(network.file))
        .map_err(failed)
}

fn output<'a>(outputs: &'a SessionOutputs<'_>, name: &str) -> Result<&'a DynValue, BackendError> {
    outputs
        .get(name)
        .ok_or_else(|| failed(format!("the encoder has no output `{name}`")))
}

fn first_output<'a>(outputs: &'a SessionOutputs<'_>) -> Result<&'a DynValue, BackendError> {
    outputs
        .keys()
        .next()
        .and_then(|name| outputs.get(name))
        .ok_or_else(|| failed("a decoder returned no output"))
}

fn values(value: &DynValue) -> Result<&[f32], BackendError> {
    Ok(value.try_extract_tensor::<f32>().map_err(failed)?.1)
}

use std::io;

use voice_abi::Model;

use super::lookahead_normalizer::LookaheadNormalizer;
use super::pcm_sink::SampleSink;

/// What happens to aligned, enhanced audio before it leaves the job.
pub enum Finishing {
    AsEnhanced,
    LoudnessNormalized(Box<LookaheadNormalizer>),
}

impl Finishing {
    pub fn for_model(model: Model) -> Self {
        match model {
            Model::Studio => Self::LoudnessNormalized(Box::new(LookaheadNormalizer::new())),
            Model::Light | Model::Standard => Self::AsEnhanced,
        }
    }

    pub fn write(&mut self, samples: &[f32], sink: &mut impl SampleSink) -> io::Result<()> {
        match self {
            Self::AsEnhanced => sink.write(samples),
            Self::LoudnessNormalized(normalizer) => normalizer.push(samples, sink),
        }
    }

    pub fn finish(&mut self, sink: &mut impl SampleSink) -> io::Result<()> {
        match self {
            Self::AsEnhanced => Ok(()),
            Self::LoudnessNormalized(normalizer) => normalizer.finish(sink),
        }
    }
}

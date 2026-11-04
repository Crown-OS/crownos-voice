use voice_engine::dfn::{
    CONV_CHANNELS, DF_BINS, DF_DECODER_FILE, EMBEDDING_LEN, ENCODER_FILE, ERB_BANDS,
    ERB_DECODER_FILE,
};

/// Longest time axis `S` of one network call; caps the encoder outputs at about 90 MB.
pub(crate) const MAX_SEQUENCE_FRAMES: usize = 2048;

/// One network input: `[1, channels, S, width]`, or `[1, S, width]` without channels.
pub(crate) struct InputShape {
    name: &'static str,
    channels: Option<usize>,
    width: usize,
}

/// One graph of the official DeepFilterNet3 export.
pub(crate) struct NetworkSpec {
    pub(crate) file: &'static str,
    inputs: &'static [InputShape],
}

const fn planes(name: &'static str, channels: usize, width: usize) -> InputShape {
    InputShape {
        name,
        channels: Some(channels),
        width,
    }
}

const fn rows(name: &'static str, width: usize) -> InputShape {
    InputShape {
        name,
        channels: None,
        width,
    }
}

pub(crate) const ENCODER: NetworkSpec = NetworkSpec {
    file: ENCODER_FILE,
    inputs: &[
        planes("feat_erb", 1, ERB_BANDS),
        planes("feat_spec", 2, DF_BINS),
    ],
};

pub(crate) const ERB_DECODER: NetworkSpec = NetworkSpec {
    file: ERB_DECODER_FILE,
    inputs: &[
        rows("emb", EMBEDDING_LEN),
        planes("e3", CONV_CHANNELS, ERB_BANDS / 4),
        planes("e2", CONV_CHANNELS, ERB_BANDS / 4),
        planes("e1", CONV_CHANNELS, ERB_BANDS / 2),
        planes("e0", CONV_CHANNELS, ERB_BANDS),
    ],
};

pub(crate) const DF_DECODER: NetworkSpec = NetworkSpec {
    file: DF_DECODER_FILE,
    inputs: &[
        rows("emb", EMBEDDING_LEN),
        planes("c0", CONV_CHANNELS, DF_BINS),
    ],
};

impl NetworkSpec {
    /// Input shapes at `steps` frames in TensorRT's profile syntax, e.g. `emb:1x300x512,c0:1x64x300x96`.
    pub(crate) fn tensor_rt_shapes(&self, steps: usize) -> String {
        let shapes: Vec<String> = self
            .inputs
            .iter()
            .map(|input| match input.channels {
                Some(channels) => format!("{}:1x{channels}x{steps}x{}", input.name, input.width),
                None => format!("{}:1x{steps}x{}", input.name, input.width),
            })
            .collect();
        shapes.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tensor_rt_shapes_list_every_input() {
        assert_eq!(
            ENCODER.tensor_rt_shapes(7),
            "feat_erb:1x1x7x32,feat_spec:1x2x7x96"
        );
        assert_eq!(DF_DECODER.tensor_rt_shapes(1), "emb:1x1x512,c0:1x64x1x96");
        assert_eq!(
            ERB_DECODER.tensor_rt_shapes(2),
            "emb:1x2x512,e3:1x64x2x8,e2:1x64x2x8,e1:1x64x2x16,e0:1x64x2x32"
        );
    }
}

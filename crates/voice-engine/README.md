# crownos-voice-engine (`voice_engine`)

Pure DSP shared by `crown-voiced` and the enhancement backends. Mono `f32` at 48 kHz, frames of
`voice_abi::FRAME_LEN` (480) samples, no allocation once warmed up. No dlopen dependency; feature
`host` adds `impl Suppressor for voice_abi::host::BackendInstance`.

| Type | Purpose |
|---|---|
| `FrameAccumulator` / `FrameDispenser` | arbitrary chunks → whole frames, and back |
| `HighPass` | 80 Hz Butterworth rumble filter |
| `LevelControl` | gentle AGC towards -20 dBFS (±12 dB, gated below -50 dBFS) + -1 dBFS peak limiter |
| `Loudness` | EBU R128 / BS.1770 integrated loudness and the gain to `TARGET_LUFS` (-16) |
| `Suppressor`, `Chain<S>` | high-pass → suppressor → `LevelControl` (Studio only) |
| `dfn` | DeepFilterNet3 features and post-network enhancement for ONNX backends |

## DeepFilterNet on ONNX Runtime

### Findings (2026-11-04)

- **`deep_filter` on crates.io is 0.2.5 from 2022** (DeepFilterNet2 era, no tract path). The libDF
  that runs DFN3 with tract (`DfTract`, `DFState`) is only in git (tag `v0.5.6`, a 180 MB repo).
  **`deep-filter-rt` 0.1.1** (crates.io, 2026-11-01) vendors libDF at upstream `d375b2d`, ported to
  tract 0.23 and ndarray 0.17, with the DFN3 model embedded. Its four model files are byte-identical
  to upstream's `DeepFilterNet3_onnx.tar.gz` (sha256 checked). The CPU backend uses it.
- `DFState` exposes the STFT (`analysis`, `synthesis`), `feat_erb`, `feat_cplx`, `apply_mask` and
  `post_filter`. Upstream's version allocates on every frame. `voice_engine::dfn` reimplements these
  allocation-free. The test `backends/cpu/tests/dfn_parity.rs::features_match_libdf` checks them
  against `deep-filter-rt` (max abs error < 1e-4).
- The official export is three graphs with a dynamic time axis `S`:

  | file | inputs | outputs |
  |---|---|---|
  | `enc.onnx` | `feat_erb [1,1,S,32]`, `feat_spec [1,2,S,96]` | `e0 [1,64,S,32]`, `e1 [1,64,S,16]`, `e2 [1,64,S,8]`, `e3 [1,64,S,8]`, `emb [1,S,512]`, `c0 [1,64,S,96]`, `lsnr [1,S,1]` |
  | `erb_dec.onnx` | `emb, e3, e2, e1, e0` | `m [1,1,S,32]` (ERB gains) |
  | `df_dec.onnx` | `emb, c0` | `coefs [1,S,96,10]` (`[bin][tap][re,im]`) |

  The convolutions are causal (zero-padded in time) and the GRU hidden states are **not** graph
  inputs or outputs. libDF can stream frame by frame only because `tract-pulse` rewrites the graphs
  (`PulsedModel`, pulse 1) so that padding and recurrence keep internal state. ONNX Runtime has no
  equivalent.
- **Real time on ORT with the official files is not feasible.** Every call starts the GRUs from
  zero, and a call with `S = 1` gives wrong output. Streaming would need a stateful re-export, with
  GRU `h` and the convolution history exposed as inputs and outputs (third-party exports of that
  kind exist, for example torchDF), and that is outside this scope. A GPU round trip per 10 ms frame
  also loses to the CPU (plan §11). Real time therefore stays on `libvoice_cpu` (DFN3 on tract,
  about 0.1–0.2 ms per frame).
- **Offline on ORT is feasible and verified, as one sequence per job.** The graphs are strictly
  causal: truncating the sequence leaves every earlier output bit-identical. Running a whole
  sequence through the stateless graphs (tract without pulsing, standing in for ORT), plus
  `voice_engine::dfn`, reproduces the streaming `DfTract` output to within **-32 dB**
  (`offline_recipe_reproduces_streaming_model`).
- **Chunking is only approximate.** A sequence that starts cold never converges back to the
  warm-state output: 2.5 s after a cold start, `lsnr` still differs by up to 5 dB and the ERB gains by
  about 0.05. Prepending earlier frames as context does not fix this reliably; on the 5 s test
  signal the result ranged from -10 to -31 dB depending on where the cold start fell. Chunked output
  still suppresses noise (see the test), but it is not bit-faithful.

### Recipe for `backends/onnx` (offline workload)

1. Load the sessions from `models_dir/dfn::MODEL_DIR_NAME/{ENCODER_FILE, ERB_DECODER_FILE,
   DF_DECODER_FILE}` (installed by `packaging/fetch-models.sh`). Bind inputs and outputs by the names
   in the table above. If the files are missing, return `BackendError::Unavailable`. Give
   `Workload::Realtime` a probe score of 0 (the CPU backend serves it).
2. Per instance keep one `dfn::FeatureExtractor`, one `dfn::Enhancer` (for Studio use
   `.with_post_filter(0.02)`), and a buffer of the features of the last `CONTEXT_FRAMES` hops.
3. `process(input, output)` with N frames:
   1. For each frame, call `extractor.extract(frame)` and append the result after the retained
      context. Write `erb` time-major into `feat_erb[0,0,t,..]`. Split `complex` into planes:
      `feat_spec[0,0,t,f] = re` and `feat_spec[0,1,t,f] = im`.
   2. Run `enc` over `S = context + N`. Then run `erb_dec(emb, e3, e2, e1, e0)` and
      `df_dec(emb, c0)`.
   3. For frame `i` with `t = context + i`, call `enhancer.enhance(&input_frame[i],
      &NetworkOutput { lsnr: lsnr[t], erb_gains: &m[t*32..][..32], df_coefs: &coefs[t*960..][..960] },
      &mut output_frame[i])`. The enhancer applies the step-`t` outputs to the spectrum
      `LOOKAHEAD_FRAMES` older, so no future frames are needed.
   4. Keep the last `CONTEXT_FRAMES` (suggested: 200, 2 s) feature frames for the next call.
4. For exact results the daemon hands a whole job to one `process` call. The encoder outputs alone
   take about 43 KB per frame (4.3 MB per second of audio), plus activations. Past roughly a minute
   of audio, split the job into calls of 30–60 s and accept the approximation above.
5. The output lags the input by `dfn::LATENCY_SAMPLES` (1440 samples, 30 ms), exactly like the CPU
   backend. An offline job should append 3 frames of silence and drop the first 1440 output samples.
6. The TensorRT and MIGraphX EPs want static or profiled shapes: give TensorRT an optimisation
   profile over `S`, or zero-pad every call to a fixed `S`.

### What would make ORT real-time and exact

A stateful export: the GRU hidden states and the causal convolution histories as explicit inputs
and outputs, run with `S = 1`. Third-party single-graph exports of this kind exist, for example
the torchDF single-frame export (`input_frame`, `states` → `enhanced_audio_frame`, `new_states`),
which the crates.io crate `anr-ort` runs in ORT. That export includes the STFT, so it would not need
`dfn` at all. They are not part of the official release and are not fetched
here.

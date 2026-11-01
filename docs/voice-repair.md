# Voice repair: clipped and distorted speech

`replay.voice_repair` repairs the separated voice where the streamer's speech was clipped or
codec-damaged at broadcast time. It runs as the recording pipeline's `voice-repair` step, between
`separation` and `paging`. It is computation only and runs on the CPU, with no GPU lease, in
`edits/.venv-voice-repair` (`--extra voice-repair`), and only when `captions.json` holds
`"voiceRepair": true`.

## Where each consumer reads the voice

| Consumer | Voice | Why |
| --- | --- | --- |
| `asr` (mono PCM), hallucination filter | separated | Recognition is safer on the original |
| `cut-plan` (quiet cuts, lineage) | separated | Cuts are measured on energy, and the lineage binds the ASR mono |
| `paging` tempo audio, then `onsets`, `compose`, `finish` and the loudness gain | repaired | This is the delivered voice |
| `deliveryAudio: original-mix` | none | The mix is delivered as it is |

## Analysis: CPU features and Polars tables

The mix is decoded once: separation's own decode (`delivery_audio.input_command`, int16,
sample-aligned with the voice).
- That one pass builds the amplitude histogram, keeps every sample at half scale or above, and
  measures the mix's per-frame waveform features. A ceiling below half scale decodes once more.
- The voice's frame features are numpy, 20 ms frames and two-minute blocks
  (`replay.voice_repair_features`).
- Everything after that is lazy Polars with no per-row Python: robust statistics, both kinds of
  threshold, run grouping, segment merging and the receipt's summary.

On 10/06 the CPU features took 3.9 s per 1.5 h, against 1.9 s on MLX. That gain does not justify
waiting for a GPU lease, so the analysis runs on the CPU. Detection takes 14–19 s per audio hour
(10/06 13.7, 10/05 18.7, 10-14b 15.2), against 31 s per hour for the first version, which decoded
twice.

## Detectors: absolute criterion or relative one

A detector flags an active frame (20 dB above the recording's 10th-percentile level) when either
its fixed criterion or its relative criterion holds, for at least 3 consecutive frames. Every
fixed value sits beyond the extremes of the clean 10-14b recording: its 99.99th or 0.1st
percentile over active frames, measured once, not per video. The relative criteria are:
- a robust z ≥ 4 against the recording's clean reference frames (active, not clipped, no fixed
  flag) for continuous features;
- 3× the reference's 99th percentile for count features, which are mostly zero.

| Kind | Detector | Signal | Fixed | Relative | Repair |
| --- | --- | --- | --- | --- | --- |
| clipping | full scale / ceiling | mix | ≥ 3 samples at ≥ 0.999 FS | ≥ 3 samples at the measured histogram ceiling | AR |
| clipping | plateau runs (any amplitude) | mix | ≥ 30 plateau samples a frame (≥ 5 near-equal samples within 0.02% of the frame peak, in its top half) | 3× p99 | AR (its runs) |
| clipping | segment-local pile-up | mix | ≥ 15% of samples within 1% of the frame peak | 3× p99 | detect only |
| clipping | AAC ringing | mix | ≥ 40 local maxima between 90% and 99% of the frame peak | 3× p99 | detect only |
| soft clipping | crest collapse | voice | crest < 3.3 dB | z ≤ −4 | detect only |
| soft clipping | odd-harmonic excess | voice, voiced frames | harmonics 3/5/7 over 2/4/6 > 15 dB | z ≥ 4 | detect only |
| soft clipping | low kurtosis | voice | < 1.0 | z ≤ −4 | detect only |
| robotic | frozen spectrum | voice | flux < 0.05 | log flux z ≤ −4 | detect only |
| robotic | noise collapse | voice, between voiced frames | harmonicity < 0.2 and flatness > −1 | z ≤ −4 and ≥ +4 | detect only |
| robotic | band holes | voice | ≥ 50% of band bins 40 dB below the median | 3× p99 | detect only |
| robotic | frozen pitch | voice, voiced frames | the same pitch lag for ≥ 10 frames | none (fixed only) | detect only |

Not implemented: cepstral-peak and modulation-spectrum anomalies. Computation cannot repair the
detect-only types: they have no runs to interpolate.

### Precision and recall on synthetic fixtures

The fixtures are 30 s of source-filter speech (a glottal pulse train through three formants,
with fricatives), with defects in four intervals. The table gives frame-level precision /
recall, counted on active frames (`edits/work/voicefix/detector_eval.py`).

| Fixture (defect seconds) | Detectors that fired (precision / recall) |
| --- | --- |
| clean (0) | none (no false positives) |
| hard clip → −6 dB → AAC 128k (3.7) | crest 1.00 / 0.05 only. Ringing and pile-up fall within the clean ranges once AAC smooths the plateau |
| hard clip → −6 dB, PCM (3.7) | ceiling 1.00 / 0.63, plateau 1.00 / 0.61, pile-up 1.00 / 0.51, crest 1.00 / 0.23 |
| soft tanh (3.7) | crest 1.00 / 0.64, ceiling 0.91 / 0.66, pile-up 1.00 / 0.51, plateau 1.00 / 0.44 |
| packet-loss frame repeats (5.5) | frozen 1.00 / 0.98 |
| vocoder-like monotone robot (3.8) | frozen pitch 1.00 / 0.85 |
| packet-loss noise bursts (3.8; noise covers 30% of it) | collapse 0.92 / 0.06 |

### Hit seconds on real recordings

| Detector | 10/06 | 10/05 | 10-14b (clean) |
| --- | --- | --- | --- |
| full scale / ceiling | 182.3 | 203.1 | 0 |
| plateau runs | 0 | 0 | 1.3 |
| pile-up | 0.3 | 6.7 | 0 |
| ringing | 0 | 2.5 | 0 |
| crest | 0.6 | 5.4 | 1.5 |
| odd harmonics | 9.7 | 2.5 | 0.5 |
| low kurtosis | 0 | 0 | 0.7 |
| frozen / collapse | 0 / 0 | 0 / 0 | 0 / 0 |
| band holes | 0 | 0 | 0.2 |
| frozen pitch | 2.0 | 1.9 | 1.1 |

## Repair: computation only

1. **AR.** Every segment gets Janssen AR interpolation (order 32) of its clipped and plateau
   samples. Each estimate is projected to stay consistent with the observation: same sign, and
   never smaller than the observed magnitude. An estimate beyond twice the window's peak, or one
   that would reach full scale, keeps the observation.
2. **Natural compressor** (`replay.voice_compressor`), over the declipped segment:
   - a 5 ms look-ahead peak detector on the louder channel;
   - a soft-knee gain computer, 8 dB knee, ratio 2.5;
   - the threshold is the recording's own 99th-percentile 60 ms peak minus 6 dB (−6.0 dBFS on
     10/06, whose peaks reach full scale);
   - one-pole smoothing, 8 ms attack and 180 ms release, at a 1 ms control rate;
   - make-up gain so the segment's K-weighted BS.1770 loudness (ungated, since a segment is
     shorter than a gating block) matches the original's within 0.5 LU;
   - a −1 dBFS safety limiter, used only when a peak would pass it.
3. **Crossfade.** The segment is crossfaded in with 10 ms ramps, then gated.

The MossFormer2 path was removed on 2026-10-29: the user heard it as unnatural and
over-compressed. It also held 17.5 GB, against 4 GiB for the computation-only step.

The Metal AR was measured and not adopted (`edits/work/voicefix/voice_repair_metal_experiment.py`).
It batches every window as normal equations, on 2000 windows with 5% of samples clipped:
- the CPU float64 path took 3.63 s;
- MLX with float32 solves took 0.78 s warm, but about 5% of windows deviated by more than 1e-3;
- MLX with float64 solves on its CPU stream took 2.14 s, and 885 of 2000 windows still deviated
  by more than 1e-4 (maximum 8.6e-3).

It misses the 1e-4 criterion, so AR stays on the CPU. Gains, crossfades and quality measurement
are a few milliseconds per segment and stay numpy.

## Gate: both criteria must pass

The gate measures the segment before and after the repair, over the same active frames.

- **Absolute.** All of these must hold:
  - at least one metric improves by its fixed margin: flat tops by ≥ 8 samples, harmonicity by
    ≥ 0.3 dB, or flatness by ≥ 0.05;
  - harmonicity drops by at most 0.5 dB, and flatness rises by at most 0.1;
  - there are no more flat-top or full-scale samples;
  - WavLM speaker similarity is at least 0.86.
- **Relative.** The same improvement must also hold against the segment itself or the recording:
  - flat tops fall by at least 25% of their own count; or
  - the harmonicity or flatness change exceeds 2 standard errors of the segment's mean, computed
    from the recording's robust frame spread;
  - and neither metric worsens beyond those 2 standard errors.
- **Dynamics.** Added after the listening test:
  - the crest factor falls by at most 1.5 dB;
  - the LRA-style short-term range (p95 − p10 of 200 ms levels) changes by at most 1.5 dB;
  - no peak goes above the AR candidate's own.

Noise-level changes, such as flatness −0.011 or flat tops 4→4, fail the absolute margins.

After the kept segments are written into a copy, the copy must match the input in length,
channels and rate, and be bit-identical outside the kept segments
(`differingOutsideSegments: 0`). When nothing is kept, no copy is written. The receipt records:
- every decision, with a Polars summary by stage, outcome and reason;
- `keptSeconds` and `keptShare`;
- the wall time of each phase.

## ML evaluation (2026-10-28, not used since 2026-10-29)

Every candidate ran after the AR repair, which is the production order.

- Test material: a clean 10 s excerpt from 10/06, artificially clipped.
- Scoring: against the clean original on the mid channel. LSD is measured from 100 to 8000 Hz over
  active frames, and similarity is WavLM-SV on MPS.

| Case | Method | SNR dB | LSD dB | HNR dB | Flatness | Similarity |
| --- | --- | --- | --- | --- | --- | --- |
| Mild (clipped at 50% of peak) | clipped | 22.22 | 1.43 | 9.09 | −7.38 | 0.9986 |
| | AR | **29.73** | **0.91** | 9.09 | −7.47 | **0.9999** |
| | AR + MossFormer2 | 29.70 | 2.92 | **9.16** | **−7.80** | 0.9997 |
| | AR + DeepFilterNet3 (MLX) | 21.81 | 1.87 | 8.30 | −7.30 | 0.9997 |
| | AR + VoiceFixer (MPS) | −2.39 | 12.89 | 6.70 | −5.58 | 0.9854 |
| Severe (clipped at 20% of peak) | clipped | 7.90 | 9.35 | 8.96 | −6.16 | 0.9848 |
| | AR | 11.29 | 7.46 | 8.93 | −6.66 | 0.9954 |
| | AR + MossFormer2 | **11.56** | **6.73** | **9.21** | **−7.62** | **0.9966** |
| | AR + DeepFilterNet3 | 9.98 | 7.57 | 8.87 | −7.33 | 0.9947 |
| | AR + VoiceFixer | −1.69 | 11.60 | 7.23 | −6.16 | 0.9752 |

**MossFormer2 SE 48K MLX was used until 2026-10-29, then removed after listening.**

- Source: starkdmi/MossFormer2_SE_48K_MLX, commit `ccd0ded0`, Apache-2.0.
- Files: `edits/models/mossformer2-se-48k-mlx-ccd0ded0/model_fp32.safetensors`, sha256 `8e47b75c…1313`. Deleted on 2026-10-31 at the user's request: a denoiser cannot restore over-suppressed speech (applied to the source mix it deepened the suppression from −25.6 to −26.7 dB).
- Performance: 0.10 s per audio second on MLX, 1.6 GB resident.
- It is the only candidate that improves on AR, and only for severe clipping. That is why the model
  runs only on segments that AR does not pass.
- The rejected candidates:
  - DeepFilterNet3 (MLX, MIT): worse than AR in both cases.
  - VoiceFixer: resynthesizes the voice (SNR ≈ −2 dB), and its weights are CC-BY-4.0.
  - Sidon (TorchScript): runs on the CPU only; it was later tried, failed by ear and deleted.
- The evaluation files are under `edits/work/voicefix/` (`eval_models.py` and `eval-models-*.json`).

## Full-length run (10/06, 2026-10-29, nice 15, with the since-removed model path)

- **Wall time:** 990 s for 1.49 h of audio, about 664 s per audio hour: detection 40 s, AR 272 s,
  model 675 s.
- **Model use:** MossFormer2 processed 945 s, 17.6% of the recording, in 1,369 spans and 63 calls
  of 18 s batches. Its Metal peak was 10.8 GB and the process footprint 17.5 GB. That is why
  batches are now 6 s and the admission reservation is 12 GiB.
- **Results:** 2,110 segments. AR alone passed none. Seven segments (1.72 s) passed with the model
  and were kept. On those, harmonicity rose 0.61 dB, flatness fell 0.54, flat tops fell by 13.7
  per segment, crest rose 0.49 dB and peaks fell slightly. The minimum similarity was 0.93.
- **Rejections:** 1,855 segments failed on the recording-relative margin and 236 on the absolute
  margins; the rest on flat tops, flatness, full scale, crest and loudness range.
- **Verification:** `differingOutsideSegments` is 0, and the copy has the same frame count
  (236,746,676).
- **Listening crops** of three kept segments, before and after:
  `<replay-work>/voicefix-full-<date>/<replay>/listen/`.
  All three sit at full-scale voice.

## Operation

- Pipeline: put `"voiceRepair": true` in the defaults' `captions` section so new recordings
  receive it. Do this only after any queue runner or driver started on older code has restarted:
  their `load_defaults` refuses the unknown key and stops. A recording prepared earlier keeps its
  `captions.json`. To opt in a named recording, add the key to its `captions.json` before `paging`
  runs.
- Legacy receipts:
  - A paging receipt bound before the step existed stays valid (`LEGACY_DEPENDENCIES`).
  - An orchestrator started before 2026-10-28 never schedules `voice-repair`, so paging then binds
    only `cut-plan` (`ADDED_UPSTREAM`).
  - When `voiceRepair` is set and the voice-repair receipt is missing, paging refuses to run.
- Environment: create it with
  `UV_PROJECT_ENVIRONMENT=edits/.venv-voice-repair uv sync --locked --extra voice-repair --no-dev --no-install-project`.
  It holds mlx and mlx-metal 0.32.4, mlx-audio 0.4.3, torch 2.14.1, transformers 5.17.0 and
  polars 2.0.0. It never touches the shared `.venv`.
- Inference is offline: `HF_HUB_OFFLINE` and `TRANSFORMERS_OFFLINE` are set, and the model digests
  are checked before loading. When the Metal or MPS device is unavailable, the step refuses to run
  rather than falling back to the CPU.

## The A clip with the compressor (10/06, 3765–3775 s)

`listen-target/A_1006_音割れ_3765秒/A5_計算+自然なコンプ_本番.m4a` holds the production
candidate, AR plus the compressor, for every detected segment in the clip, loudness-matched to
A1 (−14.2 LUFS).
- The gate kept none of its 9 segments: 8 failed the absolute improvement margins, 1 because
  flatness rose. The step's own output for this clip is therefore the original.
- Clip crest rose from 10.4 to 12.0 dB: AR restores crests and the compressor limits them, with
  no flattening.
- Clip loudness fell 1.6 LU before matching. The original sits clipped at 0 dBFS, so restoring
  its crests under the −1 dBFS ceiling cannot keep its loudness.

## Computed judge

`replay.voice_judge` decides by computation, with no listening, whether a processing stage
damaged the voice. It compares 24 eight-second windows of the stage's output with the input they
came from. For a cut-slot tempo stage the windows are the longest stretched slots, and the input
is retimed at each window's exact source position by the same band-limited interpolation that
`replay.voice_tempo` uses. A window fails on any limit:

| Measure | Limit | Source |
| --- | --- | --- |
| 10 ms block SNR against the best-lag 33-tap least-squares fit of the input, median / p10 | ≥ 30 / ≥ 8 dB | WSOLA splices (Verhelst & Roelands 1993) |
| Vertical phase-coherence drop at spectral peaks (zero-phase Hann) | ≤ 0.1 | Laroche & Dolson 1999, phasiness |
| Spectral-flux bursts beyond the input's | ≤ 0.75/s | Spectral-flux discontinuity |
| Pitch-track breaks over 4 semitones beyond the input's | ≤ 1/s | F0 continuity |
| AR(24) residual outliers over 8 robust deviations, twice the input's within 1 ms | ≤ 0.3/s | Vaseghi & Rayner 1990; Godsill & Rayner 1998 |
| Samples in flat runs at the window peak beyond the input's | ≤ 0.5 % | Clipping |

The limits are fixed. `tests/test_voice_judge.py` puts each one between the worst must-pass and
the best must-fail synthetic case:
- Must pass: gain, the natural compressor, the de-esser, and resampling at 1.00001 and at 2 %.
- Must fail: 2 clicks/s, clipping at 40 % of the peak, and FFmpeg `atempo` at 1.00001 and at 2 %.

The limits were not tuned on any recording.

### 10/28 evaluation (2026-10-29)

Medians over 24 windows:

| Stage against its input | Windows failed | SNR p50 / p10 (dB) | New clicks /s |
| --- | --- | --- | --- |
| De-esser (separated → de-essed) | 0 / 24 | 120 / 50 | 0 |
| Paging tempo, FFmpeg `atempo` | 24 / 24 | 17 / 2.0 | 5.0 |
| `replay.voice_tempo` resampling | 0 / 24 | 82 / 68 | 0 |

The tempo ratios are clock corrections near 1.00001, so the resampled pitch shift is 0.2 cents at
the median and 34 cents at most. `atempo` still re-cuts every stretched slot, which is 67 % of
the output.

Absolute AR-residual outliers are 11–15/s at every stage, from the source mix to the tempo
output. They are mostly glottal: 74 % fall in voiced frames, which are 55 % of the frames, and
62 % of the intervals between them are whole pitch periods. So no source click repair is applied.

## ML resynthesis: removed (2026-10-31)

Sidon v0.1 replaced 461 windows of the delivered 10/28 voice (the `restore` component of
`voice-remux`). NISQA rose and WavLM speaker similarity stayed at 0.96–0.97, yet the user heard
delivered 0:28–0:31, 0:45–0:48 and 0:52–1:00 as extremely unnaturally processed. The 10/29 trial
(0:51–1:17) was rejected the same way. Measured against the voice before it:

| Pair | Timbre shift (LTAS, dB RMS) | >8 kHz energy | Ear verdict |
| --- | --- | --- | --- |
| De-esser | 0.06 | −0.01 dB | approved |
| Natural compressor | 0.61 | +3.1 dB | not judged |
| MossFormer2 | 1.80 | +1.5 dB | unnatural |
| Sidon, 4 NG spans | 3.3–4.9 | +2.5 to +3.4 dB | extremely unnatural |

Sidon re-colours the voice (a different long-term spectrum) and adds high-band energy the
broadcast never had, while speaker embeddings stay close. `replay.voice_judge` now fails a stage
whose timbre shift exceeds `TIMBRE_SHIFT_DB`. The Sidon, VoiceFixer and DeepFilterNet3 weights
were deleted; the NG spans are recorded in `edits/work/voicefix/ng-spans.json`.

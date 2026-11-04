//! Converts whisper-large-v3-turbo safetensors (openai or MLX layout) into the ggml file whisper.cpp
//! loads, so a downloaded or already-present checkpoint is usable without any Python.

use crate::error::{io_context, AppError, AppResult};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const GGML_MAGIC: u32 = 0x6767_6d6c;
const N_VOCAB: i32 = 51866;
const N_AUDIO_CTX: usize = 1500;
const N_STATE: usize = 1280;
const N_HEAD: i32 = 20;
const N_AUDIO_LAYER: i32 = 32;
const N_TEXT_CTX: i32 = 448;
const N_TEXT_LAYER: i32 = 4;
const N_MELS: usize = 128;
const N_FFT_BINS: usize = 201;
const FTYPE_F16: i32 = 1;
const TENSOR_F32: i32 = 0;
const TENSOR_F16: i32 = 1;
const SAMPLE_RATE: f64 = 16_000.0;
const FFT_SIZE: f64 = 400.0;
const COPY_BUFFER_BYTES: usize = 1 << 20;
const MEL_BREAK_HZ: f64 = 1000.0;
const MEL_AT_BREAK: f64 = 15.0;
const MEL_LINEAR_SLOPE: f64 = 3.0 / 200.0;
/// Slaney's 27 mels per ln(6.4), the librosa default Whisper's filters were built with.
const MEL_LOG_SCALE: f64 = 27.0 / 1.856_297_990_365_626_3;
const POSITIONAL_MAX_PERIOD: f64 = 10_000.0;
const VOCAB_JSON: &str = include_str!("../resources/whisper-vocab.json");
/// whisper.cpp synthesizes the special tokens itself and expects only the base vocabulary.
const BASE_VOCAB: usize = 50257;
/// Shapes the file holds in a different axis order than the openai checkpoint.
const MLX_CONV_WEIGHTS: [&str; 2] = ["encoder.conv1.weight", "encoder.conv2.weight"];
const HF_RENAMES: [(&str, &str); 19] = [
    ("model.encoder.layer_norm", "encoder.ln_post"),
    ("model.decoder.layer_norm", "decoder.ln"),
    ("model.encoder.embed_positions.weight", "encoder.positional_embedding"),
    ("model.decoder.embed_positions.weight", "decoder.positional_embedding"),
    ("model.decoder.embed_tokens", "decoder.token_embedding"),
    ("model.encoder.layers", "encoder.blocks"),
    ("model.decoder.layers", "decoder.blocks"),
    ("model.encoder", "encoder"),
    ("encoder_attn_layer_norm", "cross_attn_ln"),
    ("encoder_attn.k_proj", "cross_attn.key"),
    ("encoder_attn.v_proj", "cross_attn.value"),
    ("encoder_attn.q_proj", "cross_attn.query"),
    ("encoder_attn.out_proj", "cross_attn.out"),
    ("self_attn_layer_norm", "attn_ln"),
    ("self_attn.k_proj", "attn.key"),
    ("self_attn.v_proj", "attn.value"),
    ("self_attn.q_proj", "attn.query"),
    ("self_attn.out_proj", "attn.out"),
    ("final_layer_norm", "mlp_ln"),
];
const MLP_RENAMES: [(&str, &str); 4] = [
    (".fc1.", ".mlp.0."),
    (".fc2.", ".mlp.2."),
    (".mlp1.", ".mlp.0."),
    (".mlp2.", ".mlp.2."),
];

#[derive(Debug)]
struct Tensor {
    name: String,
    source: Option<TensorSource>,
    /// Row-major shape in the openai layout.
    shape: Vec<usize>,
}

#[derive(Debug)]
struct TensorSource {
    offset: u64,
    /// `Some` when the stored order is (out, kernel, in) and must become (out, in, kernel).
    swap_conv: Option<(usize, usize, usize)>,
}

/// Maps a checkpoint tensor name to the openai name whisper.cpp expects; `None` drops it.
fn ggml_name(name: &str) -> Option<String> {
    if name == "proj_out.weight" || name == "alignment_heads" {
        return None;
    }
    let mut mapped = name.to_owned();
    for (from, to) in HF_RENAMES.iter().chain(MLP_RENAMES.iter()) {
        mapped = mapped.replace(from, to);
    }
    if mapped.starts_with("model.decoder.") {
        mapped = mapped.replacen("model.decoder.", "decoder.", 1);
    }
    Some(mapped)
}

/// Triangular slaney mel filters, identical to librosa's for Whisper's 128-bin front end.
fn mel_filters() -> Vec<f32> {
    let edges: Vec<f64> = (0..N_MELS + 2)
        .map(|index| {
            let low = hz_to_mel(0.0);
            let high = hz_to_mel(SAMPLE_RATE / 2.0);
            mel_to_hz(low + (high - low) * index as f64 / (N_MELS + 1) as f64)
        })
        .collect();
    (0..N_MELS)
        .flat_map(|mel| {
            let (lower, center, upper) = (edges[mel], edges[mel + 1], edges[mel + 2]);
            let norm = 2.0 / (upper - lower);
            (0..N_FFT_BINS).map(move |bin| {
                let hz = bin as f64 * SAMPLE_RATE / FFT_SIZE;
                let rising = (hz - lower) / (center - lower);
                let falling = (upper - hz) / (upper - center);
                (rising.min(falling).max(0.0) * norm) as f32
            })
        })
        .collect()
}

fn hz_to_mel(hz: f64) -> f64 {
    if hz < MEL_BREAK_HZ {
        hz * MEL_LINEAR_SLOPE
    } else {
        MEL_AT_BREAK + MEL_LOG_SCALE * (hz / MEL_BREAK_HZ).ln()
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    if mel < MEL_AT_BREAK {
        mel / MEL_LINEAR_SLOPE
    } else {
        MEL_BREAK_HZ * ((mel - MEL_AT_BREAK) / MEL_LOG_SCALE).exp()
    }
}

/// The positional table MLX checkpoints omit because whisper computes it from this formula.
fn sinusoids() -> Vec<f32> {
    let half = N_STATE / 2;
    let scale = POSITIONAL_MAX_PERIOD.ln() / (half - 1) as f64;
    (0..N_AUDIO_CTX)
        .flat_map(|position| {
            let sines = (0..half).map(move |i| (position as f64 * (-scale * i as f64).exp()).sin());
            let cosines = (0..half).map(move |i| (position as f64 * (-scale * i as f64).exp()).cos());
            sines.chain(cosines).map(|value| value as f32)
        })
        .collect()
}

/// Reverses GPT-2's printable-byte mapping to get each token's raw bytes.
fn gpt2_byte_decoder() -> HashMap<char, u8> {
    let printable = (0x21..=0x7E).chain(0xA1..=0xAC).chain(0xAE..=0xFF);
    let mut decoder: HashMap<char, u8> = printable.map(|b: u32| (char::from_u32(b).unwrap_or('\0'), b as u8)).collect();
    let mut next = 256u32;
    for byte in 0u32..=255 {
        let Some(key) = char::from_u32(byte) else { continue };
        if decoder.contains_key(&key) {
            continue;
        }
        decoder.insert(char::from_u32(next).unwrap_or('\0'), byte as u8);
        next += 1;
    }
    decoder
}

fn vocab_bytes() -> AppResult<Vec<Vec<u8>>> {
    let tokens: HashMap<String, usize> =
        serde_json::from_str(VOCAB_JSON).map_err(|error| AppError::Invalid(format!("embedded vocab: {error}")))?;
    let decoder = gpt2_byte_decoder();
    let mut ordered = vec![Vec::new(); BASE_VOCAB];
    for (token, id) in tokens {
        if id < BASE_VOCAB {
            ordered[id] = token.chars().filter_map(|c| decoder.get(&c).copied()).collect();
        }
    }
    Ok(ordered)
}

fn read_header(reader: &mut BufReader<File>) -> AppResult<(Value, u64)> {
    let mut length = [0u8; 8];
    reader.read_exact(&mut length).map_err(io_context("read safetensors header"))?;
    let header_bytes = u64::from_le_bytes(length);
    let mut header = vec![0u8; usize::try_from(header_bytes).map_err(|_| AppError::Invalid("safetensors header too large".into()))?];
    reader.read_exact(&mut header).map_err(io_context("read safetensors header"))?;
    let value = serde_json::from_slice(&header).map_err(|error| AppError::Invalid(format!("safetensors header: {error}")))?;
    Ok((value, 8 + header_bytes))
}

fn parse_tensor(name: &str, meta: &Value, data_start: u64) -> AppResult<Option<Tensor>> {
    let Some(mapped) = ggml_name(name) else { return Ok(None) };
    if meta["dtype"] != "F16" {
        return Err(AppError::Invalid(format!("{name}: expected F16 weights, got {}", meta["dtype"])));
    }
    let offset = meta["data_offsets"][0]
        .as_u64()
        .ok_or_else(|| AppError::Invalid(format!("{name}: missing data offset")))?;
    let stored: Vec<usize> = meta["shape"]
        .as_array()
        .ok_or_else(|| AppError::Invalid(format!("{name}: missing shape")))?
        .iter()
        .filter_map(|d| d.as_u64().and_then(|d| usize::try_from(d).ok()))
        .collect();
    let swap_conv = (MLX_CONV_WEIGHTS.contains(&mapped.as_str()) && stored[1] == 3)
        .then(|| (stored[0], stored[1], stored[2]));
    let shape = match swap_conv {
        Some((out, kernel, input)) => vec![out, input, kernel],
        None => stored,
    };
    Ok(Some(Tensor { name: mapped, source: Some(TensorSource { offset: data_start + offset, swap_conv }), shape }))
}

fn write_i32(out: &mut impl Write, value: i32) -> std::io::Result<()> {
    out.write_all(&value.to_le_bytes())
}

fn write_header(out: &mut impl Write) -> AppResult<()> {
    let n_state = N_STATE as i32;
    let fields = [N_VOCAB, N_AUDIO_CTX as i32, n_state, N_HEAD, N_AUDIO_LAYER, N_TEXT_CTX, n_state, N_HEAD, N_TEXT_LAYER, N_MELS as i32, FTYPE_F16];
    let context = "write ggml header";
    out.write_all(&GGML_MAGIC.to_le_bytes()).map_err(io_context(context))?;
    for field in fields {
        write_i32(out, field).map_err(io_context(context))?;
    }
    write_i32(out, N_MELS as i32).map_err(io_context(context))?;
    write_i32(out, N_FFT_BINS as i32).map_err(io_context(context))?;
    let filters: Vec<u8> = mel_filters().iter().flat_map(|v| v.to_le_bytes()).collect();
    out.write_all(&filters).map_err(io_context(context))?;
    let vocab = vocab_bytes()?;
    write_i32(out, BASE_VOCAB as i32).map_err(io_context(context))?;
    for token in vocab {
        write_i32(out, token.len() as i32).map_err(io_context(context))?;
        out.write_all(&token).map_err(io_context(context))?;
    }
    Ok(())
}

/// Half-float to float; whisper.cpp stores 1-D tensors and positional tables as F32.
fn f16_to_f32(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exponent = u32::from(half >> 10) & 0x1F;
    let fraction = u32::from(half & 0x03FF);
    if exponent == 0 {
        let magnitude = fraction as f32 * 2f32.powi(-24);
        return if sign == 0 { magnitude } else { -magnitude };
    }
    let bits = if exponent == 0x1F {
        sign | 0x7F80_0000 | (fraction << 13)
    } else {
        sign | ((exponent + 112) << 23) | (fraction << 13)
    };
    f32::from_bits(bits)
}

fn stores_f32(tensor: &Tensor) -> bool {
    tensor.shape.len() < 2 || tensor.name.ends_with("positional_embedding")
}

fn write_tensor_header(out: &mut impl Write, tensor: &Tensor, ftype: i32) -> std::io::Result<()> {
    // whisper.cpp reshapes conv biases to 2-D: the file must say [1, n].
    let is_conv_bias = tensor.name == "encoder.conv1.bias" || tensor.name == "encoder.conv2.bias";
    let dims: Vec<usize> = if is_conv_bias {
        vec![1, tensor.shape[0]]
    } else {
        tensor.shape.iter().rev().copied().collect()
    };
    write_i32(out, dims.len() as i32)?;
    write_i32(out, tensor.name.len() as i32)?;
    write_i32(out, ftype)?;
    for dim in dims {
        write_i32(out, dim as i32)?;
    }
    out.write_all(tensor.name.as_bytes())
}

fn copy_tensor(reader: &mut BufReader<File>, out: &mut impl Write, tensor: &Tensor, source: &TensorSource) -> AppResult<()> {
    let context = "copy tensor";
    let count: usize = tensor.shape.iter().product();
    reader.seek(SeekFrom::Start(source.offset)).map_err(io_context(context))?;
    let as_f32 = stores_f32(tensor);
    let mut raw = vec![0u8; count * 2];
    reader.read_exact(&mut raw).map_err(io_context(context))?;
    if let Some((out_channels, kernel, input)) = source.swap_conv {
        raw = swap_conv_axes(&raw, out_channels, kernel, input);
    }
    if !as_f32 {
        return out.write_all(&raw).map_err(io_context(context));
    }
    let floats: Vec<u8> = raw
        .chunks_exact(2)
        .flat_map(|pair| f16_to_f32(u16::from_le_bytes([pair[0], pair[1]])).to_le_bytes())
        .collect();
    out.write_all(&floats).map_err(io_context(context))
}

/// (out, kernel, in) to (out, in, kernel), two bytes per element.
fn swap_conv_axes(raw: &[u8], out_channels: usize, kernel: usize, input: usize) -> Vec<u8> {
    (0..out_channels)
        .flat_map(|o| (0..input).map(move |i| (o, i)))
        .flat_map(|(o, i)| (0..kernel).map(move |k| (o * kernel + k) * input + i))
        .flat_map(|element| [raw[element * 2], raw[element * 2 + 1]])
        .collect()
}

fn load_tensors(header: &Value, data_start: u64) -> AppResult<Vec<Tensor>> {
    let entries = header.as_object().ok_or_else(|| AppError::Invalid("safetensors header is not an object".into()))?;
    let mut tensors = Vec::new();
    for (name, meta) in entries.iter().filter(|(name, _)| *name != "__metadata__") {
        if let Some(tensor) = parse_tensor(name, meta, data_start)? {
            tensors.push(tensor);
        }
    }
    if !tensors.iter().any(|t| t.name == "encoder.positional_embedding") {
        tensors.push(Tensor { name: "encoder.positional_embedding".into(), source: None, shape: vec![N_AUDIO_CTX, N_STATE] });
    }
    if !tensors.iter().any(|t| t.name == "encoder.conv1.weight") {
        return Err(AppError::Invalid("not a Whisper checkpoint: encoder.conv1.weight is missing".into()));
    }
    Ok(tensors)
}

/// Writes the ggml model atomically: `output` appears only after a complete conversion.
pub fn convert(source: &Path, output: &Path, mut progress: impl FnMut(f64)) -> AppResult<()> {
    let file = File::open(source).map_err(io_context(format!("open {}", source.display())))?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_BYTES, file);
    let (header, data_start) = read_header(&mut reader)?;
    let tensors = load_tensors(&header, data_start)?;
    let partial = output.with_extension("converting");
    let created = File::create(&partial).map_err(io_context(format!("create {}", partial.display())))?;
    let mut out = BufWriter::with_capacity(COPY_BUFFER_BYTES, created);
    write_header(&mut out)?;
    for (index, tensor) in tensors.iter().enumerate() {
        let ftype = if stores_f32(tensor) { TENSOR_F32 } else { TENSOR_F16 };
        write_tensor_header(&mut out, tensor, ftype).map_err(io_context("write tensor header"))?;
        match &tensor.source {
            Some(source) => copy_tensor(&mut reader, &mut out, tensor, source)?,
            None => {
                let values: Vec<u8> = sinusoids().iter().flat_map(|v| v.to_le_bytes()).collect();
                out.write_all(&values).map_err(io_context("write positional table"))?;
            }
        }
        progress((index + 1) as f64 / tensors.len() as f64);
    }
    out.flush().map_err(io_context("flush ggml"))?;
    drop(out);
    std::fs::rename(&partial, output).map_err(io_context("publish ggml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hf_names_map_to_openai_names() {
        assert_eq!(ggml_name("model.encoder.layers.3.self_attn.k_proj.weight").unwrap(), "encoder.blocks.3.attn.key.weight");
        assert_eq!(ggml_name("model.decoder.layers.1.encoder_attn.out_proj.bias").unwrap(), "decoder.blocks.1.cross_attn.out.bias");
        assert_eq!(ggml_name("model.decoder.layers.1.encoder_attn_layer_norm.weight").unwrap(), "decoder.blocks.1.cross_attn_ln.weight");
        assert_eq!(ggml_name("model.decoder.layers.0.fc2.weight").unwrap(), "decoder.blocks.0.mlp.2.weight");
        assert_eq!(ggml_name("model.encoder.layers.0.final_layer_norm.bias").unwrap(), "encoder.blocks.0.mlp_ln.bias");
        assert_eq!(ggml_name("model.encoder.layer_norm.weight").unwrap(), "encoder.ln_post.weight");
        assert_eq!(ggml_name("model.decoder.layer_norm.weight").unwrap(), "decoder.ln.weight");
        assert_eq!(ggml_name("model.decoder.embed_tokens.weight").unwrap(), "decoder.token_embedding.weight");
        assert_eq!(ggml_name("model.decoder.embed_positions.weight").unwrap(), "decoder.positional_embedding");
        assert_eq!(ggml_name("model.encoder.conv1.weight").unwrap(), "encoder.conv1.weight");
    }

    #[test]
    fn mlx_names_map_to_openai_names() {
        assert_eq!(ggml_name("encoder.blocks.0.mlp1.weight").unwrap(), "encoder.blocks.0.mlp.0.weight");
        assert_eq!(ggml_name("decoder.blocks.2.attn_ln.bias").unwrap(), "decoder.blocks.2.attn_ln.bias");
    }

    #[test]
    fn non_model_tensors_are_dropped() {
        assert_eq!(ggml_name("proj_out.weight"), None);
        assert_eq!(ggml_name("alignment_heads"), None);
    }

    #[test]
    fn half_floats_decode() {
        assert_eq!(f16_to_f32(0x3C00), 1.0);
        assert_eq!(f16_to_f32(0xC000), -2.0);
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x7C00), f32::INFINITY);
    }

    #[test]
    fn conv_axes_swap_keeps_elements() {
        // out=1, kernel=2, in=3: stored [[a b c][d e f]] becomes [[a d][b e][c f]].
        let raw = [0, 0, 1, 0, 2, 0, 3, 0, 4, 0, 5, 0];
        assert_eq!(swap_conv_axes(&raw, 1, 2, 3), vec![0, 0, 3, 0, 1, 0, 4, 0, 2, 0, 5, 0]);
    }

    #[test]
    fn vocab_decodes_gpt2_bytes() {
        let vocab = vocab_bytes().unwrap();
        assert_eq!(vocab.len(), 50257);
        assert_eq!(vocab[0], b"!".to_vec());
    }

    #[test]
    fn mel_filter_shape_and_first_peak() {
        let filters = mel_filters();
        assert_eq!(filters.len(), 128 * 201);
        assert!(filters.iter().all(|v| *v >= 0.0));
        assert!(filters[..201].iter().any(|v| *v > 0.0));
    }

    #[test]
    fn sinusoid_table_starts_with_zero_sine_and_unit_cosine() {
        let table = sinusoids();
        assert_eq!(table.len(), 1500 * 1280);
        assert_eq!(table[0], 0.0);
        assert_eq!(table[640], 1.0);
    }
}

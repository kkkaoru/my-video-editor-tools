//! Developer tool: `convert_whisper <weights.safetensors> <out.bin>`.
use std::path::PathBuf;

fn main() -> Result<(), transcribe_app::error::AppError> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(source), Some(output)) = (args.next(), args.next()) else {
        return Err(transcribe_app::error::AppError::Invalid("usage: convert_whisper SRC OUT".into()));
    };
    transcribe_app::whisper_ggml::convert(&source, &output, |_| {})
}

//! Whole-job, catalog and process tests over shell-script fake tools and size-matched sparse models.
#![cfg(unix)]

use serde_json::Value;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use transcribe_app::catalog::{self, Backend, Gpu, GpuVendor, Hardware};
use transcribe_app::engines::{self, DiarizeModel, Tools, VideoInfo};
use transcribe_app::error::AppError;
use transcribe_app::eta::{Progress, Stage};
use transcribe_app::export::{self, Transcript};
use transcribe_app::face::{scan_faces, FaceDetector, FaceModelKind, FaceSample};
use transcribe_app::paths::AppPaths;
use transcribe_app::pipeline;
use transcribe_app::process::CancelToken;
use transcribe_app::settings::{Anchor, Placement, Settings, SubtitleFormat};

const SPEAKER_MODEL: &str = "nemotron-3.5-transcribe-q2l";
const Q2L_FILE: &str =
    "models/nemotron-3.5-transcribe-q2l/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_L.gguf";
const Q2L_BYTES: u64 = 15_547_824_288;
const ENCODER_FILE: &str =
    "models/nemotron-3.5-transcribe-q2l/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf";
const ENCODER_BYTES: u64 = 1_670_410_080;
const GGML_FILE: &str = "models/whisper-large-v3-turbo/ggml-large-v3-turbo.bin";
const WHISPER_SOURCE_BYTES: u64 = 1_617_824_864;
const WHISPER_MLX_BYTES: u64 = 1_613_977_612;
const VIDEO: VideoInfo = VideoInfo {
    width: 1280,
    height: 720,
    duration: 2.0,
    fps_num: 30,
    fps_den: 1,
    audio: None,
};
const FFMPEG: &str = r#"
case "$*" in
  *rawvideo*) head -c 1228800 /dev/zero ;;
  *) echo 'frame=1 time=00:00:01.00 bitrate=1kbits/s' >&2; echo 'no status here' >&2 ;;
esac
"#;
const FFPROBE: &str = r#"
cat <<'EOF'
{"format":{"duration":"2.0"},"streams":[
  {"codec_type":"video","width":1280,"height":720,"avg_frame_rate":"30/1"},
  {"codec_type":"audio"}]}
EOF
"#;
const WHISPER: &str = r#"
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then out="$2"; fi
  shift
done
echo 'whisper_print_progress_callback: progress =  50%' >&2
echo 'some other line' >&2
cat > "$out.json" <<'EOF'
{"transcription":[{"tokens":[
  {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
  {"text":"はい","offsets":{"from":100,"to":900}},
  {"text":"どうも","offsets":{"from":1000,"to":1800}}]}]}
EOF
"#;
const NEMO: &str = r#"
while [ $# -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  shift
done
echo '{"words":[{"word":"a","start":0.0,"end":0.95,"speaker":1},{"word":"b","start":0.95,"end":2.0,"speaker":2}]}' > "$out"
"#;
/// ModelProto with one Identity node: lets the real detector run without a real face model.
const IDENTITY_ONNX: &[u8] = b"\x08\x08\x3a\x2b\x0a\x10\x0a\x01x\x12\x01y\x22\x08Identity\x12\x01g\x5a\x09\x0a\x01x\x12\x04\x0a\x02\x08\x01\x62\x09\x0a\x01y\x12\x04\x0a\x02\x08\x01\x42\x02\x10\x0d";

fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_tool(path: &Path, script: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{script}")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sparse(path: &Path, bytes: u64) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    File::create(path).unwrap().set_len(bytes).unwrap();
}

const VOICE_DETECTOR: &str = "echo 'Speech segment 0: start = 0.00, end = 6000.00'\n";

/// Paths under `root` with every tool faked and no model installed.
fn with_tools(root: &Path) -> AppPaths {
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let runtime = paths.bundled_runtime_dir();
    write_tool(&runtime.join("ffmpeg/ffmpeg"), FFMPEG);
    write_tool(&runtime.join("ffmpeg/ffprobe"), FFPROBE);
    write_tool(&runtime.join("whisper/whisper-cli"), WHISPER);
    write_tool(
        &runtime.join("whisper/whisper-vad-speech-segments"),
        VOICE_DETECTOR,
    );
    write_tool(&runtime.join("nemo/bin/nemo-speech"), NEMO);
    install_ipadic(&paths);
    paths
}

/// A hard link: the 8 MB dictionary is shared by every job test instead of copied per test.
fn install_ipadic(paths: &AppPaths) {
    let installed = paths.resources.join("resources/ipadic-mecab-2_7_0.dic.zst");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::hard_link(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ipadic-mecab-2_7_0.dic.zst"),
        installed,
    )
    .unwrap();
}

/// As `with_tools`, plus a ready ggml file and the small diarization model.
fn ready(root: &Path) -> AppPaths {
    let paths = with_tools(root);
    sparse(&paths.data.join(GGML_FILE), 0);
    install_speech_llm(&paths);
    paths
}

/// Speaker separation only runs with a usable Nemotron 3.5 present.
fn install_speech_llm(paths: &AppPaths) {
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
}

fn install_yunet(paths: &AppPaths) {
    fs::create_dir_all(paths.yunet_model().parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/face_detection_yunet_2023mar.onnx"),
        paths.yunet_model(),
    )
    .unwrap();
}

fn tools_in(paths: &AppPaths) -> Tools {
    Tools::locate(paths, Backend::Default).unwrap()
}

fn mac() -> Hardware {
    Hardware {
        is_mac: true,
        memory_gib: 64,
        gpu: None,
    }
}

/// These jobs are not about faces, so they skip the scan the default placement asks for.
fn bottom_anchored() -> Settings {
    Settings {
        placement: Placement {
            anchor: Anchor::VideoBottom,
            offset_x_percent: 0.0,
            offset_y_percent: 0.0,
        },
        speaker_count: 0,
        diarize_model: SPEAKER_MODEL.to_owned(),
        ..Settings::default()
    }
}

fn face_anchored() -> Settings {
    Settings {
        placement: Placement {
            anchor: Anchor::FaceAbove,
            offset_x_percent: 0.0,
            offset_y_percent: 0.0,
        },
        speaker_count: 0,
        diarize_model: SPEAKER_MODEL.to_owned(),
        ..Settings::default()
    }
}

fn names(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// A tiny safetensors file with the openai layout, grown to `bytes` so the catalog accepts it.
fn write_checkpoint(path: &Path, bytes: u64) {
    let header = br#"{"encoder.conv1.weight":{"dtype":"F16","shape":[1,2],"data_offsets":[0,4]},"encoder.positional_embedding":{"dtype":"F16","shape":[1,1],"data_offsets":[4,6]}}"#;
    let mut file = (header.len() as u64).to_le_bytes().to_vec();
    file.extend_from_slice(header);
    file.extend_from_slice(&[1, 0, 2, 0, 0, 0x3C]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, file).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(bytes)
        .unwrap();
}

fn headless(root: &Path, video: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_transcribe-app"));
    command
        .arg("--headless")
        .arg(video)
        .env("TRANSCRIBE_DATA_DIR", root.join("data"))
        .env("TRANSCRIBE_RESOURCE_DIR", root.join("resources"))
        .env("HF_HUB_CACHE", root.join("hub"))
        .env("PATH", "/usr/bin:/bin");
    command
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

#[test]
fn a_job_with_the_default_model_writes_the_chosen_format_and_the_transcript() {
    let root = scratch("job-default");
    let paths = ready(&root);
    let mut stages: Vec<Stage> = Vec::new();
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            subtitle_format: SubtitleFormat::Srt,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| stages.push(progress.stage),
    )
    .unwrap();
    stages.dedup();
    assert_eq!(
        stages,
        vec![
            Stage::ExtractAudio,
            Stage::Transcribe,
            Stage::Diarize,
            Stage::Export
        ]
    );
    assert_eq!(names(&outcome.outputs), vec!["clip.srt"]);
    assert!(root.join("clip.transcript.json").exists());
    assert!(!root.join("clip.ttml").exists());
    assert!(!root.join("clip.fcpxml").exists());
    assert_eq!(outcome.speaker_count, 2);
    assert_eq!(outcome.cue_count, 2);
    assert!(!outcome.faceless);
    assert_eq!(
        fs::read_to_string(root.join("clip.srt")).unwrap(),
        "1\n00:00:00,100 --> 00:00:00,933\n<font color=\"#FFFFFF\">はい</font>\n\n2\n00:00:01,000 --> 00:00:01,870\n<font color=\"#005A9C\">どうも</font>\n\n"
    );
    assert_eq!(fs::read_dir(paths.jobs_dir()).unwrap().count(), 0);
}

#[test]
fn a_second_job_on_the_same_video_keeps_the_first_files() {
    let root = scratch("job-numbering");
    let paths = ready(&root);
    let video = root.join("clip.mp4");
    let run = || {
        pipeline::run_job(
            &video,
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {},
        )
        .unwrap()
    };
    assert_eq!(
        run().base.file_name().unwrap().to_string_lossy(),
        "clip".to_owned()
    );
    assert_eq!(
        run().base.file_name().unwrap().to_string_lossy(),
        "clip-2".to_owned()
    );
    assert_eq!(
        run().base.file_name().unwrap().to_string_lossy(),
        "clip-3".to_owned()
    );
}

#[test]
fn a_job_with_a_speech_llm_merges_word_speakers_into_turns() {
    let root = scratch("job-speech-llm");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &counting_nemo(
            &root,
            r#"{"words":[{"word":"a","start":0.0,"end":0.9,"speaker":1},{"word":"b","start":1.0,"end":2.0,"speaker":1}]}"#,
        ),
    );
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-q2l".to_owned(),
        language: "ja".to_owned(),
        ..bottom_anchored()
    };
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.speaker_count, 1);
    assert_eq!(outcome.cue_count, 1);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_job_converts_the_whisper_checkpoint_once() {
    let root = scratch("job-convert");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    write_checkpoint(
        &paths
            .data
            .join("models/whisper-large-v3-turbo/model.safetensors"),
        WHISPER_SOURCE_BYTES,
    );
    let mut stages: Vec<Stage> = Vec::new();
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| stages.push(progress.stage),
    )
    .unwrap();
    stages.dedup();
    assert_eq!(stages[0], Stage::ConvertModel);
    let ggml = fs::read(paths.data.join(GGML_FILE)).unwrap();
    assert_eq!(ggml[..4], [0x6c, 0x6d, 0x67, 0x67]);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_job_uses_the_downloaded_cuda_tools_on_a_geforce() {
    let root = scratch("job-cuda");
    let paths = ready(&root);
    let geforce = paths.downloaded_runtime_dir().join("geforce");
    write_tool(
        &geforce.join("nemo/nemo-speech"),
        "echo 'cuda build ran' >&2; exit 7",
    );
    sparse(&geforce.join("nemo/nemo-speech-cuda.zip.unpacked"), 0);
    let hardware = Hardware {
        is_mac: false,
        memory_gib: 64,
        gpu: Some(Gpu {
            vendor: GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 4090".to_owned(),
            memory_gib: 24,
        }),
    };
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &hardware,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "nemo-speech failed: exit status: 7\ncuda build ran");
}

#[test]
fn a_face_job_scans_frames_and_reports_a_missing_face() {
    let root = scratch("job-faces");
    let paths = ready(&root);
    install_yunet(&paths);
    let mut stages: Vec<Stage> = Vec::new();
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &face_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| stages.push(progress.stage),
    )
    .unwrap();
    assert!(stages.contains(&Stage::Faces));
    assert!(outcome.faceless);
}

#[test]
fn a_job_refuses_invalid_settings_before_anything_else() {
    let root = scratch("job-invalid-settings");
    let settings = Settings {
        palette: Vec::new(),
        ..bottom_anchored()
    };
    let paths = with_tools(&root);
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "the speaker palette needs at least one colour"
    );
}

#[test]
fn without_a_nemotron_3_5_a_job_has_one_speaker_whatever_is_saved() {
    let root = scratch("job-no-model");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'nemo must not run' >&2; exit 9",
    );
    let settings = Settings {
        speaker_count: 3,
        ..bottom_anchored()
    };
    let mut plan: Vec<Stage> = Vec::new();
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| plan.clone_from(&progress.plan),
    )
    .unwrap();
    assert_eq!(
        plan,
        vec![Stage::ExtractAudio, Stage::Transcribe, Stage::Export]
    );
    assert_eq!(outcome.speaker_count, 1);
    assert_eq!(outcome.requested_speakers, 1);
}

#[test]
fn a_nemotron_3_5_too_large_for_the_machine_counts_as_none() {
    let root = scratch("job-blocked-llm");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'nemo must not run' >&2; exit 9",
    );
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 2,
            ..bottom_anchored()
        },
        &Hardware {
            memory_gib: 12,
            ..mac()
        },
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.requested_speakers, 1);
}

#[test]
fn writing_captions_again_reports_one_speaker_without_a_nemotron_3_5() {
    let root = scratch("relayout-no-llm");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let outcome = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip-2"),
        &paths,
        &Settings {
            speaker_count: 3,
            ..bottom_anchored()
        },
        &Hardware {
            memory_gib: 12,
            ..mac()
        },
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.requested_speakers, 1);
}

#[test]
fn a_job_with_an_unknown_speaker_model_fails() {
    let root = scratch("job-unknown-model");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    let settings = Settings {
        diarize_model: "nope".to_owned(),
        ..bottom_anchored()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "unknown diarize model nope"
    );
}

#[test]
fn a_job_refuses_a_model_too_large_for_the_machine() {
    let root = scratch("job-too-large");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-q8".to_owned(),
        ..bottom_anchored()
    };
    let hardware = Hardware {
        memory_gib: 24,
        ..mac()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &hardware,
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "model nemotron-3.5-transcribe-q8 needs 48 GiB of memory, this machine has 24 GiB"
    );
}

#[test]
fn a_job_refuses_a_chosen_model_that_is_not_installed() {
    let root = scratch("job-not-installed");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-iq4".to_owned(),
        ..bottom_anchored()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "model nemotron-3.5-transcribe-iq4 is not installed"
    );
}

#[test]
fn a_job_without_bundled_tools_says_which_is_missing() {
    let root = scratch("job-no-tools");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    install_speech_llm(&paths);
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("ffmpeg is missing from "), "{error}");
}

#[test]
fn a_job_on_a_file_without_video_reports_the_user_error() {
    let root = scratch("job-no-video");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffprobe"),
        r#"echo '{"format":{"duration":"1"},"streams":[{"codec_type":"audio"}]}'"#,
    );
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "the file has no video stream"
    );
}

#[test]
fn a_job_without_a_whisper_model_names_it() {
    let root = scratch("job-no-whisper");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "model whisper-large-v3-turbo is not installed"
    );
}

#[test]
fn a_job_with_silent_audio_reports_no_words() {
    let root = scratch("job-no-words");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        r#"
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then out="$2"; fi
  shift
done
echo '{"transcription":[]}' > "$out.json"
"#,
    );
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "no words were detected in the audio"
    );
    assert_eq!(fs::read_dir(paths.jobs_dir()).unwrap().count(), 0);
}

#[test]
fn a_cancelled_job_stops_before_the_audio_stage() {
    let root = scratch("job-cancelled");
    let paths = ready(&root);
    let cancel = CancelToken::default();
    cancel.cancel();
    let result = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &cancel,
        &mut |_| {},
    );
    assert!(matches!(result, Err(AppError::Cancelled)));
    assert!(!paths.jobs_dir().exists());
}

#[test]
fn cancelling_during_transcription_kills_the_tool() {
    let root = scratch("job-cancel-running");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        "echo 'progress = 10%' >&2\nexec sleep 30\n",
    );
    let cancel = CancelToken::default();
    let result = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &cancel,
        &mut |progress: Progress| {
            if progress.stage == Stage::Transcribe {
                cancel.cancel();
            }
        },
    );
    assert!(matches!(result, Err(AppError::Cancelled)));
}

#[test]
fn a_job_with_an_unknown_face_model_fails_before_any_long_work() {
    let root = scratch("job-unknown-face");
    let paths = ready(&root);
    let settings = Settings {
        face_model: "nope".to_owned(),
        ..face_anchored()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "unknown face model nope"
    );
}

#[test]
fn a_job_with_a_face_model_that_is_not_installed_names_it() {
    let root = scratch("job-no-face-model");
    let paths = ready(&root);
    let settings = Settings {
        face_model: "avatar-face-yolo".to_owned(),
        ..face_anchored()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "model avatar-face-yolo is not installed"
    );
}

#[test]
fn a_job_loads_the_avatar_model_by_kind() {
    let root = scratch("job-avatar");
    let paths = ready(&root);
    let model = paths
        .models_dir()
        .join("avatar-face-yolo/avatar-face-caption-v2.onnx");
    fs::create_dir_all(model.parent().unwrap()).unwrap();
    fs::write(model, IDENTITY_ONNX).unwrap();
    let settings = Settings {
        face_model: "avatar-face-yolo".to_owned(),
        ..face_anchored()
    };
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &settings,
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "face model failed: unexpected YOLO shape [1, 3, 640, 640]"
    );
}

#[test]
fn relayout_writes_to_the_base_it_is_given() {
    let root = scratch("relayout");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let mut stages: Vec<Stage> = Vec::new();
    let outcome = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip"),
        &paths,
        &Settings {
            font_ratio: 0.1,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| stages.push(progress.stage),
    )
    .unwrap();
    assert_eq!(stages, vec![Stage::Export, Stage::Export]);
    assert_eq!(outcome.base.file_name().unwrap(), "clip");
    assert_eq!(outcome.cue_count, 2);
    let saved = Transcript::load(&root.join("clip.transcript.json")).unwrap();
    assert_eq!(saved.metrics.font_px, 72.0);
    assert_eq!(
        saved.voice,
        vec![transcribe_app::captions::VoiceSpan {
            start: 0.0,
            end: 60.0
        }]
    );
}

#[test]
fn relayout_scans_faces_the_transcript_does_not_have_yet() {
    let root = scratch("relayout-faces");
    let paths = ready(&root);
    install_yunet(&paths);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    fs::rename(root.join("clip.transcript.json"), root.join("renamed.json")).unwrap();
    let outcome = pipeline::relayout_to(
        &root.join("renamed.json"),
        &root.join("renamed"),
        &paths,
        &face_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(outcome.faceless);
    assert_eq!(outcome.base.file_name().unwrap(), "renamed");
}

#[test]
fn relayout_refuses_invalid_settings_and_missing_files() {
    let root = scratch("relayout-errors");
    let paths = with_tools(&root);
    let invalid = Settings {
        language: String::new(),
        ..bottom_anchored()
    };
    let refused = pipeline::relayout_to(
        &root.join("a.transcript.json"),
        &root.join("a"),
        &paths,
        &invalid,
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(refused, "language must be a code such as ja, got \"\"");
    let missing = pipeline::relayout_to(
        &root.join("a.transcript.json"),
        &root.join("a"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(missing.starts_with("read "), "{missing}");
}

#[test]
fn tools_are_found_below_their_folder() {
    let root = scratch("tools");
    let paths = with_tools(&root);
    let tools = Tools::locate(&paths, Backend::Default).unwrap();
    assert!(tools.ffmpeg.ends_with("resources/runtime/ffmpeg/ffmpeg"));
    assert!(tools.ffprobe.ends_with("resources/runtime/ffmpeg/ffprobe"));
    assert!(tools
        .whisper
        .ends_with("resources/runtime/whisper/whisper-cli"));
    assert!(tools
        .nemo
        .ends_with("resources/runtime/nemo/bin/nemo-speech"));
}

#[test]
fn downloaded_cuda_tools_override_the_bundled_ones_only_when_asked() {
    let root = scratch("tools-cuda");
    let paths = with_tools(&root);
    write_tool(
        &paths
            .downloaded_runtime_dir()
            .join("geforce/nemo/release/nemo-speech"),
        "",
    );
    assert!(Tools::locate(&paths, Backend::Cuda)
        .unwrap()
        .nemo
        .ends_with("data/runtime/geforce/nemo/release/nemo-speech"));
    assert!(Tools::locate(&paths, Backend::Cuda)
        .unwrap()
        .whisper
        .ends_with("resources/runtime/whisper/whisper-cli"));
    assert!(Tools::locate(&paths, Backend::Default)
        .unwrap()
        .nemo
        .ends_with("resources/runtime/nemo/bin/nemo-speech"));
}

#[test]
fn a_tool_buried_too_deep_is_not_found() {
    let root = scratch("tools-deep");
    let paths = with_tools(&root);
    let runtime = paths.bundled_runtime_dir();
    fs::remove_dir_all(runtime.join("nemo")).unwrap();
    write_tool(&runtime.join("nemo/a/b/c/d/nemo-speech"), "");
    let error = Tools::locate(&paths, Backend::Default)
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("nemo-speech is missing from "), "{error}");
}

#[test]
fn probe_reads_the_tool_output_and_reports_its_failure() {
    let root = scratch("probe");
    let paths = with_tools(&root);
    assert_eq!(
        engines::probe(&tools_in(&paths), &root.join("clip.mp4")).unwrap(),
        VideoInfo {
            width: 1280,
            height: 720,
            duration: 2.0,
            fps_num: 30,
            fps_den: 1,
            audio: None,
        }
    );
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffprobe"),
        "echo broken >&2; exit 1",
    );
    assert_eq!(
        engines::probe(&tools_in(&paths), &root.join("clip.mp4"))
            .unwrap_err()
            .to_string(),
        "ffprobe failed: broken\n"
    );
}

#[test]
fn audio_extraction_reports_the_share_of_the_duration() {
    let root = scratch("extract");
    let paths = with_tools(&root);
    let mut seen = Vec::new();
    engines::extract_audio(
        &tools_in(&paths),
        &root.join("clip.mp4"),
        &root.join("a.wav"),
        2.0,
        &CancelToken::default(),
        &mut |fraction| seen.push(fraction),
    )
    .unwrap();
    assert_eq!(seen, vec![0.5]);
}

#[test]
fn transcription_reports_progress_and_words() {
    let root = scratch("transcribe");
    let paths = with_tools(&root);
    let mut seen = Vec::new();
    let words = engines::transcribe(
        &tools_in(&paths),
        &root.join("model.bin"),
        &root.join("a.wav"),
        "ja",
        &root,
        &CancelToken::default(),
        &mut |fraction| seen.push(fraction),
    )
    .unwrap();
    assert_eq!(seen, vec![0.5]);
    assert_eq!(words[0].text, "はい");
    assert_eq!(words[1].text, "どうも");
    assert_eq!(words.len(), 2);
}

#[test]
fn a_transcriber_that_writes_nothing_is_an_error() {
    let root = scratch("transcribe-empty");
    let paths = with_tools(&root);
    write_tool(&paths.bundled_runtime_dir().join("whisper/whisper-cli"), "");
    let error = engines::transcribe(
        &tools_in(&paths),
        &root.join("model.bin"),
        &root.join("a.wav"),
        "ja",
        &root,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("read whisper output: "), "{error}");
}

#[test]
fn a_diarizer_that_writes_nothing_is_an_error() {
    let root = scratch("diarize-empty");
    let paths = with_tools(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "",
    );
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-transcribe-q2l").unwrap(),
        main_file: Path::new("m.gguf"),
        encoder: None,
        device: "auto",
        speaker_count: 0,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        "auto",
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("read speaker output: "), "{error}");
}

#[test]
fn only_diarization_models_can_diarize() {
    let root = scratch("diarize-wrong-role");
    let paths = with_tools(&root);
    let model = DiarizeModel {
        entry: catalog::find_entry("yunet").unwrap(),
        main_file: Path::new("m.gguf"),
        encoder: None,
        device: "auto",
        speaker_count: 0,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        "auto",
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "yunet is not a diarization model");
}

#[test]
fn a_blank_frame_gives_no_face_and_reports_progress() {
    let root = scratch("scan-blank");
    let paths = with_tools(&root);
    install_yunet(&paths);
    let detector = FaceDetector::load(&paths.yunet_model(), FaceModelKind::Yunet).unwrap();
    let mut seen = Vec::new();
    let samples = scan_faces(
        &tools_in(&paths),
        &detector,
        &root.join("clip.mp4"),
        &VIDEO,
        &CancelToken::default(),
        &mut |fraction| seen.push(fraction),
    )
    .unwrap();
    assert_eq!(samples, Vec::<FaceSample>::new());
    assert_eq!(seen, vec![0.5]);
}

#[test]
fn a_cancelled_scan_stops_before_the_first_frame() {
    let root = scratch("scan-cancelled");
    let paths = with_tools(&root);
    let model = root.join("identity.onnx");
    fs::write(&model, IDENTITY_ONNX).unwrap();
    let detector = FaceDetector::load(&model, FaceModelKind::Yunet).unwrap();
    let cancel = CancelToken::default();
    cancel.cancel();
    let result = scan_faces(
        &tools_in(&paths),
        &detector,
        &root.join("clip.mp4"),
        &VIDEO,
        &cancel,
        &mut |_| {},
    );
    assert!(matches!(result, Err(AppError::Cancelled)));
}

#[test]
fn a_model_with_the_wrong_outputs_fails_the_scan() {
    let root = scratch("scan-wrong-model");
    let paths = with_tools(&root);
    let model = root.join("identity.onnx");
    fs::write(&model, IDENTITY_ONNX).unwrap();
    let detector = FaceDetector::load(&model, FaceModelKind::Yunet).unwrap();
    let error = scan_faces(
        &tools_in(&paths),
        &detector,
        &root.join("clip.mp4"),
        &VIDEO,
        &CancelToken::default(),
        &mut |_| {},
    )
    .err()
    .unwrap()
    .to_string();
    assert_eq!(error, "face model failed: expected 12 outputs, got 1");
}

#[test]
fn a_scan_whose_tool_cannot_start_is_an_error() {
    let root = scratch("scan-no-tool");
    let paths = with_tools(&root);
    let model = root.join("identity.onnx");
    fs::write(&model, IDENTITY_ONNX).unwrap();
    let detector = FaceDetector::load(&model, FaceModelKind::Yunet).unwrap();
    let mut tools = tools_in(&paths);
    tools.ffmpeg = root.join("absent");
    let error = scan_faces(
        &tools,
        &detector,
        &root.join("clip.mp4"),
        &VIDEO,
        &CancelToken::default(),
        &mut |_| {},
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.starts_with("start ffmpeg: "), "{error}");
}

#[test]
fn a_missing_face_model_cannot_load() {
    let error = FaceDetector::load(Path::new("/nonexistent.onnx"), FaceModelKind::Yunet)
        .err()
        .unwrap()
        .to_string();
    assert!(error.starts_with("face model failed: "), "{error}");
}

#[test]
fn files_are_found_by_name_and_size_in_any_root() {
    let root = scratch("locate-roots");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let extra = root.join("user");
    assert_eq!(
        catalog::speech_llm_encoder(std::slice::from_ref(&extra), &paths),
        None
    );
    let file = extra.join("a/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf");
    sparse(&file, 5);
    assert_eq!(
        catalog::speech_llm_encoder(std::slice::from_ref(&extra), &paths),
        None
    );
    sparse(&file, ENCODER_BYTES);
    assert!(catalog::speech_llm_encoder(&[extra], &paths)
        .unwrap()
        .ends_with("user/a/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn hidden_and_too_deep_folders_are_not_searched() {
    let root = scratch("locate-hidden");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    sparse(
        &root.join("data/models/.cache/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"),
        ENCODER_BYTES,
    );
    sparse(
        &root.join("data/models/a/b/c/d/e/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"),
        ENCODER_BYTES,
    );
    assert_eq!(catalog::speech_llm_encoder(&[], &paths), None);
    sparse(
        &root.join("data/models/a/b/c/d/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"),
        ENCODER_BYTES,
    );
    assert!(catalog::speech_llm_encoder(&[], &paths).is_some());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_hugging_face_cache_counts_as_installed() {
    let root = scratch("locate-hf");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: Some(root.join("hub")),
    };
    assert_eq!(catalog::speech_llm_encoder(&[], &paths), None);
    sparse(
        &root.join("hub/models--AtomicChat--NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-GGUF/snapshots/abc/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"),
        ENCODER_BYTES,
    );
    assert!(catalog::speech_llm_encoder(&[], &paths)
        .unwrap()
        .ends_with("snapshots/abc/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_speech_llm_needs_both_of_its_files() {
    let root = scratch("locate-speech-llm");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let entry = catalog::find_entry("nemotron-3.5-transcribe-q2l").unwrap();
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    assert_eq!(catalog::locate(entry, &paths, &[]), None);
    assert_eq!(catalog::speech_llm_encoder(&[], &paths), None);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    assert!(catalog::locate(entry, &paths, &[])
        .unwrap()
        .ends_with("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_L.gguf"));
    assert!(catalog::speech_llm_encoder(&[], &paths)
        .unwrap()
        .ends_with("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn whisper_prefers_a_ready_ggml_file_over_a_checkpoint() {
    let root = scratch("locate-whisper");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let entry = catalog::find_entry("whisper-large-v3-turbo").unwrap();
    assert_eq!(catalog::locate(entry, &paths, &[]), None);
    assert_eq!(catalog::locate_whisper_source(&paths, &[]), None);
    sparse(
        &root.join("data/models/mlx/weights.safetensors"),
        WHISPER_MLX_BYTES - 1,
    );
    assert_eq!(catalog::locate_whisper_source(&paths, &[]), None);
    sparse(
        &root.join("data/models/mlx/weights.safetensors"),
        WHISPER_MLX_BYTES,
    );
    assert!(catalog::locate(entry, &paths, &[])
        .unwrap()
        .ends_with("mlx/weights.safetensors"));
    sparse(
        &root.join("data/models/hf/model.safetensors"),
        WHISPER_SOURCE_BYTES,
    );
    assert!(catalog::locate_whisper_source(&paths, &[])
        .unwrap()
        .ends_with("hf/model.safetensors"));
    sparse(&root.join("data/models/x/ggml-large-v3-turbo-q5_0.bin"), 1);
    assert!(catalog::locate(entry, &paths, &[])
        .unwrap()
        .ends_with("x/ggml-large-v3-turbo-q5_0.bin"));
    assert!(catalog::locate_whisper_ggml(&paths, &[])
        .unwrap()
        .ends_with("x/ggml-large-v3-turbo-q5_0.bin"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn face_models_and_the_runtime_are_located_by_their_own_rules() {
    let root = scratch("locate-others");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let yunet = catalog::find_entry("yunet").unwrap();
    let avatar = catalog::find_entry("avatar-face-yolo").unwrap();
    let runtime = catalog::find_entry("geforce-runtime").unwrap();
    assert_eq!(catalog::locate(yunet, &paths, &[]), None);
    assert_eq!(catalog::locate(avatar, &paths, &[]), None);
    assert_eq!(catalog::locate(runtime, &paths, &[]), None);
    sparse(&paths.yunet_model(), 1);
    sparse(&root.join("data/models/avatar-face-caption-v2.onnx"), 1);
    // A running download has only its partial file and folders: not installed yet.
    sparse(
        &root.join("data/runtime/geforce/nemo/nemo-speech-cuda.zip.part"),
        1,
    );
    assert!(catalog::locate(yunet, &paths, &[]).is_some());
    assert!(catalog::locate(avatar, &paths, &[]).is_some());
    assert_eq!(catalog::locate(runtime, &paths, &[]), None);
    sparse(
        &root.join("data/runtime/geforce/nemo/nemo-speech-cuda.zip.unpacked"),
        0,
    );
    assert!(catalog::locate(runtime, &paths, &[])
        .unwrap()
        .ends_with("data/runtime/geforce"));
}

#[test]
fn statuses_describe_every_entry_for_this_hardware() {
    let root = scratch("statuses");
    let paths = ready(&root);
    let rows = catalog::statuses(&mac(), &paths, &[]);
    assert_eq!(
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![
            "whisper-large-v3-turbo",
            "nemotron-3.5-transcribe-iq4",
            "nemotron-3.5-transcribe-nvfp4",
            "nemotron-3.5-transcribe-q2m",
            "nemotron-3.5-transcribe-q2l",
            "nemotron-3.5-transcribe-q8",
            "yunet",
            "avatar-face-yolo"
        ]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.recommended)
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![
            "whisper-large-v3-turbo",
            "nemotron-3.5-transcribe-q8",
            "yunet"
        ]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.found_at.is_some())
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec!["whisper-large-v3-turbo", "nemotron-3.5-transcribe-q2l",]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.blocked.is_some())
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![
            "nemotron-3.5-transcribe-nvfp4",
            "nemotron-3.5-transcribe-q2m"
        ]
    );
    assert_eq!(rows[0].download_bytes, 1_617_824_864);
}

#[test]
fn the_default_speaker_model_prefers_the_recommended_build() {
    let root = scratch("default-model");
    let paths = with_tools(&root);
    let hardware = Hardware {
        memory_gib: 24,
        ..mac()
    };
    let installed_small = catalog::statuses(&hardware, &paths, &[]);
    assert_eq!(
        catalog::default_diarize_model(&hardware, &installed_small),
        None
    );
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    let installed_both = catalog::statuses(&hardware, &paths, &[]);
    assert_eq!(
        catalog::default_diarize_model(&hardware, &installed_both),
        Some("nemotron-3.5-transcribe-q2l")
    );
    let too_small = Hardware {
        memory_gib: 1,
        ..mac()
    };
    let blocked = catalog::statuses(&too_small, &paths, &[]);
    assert_eq!(catalog::default_diarize_model(&too_small, &blocked), None);
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn this_machine_reports_its_memory() {
    let hardware = catalog::detect_hardware().unwrap();
    assert!(hardware.memory_gib > 0);
    assert_eq!(hardware.is_mac, cfg!(target_os = "macos"));
}

#[test]
fn outputs_land_beside_the_video_and_load_back() {
    let root = scratch("export-round-trip");
    let settings = bottom_anchored();
    let metrics = transcribe_app::captions::metrics_for(&VIDEO, &settings);
    let mut transcript = Transcript::new(
        root.join("a.mp4"),
        VIDEO,
        metrics,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    transcript.voice = vec![transcribe_app::captions::VoiceSpan {
        start: 1.0,
        end: 2.0,
    }];
    let outputs = export::write_outputs(&root.join("a"), &transcript, &settings).unwrap();
    assert_eq!(names(&outputs), vec!["a.drt"]);
    assert_eq!(
        Transcript::load(&root.join("a.transcript.json")).unwrap(),
        transcript
    );
}

#[test]
fn unused_base_skips_every_taken_number() {
    let root = scratch("export-unused");
    let video = root.join("v.mp4");
    assert_eq!(export::unused_base(&video).file_name().unwrap(), "v");
    fs::write(root.join("v.srt"), b"x").unwrap();
    assert_eq!(export::unused_base(&video).file_name().unwrap(), "v-2");
    fs::write(root.join("v-2.transcript.json"), b"x").unwrap();
    assert_eq!(export::unused_base(&video).file_name().unwrap(), "v-3");
}

#[test]
fn a_transcript_that_is_not_json_is_refused() {
    let root = scratch("export-bad");
    let file = root.join("a.transcript.json");
    fs::write(&file, b"{").unwrap();
    assert_eq!(
        Transcript::load(&file).unwrap_err().to_string(),
        "transcript file: EOF while parsing an object at line 1 column 1"
    );
}

#[test]
fn the_cli_runs_a_whole_job_and_prints_the_outcome() {
    let root = scratch("cli-success");
    ready(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(
        root.join("data/settings.json"),
        // One speaker: the real machine's memory decides whether a Nemotron 3.5 could run here.
        serde_json::to_vec(&Settings {
            speaker_count: 1,
            ..bottom_anchored()
        })
        .unwrap(),
    )
    .unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(stderr(&output), "");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["stage"], "extract_audio");
    let last: Value = serde_json::from_str(lines[lines.len() - 1]).unwrap();
    assert_eq!(last["speaker_count"], 1);
    assert_eq!(last["cue_count"], 1);
    assert_eq!(last["faceless"], false);
    assert_eq!(last["requested_speakers"], 1);
}

#[test]
fn the_cli_reports_failure_on_stderr_with_a_failing_status() {
    let root = scratch("cli-no-model");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("data/settings.json"), br#"{"speaker_count":0}"#).unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(stderr(&output), "model yunet is not installed");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_cli_refuses_a_broken_settings_file() {
    let root = scratch("cli-bad-settings");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("data/settings.json"), b"{").unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(
        stderr(&output),
        "settings file: EOF while parsing an object at line 1 column 1"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_cli_needs_a_home_when_no_data_folder_is_set() {
    let root = scratch("cli-no-home");
    let output = headless(&root, &root.join("clip.mp4"))
        .env_remove("TRANSCRIBE_DATA_DIR")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert_eq!(stderr(&output), "HOME is not set");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_cli_defaults_to_the_folder_under_home() {
    let root = scratch("cli-home");
    let settings = root.join("Library/Application Support/net.teadea.transcribe/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, br#"{"language":""}"#).unwrap();
    let output = headless(&root, &root.join("clip.mp4"))
        .env_remove("TRANSCRIBE_DATA_DIR")
        .env_remove("TRANSCRIBE_RESOURCE_DIR")
        .env_remove("HF_HUB_CACHE")
        .env("HOME", &root)
        .output()
        .unwrap();
    assert_eq!(
        stderr(&output),
        "language must be a code such as ja, got \"\""
    );
}

#[test]
fn a_job_reports_a_checkpoint_that_cannot_convert() {
    let root = scratch("job-bad-checkpoint");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    sparse(
        &paths
            .data
            .join("models/whisper-large-v3-turbo/model.safetensors"),
        WHISPER_SOURCE_BYTES,
    );
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "safetensors header: EOF while parsing a value at line 1 column 0"
    );
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_job_reports_a_failing_audio_tool() {
    let root = scratch("job-bad-ffmpeg");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        "echo 'bad input' >&2; exit 1",
    );
    assert_eq!(
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &bottom_anchored(),
            &mac(),
            &CancelToken::default(),
            &mut |_| {}
        )
        .unwrap_err()
        .to_string(),
        "ffmpeg failed: exit status: 1\nbad input"
    );
}

#[test]
fn relayout_reports_a_face_scan_that_cannot_start() {
    let root = scratch("relayout-no-face-model");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let error = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip"),
        &paths,
        &face_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "model yunet is not installed");
}

fn radeon() -> Hardware {
    Hardware {
        is_mac: false,
        memory_gib: 64,
        gpu: Some(Gpu {
            vendor: GpuVendor::Radeon,
            name: "AMD Radeon RX 7900 XTX".to_owned(),
            memory_gib: 24,
        }),
    }
}

#[test]
fn progress_carries_the_plan_and_reaches_the_end() {
    let root = scratch("job-progress");
    let paths = ready(&root);
    let mut seen: Vec<Progress> = Vec::new();
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| seen.push(progress),
    )
    .unwrap();
    assert_eq!(
        seen[0].plan,
        vec![
            Stage::ExtractAudio,
            Stage::Transcribe,
            Stage::Diarize,
            Stage::Export
        ]
    );
    assert!(seen.iter().any(|progress| !progress.determinate));
    let last = seen.last().unwrap();
    assert_eq!(last.overall, 1.0);
    assert_eq!(last.stage_fraction, 1.0);
    assert!(paths.stage_rates_file().exists());
}

#[test]
fn a_silent_speaker_model_still_moves_the_estimate() {
    let root = scratch("job-ticks");
    let paths = ready(&root);
    let released = root.join("released");
    // Stays silent until the job has reported a tick, whatever the machine's load; gives up in 30 s.
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &format!(
            r#"
released="{released}"
tries=0
while [ ! -f "$released" ] && [ $tries -lt 3000 ]; do
  sleep 0.01
  tries=$((tries + 1))
done
[ -f "$released" ] || exit 8
{NEMO}
"#,
            released = released.display(),
        ),
    );
    let mut diarize_updates = 0;
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| {
            if progress.stage == Stage::Diarize {
                diarize_updates += 1;
            }
            // The first update comes from the job itself, before the tool starts.
            if diarize_updates == 2 {
                fs::write(&released, b"").unwrap();
            }
        },
    )
    .unwrap();
    assert!(diarize_updates >= 2, "{diarize_updates}");
}

#[test]
fn a_radeon_job_runs_the_vulkan_build_on_its_gpu() {
    let root = scratch("job-vulkan");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo-vulkan/nemo-speech"),
        "echo \"$*\" | grep -o -- '--device [a-z]*' >&2; exit 7",
    );
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &radeon(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "nemo-speech failed: exit status: 7\n--device vulkan");
}

#[test]
fn the_vulkan_build_is_used_only_where_it_is_installed() {
    let root = scratch("tools-vulkan");
    let paths = with_tools(&root);
    let fallback = Tools::locate(&paths, Backend::Vulkan).unwrap();
    assert_eq!(fallback.backend, Backend::Default);
    assert!(fallback
        .nemo
        .ends_with("resources/runtime/nemo/bin/nemo-speech"));
    let runtime = paths.bundled_runtime_dir();
    write_tool(&runtime.join("nemo-vulkan/nemo-speech"), "");
    write_tool(&runtime.join("whisper-vulkan/whisper-cli"), "");
    let tools = Tools::locate(&paths, Backend::Vulkan).unwrap();
    assert_eq!(tools.backend, Backend::Vulkan);
    assert!(tools
        .nemo
        .ends_with("resources/runtime/nemo-vulkan/nemo-speech"));
    assert!(tools
        .whisper
        .ends_with("resources/runtime/whisper-vulkan/whisper-cli"));
}

#[test]
fn a_cuda_only_model_asks_for_the_runtime_on_vulkan() {
    let root = scratch("job-needs-cuda");
    let paths = with_tools(&root);
    sparse(
        &root.join("data/models/q2m/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-AD-Q2_0_M.gguf"),
        13_572_208_800,
    );
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-q2m".to_owned(),
        ..bottom_anchored()
    };
    let hardware = Hardware {
        is_mac: false,
        memory_gib: 64,
        gpu: Some(Gpu {
            vendor: GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 4080".to_owned(),
            memory_gib: 16,
        }),
    };
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &hardware,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "model geforce-runtime is not installed");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_video_too_long_for_the_model_fails_before_any_long_work() {
    let root = scratch("job-too-long");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffprobe"),
        r#"echo '{"format":{"duration":"3601"},"streams":[{"codec_type":"video","width":2,"height":2},{"codec_type":"audio"}]}'"#,
    );
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-q2l".to_owned(),
        ..bottom_anchored()
    };
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "model nemotron-3.5-transcribe-q2l takes recordings up to 60 minutes"
    );
    assert!(!paths.jobs_dir().exists());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn one_known_speaker_skips_separation_entirely() {
    let root = scratch("job-one-speaker");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'nemo must not run' >&2; exit 9",
    );
    let settings = Settings {
        speaker_count: 1,
        ..bottom_anchored()
    };
    let mut plan: Vec<Stage> = Vec::new();
    let mut stages: Vec<Stage> = Vec::new();
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| {
            plan.clone_from(&progress.plan);
            stages.push(progress.stage);
        },
    )
    .unwrap();
    assert_eq!(
        plan,
        vec![Stage::ExtractAudio, Stage::Transcribe, Stage::Export]
    );
    assert!(!stages.contains(&Stage::Diarize));
    assert_eq!(outcome.speaker_count, 1);
    assert_eq!(outcome.requested_speakers, 1);
}

/// Logs each call beside the job, then writes `body`.
fn counting_nemo(root: &Path, body: &str) -> String {
    format!(
        r#"
echo "$*" >> "{log}"
while [ $# -gt 0 ]; do
  if [ "$1" = "--output" ]; then out="$2"; fi
  shift
done
echo '{body}' > "$out"
"#,
        log = root.join("nemo-calls.log").display(),
    )
}

#[test]
fn a_known_count_asks_once_and_missing_voices_are_not_invented() {
    let root = scratch("job-llm-no-retry");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(Q2L_FILE), Q2L_BYTES);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    let one_word = r#"{"words":[{"word":"a","start":0.0,"end":2.0,"speaker":1}]}"#;
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &counting_nemo(&root, one_word),
    );
    let settings = Settings {
        diarize_model: "nemotron-3.5-transcribe-q2l".to_owned(),
        speaker_count: 2,
        ..bottom_anchored()
    };
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &settings,
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let log = fs::read_to_string(root.join("nemo-calls.log")).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(log.contains("--instruction"));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn relayout_reports_the_requested_speaker_count() {
    let root = scratch("relayout-requested");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let outcome = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip"),
        &paths,
        &Settings {
            speaker_count: 2,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.requested_speakers, 2);
    assert_eq!(outcome.speaker_count, 2);
}

fn geforce() -> Hardware {
    Hardware {
        is_mac: false,
        memory_gib: 64,
        gpu: Some(Gpu {
            vendor: GpuVendor::Nvidia,
            name: "NVIDIA GeForce RTX 4090".to_owned(),
            memory_gib: 24,
        }),
    }
}

#[test]
fn a_geforce_without_its_runtime_separates_speakers_on_the_cpu_build() {
    let root = scratch("job-geforce-no-runtime");
    let paths = ready(&root);
    let runtime = paths.bundled_runtime_dir();
    write_tool(&runtime.join("whisper-vulkan/whisper-cli"), WHISPER);
    write_tool(
        &runtime.join("nemo/bin/nemo-speech"),
        "echo \"$*\" | grep -o -- '--device [a-z]*' >&2; exit 7",
    );
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &geforce(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "nemo-speech failed: exit status: 7\n--device cpu");
}

#[test]
fn the_cuda_runtime_pairs_with_the_vulkan_transcriber() {
    let root = scratch("tools-cuda-vulkan");
    let paths = with_tools(&root);
    let runtime = paths.bundled_runtime_dir();
    write_tool(&runtime.join("whisper-vulkan/whisper-cli"), "");
    write_tool(
        &paths
            .downloaded_runtime_dir()
            .join("geforce/nemo/nemo-speech"),
        "",
    );
    let tools = Tools::locate(&paths, Backend::Cuda).unwrap();
    assert_eq!(tools.backend, Backend::Cuda);
    assert!(tools
        .nemo
        .ends_with("data/runtime/geforce/nemo/nemo-speech"));
    assert!(tools
        .whisper
        .ends_with("resources/runtime/whisper-vulkan/whisper-cli"));
}

#[test]
fn the_speakers_on_cpu_backend_uses_vulkan_only_for_transcription() {
    let root = scratch("tools-speakers-cpu");
    let paths = with_tools(&root);
    let runtime = paths.bundled_runtime_dir();
    write_tool(&runtime.join("nemo-vulkan/nemo-speech"), "");
    write_tool(&runtime.join("whisper-vulkan/whisper-cli"), "");
    let tools = Tools::locate(&paths, Backend::SpeakersOnCpu).unwrap();
    assert_eq!(tools.backend, Backend::SpeakersOnCpu);
    assert!(tools
        .nemo
        .ends_with("resources/runtime/nemo/bin/nemo-speech"));
    assert!(tools
        .whisper
        .ends_with("resources/runtime/whisper-vulkan/whisper-cli"));
}

#[test]
fn a_missing_vulkan_speaker_build_keeps_vulkan_transcription() {
    let root = scratch("tools-vulkan-no-nemo");
    let paths = with_tools(&root);
    write_tool(
        &paths
            .bundled_runtime_dir()
            .join("whisper-vulkan/whisper-cli"),
        "",
    );
    assert_eq!(
        Tools::locate(&paths, Backend::Vulkan).unwrap().backend,
        Backend::SpeakersOnCpu
    );
}

#[test]
fn only_a_geforce_lists_its_runtime() {
    let root = scratch("statuses-runtime");
    let paths = with_tools(&root);
    let listed = |hardware: &Hardware| {
        catalog::statuses(hardware, &paths, &[])
            .iter()
            .any(|row| row.id == "geforce-runtime")
    };
    assert!(listed(&geforce()));
    assert!(!listed(&mac()));
    assert!(!listed(&radeon()));
}

#[test]
fn a_missing_face_model_fails_before_any_work_folder_exists() {
    let root = scratch("job-face-early");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffprobe"),
        "echo 'probe must not run' >&2; exit 9",
    );
    let error = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &face_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "model yunet is not installed");
    assert!(!paths.jobs_dir().exists());
}

#[test]
fn a_job_writes_a_resolve_timeline_when_nothing_was_chosen() {
    let root = scratch("job-default-format");
    let paths = ready(&root);
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(names(&outcome.outputs), vec!["clip.drt"]);
    assert!(!root.join("clip.fcpxml").exists());
    assert!(!root.join("clip.srt").exists());
    assert!(!root.join("clip.ttml").exists());
}

#[test]
fn a_second_job_in_another_format_still_takes_a_new_name() {
    let root = scratch("job-other-format");
    let paths = ready(&root);
    let run = |format: SubtitleFormat| {
        pipeline::run_job(
            &root.join("clip.mp4"),
            &paths,
            &Settings {
                subtitle_format: format,
                ..bottom_anchored()
            },
            &mac(),
            &CancelToken::default(),
            &mut |_| {},
        )
        .unwrap()
    };
    assert_eq!(names(&run(SubtitleFormat::Srt).outputs), vec!["clip.srt"]);
    assert_eq!(
        names(&run(SubtitleFormat::Ttml).outputs),
        vec!["clip-2.ttml"]
    );
    assert!(root.join("clip-2.transcript.json").exists());
}

#[test]
fn writing_captions_again_follows_the_chosen_format() {
    let root = scratch("relayout-format");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let outcome = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip-2"),
        &paths,
        &Settings {
            subtitle_format: SubtitleFormat::Ttml,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(names(&outcome.outputs), vec!["clip-2.ttml"]);
    assert!(!root.join("clip-2.fcpxml").exists());
    assert!(root.join("clip-2.transcript.json").exists());
}

#[test]
fn the_cli_writes_the_format_in_its_settings() {
    let root = scratch("cli-format");
    ready(&root);
    fs::write(
        root.join("data/settings.json"),
        serde_json::to_vec(&Settings {
            subtitle_format: SubtitleFormat::Srt,
            ..bottom_anchored()
        })
        .unwrap(),
    )
    .unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(stderr(&output), "");
    assert!(root.join("clip.srt").exists());
    assert!(root.join("clip.transcript.json").exists());
    assert!(!root.join("clip.fcpxml").exists());
    assert!(!root.join("clip.ttml").exists());
}

#[test]
fn voice_spans_are_read_from_the_tool() {
    let root = scratch("voice-spans");
    let paths = with_tools(&root);
    let voice = engines::voice_spans(&tools_in(&paths), &root.join("a.wav")).unwrap();
    assert_eq!(
        voice,
        vec![transcribe_app::captions::VoiceSpan {
            start: 0.0,
            end: 60.0
        }]
    );
}

#[test]
fn a_voice_detector_that_fails_stops_the_job() {
    let root = scratch("voice-spans-failing");
    let paths = with_tools(&root);
    write_tool(
        &paths
            .bundled_runtime_dir()
            .join("whisper/whisper-vad-speech-segments"),
        "exit 3\n",
    );
    assert!(engines::voice_spans(&tools_in(&paths), &root.join("a.wav")).is_err());
}

#[test]
fn writing_a_timeline_from_an_old_transcript_reads_the_audio_from_the_video() {
    let root = scratch("relayout-old-audio");
    let paths = ready(&root);
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            subtitle_format: SubtitleFormat::Srt,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        Transcript::load(&root.join("clip.transcript.json"))
            .unwrap()
            .info
            .audio,
        None
    );
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffprobe"),
        r#"echo '{"format":{"duration":"2.0"},"streams":[{"codec_type":"video","width":1280,"height":720,"avg_frame_rate":"30/1","nb_frames":"60"},{"codec_type":"audio","sample_rate":"48000","channels":2,"duration":"2.0"}]}'"#,
    );
    pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip-2"),
        &paths,
        &bottom_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        Transcript::load(&root.join("clip-2.transcript.json"))
            .unwrap()
            .info
            .audio
            .map(|audio| audio.sample_rate),
        Some(48_000)
    );
    assert!(root.join("clip-2.drt").exists());
}

//! Whole-job, catalog and process tests over shell-script fake tools and size-matched sparse models.
#![cfg(unix)]

use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use transcribe_app::catalog::{self, Backend, Gpu, GpuVendor, Hardware};
use transcribe_app::engines::{self, DiarizeModel, Tools, VideoInfo};
use transcribe_app::error::AppError;
use transcribe_app::eta::{Progress, Stage};
use transcribe_app::export::{self, Transcript};
use transcribe_app::face::{scan_faces, FaceDetector, FaceModelKind, FaceSample};
use transcribe_app::paths::AppPaths;
use transcribe_app::pipeline;
use transcribe_app::process::CancelToken;
use transcribe_app::settings::{Anchor, Placement, SeparationBackend, Settings, SubtitleFormat};

const MOSSFORMER2_FILE: &str = "models/mossformer2-ss-16k/model.onnx";
const MOSSFORMER2_BYTES: u64 = 230_156_008;
const ENCODER_FILE: &str =
    "models/nemotron-3.5-speaker/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf";
const ENCODER_BYTES: u64 = 200_779_392;
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
  *) echo 'frame=1 time=00:00:01.00 bitrate=1kbits/s' >&2; echo 'no status here' >&2
     for last; do :; done
     printf 'RIFF\044\0\0\0WAVEfmt \020\0\0\0\001\0\001\0\200>\0\0\0}\0\0\002\0\020\0data\0\0\0\0' > "$last" ;;
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
echo 'whisper_print_progress_callback: progress =  50%' >&2
echo 'some other line' >&2
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then
    cat > "$2.json" <<'EOF'
{"transcription":[{"tokens":[
  {"text":"[_BEG_]","offsets":{"from":0,"to":0}},
  {"text":"はい","offsets":{"from":100,"to":900}},
  {"text":"どうも","offsets":{"from":1000,"to":1800}}]}]}
EOF
  fi
  shift
done
"#;
/// `nemo-speech serve` for one fixed answer; a job ends it by killing it, as the app does.
const NEMO_SERVER: &str = r#"
while [ $# -gt 0 ]; do
  if [ "$1" = "--port" ]; then port="$2"; fi
  shift
done
exec python3 -c '
import http.server, sys
class Fake(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass
    def answer(self, code, body):
        self.send_response(code)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        self.answer(200, b"ok")
    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        self.answer(STATUS, BODY)
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), Fake).serve_forever()
' "$port"
"#;
const TWO_VOICES: &str =
    r#"{"segments":[{"start":0.0,"end":0.95,"speaker":1},{"start":0.95,"end":2.0,"speaker":2}]}"#;
/// ModelProto with one Identity node: lets the real detector run without a real face model.
const IDENTITY_ONNX: &[u8] = b"\x08\x08\x3a\x2b\x0a\x10\x0a\x01x\x12\x01y\x22\x08Identity\x12\x01g\x5a\x09\x0a\x01x\x12\x04\x0a\x02\x08\x01\x62\x09\x0a\x01y\x12\x04\x0a\x02\x08\x01\x42\x02\x10\x0d";

fn fake_server(status: u16, body: &str) -> String {
    NEMO_SERVER
        .replace("STATUS", &status.to_string())
        .replace("BODY", &format!("b\"\"\"{body}\"\"\""))
}

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
    write_tool(
        &runtime.join("nemo/bin/nemo-speech"),
        &fake_server(200, TWO_VOICES),
    );
    install_ipadic(&paths);
    install_speaker_decoder_head(&paths);
    paths
}

fn install_speaker_decoder_head(paths: &AppPaths) {
    let head = paths.resources.join("resources/speaker-decoder-head.gguf");
    fs::create_dir_all(head.parent().unwrap()).unwrap();
    // A copy, not a hard link: tests that write this path must never reach the real file.
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/speaker-decoder-head.gguf"),
        head,
    )
    .unwrap();
}

/// A clone on APFS: the 8 MB dictionary takes no new space per test, and writing to it cannot reach the real file.
fn install_ipadic(paths: &AppPaths) {
    let installed = paths.resources.join("resources/ipadic-mecab-2_7_0.dic.zst");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::copy(
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
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    sparse(&paths.data.join(MOSSFORMER2_FILE), MOSSFORMER2_BYTES);
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
    // Each voice talks for under a second right beside the other: one caption, one colour.
    assert_eq!(outcome.cue_count, 1);
    assert!(!outcome.faceless);
    assert_eq!(
        fs::read_to_string(root.join("clip.srt")).unwrap(),
        "1\n00:00:00,100 --> 00:00:01,870\n<font color=\"#005A9C\">はいどうも</font>\n\n"
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
fn a_job_with_a_speech_llm_takes_its_speaker_turns() {
    let root = scratch("job-speech-llm");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    sparse(&paths.data.join(MOSSFORMER2_FILE), MOSSFORMER2_BYTES);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &counting_nemo(
            &root,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1}]}"#,
        ),
    );
    let settings = Settings {
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
    let mut plan: Vec<Stage> = Vec::new();
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &face_anchored(),
        &mac(),
        &CancelToken::default(),
        &mut |progress: Progress| plan = progress.plan,
    )
    .unwrap();
    assert!(plan.contains(&Stage::Faces));
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

/// `run_job` for `speaker_count` people on `hardware`, with a Nemotron that must not start.
fn refusal(paths: &AppPaths, root: &Path, speaker_count: u32, hardware: &Hardware) -> String {
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'nemo must not run' >&2; exit 9",
    );
    let mut plan: Vec<Stage> = Vec::new();
    let refused = pipeline::run_job(
        &root.join("clip.mp4"),
        paths,
        &Settings {
            speaker_count,
            ..bottom_anchored()
        },
        hardware,
        &CancelToken::default(),
        &mut |progress: Progress| plan.clone_from(&progress.plan),
    )
    .unwrap_err();
    assert_eq!(plan, Vec::<Stage>::new());
    refused.to_string()
}

#[test]
fn several_people_without_either_model_are_refused_before_any_work() {
    let root = scratch("job-no-model");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    assert_eq!(
        refusal(&paths, &root, 3, &mac()),
        "separating speakers needs models that are missing: [Diarize, Separate]"
    );
    assert_eq!(
        refusal(&paths, &root, 0, &mac()),
        "separating speakers needs models that are missing: [Diarize, Separate]"
    );
}

#[test]
fn several_people_without_mossformer2_stop_and_name_it() {
    let root = scratch("job-no-mossformer2");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    assert_eq!(
        refusal(&paths, &root, 2, &mac()),
        "separating speakers needs models that are missing: [Separate]"
    );
}

#[test]
fn several_people_on_too_little_memory_stop_and_say_how_much_it_takes() {
    let root = scratch("job-low-memory");
    let paths = ready(&root);
    let low = Hardware {
        is_mac: false,
        memory_gib: 4,
        gpu: None,
    };
    let refused = refusal(&paths, &root, 2, &low);
    assert!(refused.contains("mossformer2-ss-16k"), "{refused}");
    assert!(refused.contains('8'), "{refused}");
    assert!(refused.contains('4'), "{refused}");
}

#[test]
fn the_speaker_server_is_gone_when_the_job_ends_so_its_memory_is_free() {
    let root = scratch("job-server-exit");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    install_speech_llm(&paths);
    install_speaker_decoder_head(&paths);
    let pid_file = root.join("pid");
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &format!(
            "echo $$ > '{}'\n{}",
            pid_file.display(),
            fake_server(200, TWO_VOICES)
        ),
    );
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 0,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    let pid = fs::read_to_string(pid_file).unwrap();
    let alive = Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!alive.success());
}

#[test]
fn one_person_needs_neither_model() {
    let root = scratch("job-one-person");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'nemo must not run' >&2; exit 9",
    );
    let outcome = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 1,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.speaker_count, 1);
    assert_eq!(outcome.requested_speakers, 1);
}

#[test]
fn writing_captions_again_needs_no_speaker_models() {
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
    fs::remove_file(paths.data.join(MOSSFORMER2_FILE)).unwrap();
    let outcome = pipeline::relayout_to(
        &root.join("clip.transcript.json"),
        &root.join("clip-2"),
        &paths,
        &Settings {
            speaker_count: 3,
            ..bottom_anchored()
        },
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome.requested_speakers, 3);
}

#[test]
fn a_job_refuses_a_model_too_large_for_the_machine() {
    let root = scratch("job-too-large");
    let paths = with_tools(&root);
    install_speech_llm(&paths);
    let settings = Settings {
        ..bottom_anchored()
    };
    let hardware = Hardware {
        memory_gib: 2,
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
        "model mossformer2-ss-16k needs 8 GiB of unified memory, this machine has 2 GiB"
    );
}

#[test]
fn a_job_refuses_a_chosen_model_that_is_not_installed() {
    let root = scratch("job-not-installed");
    let paths = with_tools(&root);
    let settings = Settings {
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
        "separating speakers needs models that are missing: [Diarize, Separate]"
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
        &CancelToken::default(),
        &mut |progress: Progress| stages.push(progress.stage),
    )
    .unwrap();
    assert_eq!(stages, vec![Stage::Export, Stage::Export]);
    assert_eq!(outcome.base.file_name().unwrap(), "clip");
    assert_eq!(outcome.cue_count, 1);
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
    assert_eq!(seen, vec![0.45]);
    assert_eq!(words[0].text, "はい");
    assert_eq!(words[1].text, "どうも");
    assert_eq!(words.len(), 2);
}

#[test]
fn only_the_range_that_looped_is_heard_again_without_context() {
    let root = scratch("transcribe-loop");
    let paths = with_tools(&root);
    let audio = root.join("a.wav");
    fs::write(&audio, pcm_wav(&vec![8192; 16_000 * 40])).unwrap();
    let log = root.join("whisper-calls.log");
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        &format!(
            r#"
echo "$*" >> "{log}"
case "$*" in *"-mc 0"*) fresh=1 ;; *) fresh=0 ;; esac
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then out="$2"; fi
  shift
done
if [ $fresh = 1 ]; then
cat > "$out.json" <<'EOF'
{{"transcription":[{{"offsets":{{"from":0,"to":1000}},"text":"fresh","tokens":[
  {{"text":"fresh","offsets":{{"from":0,"to":1000}}}}]}}]}}
EOF
else
cat > "$out.json" <<'EOF'
{{"transcription":[
  {{"offsets":{{"from":0,"to":1000}},"text":"before","tokens":[{{"text":"before","offsets":{{"from":0,"to":1000}}}}]}},
  {{"offsets":{{"from":10000,"to":12000}},"text":"again","tokens":[{{"text":"again","offsets":{{"from":10000,"to":12000}}}}]}},
  {{"offsets":{{"from":12000,"to":14000}},"text":"again","tokens":[{{"text":"again","offsets":{{"from":12000,"to":14000}}}}]}},
  {{"offsets":{{"from":30000,"to":31000}},"text":"after","tokens":[{{"text":"after","offsets":{{"from":30000,"to":31000}}}}]}}]}}
EOF
fi
"#,
            log = log.display(),
        ),
    );
    let words = engines::transcribe(
        &tools_in(&paths),
        &root.join("model.bin"),
        &audio,
        "ja",
        &root,
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        words
            .iter()
            .map(|word| (word.text.as_str(), word.start))
            .collect::<Vec<_>>(),
        vec![("before", 0.0), ("fresh", 8.0), ("after", 30.0)]
    );
    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 2);
    assert!(calls.lines().nth(1).unwrap().contains("asr-loop-0.wav"));
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
fn a_server_that_exits_at_once_is_an_error() {
    let root = scratch("diarize-empty");
    let paths = with_tools(&root);
    install_speaker_decoder_head(&paths);
    fs::write(root.join("a.wav"), pcm_wav(&[0; 1600])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "",
    );
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: None,
        device: "cpu",
        resources: &paths.resources,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "nemo-speech failed: the speaker server stopped before it was ready"
    );
}

fn speech_llm_on_its_stand_in(root: &Path) -> (DiarizeModel<'_>, PathBuf) {
    let resources = root.join("resources/resources");
    fs::create_dir_all(&resources).unwrap();
    let head = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/speaker-decoder-head.gguf");
    fs::copy(head, resources.join("speaker-decoder-head.gguf")).unwrap();
    let wav = root.join("a.wav");
    fs::write(&wav, pcm_wav(&[0; 1600])).unwrap();
    (
        DiarizeModel {
            entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
            encoder: Some(Path::new("e.gguf")),
            device: "vulkan",
            resources: leak_path(root.join("resources")),
        },
        wav,
    )
}

fn leak_path(path: PathBuf) -> &'static Path {
    Box::leak(path.into_boxed_path())
}

#[test]
fn a_gpu_the_stand_in_fails_on_gives_way_to_the_cpu() {
    let root = scratch("diarize-stand-in-cpu");
    let paths = with_tools(&root);
    let log = root.join("nemo-calls.log");
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &format!(
            "echo \"$*\" >> \"{log}\"\ncase \"$*\" in *\"--device vulkan\"*) echo 'no vulkan' >&2; exit 7 ;; esac\n{server}",
            log = log.display(),
            server = fake_server(200, TWO_VOICES),
        ),
    );
    let (model, wav) = speech_llm_on_its_stand_in(&root);
    let turns = engines::diarize(
        &tools_in(&paths),
        &model,
        &wav,
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap();
    assert_eq!(turns.len(), 2);
    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 2);
    assert!(
        calls.lines().nth(1).unwrap().contains("--device cpu"),
        "{calls}"
    );
    assert!(calls.contains("speaker-decoder-head.gguf"), "{calls}");
}

#[test]
fn a_stand_in_that_fails_on_the_cpu_too_says_why() {
    let root = scratch("diarize-stand-in-fails");
    let paths = with_tools(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "echo 'bad gguf' >&2; exit 7",
    );
    let (model, wav) = speech_llm_on_its_stand_in(&root);
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &wav,
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "nemo-speech failed: exit status: 7\nbad gguf");
}

#[test]
fn without_the_head_the_job_stops_with_the_reason() {
    let root = scratch("diarize-no-head");
    let paths = with_tools(&root);
    let wav = root.join("a.wav");
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: Some(Path::new("e.gguf")),
        device: "auto",
        resources: &root,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &wav,
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("read the speaker decoder: "), "{error}");
}

#[test]
fn audio_that_cannot_be_read_stops_the_server_too() {
    let root = scratch("diarize-no-audio");
    let paths = with_tools(&root);
    install_speaker_decoder_head(&paths);
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: None,
        device: "cpu",
        resources: &paths.resources,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("missing.wav"),
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.starts_with("read the audio for speakers: "),
        "{error}"
    );
}

#[test]
fn a_refusal_from_the_server_says_why() {
    let root = scratch("diarize-refused");
    let paths = with_tools(&root);
    install_speaker_decoder_head(&paths);
    fs::write(root.join("a.wav"), pcm_wav(&[0; 1600])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(422, "bad audio"),
    );
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: None,
        device: "cpu",
        resources: &paths.resources,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "nemo-speech failed: 422 Unprocessable Entity: bad audio"
    );
}

#[test]
fn turns_come_from_the_server_and_it_is_stopped_afterwards() {
    let root = scratch("diarize-answer");
    let paths = with_tools(&root);
    install_speaker_decoder_head(&paths);
    fs::write(root.join("a.wav"), pcm_wav(&[0; 1600])).unwrap();
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: None,
        device: "cpu",
        resources: &paths.resources,
    };
    let turns = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        &root,
        &CancelToken::default(),
        &mut || {},
    )
    .unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[1].speaker, 2);
}

#[test]
fn cancelling_stops_the_speaker_server() {
    let root = scratch("diarize-cancel");
    let paths = with_tools(&root);
    install_speaker_decoder_head(&paths);
    fs::write(root.join("a.wav"), pcm_wav(&[0; 1600])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        "exec sleep 60",
    );
    let model = DiarizeModel {
        entry: catalog::find_entry("nemotron-3.5-speaker").unwrap(),
        encoder: None,
        device: "cpu",
        resources: &paths.resources,
    };
    let cancel = CancelToken::default();
    let began = std::time::Instant::now();
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
        &root,
        &cancel,
        &mut || cancel.cancel(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "cancelled");
    assert!(began.elapsed().as_secs() < 30);
}

#[test]
fn a_model_that_hears_nobody_is_reported_on_the_job() {
    let root = scratch("job-no-speakers");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    sparse(&paths.data.join(MOSSFORMER2_FILE), MOSSFORMER2_BYTES);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(200, r#"{"segments":[]}"#),
    );
    let settings = Settings {
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
    assert!(outcome.speakers_missing);
    assert_eq!(outcome.speaker_count, 1);
}

#[test]
fn only_diarization_models_can_diarize() {
    let root = scratch("diarize-wrong-role");
    let paths = with_tools(&root);
    let model = DiarizeModel {
        entry: catalog::find_entry("yunet").unwrap(),
        encoder: None,
        device: "auto",
        resources: &root,
    };
    let error = engines::diarize(
        &tools_in(&paths),
        &model,
        &root.join("a.wav"),
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
fn a_video_with_too_few_keyframes_is_read_whole_and_the_bar_does_not_go_back() {
    let root = scratch("scan-whole");
    let paths = with_tools(&root);
    install_yunet(&paths);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        "case \"$*\" in *skip_frame*) count=1 ;; *) count=6 ;; esac\nhead -c $((640 * 640 * 3 * count)) /dev/zero\n",
    );
    let detector = FaceDetector::load(&paths.yunet_model(), FaceModelKind::Yunet).unwrap();
    let video = VideoInfo {
        duration: 10.0,
        ..VIDEO
    };
    let mut seen = Vec::new();
    let samples = scan_faces(
        &tools_in(&paths),
        &detector,
        &root.join("clip.mp4"),
        &video,
        &CancelToken::default(),
        &mut |fraction| seen.push((fraction * 10.0_f64).round()),
    )
    .unwrap();
    assert_eq!(samples, Vec::<FaceSample>::new());
    assert_eq!(seen, vec![1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
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
fn a_video_ffmpeg_cannot_decode_fails_the_scan_with_the_reason() {
    let root = scratch("scan-undecodable");
    let paths = with_tools(&root);
    install_yunet(&paths);
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        "echo 'no AV1 decoder' >&2; exit 69",
    );
    let detector = FaceDetector::load(&paths.yunet_model(), FaceModelKind::Yunet).unwrap();
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
    assert_eq!(error, "ffmpeg failed: exit status: 69\nno AV1 decoder");
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
    let file = extra.join("a/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf");
    sparse(&file, 5);
    assert_eq!(
        catalog::speech_llm_encoder(std::slice::from_ref(&extra), &paths),
        None
    );
    sparse(&file, ENCODER_BYTES);
    assert!(catalog::speech_llm_encoder(&[extra], &paths)
        .unwrap()
        .ends_with("user/a/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf"));
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
        &root.join(
            "data/models/.cache/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf",
        ),
        ENCODER_BYTES,
    );
    sparse(
        &root.join(
            "data/models/a/b/c/d/e/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf",
        ),
        ENCODER_BYTES,
    );
    assert_eq!(catalog::speech_llm_encoder(&[], &paths), None);
    sparse(
        &root.join(
            "data/models/a/b/c/d/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf",
        ),
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
        &root.join("hub/models--kkk4oru--nemotron-3.5-speaker-encoder-gguf/snapshots/abc/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf"),
        ENCODER_BYTES,
    );
    assert!(catalog::speech_llm_encoder(&[], &paths).unwrap().ends_with(
        "snapshots/abc/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf"
    ));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn an_encoder_an_earlier_download_left_is_used_without_another_download() {
    let root = scratch("locate-earlier-encoder");
    let paths = with_tools(&root);
    sparse(
        &root.join("elsewhere/NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"),
        1_670_410_080,
    );
    let found = catalog::speech_llm_encoder(&[root.join("elsewhere")], &paths).unwrap();
    assert!(found.ends_with("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Encoder-F16.gguf"));
}

#[test]
fn the_speaker_branch_is_found_by_its_encoder() {
    let root = scratch("locate-speech-llm");
    let paths = AppPaths {
        data: root.join("data"),
        resources: root.join("resources"),
        hf_hub: None,
    };
    let entry = catalog::find_entry("nemotron-3.5-speaker").unwrap();
    assert_eq!(catalog::locate(entry, &paths, &[]), None);
    assert_eq!(catalog::speech_llm_encoder(&[], &paths), None);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    assert!(catalog::locate(entry, &paths, &[])
        .unwrap()
        .ends_with("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf"));
    assert!(catalog::speech_llm_encoder(&[], &paths)
        .unwrap()
        .ends_with("NVIDIA-Nemotron-3.5-Transcribe-30B-A4B-Speaker-Encoder-F16.gguf"));
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
            "nemotron-3.5-speaker",
            "mossformer2-ss-16k",
            "mossformer2-ss-16k-coreml",
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
            "nemotron-3.5-speaker",
            "mossformer2-ss-16k",
            "yunet"
        ]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.found_at.is_some())
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        vec![
            "whisper-large-v3-turbo",
            "nemotron-3.5-speaker",
            "mossformer2-ss-16k"
        ]
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.blocked.is_some())
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
    assert_eq!(rows[0].download_bytes, 1_617_824_864);
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
    fs::write(root.join("data/settings.json"), br#"{"speaker_count":1}"#).unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(stderr(&output), "model yunet is not installed");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_cli_refuses_several_people_without_the_models() {
    let root = scratch("cli-no-speaker-models");
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("data/settings.json"), br#"{"speaker_count":0}"#).unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(
        stderr(&output),
        "separating speakers needs models that are missing: [Diarize, Separate]"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// `tracks` = the wave, and the wave at half level, as `[2, 1, samples]`.
const TWO_TRACKS_MODEL: &str = "08083ab5010a1a0a04776176650a04617865731201612209556e73717565657a650a110a01610a0468616c6612016222034d756c0a230a01610a01621206747261636b732206436f6e6361742a0b0a04617869731800a001021201672a14080110074204617865734a0800000000000000002a0e1001420468616c664a040000003f5a170a0477617665120f0a0d080112090a0208010a0312016e621d0a06747261636b7312130a110801120d0a0208020a0208010a0312016e42040a001011";

fn pcm_wav(samples: &[i16]) -> Vec<u8> {
    let bytes = samples.len() as u32 * 2;
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&16_000u32.to_le_bytes());
    wav.extend_from_slice(&32_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    samples
        .iter()
        .for_each(|sample| wav.extend_from_slice(&sample.to_le_bytes()));
    wav
}

/// ONNX Runtime from the development runtime, in the app's own child process.
fn separated(root: &Path, library: &Path) -> Result<Vec<f32>, String> {
    let model = root.join("model.onnx");
    let bytes: Vec<u8> = (0..TWO_TRACKS_MODEL.len() / 2)
        .map(|at| u8::from_str_radix(&TWO_TRACKS_MODEL[at * 2..at * 2 + 2], 16).unwrap())
        .collect();
    fs::write(&model, bytes).unwrap();
    fs::write(root.join("in.wav"), pcm_wav(&[8192, -8192, 16384, -16384])).unwrap();
    let clips = [transcribe_app::separation::SeparationClip {
        input: root.join("in.wav"),
        outputs: [root.join("a.wav"), root.join("b.wav")],
    }];
    transcribe_app::separation::separate_clips(
        &transcribe_app::separation::SeparationRun {
            flag: Some(transcribe_app::separation::SEPARATE_FLAG),
            exe: Path::new(env!("CARGO_BIN_EXE_transcribe-app")),
            library,
            model: &model,
            runs_on: catalog::RunsOn::Cpu,
            work: root,
            cancel: &CancelToken::default(),
        },
        &clips,
        &mut |_| {},
    )
    .map_err(|error| error.to_string())?;
    let track = fs::read(root.join("b.wav")).unwrap();
    Ok(track[44..]
        .chunks_exact(4)
        .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
        .collect())
}

/// The fake runtime has no ONNX Runtime, so asking for it shows the overlap was acted on.
#[test]
fn people_talking_at_once_go_through_voice_separation() {
    let root = scratch("job-overlap");
    let paths = ready(&root);
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(
            200,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1},{"start":0.5,"end":2.0,"speaker":2}]}"#,
        ),
    );
    let failure = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 2,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(failure.ends_with("is missing"), "{failure}");
}

/// `TWO_TRACKS_MODEL` at the size the catalog wants: an unknown field of zeros pads it, which ONNX skips.
fn padded_model(file: &Path, model: &str, size: u64) {
    let mut bytes: Vec<u8> = (0..model.len() / 2)
        .map(|at| u8::from_str_radix(&model[at * 2..at * 2 + 2], 16).unwrap())
        .collect();
    // Field 1000, length-delimited, with a varint length the header's own size is counted out of.
    let header = [0xc2, 0x3e];
    let length_bytes = 4;
    let mut padding = size - bytes.len() as u64 - header.len() as u64 - length_bytes;
    bytes.extend_from_slice(&header);
    let mut left = length_bytes;
    while left > 0 {
        let more = if left > 1 { 0x80 } else { 0 };
        bytes.push((padding & 0x7f) as u8 | more);
        padding >>= 7;
        left -= 1;
    }
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
    File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_len(size)
        .unwrap();
}

/// The whole way from a job to its rows, with the real app, ONNX Runtime and a stand-in model.
#[test]
fn people_talking_at_once_come_out_with_the_rows_decided_from_their_tracks() {
    let root = scratch("cli-overlap");
    let paths = ready(&root);
    fs::create_dir_all(root.join("audio")).unwrap();
    let audio = root.join("audio/audio.wav");
    fs::write(&audio, pcm_wav(&vec![8192; 32_000])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        &format!(
            "echo 'frame=1 time=00:00:01.00 bitrate=1kbits/s' >&2\nfor last; do :; done\ncp '{}' \"$last\"\n",
            audio.display()
        ),
    );
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(
            200,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1},{"start":0.5,"end":2.0,"speaker":2}]}"#,
        ),
    );
    padded_model(
        &paths.data.join(MOSSFORMER2_FILE),
        TWO_TRACKS_MODEL,
        MOSSFORMER2_BYTES,
    );
    let library = paths
        .bundled_runtime_dir()
        .join("onnxruntime/libonnxruntime.dylib");
    fs::create_dir_all(library.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/onnxruntime/libonnxruntime.dylib"),
        &library,
    )
    .unwrap();
    fs::write(
        root.join("data/settings.json"),
        serde_json::to_vec(&Settings {
            speaker_count: 2,
            subtitle_format: SubtitleFormat::Srt,
            ..bottom_anchored()
        })
        .unwrap(),
    )
    .unwrap();
    let output = headless(&root, &root.join("clip.mp4"))
        .env("HOME", &root)
        .output()
        .unwrap();
    assert_eq!(stderr(&output), "");
    assert!(!root.join("Library/Application Support/Microsoft").exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let outcome: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    // Every run of the fake Whisper says the same words: one row, in the first pass's text.
    assert_eq!(outcome["lanes"]["ranges"], 1);
    assert_eq!(outcome["lanes"]["same_text"], 1);
    assert_eq!(outcome["lanes"]["first_pass_kept"], 1);
    assert_eq!(outcome["lanes"]["rows"]["1"], 1);
}

/// Speakers without a speaker model, through the app's own child process on ONNX Runtime.
/// With the hardened runtime an ad hoc signed app cannot load ONNX Runtime, so MossFormer2 fails.
#[test]
fn the_mac_app_is_signed_so_that_it_can_load_onnx_runtime() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["bundle"]["macOS"]["signingIdentity"], "-");
    assert_eq!(config["bundle"]["macOS"]["hardenedRuntime"], false);
}

/// An hour and a bit: the second batch starts inside the first and takes over where both agree.
#[test]
fn long_audio_is_heard_in_overlapping_batches_joined_where_they_agree() {
    let root = scratch("transcribe-batches");
    let paths = with_tools(&root);
    let audio = root.join("a.wav");
    let head = pcm_wav(&[]);
    let mut wav = fs::File::create(&audio).unwrap();
    wav.write_all(&head[..4]).unwrap();
    wav.write_all(&(36_u32 + 118_400_000).to_le_bytes())
        .unwrap();
    wav.write_all(&head[8..40]).unwrap();
    wav.write_all(&118_400_000_u32.to_le_bytes()).unwrap();
    wav.set_len(44 + 118_400_000).unwrap();
    let log = root.join("whisper-calls.log");
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        &format!(
            r#"
echo "$*" >> "{log}"
case "$*" in *"-mc 0"*) fresh=1 ;; *) fresh=0 ;; esac
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then out="$2"; fi
  shift
done
case "$fresh:$out" in
  1:*) body='{{"offsets":{{"from":0,"to":1000}},"text":"fresh","tokens":[{{"text":"fresh","offsets":{{"from":0,"to":1000}}}}]}}' ;;
  *batch-0) body='{{"offsets":{{"from":0,"to":3600000}},"text":"x","tokens":[
    {{"text":"はじめの","offsets":{{"from":0,"to":1000}}}},
    {{"text":"おなじはなしです","offsets":{{"from":3541000,"to":3542000}}}},
    {{"text":"おわ","offsets":{{"from":3599000,"to":3600000}}}}]}}' ;;
  *) body='{{"offsets":{{"from":1000,"to":2000}},"text":"おなじはなしです","tokens":[{{"text":"おなじはなしです","offsets":{{"from":1000,"to":2000}}}}]}},
    {{"offsets":{{"from":60000,"to":62000}},"text":"again","tokens":[{{"text":"again","offsets":{{"from":60000,"to":62000}}}}]}},
    {{"offsets":{{"from":62000,"to":64000}},"text":"again","tokens":[{{"text":"again","offsets":{{"from":62000,"to":64000}}}}]}},
    {{"offsets":{{"from":100000,"to":101000}},"text":"つづき","tokens":[{{"text":"つづき","offsets":{{"from":100000,"to":101000}}}}]}}' ;;
esac
printf '{{"transcription":[%s]}}' "$body" > "$out.json"
"#,
            log = log.display(),
        ),
    );
    let mut seen = Vec::new();
    let words = engines::transcribe(
        &tools_in(&paths),
        &root.join("model.bin"),
        &audio,
        "ja",
        &root,
        &CancelToken::default(),
        &mut |fraction| seen.push(fraction),
    )
    .unwrap();
    assert_eq!(
        words
            .iter()
            .map(|word| (word.text.as_str(), word.start))
            .collect::<Vec<_>>(),
        vec![
            ("はじめの", 0.0),
            ("おなじはなしです", 3541.0),
            ("fresh", 3598.0),
            ("つづき", 3640.0),
        ]
    );
    let calls = fs::read_to_string(&log).unwrap();
    assert_eq!(calls.lines().count(), 3);
    assert!(calls.lines().next().unwrap().contains("asr-batch-0.wav"));
    assert!(calls.lines().nth(1).unwrap().contains("asr-batch-1.wav"));
    assert!(calls.lines().nth(2).unwrap().contains("asr-loop-0.wav"));
    assert!(!root.join("asr-batch-0.wav").exists());
    assert!(!root.join("asr-batch-1.wav").exists());
}

/// The notices name what FFmpeg's license asks for, so they travel with the app.
#[test]
fn the_app_ships_its_third_party_notices() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(
        config["bundle"]["resources"]["../THIRD_PARTY_NOTICES.md"],
        "THIRD_PARTY_NOTICES.md"
    );
    assert!(include_str!("../../THIRD_PARTY_NOTICES.md").contains("LGPL-2.1-or-later"));
}

/// A job on a Mac with no ONNX model: only Core ML's files and the given runner.
fn job_on_core_ml(name: &str, runner: &str) -> Result<pipeline::JobOutcome, String> {
    let root = scratch(name);
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    install_speaker_decoder_head(&paths);
    let package = paths
        .data
        .join("models/mossformer2-ss-16k-coreml/model.mlpackage");
    sparse(&package.join("Manifest.json"), 617);
    sparse(
        &package.join("Data/com.apple.CoreML/model.mlmodel"),
        1_811_303,
    );
    sparse(
        &package.join("Data/com.apple.CoreML/weights/weight.bin"),
        129_964_736,
    );
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(
            200,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1},{"start":0.5,"end":2.0,"speaker":2}]}"#,
        ),
    );
    write_tool(
        &paths
            .bundled_runtime_dir()
            .join("separate-coreml/separate-coreml"),
        runner,
    );
    fs::create_dir_all(root.join("audio")).unwrap();
    let audio = root.join("audio/audio.wav");
    fs::write(&audio, pcm_wav(&vec![8192; 32_000])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        &format!(
            "echo 'frame=1 time=00:00:01.00 bitrate=1kbits/s' >&2\nfor last; do :; done\ncp '{}' \"$last\"\n",
            audio.display()
        ),
    );
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 2,
            separation_backend: SeparationBackend::Coreml,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .map_err(|error| error.to_string())
}

#[test]
fn a_mac_separates_through_the_core_ml_runner_with_only_the_request() {
    let outcome = job_on_core_ml(
        "job-coreml",
        r#"[ $# -eq 1 ] || exit 3
python3 -c '
import json, shutil, sys
for clip in json.load(open(sys.argv[1]))["clips"]:
    for output in clip["outputs"]:
        shutil.copy(clip["input"], output)
    print("separated 1/1", file=sys.stderr)
' "$1"
"#,
    )
    .unwrap();
    let plan = outcome.speaker_plan.unwrap();
    assert_eq!(plan.separator, "mossformer2-ss-16k-coreml");
    assert_eq!(plan.separate_on, catalog::RunsOn::Gpu);
    assert_eq!(outcome.lanes.ranges, 1);
}

#[test]
fn a_failed_core_ml_runner_says_why_when_there_is_no_onnx_model_to_fall_back_on() {
    let failure =
        job_on_core_ml("job-coreml-failed", "echo 'no GPU today' >&2; exit 1").unwrap_err();
    assert!(failure.contains("no GPU today"), "{failure}");
    assert!(
        failure.contains("MossFormer2 for ONNX Runtime is not installed to fall back on"),
        "{failure}"
    );
}

/// The real app: the runner fails, and the same job goes on with ONNX Runtime in its own child.
#[test]
fn a_failed_core_ml_runner_falls_back_to_onnx_runtime() {
    let root = scratch("cli-coreml-fallback");
    let paths = ready(&root);
    fs::create_dir_all(root.join("audio")).unwrap();
    let audio = root.join("audio/audio.wav");
    fs::write(&audio, pcm_wav(&vec![8192; 32_000])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        &format!(
            "echo 'frame=1 time=00:00:01.00 bitrate=1kbits/s' >&2\nfor last; do :; done\ncp '{}' \"$last\"\n",
            audio.display()
        ),
    );
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(
            200,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1},{"start":0.5,"end":2.0,"speaker":2}]}"#,
        ),
    );
    write_tool(
        &paths
            .bundled_runtime_dir()
            .join("separate-coreml/separate-coreml"),
        "exit 1",
    );
    let package = paths
        .data
        .join("models/mossformer2-ss-16k-coreml/model.mlpackage");
    sparse(&package.join("Manifest.json"), 617);
    sparse(
        &package.join("Data/com.apple.CoreML/model.mlmodel"),
        1_811_303,
    );
    sparse(
        &package.join("Data/com.apple.CoreML/weights/weight.bin"),
        129_964_736,
    );
    padded_model(
        &paths.data.join(MOSSFORMER2_FILE),
        TWO_TRACKS_MODEL,
        MOSSFORMER2_BYTES,
    );
    let library = paths
        .bundled_runtime_dir()
        .join("onnxruntime/libonnxruntime.dylib");
    fs::create_dir_all(library.parent().unwrap()).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime/onnxruntime/libonnxruntime.dylib"),
        &library,
    )
    .unwrap();
    fs::write(
        root.join("data/settings.json"),
        serde_json::to_vec(&Settings {
            speaker_count: 2,
            separation_backend: SeparationBackend::Coreml,
            subtitle_format: SubtitleFormat::Srt,
            ..bottom_anchored()
        })
        .unwrap(),
    )
    .unwrap();
    let output = headless(&root, &root.join("clip.mp4")).output().unwrap();
    assert_eq!(stderr(&output), "");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let outcome: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(
        outcome["speaker_plan"]["separator"],
        "mossformer2-ss-16k-coreml"
    );
    assert_eq!(outcome["lanes"]["ranges"], 1);
}

#[test]
fn a_core_ml_package_with_a_missing_file_is_not_found() {
    let root = scratch("coreml-partial");
    let paths = with_tools(&root);
    let entry = catalog::find_entry("mossformer2-ss-16k-coreml").unwrap();
    let package = paths
        .data
        .join("models/mossformer2-ss-16k-coreml/model.mlpackage");
    sparse(&package.join("Manifest.json"), 617);
    assert_eq!(catalog::locate(entry, &paths, &[]), None);
    sparse(
        &package.join("Data/com.apple.CoreML/model.mlmodel"),
        1_811_303,
    );
    sparse(
        &package.join("Data/com.apple.CoreML/weights/weight.bin"),
        129_964_736,
    );
    assert_eq!(catalog::locate(entry, &paths, &[]), Some(package));
}

#[test]
fn core_ml_is_listed_only_on_a_mac() {
    let root = scratch("coreml-listed");
    let paths = with_tools(&root);
    let lists = |hardware: &Hardware| {
        catalog::statuses(hardware, &paths, &[])
            .iter()
            .any(|row| row.id == "mossformer2-ss-16k-coreml")
    };
    assert!(lists(&mac()));
    assert!(!lists(&Hardware {
        is_mac: false,
        ..mac()
    }));
}

#[test]
fn whisper_runs_without_the_gpu_when_its_memory_is_too_small() {
    let root = scratch("whisper-cpu");
    let paths = ready(&root);
    let log = root.join("args");
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        &format!("echo \"$*\" >> '{}'\n{}", log.display(), WHISPER),
    );
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 1,
            ..bottom_anchored()
        },
        &Hardware {
            is_mac: false,
            memory_gib: 8,
            gpu: None,
        },
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(fs::read_to_string(&log).unwrap().contains("-ng"));
    fs::remove_file(&log).unwrap();
    pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 1,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap();
    assert!(!fs::read_to_string(&log).unwrap().contains("-ng"));
}

/// With the runtime missing the job stops there, rather than giving wrong rows.
#[test]
fn a_job_without_onnx_runtime_says_so_when_someone_talks_at_once() {
    let root = scratch("job-no-runtime");
    let paths = ready(&root);
    fs::create_dir_all(root.join("audio")).unwrap();
    let audio = root.join("audio/audio.wav");
    fs::write(&audio, pcm_wav(&vec![8192; 32_000])).unwrap();
    write_tool(
        &paths.bundled_runtime_dir().join("ffmpeg/ffmpeg"),
        &format!("for last; do :; done\ncp '{}' \"$last\"\n", audio.display()),
    );
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &fake_server(
            200,
            r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1},{"start":0.5,"end":2.0,"speaker":2}]}"#,
        ),
    );
    let failure = pipeline::run_job(
        &root.join("clip.mp4"),
        &paths,
        &Settings {
            speaker_count: 2,
            ..bottom_anchored()
        },
        &mac(),
        &CancelToken::default(),
        &mut |_| {},
    )
    .unwrap_err()
    .to_string();
    assert!(
        failure.ends_with("libonnxruntime.dylib is missing"),
        "{failure}"
    );
}

#[test]
fn many_clips_share_one_whisper_process_per_batch_and_report_their_progress() {
    let root = scratch("whisper-batch");
    let paths = ready(&root);
    let starts = root.join("starts");
    let script = WHISPER.replacen(
        "echo 'whisper_print_progress_callback: progress =  50%' >&2",
        &format!(
            "echo started >> '{}'\necho 'progress =  90%' >&2\necho 'progress =  10%' >&2",
            starts.display()
        ),
        1,
    );
    write_tool(
        &paths.bundled_runtime_dir().join("whisper/whisper-cli"),
        &script,
    );
    let clips: Vec<transcribe_app::engines::WhisperClip> = (0..70)
        .map(|at| transcribe_app::engines::WhisperClip {
            wav: root.join(format!("{at}.wav")),
            base: root.join(format!("{at}")),
        })
        .collect();
    let mut seen = Vec::new();
    let heard = engines::transcribe_many(
        &tools_in(&paths),
        Path::new("model"),
        &clips,
        "ja",
        &CancelToken::default(),
        &mut |fraction| seen.push(fraction),
    )
    .unwrap();
    assert_eq!(heard.len(), 70);
    assert!(heard.iter().all(|words| words.len() == 2));
    assert_eq!(fs::read_to_string(&starts).unwrap().lines().count(), 2);
    assert!(seen.iter().all(|fraction| (0.0..=1.0).contains(fraction)));
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]), "{seen:?}");
}

#[test]
fn the_app_separates_voices_in_a_process_of_its_own() {
    let root = scratch("separate-child");
    let paths = AppPaths {
        data: root.join("data"),
        resources: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        hf_hub: None,
    };
    let library = transcribe_app::separation::runtime_library(&paths).unwrap();
    // The half-level track, back at the input's level.
    let rounded: Vec<i32> = separated(&root, &library)
        .unwrap()
        .iter()
        .map(|sample| (sample * 1000.0).round() as i32)
        .collect();
    assert_eq!(rounded, vec![250, -250, 500, -500]);
}

#[test]
fn a_missing_onnx_runtime_fails_only_the_child_process() {
    let root = scratch("separate-no-runtime");
    let failure = separated(&root, &root.join("no-library.dylib")).unwrap_err();
    assert!(failure.contains("load ONNX Runtime"), "{failure}");
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
            Stage::Separate,
            Stage::Retranscribe,
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
{server}
"#,
            released = released.display(),
            server = fake_server(200, TWO_VOICES),
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
fn a_radeon_job_runs_the_vulkan_build_on_its_gpu_then_the_cpu_if_that_fails() {
    let root = scratch("job-vulkan");
    let paths = ready(&root);
    let log = root.join("devices");
    write_tool(
        &paths.bundled_runtime_dir().join("nemo-vulkan/nemo-speech"),
        &format!(
            "echo \"$*\" | grep -o -- '--device [a-z]*' >> '{}'; exit 7",
            log.display()
        ),
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
    assert!(
        error.starts_with("nemo-speech failed: exit status: 7"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "--device vulkan\n--device cpu\n"
    );
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
        "echo \"$*\" >> \"{log}\"\n{server}",
        log = root.join("nemo-calls.log").display(),
        server = fake_server(200, body),
    )
}

#[test]
fn a_known_count_asks_once_and_missing_voices_are_not_invented() {
    let root = scratch("job-llm-no-retry");
    let paths = with_tools(&root);
    sparse(&paths.data.join(GGML_FILE), 0);
    sparse(&paths.data.join(ENCODER_FILE), ENCODER_BYTES);
    sparse(&paths.data.join(MOSSFORMER2_FILE), MOSSFORMER2_BYTES);
    let one_word = r#"{"segments":[{"start":0.0,"end":2.0,"speaker":1}]}"#;
    write_tool(
        &paths.bundled_runtime_dir().join("nemo/bin/nemo-speech"),
        &counting_nemo(&root, one_word),
    );
    let settings = Settings {
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
    assert!(log.starts_with("serve --no-ui"), "{log}");
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

//! Face detection with one engine and one model on macOS and Windows, so placement matches.
//! `tract` is pure Rust; it needs no GPU drivers and behaves the same in a VM.

use crate::engines::{Tools, VideoInfo};
use crate::error::{io_context, AppError, AppResult};
use crate::process::{background_command, CancelToken};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use tract_onnx::prelude::*;

const INPUT_SIZE: usize = 640;
/// YuNet's feature strides, in its output order.
const YUNET_STRIDES: [usize; 3] = [8, 16, 32];
const YUNET_SCORE_THRESHOLD: f32 = 0.6;
const YOLO_SCORE_THRESHOLD: f32 = 0.25;
const NMS_IOU: f32 = 0.3;
/// One sample per second is plenty: captions change slower than faces move across a shot.
const SAMPLES_PER_SECOND: u32 = 1;
const RGB_CHANNELS: usize = 3;
const YUNET_OUTPUTS: usize = 12;
const YOLO_AVATAR_CLASS: usize = 0;
const YOLO_BOX_CHANNELS: usize = 4;
const BGR_BLUE_INDEX: usize = 2;
const BGR_RED_INDEX: usize = 0;

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceModelKind {
    /// Real faces.
    Yunet,
    /// `avatar-face-caption-v2` exported to ONNX, for VTuber-style faces.
    AvatarYolo,
}

/// The main face at one moment, in video pixels.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct FaceSample {
    pub time: f64,
    pub top: f64,
    pub bottom: f64,
    pub center_x: f64,
}

/// A detection in network input pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Detection {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    score: f32,
}

pub struct FaceDetector {
    model: Runnable,
    kind: FaceModelKind,
}

fn tract_error(error: impl std::fmt::Display) -> AppError {
    AppError::Tool {
        tool: "face model".to_owned(),
        detail: error.to_string(),
    }
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    let intersection = (right - left).max(0.0) * (bottom - top).max(0.0);
    let union = a.width * a.height + b.width * b.height - intersection;
    if union <= 0.0 {
        return 0.0;
    }
    intersection / union
}

fn by_descending_score(left: &Detection, right: &Detection) -> std::cmp::Ordering {
    right.score.total_cmp(&left.score)
}

fn non_max_suppression(mut detections: Vec<Detection>) -> Vec<Detection> {
    detections.sort_by(by_descending_score);
    let mut kept: Vec<Detection> = Vec::new();
    for detection in detections {
        if kept.iter().all(|other| iou(other, &detection) < NMS_IOU) {
            kept.push(detection);
        }
    }
    kept
}

/// Decodes YuNet's 12 outputs: cls×3, obj×3, bbox×3, kps×3, ordered by stride.
fn decode_yunet(outputs: &[&[f32]]) -> Vec<Detection> {
    let mut detections = Vec::new();
    for (scale, stride) in YUNET_STRIDES.iter().enumerate() {
        let (cls, obj, boxes) = (outputs[scale], outputs[scale + 3], outputs[scale + 6]);
        let columns = INPUT_SIZE / stride;
        let stride = *stride as f32;
        for (index, (class_score, object_score)) in cls.iter().zip(obj).enumerate() {
            let score = (class_score.clamp(0.0, 1.0) * object_score.clamp(0.0, 1.0)).sqrt();
            if score < YUNET_SCORE_THRESHOLD {
                continue;
            }
            let (row, column) = (index / columns, index % columns);
            let values = &boxes[index * YOLO_BOX_CHANNELS..(index + 1) * YOLO_BOX_CHANNELS];
            let center_x = (column as f32 + values[0]) * stride;
            let center_y = (row as f32 + values[1]) * stride;
            let width = values[2].exp() * stride;
            let height = values[3].exp() * stride;
            detections.push(Detection {
                x: center_x - width / 2.0,
                y: center_y - height / 2.0,
                width,
                height,
                score,
            });
        }
    }
    non_max_suppression(detections)
}

/// Decodes an Ultralytics export `[1, 4 + classes, N]` holding centre boxes in input pixels.
fn decode_yolo(values: &[f32], channels: usize, count: usize) -> Vec<Detection> {
    let detections = (0..count)
        .filter_map(|index| {
            let at = |channel: usize| values[channel * count + index];
            let score = at(YOLO_BOX_CHANNELS + YOLO_AVATAR_CLASS);
            (channels > YOLO_BOX_CHANNELS && score >= YOLO_SCORE_THRESHOLD).then(|| Detection {
                x: at(0) - at(2) / 2.0,
                y: at(1) - at(3) / 2.0,
                width: at(2),
                height: at(3),
                score,
            })
        })
        .collect();
    non_max_suppression(detections)
}

fn area(detection: &Detection) -> f32 {
    detection.width * detection.height
}

fn by_area(left: &&Detection, right: &&Detection) -> std::cmp::Ordering {
    area(left).total_cmp(&area(right))
}

/// How the letterboxed square maps back onto the video.
fn letterbox_scale(info: &VideoInfo) -> (f64, f64, f64) {
    let (width, height) = (f64::from(info.width), f64::from(info.height));
    let scale = width.max(height) / INPUT_SIZE as f64;
    let (pad_x, pad_y) = (
        (width.max(height) - width) / 2.0,
        (width.max(height) - height) / 2.0,
    );
    (scale, pad_x, pad_y)
}

/// The largest face becomes the sample: the speaker's face in a frame is usually the main subject.
fn main_face(detections: &[Detection], time: f64, info: &VideoInfo) -> Option<FaceSample> {
    let face = detections.iter().max_by(by_area)?;
    let (scale, pad_x, pad_y) = letterbox_scale(info);
    Some(FaceSample {
        time,
        top: f64::from(face.y) * scale - pad_y,
        bottom: f64::from(face.y + face.height) * scale - pad_y,
        center_x: f64::from(face.x + face.width / 2.0) * scale - pad_x,
    })
}

/// Channel order and scale: YuNet was trained on OpenCV BGR 0-255, YOLO on RGB 0-1.
fn input_layout(kind: FaceModelKind) -> ([usize; RGB_CHANNELS], f32) {
    match kind {
        FaceModelKind::Yunet => ([BGR_BLUE_INDEX, 1, BGR_RED_INDEX], 1.0),
        FaceModelKind::AvatarYolo => ([0, 1, 2], 1.0 / 255.0),
    }
}

/// Decodes the network's outputs, given as shape and values per output.
fn decode_outputs(
    kind: FaceModelKind,
    outputs: &[(&[usize], &[f32])],
) -> AppResult<Vec<Detection>> {
    match kind {
        FaceModelKind::Yunet => {
            if outputs.len() != YUNET_OUTPUTS {
                return Err(tract_error(format!(
                    "expected {YUNET_OUTPUTS} outputs, got {}",
                    outputs.len()
                )));
            }
            let slices: Vec<&[f32]> = outputs.iter().map(|output| output.1).collect();
            Ok(decode_yunet(&slices))
        }
        FaceModelKind::AvatarYolo => match outputs.first() {
            None => Err(tract_error("model has no outputs")),
            Some((&[_, channels, count], values)) => Ok(decode_yolo(values, channels, count)),
            Some((shape, _)) => Err(tract_error(format!("unexpected YOLO shape {shape:?}"))),
        },
    }
}

impl FaceDetector {
    pub fn load(path: &Path, kind: FaceModelKind) -> AppResult<Self> {
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|model| {
                model.with_input_fact(
                    0,
                    f32::fact([1, RGB_CHANNELS, INPUT_SIZE, INPUT_SIZE]).into(),
                )
            })
            .and_then(|model| model.into_optimized())
            .and_then(|model| model.into_runnable())
            .map_err(tract_error)?;
        Ok(Self { model, kind })
    }

    /// Runs on one packed RGB square of `INPUT_SIZE`.
    fn detect(&self, rgb: &[u8]) -> AppResult<Vec<Detection>> {
        let (order, scale) = input_layout(self.kind);
        let input = tract_ndarray::Array4::from_shape_fn(
            (1, RGB_CHANNELS, INPUT_SIZE, INPUT_SIZE),
            |(_, channel, row, column)| {
                f32::from(rgb[(row * INPUT_SIZE + column) * RGB_CHANNELS + order[channel]]) * scale
            },
        );
        let tensor: Tensor = input.into();
        let outputs = self.model.run(tvec!(tensor.into())).map_err(tract_error)?;
        let views: Vec<(&[usize], &[f32])> = outputs
            .iter()
            .map(|output| Ok((output.shape(), output.as_slice::<f32>()?)))
            .collect::<TractResult<_>>()
            .map_err(tract_error)?;
        decode_outputs(self.kind, &views)
    }
}

fn frame_filter() -> String {
    format!(
        "fps={SAMPLES_PER_SECOND},scale={INPUT_SIZE}:{INPUT_SIZE}:force_original_aspect_ratio=decrease,pad={INPUT_SIZE}:{INPUT_SIZE}:(ow-iw)/2:(oh-ih)/2"
    )
}

/// Samples the video once per second and records its main face.
pub fn scan_faces(
    tools: &Tools,
    detector: &FaceDetector,
    video: &Path,
    info: &VideoInfo,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> AppResult<Vec<FaceSample>> {
    let mut command = background_command(&tools.ffmpeg);
    // Keyframes alone keep long videos fast; the fps filter still gives true timestamps.
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-skip_frame",
            "nokey",
            "-i",
        ])
        .arg(video)
        .args([
            "-an",
            "-vf",
            &frame_filter(),
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(io_context("start ffmpeg"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Invalid("stdout was not captured".into()))?;
    let mut frame = vec![0u8; INPUT_SIZE * INPUT_SIZE * RGB_CHANNELS];
    let mut samples = Vec::new();
    let mut time = 0.0;
    let step = 1.0 / f64::from(SAMPLES_PER_SECOND);
    let outcome = loop {
        if let Err(error) = cancel.check() {
            break Err(error);
        }
        if stdout.read_exact(&mut frame).is_err() {
            break Ok(());
        }
        match detector.detect(&frame) {
            Ok(detections) => samples.extend(main_face(&detections, time, info)),
            Err(error) => break Err(error),
        }
        time += step;
        progress((time / info.duration).clamp(0.0, 1.0));
    };
    drop(stdout);
    let _ = child.kill();
    let _ = child.wait();
    outcome.map(|()| samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANDSCAPE: VideoInfo = VideoInfo {
        width: 1280,
        height: 720,
        duration: 1.0,
        fps_num: 30,
        fps_den: 1,
    };

    fn detection(x: f32, y: f32, width: f32, score: f32) -> Detection {
        Detection {
            x,
            y,
            width,
            height: width,
            score,
        }
    }

    #[test]
    fn overlapping_boxes_keep_the_best_score() {
        let kept = non_max_suppression(vec![
            detection(0.0, 0.0, 10.0, 0.7),
            detection(1.0, 1.0, 10.0, 0.9),
            detection(100.0, 100.0, 10.0, 0.8),
        ]);
        assert_eq!(
            kept,
            vec![
                detection(1.0, 1.0, 10.0, 0.9),
                detection(100.0, 100.0, 10.0, 0.8)
            ]
        );
    }

    #[test]
    fn disjoint_boxes_have_zero_iou() {
        assert_eq!(
            iou(
                &detection(0.0, 0.0, 1.0, 1.0),
                &detection(5.0, 5.0, 1.0, 1.0)
            ),
            0.0
        );
    }

    #[test]
    fn the_largest_face_maps_back_through_the_letterbox() {
        let detections = [
            detection(0.0, 140.0, 10.0, 0.9),
            detection(320.0, 240.0, 64.0, 0.8),
        ];
        let face = main_face(&detections, 3.0, &LANDSCAPE).unwrap();
        // 1280/640 = 2x; the 720p frame sits 140 input px (280 video px) below the square's top.
        assert_eq!(
            face,
            FaceSample {
                time: 3.0,
                top: 200.0,
                bottom: 328.0,
                center_x: 704.0
            }
        );
    }

    #[test]
    fn no_detection_means_no_sample() {
        assert_eq!(main_face(&[], 0.0, &LANDSCAPE), None);
    }

    #[test]
    fn yolo_outputs_decode_class_zero_boxes() {
        // Two boxes, channels: cx cy w h face caption.
        let values = [
            100.0, 300.0, // cx
            100.0, 300.0, // cy
            20.0, 40.0, // w
            20.0, 40.0, // h
            0.9, 0.1, // avatar_face
            0.0, 0.9, // source_caption
        ];
        assert_eq!(
            decode_yolo(&values, 6, 2),
            vec![Detection {
                x: 90.0,
                y: 90.0,
                width: 20.0,
                height: 20.0,
                score: 0.9
            }]
        );
    }

    #[test]
    fn frame_filter_letterboxes_to_the_model_input() {
        assert_eq!(frame_filter(), "fps=1,scale=640:640:force_original_aspect_ratio=decrease,pad=640:640:(ow-iw)/2:(oh-ih)/2");
    }

    #[test]
    fn yunet_score_needs_both_class_and_object_confidence() {
        let size = |stride: usize| (INPUT_SIZE / stride) * (INPUT_SIZE / stride);
        let mut cls8 = vec![0.0f32; size(8)];
        let mut obj8 = vec![0.0f32; size(8)];
        cls8[0] = 0.9;
        obj8[0] = 0.9;
        cls8[1] = 0.9;
        let boxes8 = vec![0.0f32; size(8) * 4];
        let empty16 = vec![0.0f32; size(16)];
        let empty32 = vec![0.0f32; size(32)];
        let boxes16 = vec![0.0f32; size(16) * 4];
        let boxes32 = vec![0.0f32; size(32) * 4];
        let outputs: Vec<&[f32]> = vec![
            &cls8, &empty16, &empty32, &obj8, &empty16, &empty32, &boxes8, &boxes16, &boxes32,
        ];
        let found = decode_yunet(&outputs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].width, 8.0);
    }
}

//! Runs bundled tools without console windows, streaming their progress lines and honouring cancel.

use crate::error::{io_context, AppError, AppResult};
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const ERROR_TAIL_LINES: usize = 12;

/// Shared cancel flag; cloning shares the same request.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn check(&self) -> AppResult<()> {
        if self.is_cancelled() {
            return Err(AppError::Cancelled);
        }
        Ok(())
    }
}

/// A command that opens no console window on Windows.
#[must_use]
pub fn background_command(program: &Path) -> Command {
    hide_console(Command::new(program))
}

#[cfg(windows)]
fn hide_console(mut command: Command) -> Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn hide_console(command: Command) -> Command {
    command
}

/// Splits on `\r` too: ffmpeg and whisper rewrite one progress line in place.
fn pump_lines(mut stream: impl Read, sender: &mpsc::Sender<String>) {
    let mut pending = Vec::new();
    let mut buffer = [0u8; 4096];
    while let Ok(count) = stream.read(&mut buffer) {
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            if *byte != b'\n' && *byte != b'\r' {
                pending.push(*byte);
                continue;
            }
            if !pending.is_empty() {
                let _ = sender.send(String::from_utf8_lossy(&pending).into_owned());
                pending.clear();
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender.send(String::from_utf8_lossy(&pending).into_owned());
    }
}

/// Runs to completion, feeding each stderr line to `on_line`; stdout is discarded.
pub fn run_with_progress(
    mut command: Command,
    tool: &str,
    cancel: &CancelToken,
    mut on_line: impl FnMut(&str),
) -> AppResult<()> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(io_context(format!("start {tool}")))?;
    let stderr = child.stderr.take().ok_or_else(|| AppError::Invalid("stderr was not captured".into()))?;
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || pump_lines(stderr, &sender));
    let mut tail: VecDeque<String> = VecDeque::with_capacity(ERROR_TAIL_LINES);
    let status = loop {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(line) => {
                on_line(&line);
                if tail.len() == ERROR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break child.wait().map_err(io_context(format!("wait for {tool}")))?;
            }
        }
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Cancelled);
        }
    };
    let _ = reader.join();
    if status.success() {
        return Ok(());
    }
    let detail = Vec::from(tail).join("\n");
    Err(AppError::Tool { tool: tool.to_owned(), detail: format!("{status}\n{detail}") })
}

/// Runs and returns stdout; for short queries such as ffprobe.
pub fn run_capture(mut command: Command, tool: &str) -> AppResult<Vec<u8>> {
    let output = command.stdin(Stdio::null()).output().map_err(io_context(format!("start {tool}")))?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    Err(AppError::Tool { tool: tool.to_owned(), detail: String::from_utf8_lossy(&output.stderr).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carriage_returns_split_progress_lines() {
        let (sender, receiver) = mpsc::channel();
        pump_lines(&b"a=1\rb=2\n\nc=3"[..], &sender);
        drop(sender);
        assert_eq!(receiver.iter().collect::<Vec<_>>(), vec!["a=1", "b=2", "c=3"]);
    }

    #[test]
    fn cancel_token_reports_cancellation() {
        let token = CancelToken::default();
        assert!(token.check().is_ok());
        token.clone().cancel();
        assert!(matches!(token.check(), Err(AppError::Cancelled)));
    }

    #[test]
    fn a_missing_tool_names_the_tool() {
        let command = background_command(Path::new("/nonexistent/tool"));
        let error = run_capture(command, "tool").unwrap_err().to_string();
        assert!(error.starts_with("start tool: "), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_tool_reports_its_stderr() {
        let mut command = background_command(Path::new("/bin/sh"));
        command.args(["-c", "echo first >&2; echo second >&2; exit 3"]);
        let error = run_with_progress(command, "fake", &CancelToken::default(), |_| {}).unwrap_err().to_string();
        assert!(error.starts_with("fake failed: "), "{error}");
        assert!(error.ends_with("first\nsecond"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn lines_reach_the_callback() {
        let mut command = background_command(Path::new("/bin/sh"));
        command.args(["-c", "echo one >&2; echo two >&2"]);
        let mut seen = Vec::new();
        run_with_progress(command, "fake", &CancelToken::default(), |line| seen.push(line.to_owned())).unwrap();
        assert_eq!(seen, vec!["one", "two"]);
    }

    #[cfg(unix)]
    #[test]
    fn cancel_kills_the_child() {
        let mut command = background_command(Path::new("/bin/sh"));
        command.args(["-c", "exec sleep 30"]);
        let token = CancelToken::default();
        token.cancel();
        let result = run_with_progress(command, "fake", &token, |_| {});
        assert!(matches!(result, Err(AppError::Cancelled)));
    }

    #[cfg(unix)]
    #[test]
    fn capture_returns_stdout() {
        let mut command = background_command(Path::new("/bin/sh"));
        command.args(["-c", "printf hello"]);
        assert_eq!(run_capture(command, "fake").unwrap(), b"hello".to_vec());
    }
}

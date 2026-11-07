//! Resumable, hash-verified downloads of catalogued files.

use crate::catalog::{
    locate_file, unpacked_marker, FileSpec, ModelEntry, Role, EARLIER_GEFORCE_FOLDER,
    GEFORCE_FOLDER, GEFORCE_RUNTIME_ID,
};
use crate::error::{io_context, AppError, AppResult};
use crate::paths::AppPaths;
use crate::process::CancelToken;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use ureq::Agent;

const CHUNK_BYTES: usize = 1 << 20;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const STATUS_PARTIAL: u16 = 206;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;
const STATUS_SERVER_ERRORS: u16 = 500;
/// A stalled server fails here and takes the normal retry; ureq has no limit of its own.
const STALL_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_millis(500)
} else {
    Duration::from_secs(30)
};
const MAX_RETRIES: u32 = 5;
const RETRY_WAIT: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_secs(2)
};

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
pub struct DownloadProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// What has reached the partial file so far.
struct Received {
    hasher: Sha256,
    bytes: u64,
}

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        Agent::config_builder()
            .timeout_connect(Some(STALL_TIMEOUT))
            .timeout_recv_response(Some(STALL_TIMEOUT))
            .timeout_per_read(Some(STALL_TIMEOUT))
            .build()
            .new_agent()
    })
}

/// Hashes what an interrupted run already saved so the final digest covers the whole file.
fn hash_existing(path: &Path, cancel: &CancelToken) -> AppResult<Received> {
    let mut received = Received {
        hasher: Sha256::new(),
        bytes: 0,
    };
    let Ok(mut file) = File::open(path) else {
        return Ok(received);
    };
    let mut buffer = vec![0u8; CHUNK_BYTES];
    loop {
        cancel.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(io_context("read partial download"))?;
        if count == 0 {
            return Ok(received);
        }
        received.hasher.update(&buffer[..count]);
        received.bytes += count as u64;
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn partial_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

struct Transfer<'a> {
    url: &'a str,
    partial: &'a Path,
    size: u64,
    /// Bytes of earlier files in this entry, so one bar covers the whole download.
    earlier_bytes: u64,
    entry_bytes: u64,
}

/// A refusal from the server stays a refusal. A busy server or a lost connection is worth another try.
fn request_failure(url: &str, error: &ureq::Error) -> AppError {
    let detail = format!("{url}: {error}");
    match error {
        ureq::Error::StatusCode(code)
            if *code < STATUS_SERVER_ERRORS && *code != STATUS_TOO_MANY_REQUESTS =>
        {
            AppError::Download(detail)
        }
        _ => AppError::Interrupted(detail),
    }
}

/// The first byte a `bytes 6-10/11` reply holds.
fn content_range_start(value: &str) -> Option<u64> {
    value
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

fn stream_to_file(
    transfer: &Transfer<'_>,
    received: &mut Received,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(DownloadProgress),
) -> AppResult<()> {
    // An interrupted run may have saved everything; asking for more would only get a 416.
    if received.bytes >= transfer.size {
        return Ok(());
    }
    let response = agent()
        .get(transfer.url)
        .header("Range", &format!("bytes={}-", received.bytes))
        .call()
        .map_err(|error| request_failure(transfer.url, &error))?;
    let resumed = response.status().as_u16() == STATUS_PARTIAL;
    let start = response
        .headers()
        .get("Content-Range")
        .and_then(|value| value.to_str().ok())
        .and_then(content_range_start);
    let mut file = if resumed && start.is_none_or(|start| start == received.bytes) {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(transfer.partial)
    } else {
        // The server ignored the range or answered with another one: restart from zero.
        received.hasher.reset();
        received.bytes = 0;
        File::create(transfer.partial)
    }
    .map_err(io_context("open partial download"))?;
    if resumed && start.is_some_and(|start| start != 0) && received.bytes == 0 {
        return Err(AppError::Interrupted(format!(
            "{}: the server sent an unexpected range",
            transfer.url
        )));
    }
    let mut body = response.into_body();
    let mut reader = body.as_reader();
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut last_report = Instant::now();
    loop {
        cancel.check()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| AppError::Interrupted(error.to_string()))?;
        if count == 0 {
            if received.bytes < transfer.size {
                return Err(AppError::Interrupted(format!(
                    "{}: the connection closed after {} of {} bytes",
                    transfer.url, received.bytes, transfer.size
                )));
            }
            return Ok(());
        }
        file.write_all(&buffer[..count])
            .map_err(io_context("write download"))?;
        received.hasher.update(&buffer[..count]);
        received.bytes += count as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            progress(DownloadProgress {
                done_bytes: transfer.earlier_bytes + received.bytes.min(transfer.size),
                total_bytes: transfer.entry_bytes,
            });
        }
    }
}

/// Publishes the file only when its size and SHA-256 match the pinned values.
fn verify_and_publish(
    partial: &Path,
    destination: &Path,
    file: &FileSpec,
    hasher: Sha256,
) -> AppResult<()> {
    let digest = hex(&hasher.finalize());
    if digest != file.sha256 {
        std::fs::remove_file(partial).map_err(io_context("remove corrupt download"))?;
        return Err(AppError::Download(format!(
            "{}: SHA-256 mismatch ({digest})",
            file.path
        )));
    }
    std::fs::rename(partial, destination).map_err(io_context("publish download"))
}

fn archive_failure(error: impl std::fmt::Display) -> AppError {
    AppError::Download(format!("archive: {error}"))
}

fn parent_of(path: &Path) -> AppResult<&Path> {
    path.parent()
        .ok_or_else(|| AppError::Invalid(format!("{} has no parent folder", path.display())))
}

fn unpack_archive(archive: &Path, folder: &Path) -> AppResult<()> {
    let file = File::open(archive).map_err(io_context("open archive"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(archive_failure)?;
    std::fs::create_dir_all(folder).map_err(io_context("create runtime folder"))?;
    for index in 0..zip.len() {
        let mut member = zip.by_index(index).map_err(archive_failure)?;
        // enclosed_name rejects absolute paths and `..`, so a bad archive cannot escape the folder.
        let Some(relative) = member.enclosed_name() else {
            return Err(AppError::Download(format!(
                "archive member escapes: {}",
                member.name()
            )));
        };
        let target = folder.join(relative);
        if member.is_dir() {
            std::fs::create_dir_all(&target).map_err(io_context("create archive folder"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_context("create archive folder"))?;
        }
        let mut out = File::create(&target).map_err(io_context("create archive member"))?;
        std::io::copy(&mut member, &mut out).map_err(io_context("unpack archive member"))?;
    }
    Ok(())
}

fn destination_for(entry: &ModelEntry, file: &FileSpec, paths: &AppPaths) -> PathBuf {
    if entry.role == Role::Runtime && entry.id == GEFORCE_RUNTIME_ID {
        return paths
            .downloaded_runtime_dir()
            .join(GEFORCE_FOLDER)
            .join(file.path);
    }
    paths.models_dir().join(entry.id).join(file.path)
}

fn download_file(
    transfer_file: &FileSpec,
    destination: &Path,
    earlier_bytes: u64,
    entry_bytes: u64,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(DownloadProgress),
) -> AppResult<()> {
    let parent = parent_of(destination)?;
    std::fs::create_dir_all(parent).map_err(io_context(format!("create {}", parent.display())))?;
    let partial = partial_path(destination);
    let url = transfer_file.url();
    let transfer = Transfer {
        url: &url,
        partial: &partial,
        size: transfer_file.size,
        earlier_bytes,
        entry_bytes,
    };
    // Kept across retries: hashing a 20 GB partial file again would take minutes each time.
    let mut received = hash_existing(&partial, cancel)?;
    let mut retries_left = MAX_RETRIES;
    loop {
        let before = received.bytes;
        let result = stream_to_file(&transfer, &mut received, cancel, progress);
        // Only a connection that keeps failing without progress should use up the retries.
        if received.bytes > before {
            retries_left = MAX_RETRIES;
        }
        match result {
            Ok(()) => {
                return verify_and_publish(&partial, destination, transfer_file, received.hasher)
            }
            Err(AppError::Interrupted(_)) if retries_left > 0 => {
                retries_left -= 1;
                std::thread::sleep(RETRY_WAIT);
                cancel.check()?;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Unpacks once; a run stopped halfway has no marker, so the next one unpacks the kept archive again.
fn unpack_once(archive: &Path) -> AppResult<()> {
    let marker = unpacked_marker(archive);
    if marker.exists() {
        return Ok(());
    }
    let folder = parent_of(archive)?;
    unpack_archive(archive, folder)?;
    File::create(&marker)
        .map(drop)
        .map_err(io_context("mark archive unpacked"))
}

/// About half a gigabyte the app no longer runs, once its replacement is on the way.
fn remove_earlier_runtime(paths: &AppPaths) -> AppResult<()> {
    let earlier = paths.downloaded_runtime_dir().join(EARLIER_GEFORCE_FOLDER);
    if earlier.exists() {
        std::fs::remove_dir_all(&earlier).map_err(io_context("remove the earlier runtime"))?;
    }
    Ok(())
}

/// Downloads every file of the entry; files already present at the right size are kept.
pub fn download_entry(
    entry: &ModelEntry,
    paths: &AppPaths,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(DownloadProgress),
) -> AppResult<()> {
    let entry_bytes: u64 = entry.files.iter().map(|file| file.size).sum();
    let mut earlier_bytes = 0;
    let own_models = [paths.models_dir()];
    if entry.id == GEFORCE_RUNTIME_ID {
        remove_earlier_runtime(paths)?;
    }
    for file in entry.files {
        let destination = destination_for(entry, file, paths);
        // Variants share files such as the 1.6 GB encoder: one copy anywhere is enough.
        let complete = !file.is_archive() && locate_file(file, &own_models, None).is_some()
            || std::fs::metadata(&destination).is_ok_and(|meta| meta.len() == file.size);
        if !complete {
            download_file(
                file,
                &destination,
                earlier_bytes,
                entry_bytes,
                cancel,
                progress,
            )?;
        }
        if file.is_archive() {
            unpack_once(&destination)?;
        }
        earlier_bytes += file.size;
        progress(DownloadProgress {
            done_bytes: earlier_bytes,
            total_bytes: entry_bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignore_progress(_: DownloadProgress) {}

    #[test]
    fn a_path_without_a_parent_folder_says_which() {
        assert_eq!(
            parent_of(Path::new("/")).unwrap_err().to_string(),
            "/ has no parent folder"
        );
        assert_eq!(parent_of(Path::new("/a/b")).unwrap(), Path::new("/a"));
    }
    use crate::catalog::UrlKind;
    use crate::error::AppError;
    use std::io::Cursor;
    use std::net::TcpListener;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    const HELLO: &[u8] = b"hello world";
    const HELLO_SHA256: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    /// Nothing listens here: a test using it proves no request was needed.
    const NO_SERVER: &str = "http://127.0.0.1:9/unused";

    fn scratch(name: &str) -> AppPaths {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target-test")
            .join(format!("download-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        AppPaths {
            data: root.join("data"),
            resources: root.join("resources"),
            hf_hub: None,
        }
    }

    fn file_spec(url: String, path: &'static str, size: u64, sha256: &'static str) -> FileSpec {
        FileSpec {
            url_kind: UrlKind::Direct(Box::leak(url.into_boxed_str())),
            path,
            size,
            sha256,
        }
    }

    fn entry_of(files: Vec<FileSpec>) -> &'static ModelEntry {
        Box::leak(Box::new(ModelEntry {
            id: "test-model",
            role: Role::Diarize,
            label: "",
            source_url: "",
            files: Box::leak(files.into_boxed_slice()),
            min_ram_gib: 0,
            min_gpu_memory_gib: 0,
            min_ram_with_gpu_gib: 0,
        }))
    }

    /// Answers one request: from the asked offset with 206, or the whole body with 200.
    fn serve(body: &'static [u8], honour_range: bool, pause: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let text = String::from_utf8(request).unwrap().to_lowercase();
            let offset: usize = text
                .lines()
                .find_map(|line| line.strip_prefix("range: bytes="))
                .and_then(|range| range.trim_end_matches('-').parse().ok())
                .unwrap_or(0);
            let (status, rest) = if honour_range {
                ("206 Partial Content", &body[offset..])
            } else {
                ("200 OK", body)
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Range: bytes {offset}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() - 1,
                body.len(),
                rest.len()
            );
            let middle = rest.len() / 2;
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&rest[..middle]);
            let _ = stream.flush();
            std::thread::sleep(pause);
            let _ = stream.write_all(&rest[middle..]);
        });
        url
    }

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }

    #[test]
    fn the_earlier_runtime_goes_and_a_missing_one_is_fine() {
        let paths = scratch("earlier-runtime");
        let earlier = paths.downloaded_runtime_dir().join("geforce/nemo");
        std::fs::create_dir_all(&earlier).unwrap();
        std::fs::write(earlier.join("nemo-speech.exe"), b"old").unwrap();
        remove_earlier_runtime(&paths).unwrap();
        assert!(!paths.downloaded_runtime_dir().join("geforce").exists());
        remove_earlier_runtime(&paths).unwrap();
    }

    #[test]
    fn partial_files_sit_beside_the_destination() {
        assert_eq!(
            partial_path(Path::new("/m/a.gguf")),
            PathBuf::from("/m/a.gguf.part")
        );
    }

    #[test]
    fn models_and_runtimes_have_separate_homes() {
        let paths = AppPaths {
            data: PathBuf::from("/d"),
            resources: PathBuf::from("/r"),
            hf_hub: None,
        };
        let model = crate::catalog::find_entry("whisper-large-v3-turbo").unwrap();
        let runtime = crate::catalog::find_entry("geforce-runtime").unwrap();
        assert_eq!(
            destination_for(model, &model.files[0], &paths),
            PathBuf::from("/d/models/whisper-large-v3-turbo/model.safetensors")
        );
        assert_eq!(
            destination_for(runtime, &runtime.files[0], &paths),
            PathBuf::from("/d/runtime/geforce-session/nemo/nemo-speech-cuda.zip")
        );
    }

    #[test]
    fn a_fresh_download_is_published_and_reports_completion() {
        let paths = scratch("fresh");
        let url = serve(HELLO, true, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "a/m.bin", 11, HELLO_SHA256)]);
        let mut seen = Vec::new();
        download_entry(entry, &paths, &CancelToken::default(), &mut |progress| {
            seen.push(progress)
        })
        .unwrap();
        let destination = paths.models_dir().join("test-model/a/m.bin");
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"hello world".to_vec()
        );
        assert!(!partial_path(&destination).exists());
        assert_eq!(
            seen,
            vec![DownloadProgress {
                done_bytes: 11,
                total_bytes: 11
            }]
        );
    }

    #[test]
    fn an_interrupted_download_resumes_from_its_partial_file() {
        let paths = scratch("resume");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&destination), b"hello ").unwrap();
        let url = serve(HELLO, true, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn a_server_ignoring_the_range_restarts_the_file() {
        let paths = scratch("restart");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&destination), b"junk").unwrap();
        let url = serve(HELLO, false, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn a_hash_mismatch_deletes_the_partial_file() {
        let paths = scratch("mismatch");
        let url = serve(HELLO, true, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, "00")]);
        let error = download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "download failed: m.bin: SHA-256 mismatch (b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9)"
        );
        let destination = paths.models_dir().join("test-model/m.bin");
        assert!(!destination.exists());
        assert!(!partial_path(&destination).exists());
    }

    #[test]
    fn a_partial_file_that_is_already_whole_is_published_without_a_request() {
        let paths = scratch("whole-part");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&destination), HELLO).unwrap();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "m.bin",
            11,
            HELLO_SHA256,
        )]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn a_complete_file_is_kept_without_a_request() {
        let paths = scratch("complete");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&destination, b"keep").unwrap();
        let entry = entry_of(vec![file_spec(NO_SERVER.to_owned(), "m.bin", 4, "00")]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"keep".to_vec());
    }

    #[test]
    fn cancelling_keeps_the_partial_file_for_later() {
        let paths = scratch("cancel");
        let url = serve(HELLO, true, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        let cancel = CancelToken::default();
        cancel.cancel();
        let result = download_entry(entry, &paths, &cancel, &mut ignore_progress);
        assert!(matches!(result, Err(AppError::Cancelled)));
        let destination = paths.models_dir().join("test-model/m.bin");
        assert!(!destination.exists());
        assert!(partial_path(&destination).exists());
    }

    #[test]
    fn an_unreachable_server_names_the_url() {
        let paths = scratch("unreachable");
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "m.bin",
            11,
            HELLO_SHA256,
        )]);
        let error = download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("download failed: http://127.0.0.1:9/unused: "),
            "{error}"
        );
    }

    #[test]
    fn a_slow_transfer_reports_progress_across_the_whole_entry() {
        let paths = scratch("slow");
        let slow = serve(HELLO, true, Duration::from_millis(300));
        let entry = entry_of(vec![
            file_spec(slow, "a.bin", 11, HELLO_SHA256),
            file_spec(
                serve(HELLO, true, Duration::ZERO),
                "b.bin",
                11,
                HELLO_SHA256,
            ),
        ]);
        let mut seen = Vec::new();
        download_entry(entry, &paths, &CancelToken::default(), &mut |progress| {
            seen.push((progress.done_bytes, progress.total_bytes))
        })
        .unwrap();
        // Earlier entries depend on timing; the file boundaries do not.
        assert_eq!(seen[seen.len() - 2..], [(11, 22), (22, 22)]);
    }

    #[test]
    fn an_archive_unpacks_beside_itself() {
        let paths = scratch("zip");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("sub/a.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"first").unwrap();
        writer
            .start_file("b.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"second").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let destination = paths.models_dir().join("test-model/tool.zip");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&destination, &archive).unwrap();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "tool.zip",
            archive.len() as u64,
            "00",
        )]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        let folder = paths.models_dir().join("test-model");
        assert_eq!(
            std::fs::read_to_string(folder.join("sub/a.txt")).unwrap(),
            "first"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("b.txt")).unwrap(),
            "second"
        );
    }

    #[test]
    fn folders_inside_an_archive_are_created() {
        let paths = scratch("zip-folder");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_directory("empty/", SimpleFileOptions::default())
            .unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let zip_path = paths.data.join("tool.zip");
        std::fs::create_dir_all(&paths.data).unwrap();
        std::fs::write(&zip_path, archive).unwrap();
        unpack_archive(&zip_path, &paths.data.join("out")).unwrap();
        assert!(paths.data.join("out/empty").is_dir());
    }

    #[test]
    fn an_archive_member_cannot_escape_the_folder() {
        let paths = scratch("zip-escape");
        let zip_path = paths.data.join("tool.zip");
        std::fs::create_dir_all(&paths.data).unwrap();
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("../evil.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"evil").unwrap();
        std::fs::write(&zip_path, writer.finish().unwrap().into_inner()).unwrap();
        let error = unpack_archive(&zip_path, &paths.data.join("out"))
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "download failed: archive member escapes: ../evil.txt"
        );
        assert!(!paths.data.join("evil.txt").exists());
    }

    #[test]
    fn a_broken_archive_is_refused() {
        let paths = scratch("zip-broken");
        let zip_path = paths.data.join("tool.zip");
        std::fs::create_dir_all(&paths.data).unwrap();
        std::fs::write(&zip_path, b"not a zip").unwrap();
        let error = unpack_archive(&zip_path, &paths.data.join("out"))
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("download failed: archive: "), "{error}");
        let missing = unpack_archive(&paths.data.join("absent.zip"), &paths.data)
            .unwrap_err()
            .to_string();
        assert!(missing.starts_with("open archive: "), "{missing}");
    }

    #[test]
    fn a_file_another_entry_already_downloaded_is_not_fetched_again() {
        let paths = scratch("shared");
        let shared = paths.models_dir().join("other-model/m.bin");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, b"keep").unwrap();
        let entry = entry_of(vec![file_spec(NO_SERVER.to_owned(), "m.bin", 4, "00")]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert!(!paths.models_dir().join("test-model/m.bin").exists());
    }

    #[test]
    fn a_shared_copy_of_the_wrong_size_does_not_count() {
        let paths = scratch("shared-wrong-size");
        let shared = paths.models_dir().join("other-model/m.bin");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, b"short").unwrap();
        let url = serve(HELLO, true, Duration::ZERO);
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(paths.models_dir().join("test-model/m.bin")).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn an_archive_elsewhere_does_not_replace_its_own_download() {
        let paths = scratch("shared-archive");
        let shared = paths.models_dir().join("other-model/tool.zip");
        std::fs::create_dir_all(shared.parent().unwrap()).unwrap();
        std::fs::write(&shared, b"12345678901").unwrap();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "tool.zip",
            11,
            HELLO_SHA256,
        )]);
        let error = download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("download failed: http://127.0.0.1:9/unused: "),
            "{error}"
        );
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        String::from_utf8(request).unwrap().to_lowercase()
    }

    #[test]
    fn a_lost_connection_resumes_from_what_arrived() {
        let paths = scratch("reconnect");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            read_request(&mut first);
            first
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 11\r\n\r\nhello ")
                .unwrap();
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            let request = read_request(&mut second);
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nConnection: close\r\n\r\nworld")
                .unwrap();
            request
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert!(server.join().unwrap().contains("range: bytes=6-\r\n"));
        assert_eq!(
            std::fs::read(paths.models_dir().join("test-model/m.bin")).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn a_server_refusal_is_not_retried() {
        let paths = scratch("refused");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        let error = download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress)
            .unwrap_err()
            .to_string();
        // A retry would reach the closed listener and fail differently.
        assert!(error.ends_with(": http status: 404"), "{error}");
    }

    #[test]
    fn a_connection_that_never_returns_gives_up_with_the_failure() {
        let paths = scratch("gives-up");
        let cancel = CancelToken::default();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "m.bin",
            11,
            HELLO_SHA256,
        )]);
        let result = download_entry(entry, &paths, &cancel, &mut ignore_progress);
        assert!(matches!(result, Err(AppError::Interrupted(_))));
    }

    #[test]
    fn a_silent_server_times_out_and_the_retry_reaches_it() {
        let paths = scratch("silent");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut silent, _) = listener.accept().unwrap();
            read_request(&mut silent);
            let (mut second, _) = listener.accept().unwrap();
            read_request(&mut second);
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world")
                .unwrap();
            drop(silent);
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(paths.models_dir().join("test-model/m.bin")).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn busy_servers_are_retried() {
        let paths = scratch("busy");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            read_request(&mut first);
            first
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            let (mut second, _) = listener.accept().unwrap();
            read_request(&mut second);
            second
                .write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
            let (mut third, _) = listener.accept().unwrap();
            read_request(&mut third);
            third
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world")
                .unwrap();
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(paths.models_dir().join("test-model/m.bin")).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn progress_gives_the_retries_back() {
        let paths = scratch("trickle");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        // One byte per connection: far more drops than MAX_RETRIES, yet always moving forward.
        std::thread::spawn(move || {
            (0..HELLO.len()).for_each(|offset| {
                let (mut stream, _) = listener.accept().unwrap();
                read_request(&mut stream);
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\r\n",
                    HELLO.len() - offset
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&HELLO[offset..=offset]).unwrap();
            });
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(paths.models_dir().join("test-model/m.bin")).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn an_unexpected_range_restarts_from_zero() {
        let paths = scratch("wrong-range");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&destination), b"hello ").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let asked_first = read_request(&mut first);
            first
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 3-10/11\r\nContent-Length: 8\r\nConnection: close\r\n\r\nlo world")
                .unwrap();
            let (mut second, _) = listener.accept().unwrap();
            let asked_second = read_request(&mut second);
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-10/11\r\nContent-Length: 11\r\nConnection: close\r\n\r\nhello world")
                .unwrap();
            (asked_first, asked_second)
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        let (asked_first, asked_second) = server.join().unwrap();
        assert!(asked_first.contains("range: bytes=6-\r\n"), "{asked_first}");
        assert!(
            asked_second.contains("range: bytes=0-\r\n"),
            "{asked_second}"
        );
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"hello world".to_vec()
        );
    }

    #[test]
    fn a_clean_close_before_the_end_counts_as_lost() {
        let paths = scratch("short-body");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            read_request(&mut first);
            first
                .write_all(b"HTTP/1.1 206 Partial Content\r\nConnection: close\r\n\r\nhello ")
                .unwrap();
            drop(first);
            let (mut second, _) = listener.accept().unwrap();
            let request = read_request(&mut second);
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nConnection: close\r\n\r\nworld")
                .unwrap();
            request
        });
        let entry = entry_of(vec![file_spec(url, "m.bin", 11, HELLO_SHA256)]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert!(server.join().unwrap().contains("range: bytes=6-\r\n"));
    }

    #[test]
    fn range_replies_are_read_for_their_first_byte() {
        assert_eq!(content_range_start("bytes 6-10/11"), Some(6));
        assert_eq!(content_range_start("bytes */11"), None);
        assert_eq!(content_range_start("items 6-10/11"), None);
    }

    #[test]
    fn cancelling_stops_hashing_a_partial_file() {
        let paths = scratch("cancel-hash");
        let destination = paths.models_dir().join("test-model/m.bin");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(partial_path(&destination), b"hello ").unwrap();
        let cancel = CancelToken::default();
        cancel.cancel();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "m.bin",
            11,
            HELLO_SHA256,
        )]);
        let result = download_entry(entry, &paths, &cancel, &mut ignore_progress);
        assert!(matches!(result, Err(AppError::Cancelled)));
    }

    #[test]
    fn an_archive_is_unpacked_once_and_marked() {
        let paths = scratch("zip-marker");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("a.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"first").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let folder = paths.models_dir().join("test-model");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("tool.zip"), &archive).unwrap();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "tool.zip",
            archive.len() as u64,
            "00",
        )]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert!(folder.join("tool.zip.unpacked").is_file());
        std::fs::remove_file(folder.join("a.txt")).unwrap();
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert!(!folder.join("a.txt").exists());
    }

    #[test]
    fn an_interrupted_unpack_is_redone_from_the_kept_archive() {
        let paths = scratch("zip-redo");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("a.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"first").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let folder = paths.models_dir().join("test-model");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("tool.zip"), &archive).unwrap();
        let entry = entry_of(vec![file_spec(
            NO_SERVER.to_owned(),
            "tool.zip",
            archive.len() as u64,
            "00",
        )]);
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        std::fs::remove_file(folder.join("a.txt")).unwrap();
        std::fs::remove_file(folder.join("tool.zip.unpacked")).unwrap();
        download_entry(entry, &paths, &CancelToken::default(), &mut ignore_progress).unwrap();
        assert_eq!(
            std::fs::read(folder.join("a.txt")).unwrap(),
            b"first".to_vec()
        );
    }
}

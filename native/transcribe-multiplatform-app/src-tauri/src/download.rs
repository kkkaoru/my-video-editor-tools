//! Resumable, hash-verified downloads of catalogued files.

use crate::catalog::{FileSpec, ModelEntry, Role, GEFORCE_RUNTIME_ID};
use crate::error::{io_context, AppError, AppResult};
use crate::paths::AppPaths;
use crate::process::CancelToken;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CHUNK_BYTES: usize = 1 << 20;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const STATUS_PARTIAL: u16 = 206;

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
pub struct DownloadProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

/// Hashes what an interrupted run already saved so the final digest covers the whole file.
fn hash_existing(path: &Path, hasher: &mut Sha256) -> AppResult<u64> {
    let Ok(mut file) = File::open(path) else {
        return Ok(0);
    };
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut total = 0u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(io_context("read partial download"))?;
        if count == 0 {
            return Ok(total);
        }
        hasher.update(&buffer[..count]);
        total += count as u64;
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

fn stream_to_file(
    transfer: &Transfer<'_>,
    hasher: &mut Sha256,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(DownloadProgress),
) -> AppResult<()> {
    let mut have = hash_existing(transfer.partial, hasher)?;
    // An interrupted run may have saved everything; asking for more would only get a 416.
    if have >= transfer.size {
        return Ok(());
    }
    let response = ureq::get(transfer.url)
        .header("Range", &format!("bytes={have}-"))
        .call()
        .map_err(|error| AppError::Download(format!("{}: {error}", transfer.url)))?;
    let mut file = if response.status().as_u16() == STATUS_PARTIAL {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(transfer.partial)
    } else {
        // The server ignored the range: restart from zero.
        hasher.reset();
        have = 0;
        File::create(transfer.partial)
    }
    .map_err(io_context("open partial download"))?;
    let mut body = response.into_body();
    let mut reader = body.as_reader();
    let mut buffer = vec![0u8; CHUNK_BYTES];
    let mut last_report = Instant::now();
    loop {
        cancel.check()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| AppError::Download(error.to_string()))?;
        if count == 0 {
            return Ok(());
        }
        file.write_all(&buffer[..count])
            .map_err(io_context("write download"))?;
        hasher.update(&buffer[..count]);
        have += count as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            progress(DownloadProgress {
                done_bytes: transfer.earlier_bytes + have.min(transfer.size),
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

fn unpack_archive(archive: &Path, folder: &Path) -> AppResult<()> {
    let file = File::open(archive).map_err(io_context("open archive"))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| AppError::Download(format!("archive: {error}")))?;
    std::fs::create_dir_all(folder).map_err(io_context("create runtime folder"))?;
    for index in 0..zip.len() {
        let mut member = zip
            .by_index(index)
            .map_err(|error| AppError::Download(format!("archive: {error}")))?;
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
            .join("geforce")
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
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Invalid("download path has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(io_context(format!("create {}", parent.display())))?;
    let partial = partial_path(destination);
    let url = transfer_file.url();
    let mut hasher = Sha256::new();
    let transfer = Transfer {
        url: &url,
        partial: &partial,
        size: transfer_file.size,
        earlier_bytes,
        entry_bytes,
    };
    stream_to_file(&transfer, &mut hasher, cancel, progress)?;
    verify_and_publish(&partial, destination, transfer_file, hasher)
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
    for file in entry.files {
        let destination = destination_for(entry, file, paths);
        let complete = std::fs::metadata(&destination).is_ok_and(|meta| meta.len() == file.size);
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
            let folder = destination
                .parent()
                .ok_or_else(|| AppError::Invalid("archive has no parent".into()))?;
            unpack_archive(&destination, folder)?;
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

    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
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
        let model = crate::catalog::find_entry("nemotron-3-diarization").unwrap();
        let runtime = crate::catalog::find_entry("geforce-runtime").unwrap();
        assert_eq!(
            destination_for(model, &model.files[0], &paths),
            PathBuf::from("/d/models/nemotron-3-diarization/Nemotron-3-Diarization.q8_0.gguf")
        );
        assert_eq!(
            destination_for(runtime, &runtime.files[0], &paths),
            PathBuf::from("/d/runtime/geforce/nemo/nemo-speech-cuda.zip")
        );
    }
}

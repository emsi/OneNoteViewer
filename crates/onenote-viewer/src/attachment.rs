use anyhow::{Context, Result};
use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use onenote_core::{
    LoadedNotebook, ResourceCopyControl, ResourceCopyOptions, ResourceCopyProgress, ResourceId,
    SourceFingerprint, SourceId,
};
use sanitize_filename::Options;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub(crate) const MAX_ATTACHMENT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_CACHE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_CACHE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const MAX_FILENAME_BYTES: usize = 240;

#[derive(Clone)]
pub(crate) struct CopyCancellation {
    resource: ResourceCopyControl,
    io: gio::Cancellable,
}

impl CopyCancellation {
    pub(crate) fn new() -> Self {
        Self {
            resource: ResourceCopyControl::new(),
            io: gio::Cancellable::new(),
        }
    }

    pub(crate) fn cancel(&self) {
        self.resource.cancel();
        self.io.cancel();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.resource.is_cancelled()
    }
}

pub(crate) struct CopyRequest {
    pub(crate) loaded: Arc<LoadedNotebook>,
    pub(crate) resource_id: ResourceId,
    pub(crate) destination: gio::File,
    pub(crate) cancellation: CopyCancellation,
}

pub(crate) fn copy_resource(
    request: &CopyRequest,
    progress: impl FnMut(ResourceCopyProgress),
) -> Result<u64> {
    let (stream, destination_guard) =
        replace_stream(&request.destination, &request.cancellation.io)
            .with_context(|| format!("could not prepare {}", request.destination.parse_name()))?;
    let mut writer = CancellableWriter {
        stream,
        cancellable: request.cancellation.io.clone(),
        destination_guard,
        published: false,
        aborted: false,
    };
    let result = request.loaded.resources.copy_to(
        &request.resource_id,
        &mut writer,
        ResourceCopyOptions::new(MAX_ATTACHMENT_BYTES),
        &request.cancellation.resource,
        progress,
    );
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            writer.abort();
            return Err(error.into());
        }
    };
    if let Err(error) = writer.finish() {
        if request.cancellation.is_cancelled() {
            anyhow::bail!("Attachment copy was cancelled");
        }
        return Err(error).context("could not publish the completed attachment");
    }
    Ok(report.bytes_written)
}

fn replace_stream(
    destination: &gio::File,
    cancellable: &gio::Cancellable,
) -> Result<(gio::FileOutputStream, DestinationGuard), glib::Error> {
    let (version, existed) = match query_destination(destination, cancellable) {
        Ok(version) => (Some(version), true),
        Err(error) if error.matches(gio::IOErrorEnum::NotFound) => (None, false),
        Err(error) => return Err(error),
    };
    let etag = version.as_ref().and_then(|version| version.etag.as_deref());
    let stream = destination.replace(
        etag,
        false,
        gio::FileCreateFlags::PRIVATE | gio::FileCreateFlags::REPLACE_DESTINATION,
        Some(cancellable),
    )?;
    let destination_guard = DestinationGuard {
        file: destination.clone(),
        version: if existed {
            version
        } else {
            query_destination(destination, cancellable).ok()
        },
        delete_on_abort: !existed,
    };
    Ok((stream, destination_guard))
}

fn query_destination(
    file: &gio::File,
    cancellable: &gio::Cancellable,
) -> Result<DestinationVersion, glib::Error> {
    const ATTRIBUTES: &str =
        "etag::value,id::file,standard::size,time::modified,time::modified-usec";
    file.query_info(
        ATTRIBUTES,
        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
        Some(cancellable),
    )
    .map(|info| DestinationVersion {
        etag: info
            .attribute_string(gio::FILE_ATTRIBUTE_ETAG_VALUE)
            .map(|value| value.to_string()),
        file_id: info
            .attribute_string(gio::FILE_ATTRIBUTE_ID_FILE)
            .map(|value| value.to_string()),
        size: info
            .has_attribute(gio::FILE_ATTRIBUTE_STANDARD_SIZE)
            .then(|| info.size()),
        modified: info
            .has_attribute(gio::FILE_ATTRIBUTE_TIME_MODIFIED)
            .then(|| info.attribute_uint64(gio::FILE_ATTRIBUTE_TIME_MODIFIED)),
        modified_usec: info
            .has_attribute(gio::FILE_ATTRIBUTE_TIME_MODIFIED_USEC)
            .then(|| info.attribute_uint32(gio::FILE_ATTRIBUTE_TIME_MODIFIED_USEC)),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DestinationVersion {
    etag: Option<String>,
    file_id: Option<String>,
    size: Option<i64>,
    modified: Option<u64>,
    modified_usec: Option<u32>,
}

struct DestinationGuard {
    file: gio::File,
    version: Option<DestinationVersion>,
    delete_on_abort: bool,
}

struct CancellableWriter {
    stream: gio::FileOutputStream,
    cancellable: gio::Cancellable,
    destination_guard: DestinationGuard,
    published: bool,
    aborted: bool,
}

impl CancellableWriter {
    fn finish(mut self) -> Result<()> {
        // GIO owns publication semantics here. Some backends write through the
        // visible target while others stage a sibling, so target metadata is
        // not a portable indication of an external modification.
        if let Err(error) = self.stream.flush(Some(&self.cancellable)) {
            self.abort();
            return Err(error.into());
        }
        if self.cancellable.is_cancelled() {
            self.abort();
            anyhow::bail!("Attachment copy was cancelled");
        }
        if let Err(error) = self.stream.close(Some(&self.cancellable)) {
            self.abort();
            return Err(error.into());
        }
        self.published = true;
        Ok(())
    }

    fn abort(&mut self) {
        if self.published || self.aborted {
            return;
        }
        self.aborted = true;
        let abort = gio::Cancellable::new();
        abort.cancel();
        let _ignored = self.stream.close(Some(&abort));
        let guard = &self.destination_guard;
        let current = query_destination(&guard.file, &gio::Cancellable::new()).ok();
        if guard.delete_on_abort && same_destination_entry(current.as_ref(), guard.version.as_ref())
        {
            let _ignored = guard.file.delete(None::<&gio::Cancellable>);
        }
    }
}

impl Drop for CancellableWriter {
    fn drop(&mut self) {
        if !self.published {
            self.abort();
        }
    }
}

fn same_destination_entry(
    current: Option<&DestinationVersion>,
    expected: Option<&DestinationVersion>,
) -> bool {
    match (current, expected) {
        (Some(current), Some(expected)) => match (&current.file_id, &expected.file_id) {
            (Some(current), Some(expected)) => current == expected,
            _ => current == expected,
        },
        _ => false,
    }
}

impl Write for CancellableWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream
            .write(buffer, Some(&self.cancellable))
            .and_then(|count| {
                usize::try_from(count).map_err(|_| {
                    glib::Error::new(gio::IOErrorEnum::Failed, "invalid output byte count")
                })
            })
            .map_err(|error| io::Error::other(error.to_string()))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream
            .flush(Some(&self.cancellable))
            .map_err(|error| io::Error::other(error.to_string()))
    }
}

pub(crate) fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    let (unit, suffix) = if bytes >= GIB {
        (GIB, "GiB")
    } else if bytes >= MIB {
        (MIB, "MiB")
    } else if bytes >= KIB {
        (KIB, "KiB")
    } else {
        return format!("{bytes} bytes");
    };
    let whole = bytes / unit;
    let decimal = bytes % unit * 10 / unit;
    format!("{whole}.{decimal} {suffix}")
}

pub(crate) fn sanitized_filename(name: &str) -> String {
    let source = name.trim();
    if source.is_empty() || matches!(source, "." | "..") {
        return "attachment.bin".to_owned();
    }
    let sanitized = sanitize_filename::sanitize_with_options(
        source,
        Options {
            windows: true,
            truncate: false,
            replacement: "_",
        },
    );
    let sanitized = sanitized.trim().trim_matches('.');
    let candidate = if sanitized.is_empty() {
        "attachment.bin".to_owned()
    } else if sanitized.starts_with('.') {
        format!("attachment{sanitized}")
    } else {
        sanitized.to_owned()
    };
    truncate_filename(&candidate, MAX_FILENAME_BYTES)
}

fn truncate_filename(name: &str, max_bytes: usize) -> String {
    if name.len() <= max_bytes {
        return name.to_owned();
    }
    let extension = name
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && extension.len() <= 32)
        .map(|(_, extension)| format!(".{extension}"))
        .unwrap_or_default();
    let stem_limit = max_bytes.saturating_sub(extension.len()).max(1);
    let mut end = stem_limit.min(name.len());
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &name[..end], extension)
}

pub(crate) fn cache_file(
    source_id: &SourceId,
    fingerprint: &SourceFingerprint,
    resource_id: &ResourceId,
    display_name: &str,
) -> Result<gio::File> {
    let root = cache_root();
    create_private_directory(&root)?;
    let directory = root.join(cache_key(source_id, fingerprint, resource_id));
    create_private_directory(&directory)?;
    Ok(gio::File::for_path(
        directory.join(sanitized_filename(display_name)),
    ))
}

fn cache_root() -> PathBuf {
    glib::user_cache_dir()
        .join("onenote-viewer")
        .join("attachments")
}

fn cache_key(
    source_id: &SourceId,
    fingerprint: &SourceFingerprint,
    resource_id: &ResourceId,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for value in [
        source_id.as_str(),
        fingerprint.as_str(),
        resource_id.as_str(),
    ] {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn create_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("could not create attachment cache {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("could not secure attachment cache {}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn prune_cache() {
    let root = cache_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let now = SystemTime::now();
    let mut retained = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if now.duration_since(modified).unwrap_or_default() > MAX_CACHE_AGE {
            let _ignored = std::fs::remove_dir_all(path);
            continue;
        }
        retained.push((modified, directory_size(&path), path));
    }
    retained.sort_by_key(|(modified, _, _)| *modified);
    let mut total = retained.iter().map(|(_, size, _)| *size).sum::<u64>();
    for (_, size, path) in retained {
        if total <= MAX_CACHE_BYTES {
            break;
        }
        if std::fs::remove_dir_all(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

fn directory_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() || file_type.is_symlink() {
                return None;
            }
            entry.metadata().ok().map(|metadata| metadata.len())
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_safe_portable_components() {
        for source in [
            "../../report.pdf",
            "/absolute/path",
            "CON",
            "a/b\\c\0.txt",
            "..",
            "",
        ] {
            let sanitized = sanitized_filename(source);
            assert!(!sanitized.is_empty(), "{source:?}");
            assert_ne!(sanitized, ".", "{source:?}");
            assert_ne!(sanitized, "..", "{source:?}");
            assert!(!sanitized.contains(['/', '\\', '\0']), "{source:?}");
            assert!(sanitized.len() <= MAX_FILENAME_BYTES, "{source:?}");
        }
        assert_eq!(sanitized_filename(".."), "attachment.bin");
        assert_eq!(sanitized_filename(""), "attachment.bin");
    }

    #[test]
    fn long_unicode_filename_is_bounded_and_keeps_extension() {
        let name = format!("{}.pdf", "ą".repeat(200));
        let sanitized = sanitized_filename(&name);
        assert!(sanitized.len() <= MAX_FILENAME_BYTES);
        assert!(sanitized.is_char_boundary(sanitized.len()));
        assert!(Path::new(&sanitized)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")));
    }

    #[test]
    fn cache_keys_are_source_scoped_and_unambiguous() {
        let first = cache_key(
            &SourceId::new("a"),
            &SourceFingerprint::new("bc"),
            &ResourceId::new("d"),
        );
        let second = cache_key(
            &SourceId::new("ab"),
            &SourceFingerprint::new("c"),
            &ResourceId::new("d"),
        );
        let third = cache_key(
            &SourceId::new("a"),
            &SourceFingerprint::new("bc"),
            &ResourceId::new("different"),
        );
        assert_ne!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn cancelled_replacement_preserves_existing_destination() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("attachment.bin");
        std::fs::write(&path, b"original").expect("original destination");
        let file = gio::File::for_path(&path);
        let cancellable = gio::Cancellable::new();
        let (stream, destination_guard) =
            replace_stream(&file, &cancellable).expect("replacement stream");
        let mut writer = CancellableWriter {
            stream,
            cancellable,
            destination_guard,
            published: false,
            aborted: false,
        };
        writer.write_all(b"partial replacement").expect("write");
        writer.abort();

        assert_eq!(std::fs::read(path).expect("destination"), b"original");
    }

    #[test]
    fn cancelled_replacement_does_not_publish_new_destination() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("attachment.bin");
        let file = gio::File::for_path(&path);
        let cancellable = gio::Cancellable::new();
        let (stream, destination_guard) =
            replace_stream(&file, &cancellable).expect("replacement stream");
        let mut writer = CancellableWriter {
            stream,
            cancellable,
            destination_guard,
            published: false,
            aborted: false,
        };
        writer.write_all(b"partial output").expect("write");
        writer.abort();

        assert!(!path.exists());
    }

    #[test]
    fn completed_replacement_publishes_exact_bytes() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("attachment.bin");
        std::fs::write(&path, b"old").expect("old destination");
        let file = gio::File::for_path(&path);
        let cancellable = gio::Cancellable::new();
        let (stream, destination_guard) =
            replace_stream(&file, &cancellable).expect("replacement stream");
        let mut writer = CancellableWriter {
            stream,
            cancellable,
            destination_guard,
            published: false,
            aborted: false,
        };
        writer.write_all(b"complete replacement").expect("write");
        writer.finish().expect("publish");

        assert_eq!(
            std::fs::read(path).expect("destination"),
            b"complete replacement"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(temporary.path().join("attachment.bin"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn completed_new_destination_publishes_exact_bytes() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("attachment.bin");
        let file = gio::File::for_path(&path);
        let cancellable = gio::Cancellable::new();
        let (stream, destination_guard) =
            replace_stream(&file, &cancellable).expect("replacement stream");
        let mut writer = CancellableWriter {
            stream,
            cancellable,
            destination_guard,
            published: false,
            aborted: false,
        };
        writer.write_all(b"new attachment").expect("write");
        writer.finish().expect("publish");

        assert_eq!(std::fs::read(path).expect("destination"), b"new attachment");
    }
}

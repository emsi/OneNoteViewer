use crate::{Error, Result};
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const MAX_LISTING_BYTES: usize = 16 * 1024 * 1024;
const MAX_EXTRACTED_ENTRIES: usize = 1_000_000;

/// Aggregate result of a durable `.onepkg` extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionReport {
    /// Atomically published notebook directory.
    pub destination: PathBuf,
    /// Number of native `.one` section files.
    pub section_files: usize,
    /// Number of native `.onetoc2` hierarchy files.
    pub table_of_contents_files: usize,
    /// Total regular files extracted.
    pub total_files: usize,
}

/// Managed external 7-Zip extractor for `OneNote` CAB packages.
#[derive(Clone, Debug)]
pub struct OnePkgExtractor {
    executable: PathBuf,
}

impl OnePkgExtractor {
    /// Detect `7zz` or `7z` on the current process `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ExtractorNotFound`] if neither executable is usable.
    pub fn detect() -> Result<Self> {
        for executable in ["7zz", "7z"] {
            if command_is_usable(executable) {
                return Ok(Self {
                    executable: PathBuf::from(executable),
                });
            }
        }
        Err(Error::ExtractorNotFound)
    }

    /// Construct an extractor for an explicitly selected executable.
    ///
    /// This is useful for sandbox launchers and deterministic tests. The
    /// executable is validated when extraction starts.
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    /// Return the configured executable path.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Extract a `.onepkg` to a new durable directory.
    ///
    /// Extraction happens in a private sibling staging directory. After
    /// validation, one same-filesystem rename publishes `destination`.
    /// Complete archive or payload contents are never accumulated in memory.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the source or destination is invalid,
    /// the bounded archive listing is unsafe, 7-Zip fails, extracted content
    /// is not a native notebook tree, or `cancel` becomes true.
    pub fn extract(
        &self,
        package: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        cancel: &AtomicBool,
    ) -> Result<ExtractionReport> {
        let package = canonical_file(package.as_ref())?;
        validate_package_extension(&package)?;
        let destination = absolute_destination(destination.as_ref())?;
        if destination.exists() {
            return Err(Error::DestinationExists { path: destination });
        }
        let parent = destination.parent().ok_or_else(|| Error::InvalidPackage {
            path: package.clone(),
            message: "destination has no parent directory".to_owned(),
        })?;
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::ExtractionCancelled);
        }

        self.validate_listing(&package, cancel)?;
        self.run_quiet(&package, cancel, ["t", "-bd", "-bso0", "-bsp0"])?;

        let staging = StagingDirectory::create(parent)?;
        let output_argument = format!("-o{}", staging.path().to_string_lossy());
        self.run_quiet(
            &package,
            cancel,
            ["x", "-y", "-bd", "-bso0", "-bsp0", output_argument.as_str()],
        )?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::ExtractionCancelled);
        }

        let counts = validate_extracted_tree(staging.path(), &package)?;
        let staging_path = staging.keep();
        fs::rename(&staging_path, &destination).map_err(|source| {
            let _ignored = fs::remove_dir_all(&staging_path);
            Error::Io {
                path: destination.clone(),
                source,
            }
        })?;

        Ok(ExtractionReport {
            destination,
            section_files: counts.sections,
            table_of_contents_files: counts.table_of_contents,
            total_files: counts.total,
        })
    }

    fn validate_listing(&self, package: &Path, cancel: &AtomicBool) -> Result<()> {
        let mut child = Command::new(&self.executable)
            .args(["l", "-slt", "-ba", "-bd"])
            .arg(package)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| extractor_spawn_error(&self.executable, package, &source))?;
        let stdout = child.stdout.take().ok_or_else(|| Error::ExtractionFailed {
            path: package.to_path_buf(),
            message: "extractor did not provide its archive listing".to_owned(),
        })?;
        let capture = thread::spawn(move || capture_bounded(stdout, MAX_LISTING_BYTES));
        let status = wait_child(&mut child, cancel, package)?;
        let listing = capture
            .join()
            .map_err(|_| Error::ExtractionFailed {
                path: package.to_path_buf(),
                message: "archive-listing reader stopped unexpectedly".to_owned(),
            })?
            .map_err(|source| Error::Io {
                path: package.to_path_buf(),
                source,
            })?;
        ensure_success(status, package, "listing")?;
        if listing.truncated {
            return Err(Error::InvalidPackage {
                path: package.to_path_buf(),
                message: format!("archive listing exceeds {MAX_LISTING_BYTES} bytes"),
            });
        }
        validate_listing_paths(&listing.bytes, package)
    }

    fn run_quiet<const N: usize>(
        &self,
        package: &Path,
        cancel: &AtomicBool,
        arguments: [&str; N],
    ) -> Result<()> {
        let mut child = Command::new(&self.executable)
            .args(arguments)
            .arg(package)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| extractor_spawn_error(&self.executable, package, &source))?;
        let status = wait_child(&mut child, cancel, package)?;
        ensure_success(status, package, "operation")
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeCounts {
    sections: usize,
    table_of_contents: usize,
    total: usize,
}

fn validate_extracted_tree(root: &Path, package: &Path) -> Result<TreeCounts> {
    let canonical_root = fs::canonicalize(root).map_err(|source| Error::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut pending = vec![root.to_path_buf()];
    let mut entries_seen = 0_usize;
    let mut counts = TreeCounts::default();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|source| Error::Io {
            path: directory.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Io {
                path: directory.clone(),
                source,
            })?;
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > MAX_EXTRACTED_ENTRIES {
                return Err(Error::InvalidPackage {
                    path: package.to_path_buf(),
                    message: "extracted tree has too many entries".to_owned(),
                });
            }
            let file_type = entry.file_type().map_err(|source| Error::Io {
                path: entry.path(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(Error::InvalidPackage {
                    path: package.to_path_buf(),
                    message: "extracted tree contains a symbolic link".to_owned(),
                });
            }
            let path = entry.path();
            let canonical = fs::canonicalize(&path).map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?;
            if !canonical.starts_with(&canonical_root) {
                return Err(Error::InvalidPackage {
                    path: package.to_path_buf(),
                    message: "an extracted path escapes the staging directory".to_owned(),
                });
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                counts.total += 1;
                if has_extension(&path, "one") {
                    counts.sections += 1;
                } else if has_extension(&path, "onetoc2") {
                    counts.table_of_contents += 1;
                }
            } else {
                return Err(Error::InvalidPackage {
                    path: package.to_path_buf(),
                    message: "extracted tree contains a special filesystem entry".to_owned(),
                });
            }
        }
    }
    if counts.sections == 0 || counts.table_of_contents == 0 {
        return Err(Error::InvalidPackage {
            path: package.to_path_buf(),
            message: "archive does not contain a native notebook tree".to_owned(),
        });
    }
    Ok(counts)
}

fn validate_listing_paths(listing: &[u8], package: &Path) -> Result<()> {
    let listing = String::from_utf8_lossy(listing);
    let mut paths = 0_usize;
    for value in listing
        .lines()
        .filter_map(|line| line.strip_prefix("Path = "))
    {
        paths += 1;
        if paths > MAX_EXTRACTED_ENTRIES || !is_safe_archive_path(value) {
            return Err(Error::InvalidPackage {
                path: package.to_path_buf(),
                message: "archive contains an unsafe or excessive path listing".to_owned(),
            });
        }
    }
    if paths == 0 {
        return Err(Error::InvalidPackage {
            path: package.to_path_buf(),
            message: "extractor returned an empty archive listing".to_owned(),
        });
    }
    Ok(())
}

fn is_safe_archive_path(value: &str) -> bool {
    if value.is_empty() || value.contains('\0') || value.contains(':') {
        return false;
    }
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    !path.is_absolute()
        && path
            .components()
            .all(|component| !matches!(component, Component::ParentDir | Component::RootDir))
}

fn wait_child(child: &mut Child, cancel: &AtomicBool, package: &Path) -> Result<ExitStatus> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            child.kill().map_err(|source| Error::Io {
                path: package.to_path_buf(),
                source,
            })?;
            let _status = child.wait().map_err(|source| Error::Io {
                path: package.to_path_buf(),
                source,
            })?;
            return Err(Error::ExtractionCancelled);
        }
        if let Some(status) = child.try_wait().map_err(|source| Error::Io {
            path: package.to_path_buf(),
            source,
        })? {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn ensure_success(status: ExitStatus, package: &Path, action: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(Error::ExtractionFailed {
            path: package.to_path_buf(),
            message: format!("7-Zip {action} exited with status {status}"),
        })
    }
}

fn extractor_spawn_error(executable: &Path, package: &Path, source: &io::Error) -> Error {
    if source.kind() == io::ErrorKind::NotFound {
        Error::ExtractorNotFound
    } else {
        Error::ExtractionFailed {
            path: package.to_path_buf(),
            message: format!("could not start {}: {source}", executable.display()),
        }
    }
}

fn canonical_file(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if canonical.is_file() {
        Ok(canonical)
    } else {
        Err(Error::InvalidPackage {
            path: canonical,
            message: "source is not a regular file".to_owned(),
        })
    }
}

fn validate_package_extension(package: &Path) -> Result<()> {
    if has_extension(package, "onepkg") {
        Ok(())
    } else {
        Err(Error::InvalidPackage {
            path: package.to_path_buf(),
            message: "source does not have a .onepkg extension".to_owned(),
        })
    }
}

fn absolute_destination(destination: &Path) -> Result<PathBuf> {
    if destination.is_absolute() {
        return Ok(destination.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(destination))
        .map_err(|source| Error::Io {
            path: destination.to_path_buf(),
            source,
        })
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn command_is_usable(executable: &str) -> bool {
    Command::new(executable)
        .arg("i")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

struct BoundedCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture_bounded(mut reader: impl Read, limit: usize) -> io::Result<BoundedCapture> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut truncated = false;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    Ok(BoundedCapture { bytes, truncated })
}

struct StagingDirectory {
    path: PathBuf,
    keep: bool,
}

impl StagingDirectory {
    fn create(parent: &Path) -> Result<Self> {
        for _attempt in 0..16 {
            let path = parent.join(format!(
                ".onenote-viewer-import-{}",
                Uuid::new_v4().as_simple()
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    set_private_permissions(&path)?;
                    return Ok(Self { path, keep: false });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(Error::Io {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Err(Error::Io {
            path: parent.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "could not allocate a unique staging directory",
            ),
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn keep(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if !self.keep {
            let _ignored = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{capture_bounded, is_safe_archive_path, OnePkgExtractor};
    use crate::Error;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn rejects_unsafe_archive_paths() {
        for unsafe_path in [
            "../escape.one",
            "/absolute.one",
            r"C:\absolute.one",
            r"group\..\escape.one",
        ] {
            assert!(!is_safe_archive_path(unsafe_path), "{unsafe_path}");
        }
        assert!(is_safe_archive_path(r"group\section.one"));
    }

    #[test]
    fn bounded_capture_drains_but_truncates() {
        let capture = capture_bounded(&b"0123456789"[..], 4).expect("capture");
        assert_eq!(capture.bytes, b"0123");
        assert!(capture.truncated);
    }

    #[test]
    fn cancellation_precedes_extractor_launch() {
        let directory = tempfile::tempdir().expect("temp directory");
        let package = directory.path().join("source.onepkg");
        std::fs::write(&package, b"MSCF").expect("package fixture");
        let destination = directory.path().join("output");
        let cancelled = AtomicBool::new(true);
        let error = OnePkgExtractor::new("does-not-exist")
            .extract(&package, &destination, &cancelled)
            .expect_err("must cancel");
        assert!(matches!(error, Error::ExtractionCancelled));
        assert!(!destination.exists());
    }
}

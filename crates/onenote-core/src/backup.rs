use crate::model::{DiagnosticSeverity, SourceFingerprint, SourceId};
use crate::parser::{self, LoadOptions, LoadedNotebook, ParseLimits};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

const BACKUP_PROFILE_VERSION: u32 = 1;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

/// Selection policy applied to physical backup snapshots.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BackupSelectionPolicy {
    /// Select the newest physical snapshot for every logical section.
    #[default]
    LatestPerSection,
    /// Expose every physical snapshot as a separate section.
    AllCopies,
}

/// A persisted, reusable description of one read-only `OneNote` source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceDescriptor {
    /// A standalone `.one` or manifest `.onetoc2` file.
    NativeFile {
        /// Source file path.
        path: PathBuf,
    },
    /// A manifest-free backup directory reconstructed as one notebook.
    BackupFolder {
        /// Selected backup root.
        root: PathBuf,
        /// Persisted snapshot visibility policy.
        selection: BackupSelectionPolicy,
    },
}

impl SourceDescriptor {
    /// Construct a native file descriptor.
    pub fn native(path: impl Into<PathBuf>) -> Self {
        Self::NativeFile { path: path.into() }
    }

    /// Construct a backup-folder descriptor.
    pub fn backup(root: impl Into<PathBuf>, selection: BackupSelectionPolicy) -> Self {
        Self::BackupFolder {
            root: root.into(),
            selection,
        }
    }

    /// Filesystem path used to reopen this source.
    pub fn path(&self) -> &Path {
        match self {
            Self::NativeFile { path } => path,
            Self::BackupFolder { root, .. } => root,
        }
    }

    /// Return a copy with a different backup policy, when applicable.
    #[must_use]
    pub fn with_backup_selection(&self, selection: BackupSelectionPolicy) -> Self {
        match self {
            Self::BackupFolder { root, .. } => Self::backup(root.clone(), selection),
            Self::NativeFile { path } => Self::native(path.clone()),
        }
    }

    /// Whether this source uses reconstructed backup-folder semantics.
    pub fn is_backup(&self) -> bool {
        matches!(self, Self::BackupFolder { .. })
    }
}

/// Handling of a root-level notebook table of contents during explicit backup loading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RootManifestPolicy {
    /// Refuse backup reconstruction because a normal notebook manifest is present.
    #[default]
    Reject,
    /// Ignore the manifest after the caller has explicitly confirmed fallback.
    Ignore,
}

/// Options which determine the projected backup source generation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackupFolderOptions {
    /// Snapshot visibility policy.
    pub selection: BackupSelectionPolicy,
    /// Whether an explicitly confirmed malformed root manifest may be ignored.
    pub root_manifest: RootManifestPolicy,
}

/// Resource ceilings for backup-folder discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupFolderLimits {
    /// Maximum filesystem entries visited.
    pub max_entries: usize,
    /// Maximum directory nesting below the selected root.
    pub max_depth: usize,
    /// Maximum `.one` candidates retained.
    pub max_candidates: usize,
    /// Maximum snapshots grouped under one logical section.
    pub max_snapshots_per_section: usize,
    /// Maximum diagnostics retained.
    pub max_diagnostics: usize,
    /// Maximum total bytes hashed to resolve equal-date collisions.
    pub max_collision_hash_bytes: u64,
}

impl Default for BackupFolderLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_depth: 64,
            max_candidates: 10_000,
            max_snapshots_per_section: 1_000,
            max_diagnostics: 1_000,
            max_collision_hash_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Cloneable cooperative cancellation handle for backup inspection and loading.
#[derive(Clone, Debug, Default)]
pub struct BackupLoadControl {
    cancelled: Arc<AtomicBool>,
}

impl BackupLoadControl {
    /// Create an active cancellation handle.
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, AtomicOrdering::Release);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(AtomicOrdering::Acquire)
    }
}

/// Stable phases reported by backup inspection and loading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupProgressPhase {
    /// Validate and canonicalize the selected root.
    Classifying,
    /// Traverse the source tree.
    Discovering,
    /// Group physical files into logical sections.
    Grouping,
    /// Select physical snapshots.
    Selecting,
    /// Parse selected native sections.
    Parsing,
    /// Assemble the aggregate notebook tree.
    Assembling,
    /// Verify that the source generation did not change.
    Verifying,
}

/// Progress snapshot emitted synchronously on the calling thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupLoadProgress {
    /// Current stable phase.
    pub phase: BackupProgressPhase,
    /// Completed items within the phase.
    pub completed: usize,
    /// Known phase total, or zero when not yet known.
    pub total: usize,
}

/// Validated calendar date encoded in a recognized backup filename.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BackupDate {
    /// Four-digit year.
    pub year: u16,
    /// Month from 1 to 12.
    pub month: u8,
    /// Day from 1 to 31 as permitted by the month and year.
    pub day: u8,
}

/// Why a physical snapshot was selected or excluded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupSnapshotReason {
    /// The newest recognized filename date won.
    FilenameDate,
    /// Filesystem modification time decided an undated or tied candidate.
    ModificationTime,
    /// Stable path ordering was the final deterministic tie-breaker.
    StablePath,
    /// Every copy was requested explicitly.
    AllCopies,
    /// A newer candidate represented the same logical section.
    OlderSnapshot,
}

/// Selection state of one physical snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackupSnapshotDisposition {
    /// This snapshot will be parsed and projected.
    Selected(BackupSnapshotReason),
    /// This snapshot remains inventory-only.
    Excluded(BackupSnapshotReason),
}

/// Lightweight provenance for one physical `.one` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSnapshot {
    /// Path relative to the selected backup root.
    pub relative_path: PathBuf,
    /// Relative directory reconstructed as section groups.
    pub relative_parent: PathBuf,
    /// Logical section name used only for grouping and identity.
    pub logical_name: String,
    /// Exact physical basename with only the final `.one` extension removed.
    pub display_name: String,
    /// Validated date from a recognized filename profile.
    pub filename_date: Option<BackupDate>,
    /// Source file length observed during inspection.
    pub size: u64,
    /// Snapshot selection outcome.
    pub disposition: BackupSnapshotDisposition,
    pub(crate) logical_key: Vec<u8>,
    pub(crate) modified: Option<(u64, u32)>,
}

/// Structured backup compatibility or reconstruction diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupDiagnostic {
    /// Severity.
    pub severity: DiagnosticSeverity,
    /// Stable machine-readable code.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
    /// Source-relative path when the issue is file-specific.
    pub relative_path: Option<PathBuf>,
}

/// Immutable result of bounded discovery and deterministic selection.
#[derive(Clone, Debug)]
pub struct BackupFolderInspection {
    /// Canonical selected root.
    pub root: PathBuf,
    /// Stable identity of the aggregate backup source.
    pub source_id: SourceId,
    /// Fingerprint of the complete candidate inventory and selection policy.
    pub fingerprint: SourceFingerprint,
    /// Backup folder display name.
    pub notebook_name: String,
    /// Applied options.
    pub options: BackupFolderOptions,
    /// Physical snapshot inventory.
    pub snapshots: Vec<BackupSnapshot>,
    /// Bounded diagnostics.
    pub diagnostics: Vec<BackupDiagnostic>,
}

/// A projected notebook plus its backup inventory and provenance.
#[derive(Clone, Debug)]
pub struct BackupLoadResult {
    /// Ordinary renderer/index-compatible notebook and lazy resources.
    pub loaded: LoadedNotebook,
    /// Inspection that produced this generation.
    pub inspection: BackupFolderInspection,
}

/// Errors specific to backup-folder inspection and aggregation.
#[derive(Debug, thiserror::Error)]
pub enum BackupFolderError {
    /// A filesystem operation failed.
    #[error("could not access {path}: {source}")]
    Io {
        /// Path involved in the operation.
        path: PathBuf,
        /// Underlying failure.
        #[source]
        source: io::Error,
    },
    /// The selected path is not a directory.
    #[error("{path} is not a directory")]
    NotDirectory {
        /// Rejected path.
        path: PathBuf,
    },
    /// A root manifest requires normal notebook loading.
    #[error("{path} contains a root .onetoc2; open it as a normal notebook")]
    RootManifestPresent {
        /// Manifest path.
        path: PathBuf,
    },
    /// No section candidates were found.
    #[error("{path} contains no OneNote section files")]
    NoSections {
        /// Selected root.
        path: PathBuf,
    },
    /// A configured defensive limit was exceeded.
    #[error("backup-folder limit exceeded: {message}")]
    Limit {
        /// Limit detail without private source content.
        message: String,
    },
    /// The caller cancelled the operation.
    #[error("backup-folder operation was cancelled")]
    Cancelled,
    /// The source changed while being loaded.
    #[error("backup folder changed while loading; the previous generation was preserved")]
    SourceChanged,
    /// Native parsing or projection failed.
    #[error(transparent)]
    Core(#[from] crate::Error),
}

/// Result type for backup-folder operations.
pub type BackupResult<T> = std::result::Result<T, BackupFolderError>;

/// Reusable, read-only loader for manifest-free `OneNote` backup directories.
#[derive(Clone, Copy, Debug, Default)]
pub struct BackupFolderLoader {
    limits: BackupFolderLimits,
    parse_limits: ParseLimits,
    load_options: LoadOptions,
}

impl BackupFolderLoader {
    /// Construct a loader with explicit discovery, projection, and enrichment options.
    pub fn with_options(
        limits: BackupFolderLimits,
        parse_limits: ParseLimits,
        load_options: LoadOptions,
    ) -> Self {
        Self {
            limits,
            parse_limits,
            load_options,
        }
    }

    /// Inspect and select snapshots without parsing ordinary section content.
    ///
    /// # Errors
    ///
    /// Returns an error when the root is unavailable or unsuitable, a configured
    /// resource ceiling is exceeded, or cancellation is requested.
    pub fn inspect(
        &self,
        root: impl AsRef<Path>,
        options: BackupFolderOptions,
        control: &BackupLoadControl,
        mut progress: impl FnMut(BackupLoadProgress),
    ) -> BackupResult<BackupFolderInspection> {
        progress(progress_event(BackupProgressPhase::Classifying, 0, 0));
        check_cancelled(control)?;
        let requested = root.as_ref();
        let canonical = fs::canonicalize(requested).map_err(|source| BackupFolderError::Io {
            path: requested.to_path_buf(),
            source,
        })?;
        if !canonical.is_dir() {
            return Err(BackupFolderError::NotDirectory { path: canonical });
        }

        let mut diagnostics = Vec::new();
        let mut discovered = self.discover(&canonical, control, &mut diagnostics, &mut progress)?;
        if let Some(manifest) = discovered.root_manifests.first() {
            if options.root_manifest == RootManifestPolicy::Reject {
                return Err(BackupFolderError::RootManifestPresent {
                    path: manifest.clone(),
                });
            }
            push_diagnostic(
                &mut diagnostics,
                self.limits.max_diagnostics,
                BackupDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "backup_root_manifest_ignored".to_owned(),
                    message: "The root notebook table of contents was ignored after explicit backup-folder fallback.".to_owned(),
                    relative_path: manifest.strip_prefix(&canonical).ok().map(Path::to_path_buf),
                },
            );
        }
        if discovered.candidates.is_empty() {
            return Err(BackupFolderError::NoSections { path: canonical });
        }

        progress(progress_event(
            BackupProgressPhase::Grouping,
            0,
            discovered.candidates.len(),
        ));
        let mut snapshots = discovered
            .candidates
            .drain(..)
            .map(|candidate| snapshot(candidate, &mut diagnostics, self.limits.max_diagnostics))
            .collect::<Vec<_>>();
        report_normalization_collisions(&snapshots, &mut diagnostics, self.limits.max_diagnostics);
        check_group_limits(&snapshots, self.limits.max_snapshots_per_section)?;

        progress(progress_event(
            BackupProgressPhase::Selecting,
            0,
            snapshots.len(),
        ));
        select_snapshots(&mut snapshots, options.selection);
        inspect_equal_date_collisions(
            &canonical,
            &mut snapshots,
            &mut diagnostics,
            self.limits,
            control,
        )?;
        snapshots.sort_by(snapshot_path_order);
        let source_id = aggregate_source_id(&canonical);
        let fingerprint = inventory_fingerprint(&snapshots, options);
        let notebook_name =
            display_component(canonical.file_name().unwrap_or(canonical.as_os_str()));
        push_diagnostic(
            &mut diagnostics,
            self.limits.max_diagnostics,
            BackupDiagnostic {
                severity: DiagnosticSeverity::Info,
                code: "backup_reconstructed_order".to_owned(),
                message: "Section and section-group order was reconstructed because the backup has no authoritative table of contents.".to_owned(),
                relative_path: None,
            },
        );
        Ok(BackupFolderInspection {
            root: canonical,
            source_id,
            fingerprint,
            notebook_name,
            options,
            snapshots,
            diagnostics,
        })
    }

    /// Parse selected snapshots and assemble one ordinary notebook generation.
    ///
    /// # Errors
    ///
    /// Returns an error when parsing exceeds a defensive limit, source metadata
    /// changes during loading, or cancellation is requested.
    pub fn load(
        &self,
        inspection: BackupFolderInspection,
        control: &BackupLoadControl,
        mut progress: impl FnMut(BackupLoadProgress),
    ) -> BackupResult<BackupLoadResult> {
        check_cancelled(control)?;
        let selected = inspection
            .snapshots
            .iter()
            .filter(|snapshot| {
                matches!(snapshot.disposition, BackupSnapshotDisposition::Selected(_))
            })
            .count();
        progress(progress_event(BackupProgressPhase::Parsing, 0, selected));
        let loaded = parser::load_backup_projection(
            &inspection,
            self.parse_limits,
            self.load_options,
            control,
            |completed| {
                progress(progress_event(
                    BackupProgressPhase::Parsing,
                    completed,
                    selected,
                ));
            },
        )?;
        progress(progress_event(BackupProgressPhase::Assembling, 1, 1));
        progress(progress_event(BackupProgressPhase::Verifying, 0, 0));
        let verified = self.inspect(&inspection.root, inspection.options, control, |_| {})?;
        if verified.fingerprint != inspection.fingerprint {
            return Err(BackupFolderError::SourceChanged);
        }
        Ok(BackupLoadResult { loaded, inspection })
    }

    fn discover(
        &self,
        root: &Path,
        control: &BackupLoadControl,
        diagnostics: &mut Vec<BackupDiagnostic>,
        progress: &mut impl FnMut(BackupLoadProgress),
    ) -> BackupResult<Discovery> {
        let mut pending = vec![(root.to_path_buf(), 0_usize)];
        let mut entries = 0_usize;
        let mut candidates = Vec::new();
        let mut root_manifests = Vec::new();
        while let Some((directory, depth)) = pending.pop() {
            check_cancelled(control)?;
            if depth > self.limits.max_depth {
                return Err(BackupFolderError::Limit {
                    message: format!("directory depth exceeds {}", self.limits.max_depth),
                });
            }
            let read = fs::read_dir(&directory).map_err(|source| BackupFolderError::Io {
                path: directory.clone(),
                source,
            })?;
            for entry in read {
                check_cancelled(control)?;
                let entry = entry.map_err(|source| BackupFolderError::Io {
                    path: directory.clone(),
                    source,
                })?;
                entries = entries.saturating_add(1);
                if entries > self.limits.max_entries {
                    return Err(BackupFolderError::Limit {
                        message: format!("entry count exceeds {}", self.limits.max_entries),
                    });
                }
                progress(progress_event(BackupProgressPhase::Discovering, entries, 0));
                let path = entry.path();
                let file_type = entry.file_type().map_err(|source| BackupFolderError::Io {
                    path: path.clone(),
                    source,
                })?;
                if file_type.is_symlink() {
                    push_diagnostic(
                        diagnostics,
                        self.limits.max_diagnostics,
                        BackupDiagnostic {
                            severity: DiagnosticSeverity::Warning,
                            code: "backup_symlink_skipped".to_owned(),
                            message: "A symbolic link was skipped to keep traversal inside the selected backup root.".to_owned(),
                            relative_path: path.strip_prefix(root).ok().map(Path::to_path_buf),
                        },
                    );
                    continue;
                }
                if file_type.is_dir() {
                    pending.push((path, depth.saturating_add(1)));
                } else if file_type.is_file() && has_extension(&path, "one") {
                    if candidates.len() >= self.limits.max_candidates {
                        return Err(BackupFolderError::Limit {
                            message: format!(
                                "section candidate count exceeds {}",
                                self.limits.max_candidates
                            ),
                        });
                    }
                    let metadata = entry.metadata().map_err(|source| BackupFolderError::Io {
                        path: path.clone(),
                        source,
                    })?;
                    candidates.push(Candidate {
                        relative_path: path
                            .strip_prefix(root)
                            .expect("directory entries remain below root")
                            .to_path_buf(),
                        metadata,
                    });
                } else if file_type.is_file() && depth == 0 && has_extension(&path, "onetoc2") {
                    root_manifests.push(path);
                }
            }
        }
        root_manifests.sort();
        candidates.sort_by(|left, right| {
            path_bytes(&left.relative_path).cmp(&path_bytes(&right.relative_path))
        });
        Ok(Discovery {
            root_manifests,
            candidates,
        })
    }
}

struct Discovery {
    root_manifests: Vec<PathBuf>,
    candidates: Vec<Candidate>,
}

struct Candidate {
    relative_path: PathBuf,
    metadata: Metadata,
}

fn snapshot(
    candidate: Candidate,
    diagnostics: &mut Vec<BackupDiagnostic>,
    max_diagnostics: usize,
) -> BackupSnapshot {
    let parent = candidate
        .relative_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    let file_stem = candidate
        .relative_path
        .file_stem()
        .unwrap_or_else(|| candidate.relative_path.as_os_str());
    let display_name = display_component(file_stem);
    let (logical_os, date, suspicious) = parse_backup_name(file_stem);
    if suspicious {
        push_diagnostic(
            diagnostics,
            max_diagnostics,
            BackupDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "backup_suffix_unrecognized".to_owned(),
                message: "A backup-like filename suffix was not recognized and was preserved as a distinct logical section.".to_owned(),
                relative_path: Some(candidate.relative_path.clone()),
            },
        );
    }
    BackupSnapshot {
        relative_path: candidate.relative_path,
        relative_parent: parent,
        logical_name: display_component(&logical_os),
        display_name,
        filename_date: date,
        size: candidate.metadata.len(),
        disposition: BackupSnapshotDisposition::Excluded(BackupSnapshotReason::OlderSnapshot),
        logical_key: os_bytes(&logical_os),
        modified: modified_parts(&candidate.metadata),
    }
}

fn parse_backup_name(stem: &OsStr) -> (OsString, Option<BackupDate>, bool) {
    let Some(value) = stem.to_str() else {
        return (stem.to_os_string(), None, false);
    };
    let Some(without_close) = value.strip_suffix(')') else {
        return (stem.to_os_string(), None, value.contains(" (On "));
    };
    let Some((base, date_text)) = without_close.rsplit_once(" (On ") else {
        return (stem.to_os_string(), None, false);
    };
    let Some((date, year_first)) = parse_filename_date(date_text) else {
        return (stem.to_os_string(), None, true);
    };
    let logical = if year_first {
        base.strip_suffix(".one").unwrap_or(base)
    } else {
        base
    };
    if logical.is_empty() {
        return (stem.to_os_string(), None, true);
    }
    (OsString::from(logical), Some(date), false)
}

fn parse_filename_date(value: &str) -> Option<(BackupDate, bool)> {
    if value.len() != 10
        || value.as_bytes().get(4) != Some(&b'-') && value.as_bytes().get(2) != Some(&b'-')
    {
        return None;
    }
    let year_first = value.as_bytes().get(4) == Some(&b'-');
    let parts = value.split('-').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day) = if year_first {
        (
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        )
    } else {
        (
            parts[2].parse().ok()?,
            parts[1].parse().ok()?,
            parts[0].parse().ok()?,
        )
    };
    valid_date(year, month, day).then_some((BackupDate { year, month, day }, year_first))
}

fn valid_date(year: u16, month: u8, day: u8) -> bool {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn select_snapshots(snapshots: &mut [BackupSnapshot], policy: BackupSelectionPolicy) {
    let mut groups = BTreeMap::<(Vec<u8>, Vec<u8>), Vec<usize>>::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        groups
            .entry((
                path_bytes(&snapshot.relative_parent),
                snapshot.logical_key.clone(),
            ))
            .or_default()
            .push(index);
    }
    for indices in groups.values_mut() {
        indices.sort_by(|left, right| candidate_rank(&snapshots[*right], &snapshots[*left]));
        if policy == BackupSelectionPolicy::AllCopies {
            for index in indices {
                snapshots[*index].disposition =
                    BackupSnapshotDisposition::Selected(BackupSnapshotReason::AllCopies);
            }
            continue;
        }
        if let Some((selected, excluded)) = indices.split_first() {
            let reason = selection_reason(
                &snapshots[*selected],
                excluded.first().map(|i| &snapshots[*i]),
            );
            snapshots[*selected].disposition = BackupSnapshotDisposition::Selected(reason);
            for index in excluded {
                snapshots[*index].disposition =
                    BackupSnapshotDisposition::Excluded(BackupSnapshotReason::OlderSnapshot);
            }
        }
    }
}

fn candidate_rank(left: &BackupSnapshot, right: &BackupSnapshot) -> Ordering {
    left.filename_date
        .is_some()
        .cmp(&right.filename_date.is_some())
        .then_with(|| left.filename_date.cmp(&right.filename_date))
        .then_with(|| left.modified.cmp(&right.modified))
        .then_with(|| path_bytes(&left.relative_path).cmp(&path_bytes(&right.relative_path)))
}

fn selection_reason(
    selected: &BackupSnapshot,
    runner_up: Option<&BackupSnapshot>,
) -> BackupSnapshotReason {
    let Some(runner_up) = runner_up else {
        return selected
            .filename_date
            .map_or(BackupSnapshotReason::ModificationTime, |_| {
                BackupSnapshotReason::FilenameDate
            });
    };
    if selected.filename_date != runner_up.filename_date && selected.filename_date.is_some() {
        BackupSnapshotReason::FilenameDate
    } else if selected.modified != runner_up.modified {
        BackupSnapshotReason::ModificationTime
    } else {
        BackupSnapshotReason::StablePath
    }
}

fn inspect_equal_date_collisions(
    root: &Path,
    snapshots: &mut [BackupSnapshot],
    diagnostics: &mut Vec<BackupDiagnostic>,
    limits: BackupFolderLimits,
    control: &BackupLoadControl,
) -> BackupResult<()> {
    let mut groups = BTreeMap::<(Vec<u8>, Vec<u8>, BackupDate), Vec<usize>>::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        if let Some(date) = snapshot.filename_date {
            groups
                .entry((
                    path_bytes(&snapshot.relative_parent),
                    snapshot.logical_key.clone(),
                    date,
                ))
                .or_default()
                .push(index);
        }
    }
    let mut hashed_bytes = 0_u64;
    for indices in groups.values().filter(|indices| indices.len() > 1) {
        let required = indices
            .iter()
            .map(|index| snapshots[*index].size)
            .sum::<u64>();
        if hashed_bytes.saturating_add(required) > limits.max_collision_hash_bytes {
            push_diagnostic(
                diagnostics,
                limits.max_diagnostics,
                BackupDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: "backup_equal_date_unverified".to_owned(),
                    message: "Equal-date snapshots exceeded the collision-verification byte limit; stable path ordering was used.".to_owned(),
                    relative_path: None,
                },
            );
            continue;
        }
        let mut hashes = BTreeSet::new();
        for index in indices {
            check_cancelled(control)?;
            let path = root.join(&snapshots[*index].relative_path);
            hashes.insert(hash_file(&path, control)?);
        }
        hashed_bytes = hashed_bytes.saturating_add(required);
        push_diagnostic(
            diagnostics,
            limits.max_diagnostics,
            BackupDiagnostic {
                severity: if hashes.len() > 1 {
                    DiagnosticSeverity::Warning
                } else {
                    DiagnosticSeverity::Info
                },
                code: if hashes.len() > 1 {
                    "backup_equal_date_conflict"
                } else {
                    "backup_equal_date_duplicate"
                }
                .to_owned(),
                message: if hashes.len() > 1 {
                    "Equal-date snapshots contain different data; deterministic metadata and path tie-breakers selected the visible copy."
                } else {
                    "Equal-date snapshots contain identical data; deterministic path ordering selected the visible copy."
                }
                .to_owned(),
                relative_path: None,
            },
        );
    }
    Ok(())
}

fn report_normalization_collisions(
    snapshots: &[BackupSnapshot],
    diagnostics: &mut Vec<BackupDiagnostic>,
    max_diagnostics: usize,
) {
    let mut shadows = BTreeMap::<(Vec<u8>, String), BTreeSet<Vec<u8>>>::new();
    for snapshot in snapshots {
        let shadow = snapshot
            .logical_name
            .nfc()
            .collect::<String>()
            .to_lowercase();
        shadows
            .entry((path_bytes(&snapshot.relative_parent), shadow))
            .or_default()
            .insert(snapshot.logical_key.clone());
    }
    for keys in shadows.values().filter(|keys| keys.len() > 1) {
        let _ = keys;
        push_diagnostic(
            diagnostics,
            max_diagnostics,
            BackupDiagnostic {
                severity: DiagnosticSeverity::Warning,
                code: "backup_logical_name_collision".to_owned(),
                message: "Section names differ only by case or Unicode normalization and were kept distinct.".to_owned(),
                relative_path: None,
            },
        );
    }
}

fn check_group_limits(snapshots: &[BackupSnapshot], maximum: usize) -> BackupResult<()> {
    let mut counts = BTreeMap::<(Vec<u8>, Vec<u8>), usize>::new();
    for snapshot in snapshots {
        let count = counts
            .entry((
                path_bytes(&snapshot.relative_parent),
                snapshot.logical_key.clone(),
            ))
            .or_default();
        *count = count.saturating_add(1);
        if *count > maximum {
            return Err(BackupFolderError::Limit {
                message: format!("snapshots per logical section exceed {maximum}"),
            });
        }
    }
    Ok(())
}

fn inventory_fingerprint(
    snapshots: &[BackupSnapshot],
    options: BackupFolderOptions,
) -> SourceFingerprint {
    let mut hasher = Hasher::new();
    hasher.update(&BACKUP_PROFILE_VERSION.to_le_bytes());
    hasher.update(&[match options.selection {
        BackupSelectionPolicy::LatestPerSection => 0,
        BackupSelectionPolicy::AllCopies => 1,
    }]);
    for snapshot in snapshots {
        update_part(&mut hasher, &path_bytes(&snapshot.relative_path));
        update_part(&mut hasher, &snapshot.logical_key);
        hasher.update(&snapshot.size.to_le_bytes());
        let (seconds, nanos) = snapshot.modified.unwrap_or_default();
        hasher.update(&seconds.to_le_bytes());
        hasher.update(&nanos.to_le_bytes());
        if let Some(date) = snapshot.filename_date {
            hasher.update(&date.year.to_le_bytes());
            hasher.update(&[date.month, date.day]);
        } else {
            hasher.update(&0_u16.to_le_bytes());
            hasher.update(&[0, 0]);
        }
        hasher.update(&[u8::from(matches!(
            snapshot.disposition,
            BackupSnapshotDisposition::Selected(_)
        ))]);
    }
    SourceFingerprint::new(hasher.finalize().to_hex().to_string())
}

fn aggregate_source_id(root: &Path) -> SourceId {
    SourceId::new(stable_id(&[b"backup-folder-source-v1", &path_bytes(root)]))
}

pub(crate) fn backup_entry_id(source_id: &SourceId, kind: &str, key: &[u8]) -> String {
    stable_id(&[
        source_id.as_str().as_bytes(),
        b"backup-entry-v1",
        kind.as_bytes(),
        key,
    ])
}

pub(crate) fn snapshot_instance_key(
    snapshot: &BackupSnapshot,
    policy: BackupSelectionPolicy,
) -> Vec<u8> {
    let mut key = path_bytes(&snapshot.relative_parent);
    key.push(0);
    key.extend_from_slice(&snapshot.logical_key);
    if policy == BackupSelectionPolicy::AllCopies {
        key.push(0);
        key.extend_from_slice(&path_bytes(&snapshot.relative_path));
    }
    key
}

pub(crate) fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left = left.chars().peekable();
    let mut right = right.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let left_digits = take_digits(&mut left);
                let right_digits = take_digits(&mut right);
                let left_trimmed = left_digits.trim_start_matches('0');
                let right_trimmed = right_digits.trim_start_matches('0');
                let number_order = left_trimmed
                    .len()
                    .cmp(&right_trimmed.len())
                    .then_with(|| left_trimmed.cmp(right_trimmed))
                    .then_with(|| left_digits.len().cmp(&right_digits.len()));
                if number_order != Ordering::Equal {
                    return number_order;
                }
            }
            (Some(a), Some(b)) => {
                left.next();
                right.next();
                let order = a
                    .to_lowercase()
                    .collect::<String>()
                    .cmp(&b.to_lowercase().collect::<String>())
                    .then_with(|| a.cmp(&b));
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn take_digits(iter: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while iter.peek().is_some_and(char::is_ascii_digit) {
        digits.push(iter.next().expect("peeked digit"));
    }
    digits
}

pub(crate) fn file_stamp(path: &Path) -> BackupResult<(u64, Option<(u64, u32)>)> {
    let metadata = fs::metadata(path).map_err(|source| BackupFolderError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((metadata.len(), modified_parts(&metadata)))
}

fn stable_id(parts: &[&[u8]]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part);
    }
    Uuid::new_v5(&Uuid::NAMESPACE_URL, &bytes).to_string()
}

fn hash_file(path: &Path, control: &BackupLoadControl) -> BackupResult<String> {
    let mut file = File::open(path).map_err(|source| BackupFolderError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Hasher::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES].into_boxed_slice();
    loop {
        check_cancelled(control)?;
        let read = file
            .read(&mut buffer)
            .map_err(|source| BackupFolderError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn modified_parts(metadata: &Metadata) -> Option<(u64, u32)> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()))
}

fn push_diagnostic(
    diagnostics: &mut Vec<BackupDiagnostic>,
    maximum: usize,
    diagnostic: BackupDiagnostic,
) {
    if diagnostics.len() < maximum {
        diagnostics.push(diagnostic);
    }
}

fn progress_event(
    phase: BackupProgressPhase,
    completed: usize,
    total: usize,
) -> BackupLoadProgress {
    BackupLoadProgress {
        phase,
        completed,
        total,
    }
}

fn check_cancelled(control: &BackupLoadControl) -> BackupResult<()> {
    if control.is_cancelled() {
        Err(BackupFolderError::Cancelled)
    } else {
        Ok(())
    }
}

fn update_part(hasher: &mut Hasher, part: &[u8]) {
    hasher.update(&(part.len() as u64).to_le_bytes());
    hasher.update(part);
}

fn snapshot_path_order(left: &BackupSnapshot, right: &BackupSnapshot) -> Ordering {
    path_bytes(&left.relative_path).cmp(&path_bytes(&right.relative_path))
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn display_component(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(unix)]
pub(crate) fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
pub(crate) fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    value.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_supported_backup_filename_profiles() {
        let first = parse_backup_name(OsStr::new("Research.one (On 2026-08-15)"));
        assert_eq!(first.0, OsStr::new("Research"));
        assert_eq!(
            first.1,
            Some(BackupDate {
                year: 2026,
                month: 8,
                day: 15
            })
        );
        assert!(!first.2);

        let second = parse_backup_name(OsStr::new("Research (On 15-08-2026)"));
        assert_eq!(second.0, OsStr::new("Research"));
        assert_eq!(second.1, first.1);
        assert!(!second.2);
    }

    #[test]
    fn invalid_dates_are_preserved_in_logical_names() {
        let parsed = parse_backup_name(OsStr::new("Research (On 31-02-2026)"));
        assert_eq!(parsed.0, OsStr::new("Research (On 31-02-2026)"));
        assert_eq!(parsed.1, None);
        assert!(parsed.2);
    }

    #[test]
    fn leap_year_validation_is_gregorian() {
        assert!(valid_date(2024, 2, 29));
        assert!(!valid_date(2025, 2, 29));
        assert!(!valid_date(2100, 2, 29));
        assert!(valid_date(2000, 2, 29));
    }

    #[test]
    fn inspection_groups_and_selects_latest_without_erasing_display_date() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let group = temporary.path().join("Group");
        fs::create_dir(&group).expect("group");
        fs::write(group.join("Research (On 14-08-2026).one"), b"old").expect("old");
        fs::write(group.join("Research (On 15-08-2026).one"), b"new").expect("new");

        let inspection = BackupFolderLoader::default()
            .inspect(
                temporary.path(),
                BackupFolderOptions::default(),
                &BackupLoadControl::new(),
                |_| {},
            )
            .expect("inspection");

        assert_eq!(inspection.snapshots.len(), 2);
        let selected = inspection
            .snapshots
            .iter()
            .find(|snapshot| matches!(snapshot.disposition, BackupSnapshotDisposition::Selected(_)))
            .expect("selected");
        assert_eq!(selected.logical_name, "Research");
        assert_eq!(selected.display_name, "Research (On 15-08-2026)");
        assert_eq!(selected.relative_parent, Path::new("Group"));
    }

    #[test]
    fn all_copies_selects_each_snapshot_without_changing_source_identity() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(
            temporary.path().join("Research (On 14-08-2026).one"),
            b"old",
        )
        .expect("old");
        fs::write(
            temporary.path().join("Research (On 15-08-2026).one"),
            b"new",
        )
        .expect("new");
        let loader = BackupFolderLoader::default();
        let latest = loader
            .inspect(
                temporary.path(),
                BackupFolderOptions::default(),
                &BackupLoadControl::new(),
                |_| {},
            )
            .expect("latest inspection");
        let all = loader
            .inspect(
                temporary.path(),
                BackupFolderOptions {
                    selection: BackupSelectionPolicy::AllCopies,
                    ..BackupFolderOptions::default()
                },
                &BackupLoadControl::new(),
                |_| {},
            )
            .expect("all-copies inspection");

        assert_eq!(latest.source_id, all.source_id);
        assert_ne!(latest.fingerprint, all.fingerprint);
        assert_eq!(
            latest
                .snapshots
                .iter()
                .filter(|snapshot| matches!(
                    snapshot.disposition,
                    BackupSnapshotDisposition::Selected(_)
                ))
                .count(),
            1
        );
        assert!(all.snapshots.iter().all(|snapshot| matches!(
            snapshot.disposition,
            BackupSnapshotDisposition::Selected(BackupSnapshotReason::AllCopies)
        )));
    }

    #[test]
    fn source_descriptors_preserve_backup_policy_in_json() {
        let descriptor =
            SourceDescriptor::backup("/backups/Notebook", BackupSelectionPolicy::AllCopies);
        let encoded = serde_json::to_string(&descriptor).expect("serialize descriptor");
        let decoded: SourceDescriptor =
            serde_json::from_str(&encoded).expect("deserialize descriptor");

        assert_eq!(decoded, descriptor);
        assert!(encoded.contains("all_copies"));
    }

    #[test]
    fn root_manifest_is_authoritative_unless_explicitly_ignored() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("Open Notebook.onetoc2"), b"toc").expect("toc");
        fs::write(temporary.path().join("Section.one"), b"section").expect("section");
        let loader = BackupFolderLoader::default();
        let control = BackupLoadControl::new();

        assert!(matches!(
            loader.inspect(
                temporary.path(),
                BackupFolderOptions::default(),
                &control,
                |_| {}
            ),
            Err(BackupFolderError::RootManifestPresent { .. })
        ));
        let ignored = loader
            .inspect(
                temporary.path(),
                BackupFolderOptions {
                    root_manifest: RootManifestPolicy::Ignore,
                    ..BackupFolderOptions::default()
                },
                &control,
                |_| {},
            )
            .expect("explicit fallback");
        assert_eq!(ignored.snapshots.len(), 1);
        assert!(ignored
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "backup_root_manifest_ignored"));
    }

    #[test]
    fn unreadable_selected_snapshot_remains_a_diagnostic_section() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(
            temporary.path().join("Unreadable (On 15-08-2026).one"),
            b"not a OneNote section",
        )
        .expect("section");
        let loader = BackupFolderLoader::default();
        let control = BackupLoadControl::new();
        let inspection = loader
            .inspect(
                temporary.path(),
                BackupFolderOptions::default(),
                &control,
                |_| {},
            )
            .expect("inspection");
        let aggregate = loader
            .load(inspection, &control, |_| {})
            .expect("diagnostic aggregate");
        let section = aggregate
            .loaded
            .notebook
            .sections()
            .next()
            .expect("failed section remains visible");

        assert!(section.pages.is_empty());
        assert!(section
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "backup_selected_snapshot_parse_failed" }));
    }

    #[test]
    fn inspection_never_follows_symlinks() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let temporary = tempfile::tempdir().expect("temporary directory");
            let outside = tempfile::tempdir().expect("outside directory");
            fs::write(outside.path().join("Private.one"), b"outside").expect("outside");
            fs::write(temporary.path().join("Visible.one"), b"inside").expect("inside");
            symlink(outside.path(), temporary.path().join("escape")).expect("symlink");

            let inspection = BackupFolderLoader::default()
                .inspect(
                    temporary.path(),
                    BackupFolderOptions::default(),
                    &BackupLoadControl::new(),
                    |_| {},
                )
                .expect("inspection");
            assert_eq!(inspection.snapshots.len(), 1);
            assert!(inspection
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "backup_symlink_skipped"));
        }
    }

    #[test]
    fn cancelled_inspection_stops_before_traversal() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let control = BackupLoadControl::new();
        control.cancel();
        assert!(matches!(
            BackupFolderLoader::default().inspect(
                temporary.path(),
                BackupFolderOptions::default(),
                &control,
                |_| {}
            ),
            Err(BackupFolderError::Cancelled)
        ));
    }

    #[test]
    fn natural_order_compares_numeric_runs_by_value() {
        assert_eq!(natural_cmp("Section 2", "Section 10"), Ordering::Less);
        assert_eq!(natural_cmp("Section 02", "Section 2"), Ordering::Greater);
    }
}

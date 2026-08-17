use anyhow::{bail, Context, Result};
use onenote_core::{BackupSelectionPolicy, PageId, SectionId, SourceDescriptor, SourceId};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_DISCOVERY_ENTRIES: usize = 100_000;
pub(crate) const WORKSPACE_UI_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkspaceConfig {
    #[serde(default, deserialize_with = "deserialize_sources")]
    pub(crate) sources: Vec<SourceDescriptor>,
    #[serde(default)]
    pub(crate) navigation: WorkspaceNavigation,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_ui_state"
    )]
    pub(crate) ui: Option<WorkspaceUiState>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkspaceNavigation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_page: Option<PersistedPageLocation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PersistedPageLocation {
    pub(crate) source_path: PathBuf,
    pub(crate) source_id: SourceId,
    pub(crate) section_id: SectionId,
    pub(crate) page_id: PageId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkspaceUiState {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) panes: PersistedPaneState,
    #[serde(default)]
    pub(crate) sources: Vec<PersistedSourceTreeState>,
}

impl WorkspaceUiState {
    pub(crate) fn new(panes: PersistedPaneState, sources: Vec<PersistedSourceTreeState>) -> Self {
        Self {
            version: WORKSPACE_UI_VERSION,
            panes,
            sources: normalize_source_states(sources),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PersistedPaneState {
    #[serde(default)]
    pub(crate) notebooks_collapsed: bool,
    #[serde(default)]
    pub(crate) pages_collapsed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PersistedSourceTreeState {
    pub(crate) source_path: PathBuf,
    pub(crate) source_id: SourceId,
    #[serde(default)]
    pub(crate) notebook_expanded: bool,
    #[serde(default)]
    pub(crate) expanded_groups: Vec<SectionId>,
}

impl PersistedSourceTreeState {
    pub(crate) fn normalize(&mut self) {
        self.expanded_groups.sort();
        self.expanded_groups.dedup();
    }
}

fn normalize_source_states(
    mut sources: Vec<PersistedSourceTreeState>,
) -> Vec<PersistedSourceTreeState> {
    for source in &mut sources {
        source.normalize();
    }
    let mut seen = BTreeSet::new();
    sources.retain(|source| seen.insert(source.source_id.clone()));
    sources
}

fn deserialize_ui_state<'de, D>(deserializer: D) -> Result<Option<WorkspaceUiState>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let state = serde_json::from_value::<WorkspaceUiState>(value).ok();
    Ok(state.filter(|state| state.version == WORKSPACE_UI_VERSION))
}

pub(crate) fn paths() -> Result<(PathBuf, PathBuf)> {
    let state_root = xdg_root("XDG_STATE_HOME", ".local/state")?.join("onenote-viewer");
    let cache_root = xdg_root("XDG_CACHE_HOME", ".cache")?.join("onenote-viewer");
    Ok((
        state_root.join("workspace.json"),
        cache_root.join("search.sqlite"),
    ))
}

pub(crate) fn load(path: &Path) -> Result<WorkspaceConfig> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid workspace file {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(WorkspaceConfig::default())
        }
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

pub(crate) fn save(path: &Path, config: &WorkspaceConfig) -> Result<()> {
    let parent = path
        .parent()
        .context("workspace path does not have a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    set_private_directory(parent)?;
    let temporary = path.with_extension("json.new");
    let bytes = serde_json::to_vec_pretty(config).context("could not serialize workspace")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    set_private_file(&temporary)?;
    fs::rename(&temporary, path).with_context(|| format!("could not publish {}", path.display()))
}

pub(crate) fn ensure_index_parent(path: &Path) -> Result<()> {
    let parent = path.parent().context("index path does not have a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    set_private_directory(parent)
}

pub(crate) fn discover(requested: &Path) -> Result<Vec<SourceDescriptor>> {
    let canonical = canonical_source(requested)?;
    if canonical.is_file() {
        return discover_file(canonical);
    }
    let (manifests, sections) = discover_directory_contents(&canonical)?;
    let roots = root_manifests(&canonical, manifests);
    if !roots.is_empty() {
        return Ok(roots.into_iter().map(SourceDescriptor::native).collect());
    }
    if sections.is_empty() {
        bail!("{} contains no .onetoc2 or .one files", canonical.display());
    }
    bail!(
        "{} has OneNote sections but no root .onetoc2; use Open OneNote Backup Folder",
        canonical.display()
    )
}

pub(crate) fn discover_library(requested: &Path) -> Result<Vec<SourceDescriptor>> {
    let canonical = canonical_source(requested)?;
    if canonical.is_file() {
        return discover_file(canonical);
    }
    let mut sources = Vec::new();
    let read = fs::read_dir(&canonical)
        .with_context(|| format!("could not read {}", canonical.display()))?;
    for entry in read {
        let entry = entry.with_context(|| format!("could not read {}", canonical.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("could not inspect {}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file() {
            if matches!(extension(&path).as_deref(), Some("one" | "onetoc2")) {
                sources.push(SourceDescriptor::native(path));
            }
            continue;
        }
        if !file_type.is_dir() {
            continue;
        }
        let (manifests, sections) = discover_directory_contents(&path)?;
        let roots = root_manifests(&path, manifests);
        if roots.is_empty() {
            if !sections.is_empty() {
                sources.push(SourceDescriptor::backup(
                    path,
                    BackupSelectionPolicy::LatestPerSection,
                ));
            }
        } else {
            sources.extend(roots.into_iter().map(SourceDescriptor::native));
        }
    }
    sources.sort_by(|left, right| left.path().cmp(right.path()));
    sources.dedup_by(|left, right| left == right);
    Ok(sources)
}

fn canonical_source(requested: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(requested)
        .with_context(|| format!("could not access {}", requested.display()))?;
    if !canonical.is_file() && !canonical.is_dir() {
        bail!("{} is not a regular file or directory", canonical.display());
    }
    Ok(canonical)
}

fn discover_file(canonical: PathBuf) -> Result<Vec<SourceDescriptor>> {
    match extension(&canonical).as_deref() {
        Some("one" | "onetoc2") => Ok(vec![SourceDescriptor::native(canonical)]),
        Some("onepkg") => bail!(
            "{} is a package; use Import OneNote Package so it is extracted on disk",
            canonical.display()
        ),
        _ => bail!("{} is not a supported OneNote source", canonical.display()),
    }
}

fn discover_directory_contents(canonical: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut pending = vec![canonical.to_path_buf()];
    let mut table_of_contents = Vec::new();
    let mut sections = Vec::new();
    let mut entries = 0_usize;
    while let Some(directory) = pending.pop() {
        let read = fs::read_dir(&directory)
            .with_context(|| format!("could not read {}", directory.display()))?;
        for entry in read {
            let entry = entry.with_context(|| format!("could not read {}", directory.display()))?;
            entries = entries.saturating_add(1);
            if entries > MAX_DISCOVERY_ENTRIES {
                bail!("source discovery exceeded {MAX_DISCOVERY_ENTRIES} entries");
            }
            let file_type = entry
                .file_type()
                .with_context(|| format!("could not inspect {}", entry.path().display()))?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                match extension(&path).as_deref() {
                    Some("onetoc2") => table_of_contents.push(path),
                    Some("one") => sections.push(path),
                    _ => {}
                }
            }
        }
    }

    table_of_contents.sort();
    sections.sort();
    sections.dedup();
    Ok((table_of_contents, sections))
}

pub(crate) fn source_is_in_location(source: &Path, location: &Path) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(location)) {
        (Ok(source), Ok(location)) => source.starts_with(location),
        _ => source.starts_with(location),
    }
}

pub(crate) fn source_is_in_workspace(
    source: &Path,
    sources: &[SourceDescriptor],
    notebooks_location: &Path,
) -> bool {
    source_is_in_location(source, notebooks_location)
        || sources
            .iter()
            .any(|configured| source_is_in_location(source, configured.path()))
}

fn deserialize_sources<'de, D>(deserializer: D) -> Result<Vec<SourceDescriptor>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum PersistedSource {
        Legacy(PathBuf),
        Descriptor(SourceDescriptor),
    }

    let sources = Vec::<PersistedSource>::deserialize(deserializer)?;
    Ok(sources
        .into_iter()
        .map(|source| match source {
            PersistedSource::Legacy(path) => SourceDescriptor::native(path),
            PersistedSource::Descriptor(source) => source,
        })
        .collect())
}

fn root_manifests(root: &Path, manifests: Vec<PathBuf>) -> Vec<PathBuf> {
    let manifest_parents: BTreeSet<PathBuf> = manifests
        .iter()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect();
    manifests
        .into_iter()
        .filter(|manifest| {
            manifest.parent().is_some_and(|parent| {
                !parent
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor.starts_with(root))
                    .any(|ancestor| manifest_parents.contains(ancestor))
            })
        })
        .collect()
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
}

fn xdg_root(variable: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(fallback))
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("could not secure {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("could not secure {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_omits_nested_group_manifests() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let notebook = temporary.path().join("Notebook");
        let group = notebook.join("Group");
        fs::create_dir_all(&group).expect("group");
        fs::write(notebook.join("Open Notebook.onetoc2"), b"root").expect("root manifest");
        fs::write(group.join("Open Notebook.onetoc2"), b"group").expect("group manifest");
        fs::write(group.join("Section.one"), b"section").expect("section");

        let discovered = discover(temporary.path()).expect("discovery");

        assert_eq!(
            discovered,
            vec![SourceDescriptor::native(
                notebook.join("Open Notebook.onetoc2")
            )]
        );
    }

    #[test]
    fn workspace_round_trip_is_atomic() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("state/workspace.json");
        let expected = WorkspaceConfig {
            sources: vec![SourceDescriptor::native("/notes/Notebook.onetoc2")],
            navigation: WorkspaceNavigation {
                last_page: Some(PersistedPageLocation {
                    source_path: PathBuf::from("/notes/Notebook.onetoc2"),
                    source_id: SourceId::new("source"),
                    section_id: SectionId::new("section"),
                    page_id: PageId::new("page"),
                }),
            },
            ui: Some(WorkspaceUiState::new(
                PersistedPaneState {
                    notebooks_collapsed: true,
                    pages_collapsed: false,
                },
                vec![PersistedSourceTreeState {
                    source_path: PathBuf::from("/notes/Notebook.onetoc2"),
                    source_id: SourceId::new("source"),
                    notebook_expanded: true,
                    expanded_groups: vec![SectionId::new("nested"), SectionId::new("group")],
                }],
            )),
        };

        save(&path, &expected).expect("save");
        let actual = load(&path).expect("load");

        assert_eq!(actual, expected);
        assert!(!path.with_extension("json.new").exists());
    }

    #[test]
    fn legacy_workspace_without_navigation_uses_empty_history() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("workspace.json");
        fs::write(&path, r#"{"sources":["/notes/Notebook.onetoc2"]}"#).expect("legacy workspace");

        let actual = load(&path).expect("load legacy workspace");

        assert_eq!(
            actual.sources,
            vec![SourceDescriptor::native("/notes/Notebook.onetoc2")]
        );
        assert_eq!(actual.navigation, WorkspaceNavigation::default());
        assert_eq!(actual.ui, None);
    }

    #[test]
    fn malformed_optional_ui_does_not_discard_workspace_navigation() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("workspace.json");
        fs::write(
            &path,
            r#"{
                "sources":["/notes/Notebook.onetoc2"],
                "navigation":{"last_page":{
                    "source_path":"/notes/Notebook.onetoc2",
                    "source_id":"source",
                    "section_id":"section",
                    "page_id":"page"
                }},
                "ui":{"version":"invalid","panes":[]}
            }"#,
        )
        .expect("workspace");

        let actual = load(&path).expect("load workspace");

        assert_eq!(actual.sources.len(), 1);
        assert_eq!(
            actual
                .navigation
                .last_page
                .as_ref()
                .map(|page| &page.page_id),
            Some(&PageId::new("page"))
        );
        assert_eq!(actual.ui, None);
    }

    #[test]
    fn unknown_ui_schema_version_is_ignored() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("workspace.json");
        fs::write(
            &path,
            r#"{"sources":["/notes/Notebook.onetoc2"],"ui":{"version":99}}"#,
        )
        .expect("workspace");

        let actual = load(&path).expect("load workspace");

        assert_eq!(actual.sources.len(), 1);
        assert_eq!(actual.ui, None);
    }

    #[test]
    fn ui_state_preserves_source_order_while_normalizing_identifiers() {
        let ui = WorkspaceUiState::new(
            PersistedPaneState::default(),
            vec![
                PersistedSourceTreeState {
                    source_path: PathBuf::from("/notes/B.onetoc2"),
                    source_id: SourceId::new("b"),
                    notebook_expanded: true,
                    expanded_groups: vec![SectionId::new("two"), SectionId::new("one")],
                },
                PersistedSourceTreeState {
                    source_path: PathBuf::from("/notes/A.onetoc2"),
                    source_id: SourceId::new("a"),
                    notebook_expanded: false,
                    expanded_groups: vec![SectionId::new("same"), SectionId::new("same")],
                },
            ],
        );

        assert_eq!(ui.sources[0].source_id, SourceId::new("b"));
        assert_eq!(
            ui.sources[0].expanded_groups,
            [SectionId::new("one"), SectionId::new("two")]
        );
        assert_eq!(ui.sources[1].source_id, SourceId::new("a"));
        assert_eq!(ui.sources[1].expanded_groups, [SectionId::new("same")]);
    }

    #[test]
    fn every_pane_collapse_combination_round_trips() {
        for notebooks_collapsed in [false, true] {
            for pages_collapsed in [false, true] {
                let value = WorkspaceConfig {
                    ui: Some(WorkspaceUiState::new(
                        PersistedPaneState {
                            notebooks_collapsed,
                            pages_collapsed,
                        },
                        Vec::new(),
                    )),
                    ..WorkspaceConfig::default()
                };
                let encoded = serde_json::to_vec(&value).expect("serialize");
                let decoded: WorkspaceConfig =
                    serde_json::from_slice(&encoded).expect("deserialize");
                assert_eq!(decoded, value);
            }
        }
    }

    #[test]
    fn empty_library_is_valid() {
        let temporary = tempfile::tempdir().expect("temporary directory");

        let discovered = discover_library(temporary.path()).expect("library discovery");

        assert!(discovered.is_empty());
    }

    #[test]
    fn library_discovers_sibling_notebooks_without_nested_group_manifests() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        for name in ["Notebook A", "Notebook B"] {
            let notebook = temporary.path().join(name);
            let group = notebook.join("Group");
            fs::create_dir_all(&group).expect("group");
            fs::write(notebook.join("Open Notebook.onetoc2"), b"root").expect("root manifest");
            fs::write(group.join("Open Notebook.onetoc2"), b"group").expect("group manifest");
            fs::write(group.join("Section.one"), b"section").expect("section");
        }

        let discovered = discover_library(temporary.path()).expect("library discovery");

        assert_eq!(
            discovered,
            vec![
                SourceDescriptor::native(temporary.path().join("Notebook A/Open Notebook.onetoc2")),
                SourceDescriptor::native(temporary.path().join("Notebook B/Open Notebook.onetoc2")),
            ]
        );
    }

    #[test]
    fn library_groups_a_manifest_free_backup_directory_as_one_source() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let backup = temporary.path().join("Backup");
        fs::create_dir_all(backup.join("Nested")).expect("nested group");
        fs::write(backup.join("Root (On 15-08-2026).one"), b"root").expect("root section");
        fs::write(backup.join("Nested/Section (On 15-08-2026).one"), b"nested")
            .expect("nested section");

        let discovered = discover_library(temporary.path()).expect("library discovery");

        assert_eq!(
            discovered,
            vec![SourceDescriptor::backup(
                backup,
                BackupSelectionPolicy::LatestPerSection
            )]
        );
    }

    #[test]
    fn ordinary_folder_open_requires_explicit_backup_mode_without_manifest() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        fs::write(temporary.path().join("Section.one"), b"section").expect("section");

        let error = discover(temporary.path()).expect_err("backup mode must be explicit");

        assert!(error.to_string().contains("Open OneNote Backup Folder"));
    }

    #[test]
    fn source_membership_uses_path_components() {
        let root = Path::new("/home/user/Documents/OneNoteViewer");

        assert!(source_is_in_location(
            Path::new("/home/user/Documents/OneNoteViewer/Work/Open Notebook.onetoc2"),
            root
        ));
        assert!(!source_is_in_location(
            Path::new("/home/user/Documents/OneNoteViewer-old/Work.one"),
            root
        ));
    }

    #[test]
    fn workspace_membership_accepts_default_and_explicit_sources() {
        let default = Path::new("/home/user/Documents/OneNoteViewer");
        let explicit = vec![SourceDescriptor::native(
            "/mnt/archive/Notebook/Open Notebook.onetoc2",
        )];

        assert!(source_is_in_workspace(
            Path::new("/home/user/Documents/OneNoteViewer/Work/Open Notebook.onetoc2"),
            &explicit,
            default
        ));
        assert!(source_is_in_workspace(
            Path::new("/mnt/archive/Notebook/Open Notebook.onetoc2"),
            &explicit,
            default
        ));
        assert!(!source_is_in_workspace(
            Path::new("/mnt/archive/Closed/Open Notebook.onetoc2"),
            &explicit,
            default
        ));
    }
}

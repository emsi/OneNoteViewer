use anyhow::{bail, Context, Result};
use onenote_core::{PageId, SectionId, SourceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_DISCOVERY_ENTRIES: usize = 100_000;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct WorkspaceConfig {
    #[serde(default)]
    pub(crate) sources: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) navigation: WorkspaceNavigation,
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

pub(crate) fn discover(requested: &Path) -> Result<Vec<PathBuf>> {
    discover_path(requested, false)
}

pub(crate) fn discover_library(requested: &Path) -> Result<Vec<PathBuf>> {
    discover_path(requested, true)
}

fn discover_path(requested: &Path, allow_empty: bool) -> Result<Vec<PathBuf>> {
    let canonical = fs::canonicalize(requested)
        .with_context(|| format!("could not access {}", requested.display()))?;
    if canonical.is_file() {
        return match extension(&canonical).as_deref() {
            Some("one" | "onetoc2") => Ok(vec![canonical]),
            Some("onepkg") => bail!(
                "{} is a package; use Import OneNote Package so it is extracted on disk",
                canonical.display()
            ),
            _ => bail!("{} is not a supported OneNote source", canonical.display()),
        };
    }
    if !canonical.is_dir() {
        bail!("{} is not a regular file or directory", canonical.display());
    }

    let mut pending = vec![canonical.clone()];
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
    let roots = root_manifests(&canonical, table_of_contents);
    if !roots.is_empty() {
        return Ok(roots);
    }
    sections.sort();
    sections.dedup();
    if sections.is_empty() && !allow_empty {
        bail!("{} contains no .onetoc2 or .one files", canonical.display());
    }
    Ok(sections)
}

pub(crate) fn source_is_in_location(source: &Path, location: &Path) -> bool {
    match (fs::canonicalize(source), fs::canonicalize(location)) {
        (Ok(source), Ok(location)) => source.starts_with(location),
        _ => source.starts_with(location),
    }
}

pub(crate) fn source_is_in_workspace(
    source: &Path,
    sources: &[PathBuf],
    notebooks_location: &Path,
) -> bool {
    source_is_in_location(source, notebooks_location)
        || sources
            .iter()
            .any(|configured| source_is_in_location(source, configured))
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

        assert_eq!(discovered, vec![notebook.join("Open Notebook.onetoc2")]);
    }

    #[test]
    fn workspace_round_trip_is_atomic() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("state/workspace.json");
        let expected = WorkspaceConfig {
            sources: vec![PathBuf::from("/notes/Notebook.onetoc2")],
            navigation: WorkspaceNavigation {
                last_page: Some(PersistedPageLocation {
                    source_path: PathBuf::from("/notes/Notebook.onetoc2"),
                    source_id: SourceId::new("source"),
                    section_id: SectionId::new("section"),
                    page_id: PageId::new("page"),
                }),
            },
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
            vec![PathBuf::from("/notes/Notebook.onetoc2")]
        );
        assert_eq!(actual.navigation, WorkspaceNavigation::default());
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
                temporary.path().join("Notebook A/Open Notebook.onetoc2"),
                temporary.path().join("Notebook B/Open Notebook.onetoc2"),
            ]
        );
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
        let explicit = vec![PathBuf::from("/mnt/archive/Notebook/Open Notebook.onetoc2")];

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

use anyhow::{Context, Result};
use onenote_render_gtk::InkEditDocument;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const FILE_VERSION: u32 = 1;

#[derive(Default, Deserialize, Serialize)]
struct InkEditFile {
    version: u32,
    pages: BTreeMap<String, InkEditDocument>,
}

pub(crate) fn page_key(source_id: &str, fingerprint: &str, page_id: &str) -> String {
    blake3::hash(format!("{source_id}\n{fingerprint}\n{page_id}").as_bytes())
        .to_hex()
        .to_string()
}

pub(crate) fn load_page(key: &str) -> Result<InkEditDocument> {
    let file = load_file(&path())?;
    Ok(file.pages.get(key).cloned().unwrap_or_default())
}

pub(crate) fn save_page(key: &str, edits: &InkEditDocument) -> Result<()> {
    let path = path();
    let mut file = load_file(&path)?;
    if edits.added_strokes.is_empty()
        && edits.erased_strokes.is_empty()
        && edits.text_replacements.is_empty()
    {
        file.pages.remove(key);
    } else {
        file.pages.insert(key.to_owned(), edits.clone());
    }
    file.version = FILE_VERSION;
    let parent = path
        .parent()
        .context("ink edit storage path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    set_private_dir(parent)?;
    let temporary = path.with_extension("json.new");
    let bytes = serde_json::to_vec(&file).context("could not encode ink edits")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    set_private_file(&temporary)?;
    fs::rename(&temporary, &path).with_context(|| format!("could not publish {}", path.display()))
}

fn load_file(path: &Path) -> Result<InkEditFile> {
    match fs::read(path) {
        Ok(bytes) => {
            let file: InkEditFile =
                serde_json::from_slice(&bytes).context("ink edit file is invalid")?;
            anyhow::ensure!(
                file.version == FILE_VERSION,
                "unsupported ink edit file version"
            );
            Ok(file)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(InkEditFile {
            version: FILE_VERSION,
            ..InkEditFile::default()
        }),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

fn path() -> PathBuf {
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|| std::env::temp_dir().join("onenote-viewer-state"));
    state_dir.join("onenote-viewer").join("ink-edits.json")
}

fn set_private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("could not protect {}", path.display()))
}

fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("could not protect {}", path.display()))
}

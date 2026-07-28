use anyhow::{bail, Context, Result};
use gtk::glib;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AppSettings {
    #[serde(default = "default_notebooks_location")]
    pub(crate) notebooks_location: PathBuf,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            notebooks_location: default_notebooks_location(),
        }
    }
}

pub(crate) fn path() -> PathBuf {
    glib::user_config_dir()
        .join("onenote-viewer")
        .join("settings.json")
}

pub(crate) fn default_notebooks_location() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Documents)
        .unwrap_or_else(|| glib::home_dir().join("Documents"))
        .join("OneNoteViewer")
}

pub(crate) fn load(path: &Path) -> Result<AppSettings> {
    let settings = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid settings file {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
        Err(error) => {
            return Err(error).with_context(|| format!("could not read {}", path.display()));
        }
    };
    validate(&settings)?;
    Ok(settings)
}

pub(crate) fn save(path: &Path, settings: &AppSettings) -> Result<()> {
    validate(settings)?;
    let parent = path
        .parent()
        .context("settings path does not have a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("could not create {}", parent.display()))?;
    let temporary = path.with_extension("json.new");
    let bytes = serde_json::to_vec_pretty(settings).context("could not serialize settings")?;
    fs::write(&temporary, bytes)
        .with_context(|| format!("could not write {}", temporary.display()))?;
    set_private_file(&temporary)?;
    fs::rename(&temporary, path).with_context(|| format!("could not publish {}", path.display()))
}

pub(crate) fn ensure_notebooks_location(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("notebooks location must be an absolute path");
    }
    fs::create_dir_all(path).with_context(|| {
        format!(
            "could not create default notebooks location {}",
            path.display()
        )
    })?;
    if !path.is_dir() {
        bail!("notebooks location is not a directory: {}", path.display());
    }
    Ok(())
}

fn validate(settings: &AppSettings) -> Result<()> {
    if settings.notebooks_location.as_os_str().is_empty() {
        bail!("default notebooks location cannot be empty");
    }
    if !settings.notebooks_location.is_absolute() {
        bail!("default notebooks location must be an absolute path");
    }
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
    fn settings_round_trip_is_atomic() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("config/settings.json");
        let expected = AppSettings {
            notebooks_location: temporary.path().join("Documents/OneNoteViewer"),
        };

        save(&path, &expected).expect("save");
        let actual = load(&path).expect("load");

        assert_eq!(actual.notebooks_location, expected.notebooks_location);
        assert!(!path.with_extension("json.new").exists());
    }

    #[test]
    fn missing_notebooks_location_migrates_to_default() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("settings.json");
        fs::write(&path, b"{}").expect("legacy settings");

        let actual = load(&path).expect("load");

        assert_eq!(actual.notebooks_location, default_notebooks_location());
    }

    #[test]
    fn relative_notebooks_location_is_rejected() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let path = temporary.path().join("settings.json");
        fs::write(&path, br#"{"notebooks_location":"relative"}"#).expect("settings");

        let error = load(&path).expect_err("relative path must fail");

        assert!(error.to_string().contains("absolute"));
    }
}

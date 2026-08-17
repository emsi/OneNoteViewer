use onenote_core::{
    BackupFolderLoader, BackupFolderOptions, BackupLoadControl, LoadedNotebook, OneNoteLoader,
};
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("missing source path");
        return ExitCode::from(2);
    };

    match load_source(Path::new(&path)) {
        Ok(loaded) => {
            let diagnostics = loaded
                .notebook
                .diagnostics
                .iter()
                .chain(
                    loaded
                        .notebook
                        .sections()
                        .flat_map(|section| section.diagnostics.iter()),
                )
                .collect::<Vec<_>>();
            println!(
                "sections={}\tpages={}\tdiagnostics={}\tresources={}",
                loaded.notebook.sections().count(),
                loaded.notebook.pages().count(),
                diagnostics.len(),
                loaded.resources.len(),
            );
            for diagnostic in diagnostics {
                eprintln!(
                    "DIAG\t{:?}\t{}\t{}\t{}",
                    diagnostic.severity,
                    diagnostic.code,
                    diagnostic
                        .page_id
                        .as_ref()
                        .map_or("-", |page_id| page_id.as_str()),
                    diagnostic.message.replace(['\r', '\n', '\t'], " "),
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}

fn load_source(path: &Path) -> Result<LoadedNotebook, String> {
    if !path.is_dir() {
        return OneNoteLoader::default()
            .load(path)
            .map_err(|error| error.to_string());
    }
    let loader = BackupFolderLoader::default();
    let control = BackupLoadControl::new();
    let inspection = loader
        .inspect(path, BackupFolderOptions::default(), &control, |_| {})
        .map_err(|error| error.to_string())?;
    loader
        .load(inspection, &control, |_| {})
        .map(|result| result.loaded)
        .map_err(|error| error.to_string())
}

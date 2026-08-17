use gtk::gio;
use onenote_core::{
    BackupFolderLimits, BackupFolderLoader, BackupFolderOptions, BackupLoadControl,
    BackupLoadProgress, ExtractionPhase, LoadOptions, LoadedNotebook, OneNoteLoader,
    OnePkgExtractor, ParseLimits, RootManifestPolicy, SourceDescriptor, SourceId,
};
use onenote_index::{IndexProfile, IndexUpdate, SearchHit, SearchIndex, SearchQuery};
use onenote_render::{PageScene, SceneBuilder, SceneOptions};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

pub(crate) enum SourceCommand {
    Discover(PathBuf),
    DiscoverLibrary(PathBuf),
    Load {
        source: SourceDescriptor,
        options: LoadOptions,
        index_profile: IndexProfile,
        operation_id: Option<u64>,
        control: BackupLoadControl,
        root_manifest: RootManifestPolicy,
    },
    Shutdown,
}

pub(crate) enum IndexCommand {
    Ensure {
        loaded: Arc<LoadedNotebook>,
        profile: IndexProfile,
        generation: u64,
        cancel: Arc<AtomicBool>,
    },
    Remove(SourceId),
    #[cfg(test)]
    Pause {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    },
    Shutdown,
}

pub(crate) enum Event {
    Discovered {
        requested: PathBuf,
        result: Result<Vec<SourceDescriptor>, String>,
    },
    LibraryDiscovered {
        location: PathBuf,
        result: Result<Vec<SourceDescriptor>, String>,
    },
    Loaded {
        source: SourceDescriptor,
        index_profile: IndexProfile,
        operation_id: Option<u64>,
        result: Result<SourceLoad, String>,
    },
    BackupProgress {
        operation_id: Option<u64>,
        progress: BackupLoadProgress,
    },
    BackupFallbackRequired {
        operation_id: u64,
        root: PathBuf,
        manifest_error: String,
    },
    Indexed {
        source_id: SourceId,
        generation: u64,
        result: Result<IndexUpdate, String>,
    },
    Search {
        generation: u64,
        result: Result<Vec<SearchHit>, String>,
    },
    Scene {
        generation: u64,
        result: Result<Arc<PageScene>, String>,
    },
    Extracted {
        operation_id: u64,
        result: Result<PathBuf, String>,
    },
    ExtractionProgress {
        operation_id: u64,
        phase: ExtractionPhase,
    },
    AttachmentProgress {
        operation_id: u64,
        copied_bytes: u64,
        declared_bytes: Option<u64>,
    },
    AttachmentCopied {
        operation_id: u64,
        purpose: AttachmentPurpose,
        destination: gio::File,
        result: Result<u64, String>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct SourceLoad {
    pub(crate) loaded: Arc<LoadedNotebook>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttachmentPurpose {
    Open,
    Save,
}

pub(crate) fn start_source_worker(events: mpsc::Sender<Event>) -> mpsc::Sender<SourceCommand> {
    let (commands, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(command) = receiver.recv() {
            match command {
                SourceCommand::Discover(requested) => {
                    let result =
                        crate::workspace::discover(&requested).map_err(|error| error.to_string());
                    if events
                        .send(Event::Discovered { requested, result })
                        .is_err()
                    {
                        return;
                    }
                }
                SourceCommand::DiscoverLibrary(location) => {
                    let result = crate::workspace::discover_library(&location)
                        .map_err(|error| error.to_string());
                    if events
                        .send(Event::LibraryDiscovered { location, result })
                        .is_err()
                    {
                        return;
                    }
                }
                SourceCommand::Load {
                    source,
                    options,
                    index_profile,
                    operation_id,
                    control,
                    root_manifest,
                } => {
                    if let Some(event) = load_source_event(
                        &source,
                        options,
                        index_profile,
                        operation_id,
                        &control,
                        root_manifest,
                        &events,
                    ) {
                        if events.send(event).is_err() {
                            return;
                        }
                    }
                }
                SourceCommand::Shutdown => return,
            }
        }
    });
    commands
}

fn load_source_event(
    source: &SourceDescriptor,
    options: LoadOptions,
    index_profile: IndexProfile,
    operation_id: Option<u64>,
    control: &BackupLoadControl,
    root_manifest: RootManifestPolicy,
    events: &mpsc::Sender<Event>,
) -> Option<Event> {
    let mut published_source = source.clone();
    let result = match source {
        SourceDescriptor::NativeFile { path } => OneNoteLoader::with_options(options)
            .load(path)
            .map(|notebook| SourceLoad {
                loaded: Arc::new(notebook),
            })
            .map_err(|error| error.to_string()),
        SourceDescriptor::BackupFolder { root, selection } => {
            let loader = BackupFolderLoader::with_options(
                BackupFolderLimits::default(),
                ParseLimits::default(),
                options,
            );
            let inspection = loader.inspect(
                root,
                BackupFolderOptions {
                    selection: *selection,
                    root_manifest,
                },
                control,
                |progress| {
                    let _ignored = events.send(Event::BackupProgress {
                        operation_id,
                        progress,
                    });
                },
            );
            match inspection {
                Err(onenote_core::BackupFolderError::RootManifestPresent { path })
                    if root_manifest == RootManifestPolicy::Reject =>
                {
                    match OneNoteLoader::with_options(options).load(&path) {
                        Ok(notebook) => {
                            published_source = SourceDescriptor::native(path);
                            Ok(SourceLoad {
                                loaded: Arc::new(notebook),
                            })
                        }
                        Err(error) => {
                            let operation_id = operation_id?;
                            let _ignored = events.send(Event::BackupFallbackRequired {
                                operation_id,
                                root: root.clone(),
                                manifest_error: error.to_string(),
                            });
                            return None;
                        }
                    }
                }
                Err(error) => Err(error.to_string()),
                Ok(inspection) => {
                    published_source =
                        SourceDescriptor::backup(inspection.root.clone(), *selection);
                    loader
                        .load(inspection, control, |progress| {
                            let _ignored = events.send(Event::BackupProgress {
                                operation_id,
                                progress,
                            });
                        })
                        .map(|result| SourceLoad {
                            loaded: Arc::new(result.loaded),
                        })
                        .map_err(|error| error.to_string())
                }
            }
        }
    };
    Some(Event::Loaded {
        source: published_source,
        index_profile,
        operation_id,
        result,
    })
}

pub(crate) fn start_index_worker(
    index_path: PathBuf,
    events: mpsc::Sender<Event>,
) -> mpsc::Sender<IndexCommand> {
    let (commands, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut index = SearchIndex::open(&index_path).map_err(|error| error.to_string());
        while let Ok(command) = receiver.recv() {
            match command {
                IndexCommand::Ensure {
                    loaded,
                    profile,
                    generation,
                    cancel,
                } => {
                    let source_id = loaded.notebook.source_id.clone();
                    let result = match &mut index {
                        Ok(index) => index
                            .ensure_source(&loaded.notebook, &profile, &cancel, |_| {})
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error.clone()),
                    };
                    if events
                        .send(Event::Indexed {
                            source_id,
                            generation,
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                IndexCommand::Remove(source_id) => {
                    if let Ok(index) = &mut index {
                        let _ignored = index.remove_source(&source_id);
                    }
                }
                #[cfg(test)]
                IndexCommand::Pause { entered, release } => {
                    let _ignored = entered.send(());
                    let _ignored = release.recv();
                }
                IndexCommand::Shutdown => return,
            }
        }
    });
    commands
}

pub(crate) fn search(
    index_path: PathBuf,
    generation: u64,
    query: SearchQuery,
    events: mpsc::Sender<Event>,
) {
    std::thread::spawn(move || {
        let result = SearchIndex::open(index_path)
            .and_then(|index| index.search(&query, &AtomicBool::new(false)))
            .map_err(|error| error.to_string());
        let _ignored = events.send(Event::Search { generation, result });
    });
}

pub(crate) fn build_scene(
    generation: u64,
    page: onenote_core::Page,
    cancel: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
) {
    std::thread::spawn(move || {
        let result = SceneBuilder::with_options(SceneOptions {
            include_page_title: false,
            crop_to_content: true,
            ..SceneOptions::default()
        })
        .build(&page, &cancel)
        .map(Arc::new)
        .map_err(|error| error.to_string());
        let _ignored = events.send(Event::Scene { generation, result });
    });
}

pub(crate) fn extract(
    operation_id: u64,
    package: PathBuf,
    destination: PathBuf,
    cancel: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
) {
    std::thread::spawn(move || {
        let progress_events = events.clone();
        let result = OnePkgExtractor::detect()
            .and_then(|extractor| {
                extractor.extract_with_progress(&package, destination, &cancel, move |phase| {
                    let _ignored = progress_events.send(Event::ExtractionProgress {
                        operation_id,
                        phase,
                    });
                })
            })
            .map(|report| report.destination)
            .map_err(|error| error.to_string());
        let _ignored = events.send(Event::Extracted {
            operation_id,
            result,
        });
    });
}

pub(crate) fn copy_attachment(
    operation_id: u64,
    purpose: AttachmentPurpose,
    request: crate::attachment::CopyRequest,
    events: mpsc::Sender<Event>,
) {
    std::thread::spawn(move || {
        let progress_events = events.clone();
        let mut last_reported = 0_u64;
        let result = crate::attachment::copy_resource(&request, move |progress| {
            let completed = progress
                .declared_bytes
                .is_some_and(|declared| progress.copied_bytes == declared);
            if progress.copied_bytes == 0
                || completed
                || progress.copied_bytes.saturating_sub(last_reported) >= 1024 * 1024
            {
                last_reported = progress.copied_bytes;
                let _ignored = progress_events.send(Event::AttachmentProgress {
                    operation_id,
                    copied_bytes: progress.copied_bytes,
                    declared_bytes: progress.declared_bytes,
                });
            }
        });
        let _ignored = events.send(Event::AttachmentCopied {
            operation_id,
            purpose,
            destination: request.destination,
            result: result.map_err(|error| format!("{error:#}")),
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn source_loading_is_not_blocked_by_index_maintenance() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let index_path = temporary.path().join("index.sqlite");
        let (events, receiver) = mpsc::channel();
        let index_commands = start_index_worker(index_path, events.clone());
        let source_commands = start_source_worker(events);
        let (entered_sender, entered_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel();
        index_commands
            .send(IndexCommand::Pause {
                entered: entered_sender,
                release: release_receiver,
            })
            .expect("pause index worker");
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("index worker entered pause");

        let missing = temporary.path().join("missing.one");
        source_commands
            .send(SourceCommand::Load {
                source: SourceDescriptor::native(missing.clone()),
                options: LoadOptions::default(),
                index_profile: IndexProfile::new("test"),
                operation_id: None,
                control: BackupLoadControl::new(),
                root_manifest: RootManifestPolicy::Ignore,
            })
            .expect("queue source load");
        let event = receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("source worker remained responsive");
        assert!(matches!(
            event,
            Event::Loaded {
                source,
                result: Err(_),
                ..
            } if source.path() == missing
        ));

        release_sender.send(()).expect("release index worker");
        source_commands
            .send(SourceCommand::Shutdown)
            .expect("stop source worker");
        index_commands
            .send(IndexCommand::Shutdown)
            .expect("stop index worker");
    }

    #[test]
    fn unreadable_root_manifest_requires_explicit_backup_fallback() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        std::fs::write(temporary.path().join("Open Notebook.onetoc2"), b"invalid")
            .expect("manifest");
        std::fs::write(temporary.path().join("Section.one"), b"invalid").expect("section");
        let (events, receiver) = mpsc::channel();
        let commands = start_source_worker(events);
        commands
            .send(SourceCommand::Load {
                source: SourceDescriptor::backup(
                    temporary.path(),
                    onenote_core::BackupSelectionPolicy::LatestPerSection,
                ),
                options: LoadOptions::default(),
                index_profile: IndexProfile::new("test"),
                operation_id: Some(7),
                control: BackupLoadControl::new(),
                root_manifest: RootManifestPolicy::Reject,
            })
            .expect("queue source load");

        let fallback = (0..10).find_map(|_| {
            receiver
                .recv_timeout(Duration::from_secs(1))
                .ok()
                .and_then(|event| match event {
                    Event::BackupFallbackRequired {
                        operation_id, root, ..
                    } => Some((operation_id, root)),
                    _ => None,
                })
        });
        assert_eq!(fallback, Some((7, temporary.path().to_path_buf())));
        commands
            .send(SourceCommand::Shutdown)
            .expect("stop source worker");
    }
}

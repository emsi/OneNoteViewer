use onenote_core::{ExtractionPhase, LoadedNotebook, OneNoteLoader, OnePkgExtractor, SourceId};
use onenote_index::{SearchHit, SearchIndex, SearchQuery};
use onenote_render::{PageScene, SceneBuilder, SceneOptions};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

pub(crate) enum Command {
    Discover(PathBuf),
    DiscoverLibrary(PathBuf),
    Load(PathBuf),
    Remove(SourceId),
    Shutdown,
}

pub(crate) enum Event {
    Discovered {
        requested: PathBuf,
        result: Result<Vec<PathBuf>, String>,
    },
    LibraryDiscovered {
        location: PathBuf,
        result: Result<Vec<PathBuf>, String>,
    },
    Loaded {
        path: PathBuf,
        result: Result<Arc<LoadedNotebook>, String>,
    },
    Indexed {
        source_id: SourceId,
        result: Result<(), String>,
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
        result: Result<PathBuf, String>,
    },
    ExtractionProgress {
        phase: ExtractionPhase,
    },
}

pub(crate) fn start_index_worker(
    index_path: PathBuf,
    events: mpsc::Sender<Event>,
) -> mpsc::Sender<Command> {
    let (commands, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut index = SearchIndex::open(&index_path).map_err(|error| error.to_string());
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Discover(requested) => {
                    let result =
                        crate::workspace::discover(&requested).map_err(|error| error.to_string());
                    if events
                        .send(Event::Discovered { requested, result })
                        .is_err()
                    {
                        return;
                    }
                }
                Command::DiscoverLibrary(location) => {
                    let result = crate::workspace::discover_library(&location)
                        .map_err(|error| error.to_string());
                    if events
                        .send(Event::LibraryDiscovered { location, result })
                        .is_err()
                    {
                        return;
                    }
                }
                Command::Load(path) => {
                    let result = OneNoteLoader::default()
                        .load(&path)
                        .map(Arc::new)
                        .map_err(|error| error.to_string());
                    let loaded = result.as_ref().ok().cloned();
                    if events.send(Event::Loaded { path, result }).is_err() {
                        return;
                    }
                    if let Some(loaded) = loaded {
                        let source_id = loaded.notebook.source_id.clone();
                        let result = match &mut index {
                            Ok(index) => index
                                .replace_source(&loaded.notebook, &AtomicBool::new(false), |_| {})
                                .map_err(|error| error.to_string()),
                            Err(error) => Err(error.clone()),
                        };
                        if events.send(Event::Indexed { source_id, result }).is_err() {
                            return;
                        }
                    }
                }
                Command::Remove(source_id) => {
                    if let Ok(index) = &mut index {
                        let _ignored = index.remove_source(&source_id);
                    }
                }
                Command::Shutdown => return,
            }
        }
    });
    commands
}

pub(crate) fn search(
    index_path: PathBuf,
    generation: u64,
    text: String,
    events: mpsc::Sender<Event>,
) {
    std::thread::spawn(move || {
        let result = SearchIndex::open(index_path)
            .and_then(|index| index.search(&SearchQuery::simple(text), &AtomicBool::new(false)))
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
                    let _ignored = progress_events.send(Event::ExtractionProgress { phase });
                })
            })
            .map(|report| report.destination)
            .map_err(|error| error.to_string());
        let _ignored = events.send(Event::Extracted { result });
    });
}

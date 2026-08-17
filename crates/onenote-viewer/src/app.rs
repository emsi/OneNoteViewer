use crate::input::{focus_initial_navigation, history_mouse_controller};
use crate::navigation::{NavigationTarget, NotebookTree, SourceTreeExpansion};
use crate::navigation_history::{HistoryDirection, NavigationHistory, PageLocation};
use crate::navigation_state::{location_for_section, preferred_location, SectionLocation};
use crate::settings::{self, AppSettings, ThemePreference};
use crate::worker::{self, Event, IndexCommand, SourceCommand};
use crate::workspace::{
    self, PersistedPageLocation, PersistedPaneState, PersistedSourceTreeState, WorkspaceConfig,
    WorkspaceNavigation, WorkspaceUiState,
};
use anyhow::Result;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{
    BackupLoadControl, BackupLoadProgress, BackupProgressPhase, BackupSelectionPolicy,
    ExtractionPhase, LoadOptions, LoadedNotebook, Notebook, ObjectId, Page, PageId, Rect,
    ResourceId, ResourceRef, SectionId, SourceDescriptor, SourceId,
};
use onenote_index::{IndexProfile, IndexUpdate, MatchedField, SearchHit, SearchQuery, TextRange};
use onenote_render::{HitAction, ScenePrimitive};
use onenote_render_gtk::{
    find_text_ranges, FindMatch, FindOptions, FindTextRange, PageView, DEFAULT_ZOOM,
};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

const NO_SELECTION: u32 = gtk::INVALID_LIST_POSITION;
const NOTEBOOK_NAVIGATION_WIDTH: i32 = 320;
const PAGE_NAVIGATION_WIDTH: i32 = 280;
const COLLAPSED_NAVIGATION_WIDTH: i32 = 42;
const NAVIGATION_SEPARATOR_WIDTH: i32 = 1;
const SEARCH_RESULTS_WIDTH: i32 = 520;
const PAGE_FIND_LIMIT: usize = 5_000;
const APP_ICON_NAME: &str = "io.github.emsi.OneNoteViewer";
const SYMBOLIC_ICON_NAMES: [&str; 22] = [
    "onenote-chevron-down-symbolic",
    "onenote-chevron-right-symbolic",
    "onenote-close-symbolic",
    "onenote-find-next-symbolic",
    "onenote-find-previous-symbolic",
    "onenote-folder-symbolic",
    "onenote-import-package-symbolic",
    "onenote-menu-symbolic",
    "onenote-notebook-symbolic",
    "onenote-open-file-symbolic",
    "onenote-open-folder-symbolic",
    "onenote-panel-collapse-symbolic",
    "onenote-panel-expand-symbolic",
    "onenote-settings-symbolic",
    "onenote-search-symbolic",
    "onenote-zoom-in-symbolic",
    "onenote-zoom-out-symbolic",
    "onenote-zoom-reset-symbolic",
    "window-close-symbolic",
    "window-maximize-symbolic",
    "window-minimize-symbolic",
    "window-restore-symbolic",
];

pub(crate) fn run(requested_sources: Vec<PathBuf>) -> Result<()> {
    register_resources()?;
    std::thread::spawn(crate::attachment::prune_cache);
    let (workspace_path, index_path) = workspace::paths()?;
    workspace::ensure_index_parent(&index_path)?;
    let persisted = workspace::load(&workspace_path).unwrap_or_default();
    let settings_path = settings::path();
    let persisted_settings = settings::load(&settings_path).unwrap_or_default();
    let notebooks_location = persisted_settings.notebooks_location.clone();
    let (restore, initial_sources, restore_is_outside_default_location) =
        prepare_workspace_restore(persisted, &requested_sources, &notebooks_location);

    let application = gtk::Application::builder()
        .application_id("io.github.emsi.OneNoteViewer")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let viewer = Rc::new(RefCell::new(None::<Rc<Viewer>>));
    let viewer_on_activate = Rc::clone(&viewer);
    application.connect_activate(move |application| {
        if let Some(viewer) = viewer_on_activate.borrow().as_ref() {
            viewer.window.present();
            return;
        }
        let style_provider = install_resources(persisted_settings.theme);
        let instance = Viewer::new(
            application,
            workspace_path.clone(),
            index_path.clone(),
            settings_path.clone(),
            persisted_settings.clone(),
            restore.clone(),
            style_provider,
        );
        if !requested_sources.is_empty() {
            for source in &requested_sources {
                instance.discover(source.clone());
            }
        } else if restore_is_outside_default_location {
            for source in &initial_sources {
                instance.load_source(source.clone(), None);
            }
        }
        instance.open_notebooks_location();
        if requested_sources.is_empty() && !restore_is_outside_default_location {
            for source in &initial_sources {
                instance.load_source(source.clone(), None);
            }
        }
        instance.window.present();
        focus_initial_navigation(instance.window.upcast_ref(), &instance.notebook_tree.view);
        if let Some(delay) = smoke_quit_delay() {
            let application = application.clone();
            glib::timeout_add_local_once(delay, move || application.quit());
        }
        *viewer_on_activate.borrow_mut() = Some(instance);
    });

    let status = application.run_with_args::<&str>(&[]);
    if let Some(viewer) = viewer.borrow().as_ref() {
        viewer.cancel_foreground_operation_for_shutdown(Duration::from_secs(5));
        let workspace_result = viewer.flush_workspace();
        let settings_result = viewer.flush_settings();
        viewer.cancel_index_jobs();
        let _ignored = viewer.source_commands.send(SourceCommand::Shutdown);
        let _ignored = viewer.index_commands.send(IndexCommand::Shutdown);
        workspace_result?;
        settings_result?;
    }
    if status == glib::ExitCode::SUCCESS {
        Ok(())
    } else {
        anyhow::bail!("GTK application exited with status {status:?}")
    }
}

pub(crate) fn check_icons() -> Result<()> {
    register_resources()?;
    gtk::init()?;
    let _style_provider = install_resources(ThemePreference::Light);

    let display = gdk_display();
    let theme = gtk::IconTheme::for_display(&display);
    if !theme.has_icon(APP_ICON_NAME) {
        anyhow::bail!("application icon is not registered: {APP_ICON_NAME}");
    }
    let renderer = gtk::gsk::CairoRenderer::new();
    renderer.realize_for_display(&display)?;
    let colors = [
        gtk::gdk::RGBA::new(0.125, 0.129, 0.141, 1.0),
        gtk::gdk::RGBA::new(0.75, 0.0, 0.0, 1.0),
        gtk::gdk::RGBA::new(0.8, 0.45, 0.0, 1.0),
        gtk::gdk::RGBA::new(0.0, 0.55, 0.2, 1.0),
        gtk::gdk::RGBA::new(0.35, 0.18, 0.56, 1.0),
    ];
    let viewport = gtk::graphene::Rect::new(0.0, 0.0, 24.0, 24.0);

    for name in SYMBOLIC_ICON_NAMES {
        if !theme.has_icon(name) {
            anyhow::bail!("symbolic icon is not registered: {name}");
        }
        let paintable = theme.lookup_icon(
            name,
            &[],
            24,
            1,
            gtk::TextDirection::None,
            gtk::IconLookupFlags::FORCE_SYMBOLIC,
        );
        let symbolic = paintable
            .dynamic_cast::<gtk::SymbolicPaintable>()
            .map_err(|_| anyhow::anyhow!("icon is not symbolic: {name}"))?;
        let snapshot = gtk::Snapshot::new();
        symbolic.snapshot_symbolic(&snapshot, 24.0, 24.0, &colors);
        let node = snapshot
            .to_node()
            .ok_or_else(|| anyhow::anyhow!("symbolic icon rendered no content: {name}"))?;
        let texture = renderer.render_texture(&node, Some(&viewport));
        let mut pixels = vec![0; 24 * 24 * 4];
        texture.download(&mut pixels, 24 * 4);
        let painted = pixels
            .chunks_exact(4)
            .filter(|pixel| native_pixel_alpha(pixel) != 0)
            .count();
        if !(4..=432).contains(&painted) {
            anyhow::bail!(
                "symbolic icon has implausible painted coverage: {name} ({painted}/576 pixels)"
            );
        }
        if let Some((x, y)) = transparent_probe(name) {
            let offset = (y * 24 + x) * 4;
            if native_pixel_alpha(&pixels[offset..offset + 4]) != 0 {
                anyhow::bail!(
                    "symbolic icon filled a required transparent interior: {name} ({x}, {y})"
                );
            }
        }
    }
    renderer.unrealize();
    let collapsed_minimum = collapsed_navigation_minimum_width();
    if collapsed_minimum > COLLAPSED_NAVIGATION_WIDTH {
        anyhow::bail!(
            "collapsed navigation requires {collapsed_minimum}px but receives \
             {COLLAPSED_NAVIGATION_WIDTH}px"
        );
    }
    println!(
        "Verified {} symbolic icons and collapsed navigation layout with GTK {}.{}.{}",
        SYMBOLIC_ICON_NAMES.len(),
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version()
    );
    Ok(())
}

fn native_pixel_alpha(pixel: &[u8]) -> u8 {
    (u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]) >> 24) as u8
}

fn transparent_probe(name: &str) -> Option<(usize, usize)> {
    match name {
        "onenote-folder-symbolic" | "onenote-settings-symbolic" | "onenote-zoom-reset-symbolic" => {
            Some((12, 12))
        }
        "onenote-import-package-symbolic" => Some((12, 8)),
        "onenote-notebook-symbolic" | "onenote-open-file-symbolic" => Some((10, 12)),
        "onenote-open-folder-symbolic" => Some((12, 15)),
        "onenote-panel-collapse-symbolic" | "onenote-panel-expand-symbolic" => Some((5, 5)),
        "onenote-zoom-in-symbolic" | "onenote-zoom-out-symbolic" => Some((7, 7)),
        _ => None,
    }
}

fn collapsed_navigation_minimum_width() -> i32 {
    let (_, page_list) = list_view(&gtk::StringList::new(&[]), "page-list");
    let pages =
        CollapsibleNavigationBand::new("PAGES", &page_list, PAGE_NAVIGATION_WIDTH, None, false);
    pages.connect_width_changed(|_| {});
    pages.toggle.emit_clicked();
    let (minimum, _, _, _) = pages.root.measure(gtk::Orientation::Horizontal, -1);
    minimum
}

fn register_resources() -> Result<()> {
    gio::resources_register_include!("onenote-viewer.gresource")?;
    Ok(())
}

fn smoke_quit_delay() -> Option<Duration> {
    std::env::var("ONENOTE_VIEWER_SMOKE_QUIT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
}

struct Source {
    descriptor: SourceDescriptor,
    loaded: Arc<LoadedNotebook>,
    last_location: Option<SectionLocation>,
}

#[derive(Clone)]
struct PageRow {
    source: SourceId,
    section: SectionId,
    page: PageId,
}

#[derive(Clone)]
struct ActiveLocation {
    source: SourceId,
    section: Option<SectionLocation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceSearchScope {
    AllNotebooks,
    Notebook,
    SectionGroup,
    Section,
}

impl WorkspaceSearchScope {
    const fn label(self) -> &'static str {
        match self {
            Self::AllNotebooks => "All notebooks",
            Self::Notebook => "This notebook",
            Self::SectionGroup => "This section group",
            Self::Section => "This section",
        }
    }

    const fn status_label(self) -> &'static str {
        match self {
            Self::AllNotebooks => "all open notebooks",
            Self::Notebook => "this notebook",
            Self::SectionGroup => "this section group",
            Self::Section => "this section",
        }
    }
}

#[derive(Clone)]
struct WorkspaceSearchControls {
    button: gtk::MenuButton,
    all_notebooks: gtk::CheckButton,
    notebook: gtk::CheckButton,
    section_group: gtk::CheckButton,
    section: gtk::CheckButton,
}

impl WorkspaceSearchControls {
    fn choice(&self, scope: WorkspaceSearchScope) -> &gtk::CheckButton {
        match scope {
            WorkspaceSearchScope::AllNotebooks => &self.all_notebooks,
            WorkspaceSearchScope::Notebook => &self.notebook,
            WorkspaceSearchScope::SectionGroup => &self.section_group,
            WorkspaceSearchScope::Section => &self.section,
        }
    }
}

#[derive(Clone)]
struct SearchResultRow {
    title: String,
    path: String,
    snippet: String,
    highlights: Vec<TextRange>,
    matched_field: MatchedField,
}

#[derive(Default)]
struct PageFindState {
    title_matches: Vec<FindTextRange>,
    page_matches: Vec<FindMatch>,
    active: Option<usize>,
}

impl PageFindState {
    fn len(&self) -> usize {
        self.title_matches.len() + self.page_matches.len()
    }
}

#[derive(Clone)]
struct RevealTarget {
    object_id: Option<ObjectId>,
    bounds: Rect,
}

#[derive(Clone)]
struct AttachmentContext {
    resource: ResourceRef,
    loaded: Arc<LoadedNotebook>,
    source_id: SourceId,
    fingerprint: onenote_core::SourceFingerprint,
}

enum ForegroundOperationKind {
    PackageImport(Arc<AtomicBool>),
    Attachment(crate::attachment::CopyCancellation),
    BackupLoad(Rc<BackupOperationCancellation>),
}

struct BackupOperationCancellation {
    source_path: PathBuf,
    load: BackupLoadControl,
    index: RefCell<Option<Arc<AtomicBool>>>,
}

impl BackupOperationCancellation {
    fn new(source_path: PathBuf) -> Self {
        Self {
            source_path,
            load: BackupLoadControl::new(),
            index: RefCell::new(None),
        }
    }

    fn cancel(&self) {
        self.load.cancel();
        if let Some(index) = self.index.borrow().as_ref() {
            index.store(true, Ordering::Release);
        }
    }

    fn is_cancelled(&self) -> bool {
        self.load.is_cancelled()
            || self
                .index
                .borrow()
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Acquire))
    }

    fn set_index_cancel(&self, cancel: Arc<AtomicBool>) {
        if self.load.is_cancelled() {
            cancel.store(true, Ordering::Release);
        }
        *self.index.borrow_mut() = Some(cancel);
    }
}

struct ForegroundOperation {
    id: u64,
    kind: ForegroundOperationKind,
}

impl ForegroundOperation {
    fn cancel(&self) {
        match &self.kind {
            ForegroundOperationKind::PackageImport(cancel) => {
                cancel.store(true, Ordering::Release);
            }
            ForegroundOperationKind::Attachment(cancel) => cancel.cancel(),
            ForegroundOperationKind::BackupLoad(cancel) => cancel.cancel(),
        }
    }

    fn is_cancelled(&self) -> bool {
        match &self.kind {
            ForegroundOperationKind::PackageImport(cancel) => cancel.load(Ordering::Acquire),
            ForegroundOperationKind::Attachment(cancel) => cancel.is_cancelled(),
            ForegroundOperationKind::BackupLoad(cancel) => cancel.is_cancelled(),
        }
    }
}

#[derive(Default)]
struct State {
    sources: Vec<Source>,
    source_order: SourceDisplayOrder,
    pages: Vec<PageRow>,
    search_hits: Vec<SearchHit>,
    active: Option<ActiveLocation>,
    search_generation: u64,
    scene_generation: u64,
    pending_reveal: Option<RevealTarget>,
    restore_target: Option<PersistedPageLocation>,
    pending_expansions: BTreeMap<SourceId, PersistedSourceTreeState>,
    history: NavigationHistory,
    configured_sources: Vec<SourceDescriptor>,
    pending_publications: BTreeMap<(SourceId, u64), PendingPublication>,
}

struct PendingPublication {
    descriptor: SourceDescriptor,
    load: worker::SourceLoad,
    operation_id: Option<u64>,
}

#[derive(Default)]
struct SourceDisplayOrder {
    paths: Vec<PathBuf>,
}

impl SourceDisplayOrder {
    fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut order = Self::default();
        order.register(paths);
        order
    }

    fn register(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if !self
                .paths
                .iter()
                .any(|known| same_source_path(known, &path))
            {
                self.paths.push(path);
            }
        }
    }

    fn remove(&mut self, path: &std::path::Path) {
        self.paths.retain(|known| !same_source_path(known, path));
    }

    fn rank(&self, path: &std::path::Path) -> Option<usize> {
        self.paths
            .iter()
            .position(|known| same_source_path(known, path))
    }

    fn insertion_index<'a>(
        &self,
        path: &std::path::Path,
        loaded: impl IntoIterator<Item = &'a std::path::Path>,
    ) -> usize {
        let rank = self.rank(path).unwrap_or(usize::MAX);
        let mut position = 0;
        for existing in loaded {
            if self.rank(existing).unwrap_or(usize::MAX) > rank {
                break;
            }
            position += 1;
        }
        position
    }
}

impl State {
    fn upsert_source(
        &mut self,
        descriptor: SourceDescriptor,
        loaded: Arc<LoadedNotebook>,
        restored_location: Option<SectionLocation>,
    ) -> usize {
        let source_id = &loaded.notebook.source_id;
        if let Some(position) = self
            .sources
            .iter()
            .position(|source| source.loaded.notebook.source_id == *source_id)
        {
            let existing = &mut self.sources[position];
            existing.descriptor = descriptor;
            existing.loaded = loaded;
            if restored_location.is_some() {
                existing.last_location = restored_location;
            }
            return position;
        }

        let path = descriptor.path().to_path_buf();
        self.source_order.register(std::iter::once(path.clone()));
        let position = self.source_order.insertion_index(
            &path,
            self.sources.iter().map(|source| source.descriptor.path()),
        );
        self.sources.insert(
            position,
            Source {
                descriptor,
                loaded,
                last_location: restored_location,
            },
        );
        position
    }
}

struct PendingIndexJob {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

struct IndexActivity {
    next_generation: u64,
    jobs: BTreeMap<SourceId, PendingIndexJob>,
    reused: usize,
    rebuilt: usize,
    failed: usize,
}

impl Default for IndexActivity {
    fn default() -> Self {
        Self {
            next_generation: 1,
            jobs: BTreeMap::new(),
            reused: 0,
            rebuilt: 0,
            failed: 0,
        }
    }
}

impl IndexActivity {
    fn begin(&mut self, source_id: SourceId) -> (u64, Arc<AtomicBool>) {
        let generation = self.next_generation;
        self.next_generation = generation.wrapping_add(1).max(1);
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self.jobs.insert(
            source_id,
            PendingIndexJob {
                generation,
                cancel: Arc::clone(&cancel),
            },
        ) {
            previous.cancel.store(true, Ordering::Release);
        }
        (generation, cancel)
    }

    fn take_current(&mut self, source_id: &SourceId, generation: u64) -> bool {
        let current = self
            .jobs
            .get(source_id)
            .is_some_and(|job| job.generation == generation);
        if current {
            self.jobs.remove(source_id);
        }
        current
    }

    fn cancel_source(&mut self, source_id: &SourceId) {
        if let Some(job) = self.jobs.remove(source_id) {
            job.cancel.store(true, Ordering::Release);
        }
    }

    fn cancel_all(&mut self) {
        for (_, job) in std::mem::take(&mut self.jobs) {
            job.cancel.store(true, Ordering::Release);
        }
    }

    fn status(&self) -> String {
        format!(
            "Search index: {} reused, {} rebuilt, {} failed, {} pending",
            self.reused,
            self.rebuilt,
            self.failed,
            self.jobs.len()
        )
    }
}

#[derive(Clone)]
struct WorkspaceRestore {
    page: Option<PersistedPageLocation>,
    ui: Option<WorkspaceUiState>,
    configured_sources: Vec<SourceDescriptor>,
}

fn prepare_workspace_restore(
    persisted: WorkspaceConfig,
    requested_sources: &[PathBuf],
    notebooks_location: &std::path::Path,
) -> (WorkspaceRestore, Vec<SourceDescriptor>, bool) {
    let configured_sources = if requested_sources.is_empty() {
        persisted.sources
    } else {
        requested_sources
            .iter()
            .cloned()
            .map(SourceDescriptor::native)
            .collect()
    };
    let page = persisted.navigation.last_page.filter(|location| {
        workspace::source_is_in_workspace(
            &location.source_path,
            &configured_sources,
            notebooks_location,
        )
    });
    let ui = persisted.ui.map(|mut ui| {
        ui.sources.retain(|source| {
            workspace::source_is_in_workspace(
                &source.source_path,
                &configured_sources,
                notebooks_location,
            )
        });
        ui
    });
    let mut initial_sources = configured_sources
        .iter()
        .filter(|source| !workspace::source_is_in_location(source.path(), notebooks_location))
        .cloned()
        .collect();
    prioritize_source_roots(&mut initial_sources, page.as_ref());
    let outside_default = page.as_ref().is_some_and(|target| {
        !workspace::source_is_in_location(&target.source_path, notebooks_location)
    });
    let restore = WorkspaceRestore {
        page,
        ui,
        configured_sources: requested_sources
            .is_empty()
            .then_some(configured_sources)
            .unwrap_or_default(),
    };
    (restore, initial_sources, outside_default)
}

#[derive(Clone, Copy)]
enum HistoryUpdate {
    Record,
    Replace,
    Preserve,
}

struct Viewer {
    window: gtk::ApplicationWindow,
    notebook_tree: NotebookTree,
    page_model: gtk::StringList,
    page_selection: gtk::SingleSelection,
    result_model: gio::ListStore,
    result_selection: gtk::SingleSelection,
    navigation_stack: gtk::Stack,
    content_paned: gtk::Paned,
    page_navigation_width: Rc<Cell<i32>>,
    notebooks_collapsed: Rc<Cell<bool>>,
    pages_collapsed: Rc<Cell<bool>>,
    page_view: PageView,
    canvas_stack: gtk::Stack,
    page_title: gtk::Label,
    page_date: gtk::Label,
    page_context: gtk::Label,
    search_entry: gtk::Entry,
    search_scope: WorkspaceSearchControls,
    page_find_revealer: gtk::Revealer,
    page_find_entry: gtk::SearchEntry,
    page_find_count: gtk::Label,
    page_find_previous: gtk::Button,
    page_find_next: gtk::Button,
    page_find_case: gtk::CheckButton,
    page_find_words: gtk::CheckButton,
    page_find_diacritics: gtk::CheckButton,
    page_find_highlight_all: gtk::CheckButton,
    status: gtk::Label,
    spinner: gtk::Spinner,
    zoom_label: gtk::Label,
    operation_activity: gtk::Revealer,
    operation_activity_title: gtk::Label,
    operation_activity_phase: gtk::Label,
    operation_progress: gtk::ProgressBar,
    operation_cancel_button: gtk::Button,
    import_package_action: gio::SimpleAction,
    refresh_source_action: gio::SimpleAction,
    show_backup_copies_action: gio::SimpleAction,
    page_find_action: gio::SimpleAction,
    history_back_action: gio::SimpleAction,
    history_forward_action: gio::SimpleAction,
    foreground_operation: RefCell<Option<ForegroundOperation>>,
    next_operation_id: Cell<u64>,
    operation_pulsing: Cell<bool>,
    scene_cancel: RefCell<Option<Arc<AtomicBool>>>,
    selection_syncing: Cell<bool>,
    state: RefCell<State>,
    workspace_path: PathBuf,
    index_path: PathBuf,
    settings_path: PathBuf,
    settings: RefCell<AppSettings>,
    style_provider: gtk::CssProvider,
    source_commands: mpsc::Sender<SourceCommand>,
    index_commands: mpsc::Sender<IndexCommand>,
    index_activity: RefCell<IndexActivity>,
    events: mpsc::Sender<Event>,
    receiver: RefCell<mpsc::Receiver<Event>>,
    search_timer: RefCell<Option<glib::SourceId>>,
    page_find_timer: RefCell<Option<glib::SourceId>>,
    page_find_state: RefCell<PageFindState>,
    workspace_search_scope: Cell<WorkspaceSearchScope>,
    workspace_save_timer: RefCell<Option<glib::SourceId>>,
    settings_save_timer: RefCell<Option<glib::SourceId>>,
}

impl Viewer {
    // Keeping the static GTK hierarchy together makes parent/child ownership
    // auditable; event handling and application behavior live below.
    #[allow(clippy::too_many_lines)]
    fn new(
        application: &gtk::Application,
        workspace_path: PathBuf,
        index_path: PathBuf,
        settings_path: PathBuf,
        settings: AppSettings,
        restore: WorkspaceRestore,
        style_provider: gtk::CssProvider,
    ) -> Rc<Self> {
        let restored_panes = restore.ui.as_ref().map(|ui| ui.panes).unwrap_or_default();
        let source_order = restore
            .ui
            .as_ref()
            .map(|ui| {
                SourceDisplayOrder::new(ui.sources.iter().map(|source| source.source_path.clone()))
            })
            .unwrap_or_default();
        let pending_expansions = restore
            .ui
            .map(|ui| {
                ui.sources
                    .into_iter()
                    .map(|source| (source.source_id.clone(), source))
                    .collect()
            })
            .unwrap_or_default();
        let notebook_tree = NotebookTree::new();
        let page_model = gtk::StringList::new(&[]);
        let (page_selection, page_list) = list_view(&page_model, "page-list");
        let result_model = gio::ListStore::new::<glib::BoxedAnyObject>();
        let (result_selection, result_list) = result_list(&result_model);
        let page_view = PageView::new();
        page_view.set_zoom(settings.zoom);
        page_view
            .set_default_text_color(&theme_default_text_color(effective_theme(settings.theme)));
        let search_entry = gtk::Entry::builder()
            .placeholder_text("Search all notebooks")
            .hexpand(true)
            .width_request(320)
            .build();
        search_entry.set_secondary_icon_activatable(true);
        search_entry.set_secondary_icon_tooltip_text(Some("Clear search"));

        let search_scope_all = gtk::CheckButton::with_label("All notebooks");
        let search_scope_notebook = gtk::CheckButton::with_label("This notebook");
        let search_scope_group = gtk::CheckButton::with_label("This section group");
        let search_scope_section = gtk::CheckButton::with_label("This section");
        search_scope_notebook.set_group(Some(&search_scope_all));
        search_scope_group.set_group(Some(&search_scope_all));
        search_scope_section.set_group(Some(&search_scope_all));
        search_scope_all.set_active(true);
        let search_scope_choices = gtk::Box::new(gtk::Orientation::Vertical, 2);
        search_scope_choices.set_margin_start(8);
        search_scope_choices.set_margin_end(8);
        search_scope_choices.set_margin_top(8);
        search_scope_choices.set_margin_bottom(8);
        for choice in [
            &search_scope_all,
            &search_scope_notebook,
            &search_scope_group,
            &search_scope_section,
        ] {
            choice.set_halign(gtk::Align::Start);
            search_scope_choices.append(choice);
        }
        let search_scope_popover = gtk::Popover::builder().child(&search_scope_choices).build();
        let search_scope_button = gtk::MenuButton::builder()
            .icon_name("onenote-search-symbolic")
            .always_show_arrow(true)
            .popover(&search_scope_popover)
            .build();
        search_scope_button.add_css_class("search-scope");
        search_scope_button.set_tooltip_text(Some("Search scope: All notebooks"));
        let search_scope = WorkspaceSearchControls {
            button: search_scope_button,
            all_notebooks: search_scope_all,
            notebook: search_scope_notebook,
            section_group: search_scope_group,
            section: search_scope_section,
        };
        let workspace_search = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        workspace_search.add_css_class("linked");
        workspace_search.add_css_class("workspace-search");
        workspace_search.append(&search_scope.button);
        workspace_search.append(&search_entry);

        let open_file = gio::SimpleAction::new("open-file", None);
        let open_folder = gio::SimpleAction::new("open-folder", None);
        let open_backup_folder = gio::SimpleAction::new("open-backup-folder", None);
        let import_package = gio::SimpleAction::new("import-package", None);
        let refresh_source = gio::SimpleAction::new("refresh-source", None);
        refresh_source.set_enabled(false);
        let show_backup_copies =
            gio::SimpleAction::new_stateful("show-backup-copies", None, &false.to_variant());
        show_backup_copies.set_enabled(false);
        let open_settings = gio::SimpleAction::new("settings", None);
        let show_about = gio::SimpleAction::new("about", None);
        let quit = gio::SimpleAction::new("quit", None);
        let history_back = gio::SimpleAction::new("history-back", None);
        history_back.set_enabled(false);
        let history_forward = gio::SimpleAction::new("history-forward", None);
        history_forward.set_enabled(false);
        let focus_search = gio::SimpleAction::new("focus-search", None);
        let page_find = gio::SimpleAction::new("find-page", None);
        page_find.set_enabled(false);
        let page_find_next_action = gio::SimpleAction::new("find-page-next", None);
        let page_find_previous_action = gio::SimpleAction::new("find-page-previous", None);
        let close_source = icon_button("onenote-close-symbolic", "Close selected notebook");
        let spinner = gtk::Spinner::new();
        spinner.set_tooltip_text(Some("Background activity"));

        let brand = gtk::Label::new(Some("OneNote Viewer"));
        brand.add_css_class("brand");

        let file_menu = gio::Menu::new();
        file_menu.append(Some("Open OneNote File..."), Some("win.open-file"));
        file_menu.append(Some("Open Notebook Folder..."), Some("win.open-folder"));
        file_menu.append(
            Some("Open OneNote Backup Folder..."),
            Some("win.open-backup-folder"),
        );
        file_menu.append(
            Some("Import OneNote Package..."),
            Some("win.import-package"),
        );
        let application_menu = gio::Menu::new();
        application_menu.append_section(None, &file_menu);
        let source_menu = gio::Menu::new();
        source_menu.append(
            Some("Refresh Selected Notebook"),
            Some("win.refresh-source"),
        );
        source_menu.append(
            Some("Show All Backup Copies"),
            Some("win.show-backup-copies"),
        );
        application_menu.append_section(None, &source_menu);
        let navigation_menu = gio::Menu::new();
        navigation_menu.append(Some("Back"), Some("win.history-back"));
        navigation_menu.append(Some("Forward"), Some("win.history-forward"));
        navigation_menu.append(Some("Find in Page"), Some("win.find-page"));
        application_menu.append_section(None, &navigation_menu);
        let preferences_menu = gio::Menu::new();
        preferences_menu.append(Some("Settings"), Some("win.settings"));
        application_menu.append_section(None, &preferences_menu);
        let information_menu = gio::Menu::new();
        information_menu.append(Some("About OneNote Viewer"), Some("win.about"));
        application_menu.append_section(None, &information_menu);
        let quit_menu = gio::Menu::new();
        quit_menu.append(Some("Quit OneNote Viewer"), Some("win.quit"));
        application_menu.append_section(None, &quit_menu);
        let menu = gtk::MenuButton::builder()
            .icon_name("onenote-menu-symbolic")
            .menu_model(&application_menu)
            .build();
        menu.add_css_class("icon-button");
        menu.add_css_class("main-menu");
        menu.set_width_request(36);
        menu.set_margin_end(10);
        menu.set_tooltip_text(Some("Main menu"));
        let header = gtk::HeaderBar::new();
        header.set_show_title_buttons(true);
        header.pack_start(&brand);
        header.set_title_widget(Some(&workspace_search));
        header.pack_end(&menu);

        let notebooks = CollapsibleNavigationBand::new(
            "NOTEBOOKS",
            &notebook_tree.view,
            NOTEBOOK_NAVIGATION_WIDTH,
            Some(&close_source),
            restored_panes.notebooks_collapsed,
        );
        let pages = CollapsibleNavigationBand::new(
            "PAGES",
            &page_list,
            PAGE_NAVIGATION_WIDTH,
            None,
            restored_panes.pages_collapsed,
        );
        let results = navigation_band("SEARCH RESULTS", &result_list, SEARCH_RESULTS_WIDTH);

        let navigation_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hhomogeneous(false)
            .build();
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        navigation.append(&notebooks.root);
        navigation.append(&separator());
        navigation.append(&pages.root);
        navigation_stack.add_named(&navigation, Some("pages"));
        navigation_stack.add_named(&results, Some("results"));
        navigation_stack.set_visible_child_name("pages");
        let initial_navigation_width =
            notebooks.width() + pages.width() + NAVIGATION_SEPARATOR_WIDTH;
        navigation_stack.set_width_request(initial_navigation_width);

        let page_title = selectable_page_header_label("page-title");
        let page_date = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        page_date.add_css_class("page-date");
        let page_context = selectable_page_header_label("page-context");
        let title_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        title_box.set_margin_start(20);
        title_box.set_margin_end(16);
        title_box.set_margin_top(10);
        title_box.set_margin_bottom(10);
        title_box.append(&page_title);
        title_box.append(&page_date);
        title_box.append(&page_context);

        let page_find_entry = gtk::SearchEntry::builder()
            .placeholder_text("Find text on this page")
            .hexpand(true)
            .width_request(140)
            .build();
        let page_find_count = gtk::Label::new(Some("No matches"));
        page_find_count.add_css_class("page-find-count");
        let page_find_previous = icon_button(
            "onenote-find-previous-symbolic",
            "Previous match (Shift+F3)",
        );
        let page_find_next = icon_button("onenote-find-next-symbolic", "Next match (F3)");
        page_find_previous.set_sensitive(false);
        page_find_next.set_sensitive(false);
        let page_find_case = gtk::CheckButton::with_label("Case");
        page_find_case.set_tooltip_text(Some("Match case"));
        let page_find_words = gtk::CheckButton::with_label("Whole words");
        page_find_words.set_tooltip_text(Some("Match whole words"));
        let page_find_diacritics = gtk::CheckButton::with_label("Diacritics");
        page_find_diacritics.set_tooltip_text(Some("Match diacritics"));
        let page_find_highlight_all = gtk::CheckButton::with_label("Highlight");
        page_find_highlight_all.set_tooltip_text(Some("Highlight all matches"));
        page_find_highlight_all.set_active(true);
        for toggle in [
            &page_find_case,
            &page_find_words,
            &page_find_diacritics,
            &page_find_highlight_all,
        ] {
            toggle.add_css_class("page-find-option");
        }
        let close_page_find = icon_button("onenote-close-symbolic", "Close Find in Page");
        let page_find_bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        page_find_bar.set_margin_start(6);
        page_find_bar.set_margin_end(4);
        page_find_bar.set_margin_top(4);
        page_find_bar.set_margin_bottom(4);
        page_find_bar.append(&page_find_entry);
        page_find_bar.append(&page_find_count);
        page_find_bar.append(&page_find_previous);
        page_find_bar.append(&page_find_next);
        page_find_bar.append(&page_find_highlight_all);
        page_find_bar.append(&page_find_case);
        page_find_bar.append(&page_find_diacritics);
        page_find_bar.append(&page_find_words);
        page_find_bar.append(&close_page_find);
        page_find_bar.add_css_class("page-find-bar");
        let page_find_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .child(&page_find_bar)
            .build();

        let empty = gtk::Box::new(gtk::Orientation::Vertical, 12);
        empty.set_halign(gtk::Align::Center);
        empty.set_valign(gtk::Align::Center);
        let empty_icon = gtk::Image::from_icon_name("onenote-open-file-symbolic");
        empty_icon.set_pixel_size(48);
        empty_icon.add_css_class("empty-icon");
        let empty_title = gtk::Label::new(Some("Open a notebook to begin"));
        empty_title.add_css_class("empty-title");
        let empty_detail =
            gtk::Label::new(Some("Choose a native .one, .onetoc2, or notebook folder"));
        empty_detail.add_css_class("dim-label");
        empty.append(&empty_icon);
        empty.append(&empty_title);
        empty.append(&empty_detail);

        let document = gtk::Box::new(gtk::Orientation::Vertical, 0);
        document.append(&title_box);
        document.append(&separator_horizontal());
        document.append(page_view.widget());
        document.append(&page_find_revealer);
        let canvas_stack = gtk::Stack::builder()
            .hexpand(true)
            .vexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        canvas_stack.add_named(&empty, Some("empty"));
        canvas_stack.add_named(&document, Some("document"));
        canvas_stack.set_visible_child_name("empty");

        let zoom_out = icon_button("onenote-zoom-out-symbolic", "Zoom out");
        let zoom_in = icon_button("onenote-zoom-in-symbolic", "Zoom in");
        let zoom_reset = icon_button("onenote-zoom-reset-symbolic", "Reset zoom");
        let zoom_label = gtk::Label::new(Some(&format_zoom(page_view.zoom())));
        zoom_label.set_width_chars(5);
        let status = gtk::Label::builder()
            .label("Ready")
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        status.add_css_class("status");
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        footer.set_margin_start(12);
        footer.set_margin_end(8);
        footer.set_margin_top(4);
        footer.set_margin_bottom(4);
        footer.append(&status);
        footer.append(&spinner);
        footer.append(&zoom_out);
        footer.append(&zoom_label);
        footer.append(&zoom_in);
        footer.append(&zoom_reset);

        let operation_activity_title = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        operation_activity_title.add_css_class("activity-title");
        let operation_activity_phase = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        operation_activity_phase.add_css_class("activity-phase");
        let activity_labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        activity_labels.set_hexpand(true);
        activity_labels.append(&operation_activity_title);
        activity_labels.append(&operation_activity_phase);
        let operation_progress = gtk::ProgressBar::builder()
            .width_request(240)
            .valign(gtk::Align::Center)
            .build();
        operation_progress.set_pulse_step(0.025);
        let operation_cancel_button = gtk::Button::with_label("Cancel");
        let activity_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        activity_content.set_margin_start(16);
        activity_content.set_margin_end(16);
        activity_content.set_margin_top(10);
        activity_content.set_margin_bottom(10);
        activity_content.append(&activity_labels);
        activity_content.append(&operation_progress);
        activity_content.append(&operation_cancel_button);
        activity_content.add_css_class("operation-activity");
        let operation_activity = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&activity_content)
            .build();

        let content_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&navigation_stack)
            .end_child(&canvas_stack)
            .resize_start_child(false)
            .shrink_start_child(true)
            .position(initial_navigation_width)
            .build();
        let notebook_width = Rc::new(Cell::new(notebooks.width()));
        let page_width = Rc::new(Cell::new(pages.width()));
        let page_navigation_width = Rc::new(Cell::new(initial_navigation_width));
        connect_navigation_band_width(
            &notebooks,
            Rc::clone(&notebook_width),
            Rc::clone(&page_width),
            Rc::clone(&page_navigation_width),
            &navigation_stack,
            &content_paned,
        );
        connect_navigation_band_width(
            &pages,
            Rc::clone(&page_width),
            Rc::clone(&notebook_width),
            Rc::clone(&page_navigation_width),
            &navigation_stack,
            &content_paned,
        );

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&operation_activity);
        root.append(&content_paned);
        root.append(&separator_horizontal());
        root.append(&footer);
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("OneNote Viewer")
            .icon_name(APP_ICON_NAME)
            .default_width(1_500)
            .default_height(920)
            .titlebar(&header)
            .child(&root)
            .build();

        let (event_sender, event_receiver) = mpsc::channel();
        let source_commands = worker::start_source_worker(event_sender.clone());
        let index_commands = worker::start_index_worker(index_path.clone(), event_sender.clone());
        let viewer = Rc::new(Self {
            window,
            notebook_tree,
            page_model,
            page_selection,
            result_model,
            result_selection,
            navigation_stack,
            content_paned,
            page_navigation_width,
            notebooks_collapsed: Rc::clone(&notebooks.collapsed),
            pages_collapsed: Rc::clone(&pages.collapsed),
            page_view,
            canvas_stack,
            page_title,
            page_date,
            page_context,
            search_entry,
            search_scope,
            page_find_revealer,
            page_find_entry,
            page_find_count,
            page_find_previous,
            page_find_next,
            page_find_case,
            page_find_words,
            page_find_diacritics,
            page_find_highlight_all,
            status,
            spinner,
            zoom_label,
            operation_activity,
            operation_activity_title,
            operation_activity_phase,
            operation_progress,
            operation_cancel_button,
            import_package_action: import_package.clone(),
            refresh_source_action: refresh_source.clone(),
            show_backup_copies_action: show_backup_copies.clone(),
            page_find_action: page_find.clone(),
            history_back_action: history_back.clone(),
            history_forward_action: history_forward.clone(),
            foreground_operation: RefCell::default(),
            next_operation_id: Cell::new(1),
            operation_pulsing: Cell::new(false),
            scene_cancel: RefCell::default(),
            selection_syncing: Cell::new(false),
            state: RefCell::new(State {
                restore_target: restore.page,
                pending_expansions,
                source_order,
                configured_sources: restore.configured_sources,
                ..State::default()
            }),
            workspace_path,
            index_path,
            settings_path,
            settings: RefCell::new(settings),
            style_provider,
            source_commands,
            index_commands,
            index_activity: RefCell::default(),
            events: event_sender,
            receiver: RefCell::new(event_receiver),
            search_timer: RefCell::default(),
            page_find_timer: RefCell::default(),
            page_find_state: RefCell::default(),
            workspace_search_scope: Cell::new(WorkspaceSearchScope::AllNotebooks),
            workspace_save_timer: RefCell::default(),
            settings_save_timer: RefCell::default(),
        });
        viewer.connect_navigation();
        let weak = Rc::downgrade(&viewer);
        viewer.notebook_tree.connect_expansion_changed(move || {
            if let Some(viewer) = weak.upgrade() {
                viewer.schedule_workspace_save();
            }
        });
        for toggle in [&notebooks.toggle, &pages.toggle] {
            let weak = Rc::downgrade(&viewer);
            toggle.connect_clicked(move |_| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.schedule_workspace_save();
                }
            });
        }
        viewer.window.add_action(&open_file);
        viewer.window.add_action(&open_folder);
        viewer.window.add_action(&open_backup_folder);
        viewer.window.add_action(&import_package);
        viewer.window.add_action(&refresh_source);
        viewer.window.add_action(&show_backup_copies);
        viewer.window.add_action(&open_settings);
        viewer.window.add_action(&show_about);
        viewer.window.add_action(&quit);
        viewer.window.add_action(&history_back);
        viewer.window.add_action(&history_forward);
        viewer.window.add_action(&focus_search);
        viewer.window.add_action(&page_find);
        viewer.window.add_action(&page_find_next_action);
        viewer.window.add_action(&page_find_previous_action);
        application.set_accels_for_action("win.open-file", &["<Primary>o"]);
        application.set_accels_for_action("win.open-folder", &["<Primary><Shift>o"]);
        application.set_accels_for_action("win.import-package", &["<Primary><Shift>i"]);
        application.set_accels_for_action("win.settings", &["<Primary>comma"]);
        application.set_accels_for_action("win.quit", &["<Primary>q"]);
        application.set_accels_for_action("win.history-back", &["<Alt>Left", "Back"]);
        application.set_accels_for_action("win.history-forward", &["<Alt>Right", "Forward"]);
        application.set_accels_for_action("win.focus-search", &["<Primary>e"]);
        application.set_accels_for_action("win.find-page", &["<Primary>f"]);
        application.set_accels_for_action("win.find-page-next", &["F3"]);
        application.set_accels_for_action("win.find-page-previous", &["<Shift>F3"]);
        viewer.connect_header(
            &open_file,
            &open_folder,
            &import_package,
            &open_settings,
            &quit,
            &close_source,
        );
        viewer.connect_source_actions(&open_backup_folder, &refresh_source, &show_backup_copies);
        viewer.connect_about(&show_about);
        viewer.connect_history();
        viewer.connect_workspace_search(&focus_search);
        viewer.connect_page_find(
            &page_find,
            &page_find_next_action,
            &page_find_previous_action,
            &close_page_find,
        );
        viewer.connect_system_theme();
        viewer.connect_operation_activity();
        viewer.connect_zoom(&zoom_out, &zoom_in, &zoom_reset);
        viewer.connect_page_actions();
        viewer.poll_events();
        viewer
    }

    fn connect_navigation(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.notebook_tree
            .selection
            .connect_selected_notify(move |_| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.notebook_selection_changed();
                }
            });
        let weak = Rc::downgrade(self);
        self.page_selection
            .connect_selected_notify(move |selection| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.page_selection_changed(selection.selected());
                }
            });
        let weak = Rc::downgrade(self);
        self.result_selection
            .connect_selected_notify(move |selection| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.result_selection_changed(selection.selected());
                }
            });

        let weak = Rc::downgrade(self);
        self.search_entry.connect_changed(move |entry| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            entry.set_secondary_icon_name(
                (!entry.text().is_empty()).then_some("onenote-close-symbolic"),
            );
            viewer.workspace_search_text_changed();
        });
        self.search_entry.connect_icon_press(|entry, position| {
            if position == gtk::EntryIconPosition::Secondary {
                entry.set_text("");
            }
        });

        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(viewer) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Down => viewer.move_search_result(1),
                gtk::gdk::Key::Up => viewer.move_search_result(-1),
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => viewer.commit_search_result(),
                gtk::gdk::Key::Escape => viewer.dismiss_workspace_search(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.search_entry.add_controller(keys);
    }

    fn connect_workspace_search(self: &Rc<Self>, focus: &gio::SimpleAction) {
        let weak = Rc::downgrade(self);
        focus.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.search_entry.grab_focus();
                viewer.search_entry.select_region(0, -1);
            }
        });
        for (choice, scope) in [
            (
                &self.search_scope.all_notebooks,
                WorkspaceSearchScope::AllNotebooks,
            ),
            (&self.search_scope.notebook, WorkspaceSearchScope::Notebook),
            (
                &self.search_scope.section_group,
                WorkspaceSearchScope::SectionGroup,
            ),
            (&self.search_scope.section, WorkspaceSearchScope::Section),
        ] {
            let weak = Rc::downgrade(self);
            choice.connect_toggled(move |choice| {
                if choice.is_active() {
                    let Some(viewer) = weak.upgrade() else {
                        return;
                    };
                    viewer.set_workspace_search_scope(scope);
                }
            });
        }
        self.refresh_workspace_search_scope();
    }

    fn connect_page_find(
        self: &Rc<Self>,
        find: &gio::SimpleAction,
        next: &gio::SimpleAction,
        previous: &gio::SimpleAction,
        close: &gtk::Button,
    ) {
        let weak = Rc::downgrade(self);
        find.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.show_page_find();
            }
        });
        let weak = Rc::downgrade(self);
        next.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_page_find(1);
            }
        });
        let weak = Rc::downgrade(self);
        previous.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_page_find(-1);
            }
        });
        let weak = Rc::downgrade(self);
        self.page_find_previous.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_page_find(-1);
            }
        });
        let weak = Rc::downgrade(self);
        self.page_find_next.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_page_find(1);
            }
        });
        let weak = Rc::downgrade(self);
        close.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.close_page_find();
            }
        });
        let weak = Rc::downgrade(self);
        self.page_find_entry.connect_search_changed(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.schedule_page_find();
            }
        });
        let weak = Rc::downgrade(self);
        self.page_find_entry.connect_stop_search(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.close_page_find();
            }
        });
        for toggle in [
            &self.page_find_case,
            &self.page_find_words,
            &self.page_find_diacritics,
        ] {
            let weak = Rc::downgrade(self);
            toggle.connect_toggled(move |_| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.schedule_page_find();
                }
            });
        }
        let weak = Rc::downgrade(self);
        self.page_find_highlight_all.connect_toggled(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.apply_page_find_state(true);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(viewer) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => viewer.navigate_page_find(
                    if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                        -1
                    } else {
                        1
                    },
                ),
                gtk::gdk::Key::Escape => viewer.close_page_find(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.page_find_entry.add_controller(keys);
        self.refresh_page_find_action();
    }

    fn connect_history(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.history_back_action.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_history(HistoryDirection::Back);
            }
        });
        let weak = Rc::downgrade(self);
        self.history_forward_action.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.navigate_history(HistoryDirection::Forward);
            }
        });

        let weak = Rc::downgrade(self);
        let mouse_history = history_mouse_controller(move |direction| {
            if let Some(viewer) = weak.upgrade() {
                match direction {
                    HistoryDirection::Back => viewer.history_back_action.activate(None),
                    HistoryDirection::Forward => viewer.history_forward_action.activate(None),
                }
            }
        });
        self.window.add_controller(mouse_history);
    }

    fn connect_header(
        self: &Rc<Self>,
        open_file: &gio::SimpleAction,
        open_folder: &gio::SimpleAction,
        import_package: &gio::SimpleAction,
        open_settings: &gio::SimpleAction,
        quit: &gio::SimpleAction,
        close_source: &gtk::Button,
    ) {
        let weak = Rc::downgrade(self);
        open_file.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_file();
            }
        });
        let weak = Rc::downgrade(self);
        open_folder.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_folder();
            }
        });
        let weak = Rc::downgrade(self);
        import_package.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_package();
            }
        });
        let weak = Rc::downgrade(self);
        open_settings.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.show_settings();
            }
        });
        let weak = Rc::downgrade(self);
        quit.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.window.application().expect("application").quit();
            }
        });
        let weak = Rc::downgrade(self);
        close_source.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.close_active_source();
            }
        });
    }

    fn connect_source_actions(
        self: &Rc<Self>,
        open_backup_folder: &gio::SimpleAction,
        refresh_source: &gio::SimpleAction,
        show_backup_copies: &gio::SimpleAction,
    ) {
        let weak = Rc::downgrade(self);
        open_backup_folder.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_backup_folder();
            }
        });
        let weak = Rc::downgrade(self);
        refresh_source.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                viewer.refresh_active_source();
            }
        });
        let weak = Rc::downgrade(self);
        show_backup_copies.connect_activate(move |action, _| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            let enabled = action
                .state()
                .and_then(|state| state.get::<bool>())
                .unwrap_or(false);
            viewer.set_active_backup_policy(if enabled {
                BackupSelectionPolicy::LatestPerSection
            } else {
                BackupSelectionPolicy::AllCopies
            });
        });
    }

    fn connect_about(self: &Rc<Self>, show_about: &gio::SimpleAction) {
        let weak = Rc::downgrade(self);
        show_about.connect_activate(move |_, _| {
            if let Some(viewer) = weak.upgrade() {
                crate::dialogs::present_about(&viewer.window);
            }
        });
    }

    fn connect_system_theme(self: &Rc<Self>) {
        let Some(settings) = gtk::Settings::default() else {
            return;
        };
        let weak = Rc::downgrade(self);
        settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            if viewer.settings.borrow().theme == ThemePreference::System {
                viewer.apply_theme(ThemePreference::System);
            }
        });
    }

    fn connect_operation_activity(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.operation_cancel_button.connect_clicked(move |button| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            let operation = viewer.foreground_operation.borrow();
            let Some(operation) = operation.as_ref() else {
                return;
            };
            operation.cancel();
            button.set_sensitive(false);
            viewer
                .operation_activity_phase
                .set_label("Cancelling operation...");
        });
    }

    fn connect_zoom(
        self: &Rc<Self>,
        zoom_out: &gtk::Button,
        zoom_in: &gtk::Button,
        zoom_reset: &gtk::Button,
    ) {
        let weak = Rc::downgrade(self);
        self.page_view.connect_zoom_changed(move |zoom| {
            if let Some(viewer) = weak.upgrade() {
                viewer.zoom_changed(zoom);
            }
        });
        let weak = Rc::downgrade(self);
        zoom_out.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.page_view.set_zoom(viewer.page_view.zoom() / 1.1);
            }
        });
        let weak = Rc::downgrade(self);
        zoom_in.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.page_view.set_zoom(viewer.page_view.zoom() * 1.1);
            }
        });
        let weak = Rc::downgrade(self);
        zoom_reset.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.page_view.set_zoom(DEFAULT_ZOOM);
            }
        });
    }

    fn connect_page_actions(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.page_view.set_action_handler(Some(move |action| {
            if let Some(viewer) = weak.upgrade() {
                viewer.handle_page_action(action);
            }
        }));
    }

    fn handle_page_action(self: &Rc<Self>, action: HitAction) {
        match action {
            HitAction::OpenLink(target) => self.open_link(&target),
            HitAction::OpenAttachment(resource_id) => self.show_attachment(&resource_id),
            HitAction::SelectObject(_) => {}
        }
    }

    fn show_attachment(self: &Rc<Self>, resource_id: &ResourceId) {
        let Some(context) = self.attachment_context(resource_id) else {
            self.show_error(
                "Could not access attachment",
                "The selected attachment no longer belongs to the displayed page.",
            );
            return;
        };
        let open_context = context.clone();
        let save_context = context.clone();
        let weak_open = Rc::downgrade(self);
        let weak_save = weak_open.clone();
        crate::dialogs::present_attachment(
            &self.window,
            &context.resource,
            move || {
                if let Some(viewer) = weak_open.upgrade() {
                    viewer.open_attachment(open_context.clone());
                }
            },
            move || {
                if let Some(viewer) = weak_save.upgrade() {
                    viewer.choose_attachment_destination(save_context.clone());
                }
            },
        );
    }

    fn attachment_context(&self, resource_id: &ResourceId) -> Option<AttachmentContext> {
        let scene = self.page_view.canvas().scene()?;
        let resource = scene.nodes.iter().find_map(|node| match &node.primitive {
            ScenePrimitive::Attachment(attachment) if attachment.resource.id == *resource_id => {
                Some(attachment.resource.clone())
            }
            _ => None,
        })?;
        let active_source = self.state.borrow().active.as_ref()?.source.clone();
        let state = self.state.borrow();
        let source = state
            .sources
            .iter()
            .find(|source| source.loaded.notebook.source_id == active_source)?;
        Some(AttachmentContext {
            resource,
            loaded: source.loaded.clone(),
            source_id: source.loaded.notebook.source_id.clone(),
            fingerprint: source.loaded.notebook.fingerprint.clone(),
        })
    }

    fn choose_attachment_destination(self: &Rc<Self>, context: AttachmentContext) {
        let initial_name = crate::attachment::sanitized_filename(&context.resource.name);
        let dialog = gtk::FileDialog::builder()
            .title("Save Attachment")
            .accept_label("Save")
            .initial_name(initial_name)
            .modal(true)
            .build();
        let weak = Rc::downgrade(self);
        dialog.save(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| match result {
                Ok(destination) => {
                    if let Some(viewer) = weak.upgrade() {
                        viewer.start_attachment_copy(
                            context,
                            destination,
                            worker::AttachmentPurpose::Save,
                        );
                    }
                }
                Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                Err(error) => {
                    if let Some(viewer) = weak.upgrade() {
                        viewer.show_error(
                            "Could not choose attachment destination",
                            &error.to_string(),
                        );
                    }
                }
            },
        );
    }

    fn open_attachment(self: &Rc<Self>, context: AttachmentContext) {
        let destination = match crate::attachment::cache_file(
            &context.source_id,
            &context.fingerprint,
            &context.resource.id,
            &context.resource.name,
        ) {
            Ok(destination) => destination,
            Err(error) => {
                self.show_error("Could not prepare attachment cache", &error.to_string());
                return;
            }
        };
        self.start_attachment_copy(context, destination, worker::AttachmentPurpose::Open);
    }

    fn start_attachment_copy(
        self: &Rc<Self>,
        context: AttachmentContext,
        destination: gio::File,
        purpose: worker::AttachmentPurpose,
    ) {
        if self.foreground_operation.borrow().is_some() {
            self.status
                .set_label("Another file operation is already running");
            return;
        }
        let operation_id = self.allocate_operation_id();
        let cancellation = crate::attachment::CopyCancellation::new();
        *self.foreground_operation.borrow_mut() = Some(ForegroundOperation {
            id: operation_id,
            kind: ForegroundOperationKind::Attachment(cancellation.clone()),
        });
        self.operation_pulsing.set(context.resource.size == 0);
        self.import_package_action.set_enabled(false);
        self.operation_cancel_button.set_sensitive(true);
        self.operation_progress.set_fraction(0.0);
        self.operation_activity_title.set_label(&format!(
            "{} {}",
            if purpose == worker::AttachmentPurpose::Open {
                "Preparing"
            } else {
                "Saving"
            },
            gtk_text(&context.resource.name)
        ));
        self.operation_activity_phase
            .set_label("Copying attachment safely...");
        self.operation_activity.set_reveal_child(true);
        self.set_busy("Copying attachment");
        worker::copy_attachment(
            operation_id,
            purpose,
            crate::attachment::CopyRequest {
                loaded: context.loaded,
                resource_id: context.resource.id,
                destination,
                cancellation,
            },
            self.events.clone(),
        );
    }

    fn launch_attachment(self: &Rc<Self>, file: &gio::File) {
        let launcher = gtk::FileLauncher::new(Some(file));
        launcher.set_writable(false);
        let weak = Rc::downgrade(self);
        launcher.launch(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                if let (Err(error), Some(viewer)) = (result, weak.upgrade()) {
                    viewer.show_error("Could not open attachment", &error.to_string());
                }
            },
        );
    }

    fn open_link(self: &Rc<Self>, target: &str) {
        let target = target.trim();
        if target.is_empty() || target.contains('\0') {
            self.show_error(
                "Could not open link",
                "The note contains an invalid empty link.",
            );
            return;
        }
        match uri_scheme(target).as_deref() {
            Some("onenote") => self.open_internal_onenote_link(target),
            Some("http" | "https" | "mailto" | "ftp" | "tel" | "sms") => {
                self.launch_uri(target);
            }
            Some(_) | None if is_local_link(target) => {
                self.confirm_link(target, "Open local file link?");
            }
            Some(_) => self.confirm_link(target, "Open external link?"),
            None => self.show_error(
                "Could not open link",
                &format!("The link has no recognized URI scheme:\n\n{target}"),
            ),
        }
    }

    fn open_internal_onenote_link(self: &Rc<Self>, target: &str) {
        let Some(native_page_id) = onenote_page_id(target) else {
            self.show_error(
                "Could not open OneNote link",
                &format!("The link does not contain a page target:\n\n{target}"),
            );
            return;
        };
        let destination = {
            let state = self.state.borrow();
            let active = state.active.as_ref().map(|active| &active.source);
            state
                .sources
                .iter()
                .filter(|source| active == Some(&source.loaded.notebook.source_id))
                .chain(
                    state
                        .sources
                        .iter()
                        .filter(|source| active != Some(&source.loaded.notebook.source_id)),
                )
                .find_map(|source| {
                    source.loaded.notebook.sections().find_map(|section| {
                        section
                            .pages
                            .iter()
                            .find(|page| native_ids_equal(&page.native_id, &native_page_id))
                            .map(|page| {
                                (
                                    source.loaded.notebook.source_id.clone(),
                                    SectionLocation {
                                        section_id: section.id.clone(),
                                        page_id: Some(page.id.clone()),
                                    },
                                )
                            })
                    })
                })
        };
        if let Some((source_id, location)) = destination {
            self.cancel_pending_restore();
            self.activate_location(&source_id, &location, None, HistoryUpdate::Record);
        } else {
            self.show_error(
                "OneNote page is not available",
                &format!(
                    "The linked page is not present in any open notebook.\n\nPage ID: {native_page_id}"
                ),
            );
        }
    }

    fn confirm_link(self: &Rc<Self>, target: &str, title: &str) {
        let target = target.to_owned();
        let dialog = gtk::Window::builder()
            .title(title)
            .transient_for(&self.window)
            .modal(true)
            .resizable(false)
            .default_width(620)
            .build();
        dialog.add_css_class("link-dialog");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        let explanation = gtk::Label::builder()
            .label("This note requests opening a link with another application.")
            .wrap(true)
            .xalign(0.0)
            .selectable(true)
            .build();
        content.append(&explanation);
        let destination = gtk::Label::builder()
            .label(&target)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Char)
            .xalign(0.0)
            .selectable(true)
            .build();
        destination.add_css_class("link-destination");
        content.append(&destination);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let open = gtk::Button::with_label("Open Link");
        open.add_css_class("suggested-action");
        actions.append(&cancel);
        actions.append(&open);
        content.append(&actions);
        let close_dialog = dialog.clone();
        cancel.connect_clicked(move |_| close_dialog.close());
        let close_dialog = dialog.clone();
        let weak = Rc::downgrade(self);
        open.connect_clicked(move |_| {
            close_dialog.close();
            if let Some(viewer) = weak.upgrade() {
                viewer.launch_link_target(&target);
            }
        });
        dialog.set_child(Some(&content));
        cancel.grab_focus();
        dialog.present();
    }

    fn launch_link_target(self: &Rc<Self>, target: &str) {
        if is_plain_local_path(target) {
            self.launch_local_path(target);
        } else {
            self.launch_uri(target);
        }
    }

    fn launch_local_path(self: &Rc<Self>, target: &str) {
        let file = gio::File::for_path(target);
        let launcher = gtk::FileLauncher::new(Some(&file));
        let weak = Rc::downgrade(self);
        launcher.launch(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                if let (Err(error), Some(viewer)) = (result, weak.upgrade()) {
                    viewer.show_error("Could not open local file", &error.to_string());
                }
            },
        );
    }

    fn launch_uri(self: &Rc<Self>, target: &str) {
        let launcher = gtk::UriLauncher::new(target);
        let weak = Rc::downgrade(self);
        launcher.launch(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                if let (Err(error), Some(viewer)) = (result, weak.upgrade()) {
                    viewer.show_error("Could not open link", &error.to_string());
                }
            },
        );
    }

    fn poll_events(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let Some(viewer) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let events: Vec<_> = viewer.receiver.borrow().try_iter().collect();
            for event in events {
                viewer.handle_event(event);
            }
            if viewer.operation_activity.reveals_child()
                && viewer.foreground_operation.borrow().is_some()
                && viewer.operation_pulsing.get()
            {
                viewer.operation_progress.pulse();
            }
            glib::ControlFlow::Continue
        });
    }
    fn handle_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Discovered { requested, result } => {
                self.handle_discovered(&requested, result);
            }
            Event::LibraryDiscovered { location, result } => {
                self.handle_library_discovered(&location, result);
            }
            Event::Loaded {
                source,
                index_profile,
                operation_id,
                result,
            } => self.handle_source_loaded(source, index_profile, operation_id, result),
            Event::BackupProgress {
                operation_id,
                progress,
            } => self.handle_backup_progress(operation_id, progress),
            Event::BackupFallbackRequired {
                operation_id,
                root,
                manifest_error,
            } => {
                if self.operation_is_active(operation_id) {
                    self.finish_operation(operation_id);
                    self.show_backup_fallback_warning(root, &manifest_error);
                }
            }
            Event::Indexed {
                source_id,
                generation,
                result,
            } => self.finish_index(&source_id, generation, result),
            Event::Search { generation, result } => {
                if generation != self.state.borrow().search_generation {
                    return;
                }
                self.spinner.stop();
                match result {
                    Ok(hits) => self.show_search_results(hits),
                    Err(error) => self.show_error("Search failed", &error),
                }
            }
            Event::Scene { generation, result } => {
                if generation != self.state.borrow().scene_generation {
                    return;
                }
                self.scene_cancel.borrow_mut().take();
                self.spinner.stop();
                match result {
                    Ok(scene) => {
                        self.page_view.set_scene(Some(scene));
                        self.canvas_stack.set_visible_child_name("document");
                        self.refresh_page_find_action();
                        if self.page_find_revealer.reveals_child() {
                            self.recompute_page_find(true);
                        }
                        self.reveal_pending_target();
                        self.status.set_label("Page ready");
                    }
                    Err(error) => self.show_error("Could not render page", &error),
                }
            }
            Event::Extracted {
                operation_id,
                result,
            } => self.handle_extracted(operation_id, result),
            Event::ExtractionProgress {
                operation_id,
                phase,
            } => self.handle_extraction_progress(operation_id, phase),
            Event::AttachmentProgress {
                operation_id,
                copied_bytes,
                declared_bytes,
            } => self.handle_attachment_progress(operation_id, copied_bytes, declared_bytes),
            Event::AttachmentCopied {
                operation_id,
                purpose,
                destination,
                result,
            } => self.handle_attachment_copied(operation_id, purpose, &destination, result),
        }
    }

    fn handle_source_loaded(
        self: &Rc<Self>,
        source: SourceDescriptor,
        index_profile: IndexProfile,
        operation_id: Option<u64>,
        result: std::result::Result<worker::SourceLoad, String>,
    ) {
        if operation_id.is_some_and(|operation_id| !self.operation_is_active(operation_id)) {
            return;
        }
        match result {
            Ok(load) if source.is_backup() || operation_id.is_some() => {
                self.queue_staged_publication(source, load, index_profile, operation_id);
            }
            Ok(load) => {
                self.add_source(source, Arc::clone(&load.loaded));
                self.queue_index(load.loaded, index_profile);
            }
            Err(error) => {
                self.finish_restore_load(source.path());
                if let Some(operation_id) = operation_id {
                    let cancelled = self.operation_was_cancelled(operation_id);
                    self.finish_operation(operation_id);
                    if cancelled {
                        self.status.set_label("Backup loading cancelled");
                        return;
                    }
                }
                self.show_error("Could not read notebook", &error);
            }
        }
    }

    fn handle_discovered(
        self: &Rc<Self>,
        requested: &std::path::Path,
        result: std::result::Result<Vec<SourceDescriptor>, String>,
    ) {
        match result {
            Ok(mut paths) => {
                apply_source_overrides(&mut paths, &self.state.borrow().configured_sources);
                self.register_source_order(&paths);
                prioritize_discovered_sources(
                    &mut paths,
                    self.state.borrow().restore_target.as_ref(),
                );
                self.finish_restore_discovery(requested, &paths);
                for source in paths {
                    self.load_source(source, None);
                }
                self.set_busy(&format!("Opening {}", requested.display()));
            }
            Err(error) => {
                self.finish_restore_discovery(requested, &[]);
                self.show_error("Could not open source", &error);
            }
        }
    }

    fn handle_library_discovered(
        self: &Rc<Self>,
        location: &std::path::Path,
        result: std::result::Result<Vec<SourceDescriptor>, String>,
    ) {
        match result {
            Ok(paths) if paths.is_empty() => {
                self.finish_restore_discovery(location, &[]);
                self.spinner.stop();
                self.status.set_label(&format!(
                    "Default notebooks location is empty: {}",
                    location.display()
                ));
            }
            Ok(mut paths) => {
                apply_source_overrides(&mut paths, &self.state.borrow().configured_sources);
                self.register_source_order(&paths);
                prioritize_discovered_sources(
                    &mut paths,
                    self.state.borrow().restore_target.as_ref(),
                );
                self.finish_restore_discovery(location, &paths);
                let count = paths.len();
                for source in paths {
                    self.load_source(source, None);
                }
                self.set_busy(&format!(
                    "Opening {count} notebook{} from the default location",
                    if count == 1 { "" } else { "s" }
                ));
            }
            Err(error) => {
                self.finish_restore_discovery(location, &[]);
                self.show_error(
                    "Could not scan default notebooks location",
                    &format!("{}\n\n{error}", location.display()),
                );
            }
        }
    }

    fn handle_extracted(
        self: &Rc<Self>,
        operation_id: u64,
        result: std::result::Result<PathBuf, String>,
    ) {
        if !self.operation_is_active(operation_id) {
            return;
        }
        match result {
            Ok(destination) => {
                self.status.set_label("Package imported; opening notebooks");
                self.operation_cancel_button.set_sensitive(false);
                self.operation_progress.set_fraction(1.0);
                self.operation_activity_phase
                    .set_label("Imported successfully; opening notebooks...");
                self.hide_operation_activity_after(operation_id, Duration::from_millis(1_800));
                self.discover(destination);
            }
            Err(error) => {
                let cancelled = self.operation_was_cancelled(operation_id);
                self.finish_operation(operation_id);
                if cancelled || error == "OneNote package extraction was cancelled" {
                    self.status.set_label("Package import cancelled");
                } else {
                    self.show_error("Package import failed", &error);
                }
            }
        }
    }

    fn handle_extraction_progress(&self, operation_id: u64, phase: ExtractionPhase) {
        if self.operation_is_active(operation_id) {
            self.operation_activity_phase
                .set_label(extraction_phase_label(phase));
        }
    }

    fn handle_attachment_progress(
        &self,
        operation_id: u64,
        copied_bytes: u64,
        declared_bytes: Option<u64>,
    ) {
        if !self.operation_is_active(operation_id) {
            return;
        }
        if let Some(total) = declared_bytes.filter(|total| *total > 0) {
            self.operation_pulsing.set(false);
            self.operation_progress
                .set_fraction(attachment_progress_fraction(copied_bytes, total));
            self.operation_activity_phase.set_label(&format!(
                "Copied {} of {}",
                crate::attachment::format_size(copied_bytes),
                crate::attachment::format_size(total)
            ));
        } else {
            self.operation_pulsing.set(true);
            self.operation_activity_phase.set_label(&format!(
                "Copied {}",
                crate::attachment::format_size(copied_bytes)
            ));
        }
    }

    fn handle_attachment_copied(
        self: &Rc<Self>,
        operation_id: u64,
        purpose: worker::AttachmentPurpose,
        destination: &gio::File,
        result: std::result::Result<u64, String>,
    ) {
        if !self.operation_is_active(operation_id) {
            return;
        }
        let cancelled = self.operation_was_cancelled(operation_id);
        self.finish_operation(operation_id);
        match result {
            Ok(bytes) if purpose == worker::AttachmentPurpose::Open => {
                self.status.set_label(&format!(
                    "Attachment ready ({})",
                    crate::attachment::format_size(bytes)
                ));
                self.launch_attachment(destination);
            }
            Ok(bytes) => self.status.set_label(&format!(
                "Attachment saved to {} ({})",
                destination.parse_name(),
                crate::attachment::format_size(bytes)
            )),
            Err(_) if cancelled => self.status.set_label("Attachment copy cancelled"),
            Err(error) => self.show_error("Could not copy attachment", &error),
        }
    }

    fn reveal_pending_target(&self) {
        let Some(target) = self.state.borrow_mut().pending_reveal.take() else {
            return;
        };
        if let Some(object_id) = target.object_id {
            self.page_view
                .reveal_source_object(&object_id, target.bounds);
        } else {
            self.page_view.reveal(target.bounds);
        }
    }

    fn discover(&self, path: PathBuf) {
        self.set_busy(&format!("Discovering {}", path.display()));
        let _ignored = self.source_commands.send(SourceCommand::Discover(path));
    }

    fn load_source(&self, source: SourceDescriptor, operation: Option<(u64, BackupLoadControl)>) {
        self.load_source_with_manifest_policy(
            source,
            operation,
            onenote_core::RootManifestPolicy::Ignore,
        );
    }

    fn load_source_with_manifest_policy(
        &self,
        source: SourceDescriptor,
        operation: Option<(u64, BackupLoadControl)>,
        root_manifest: onenote_core::RootManifestPolicy,
    ) {
        let settings = self.settings.borrow();
        let options = load_options(&settings);
        let index_profile = index_profile(&settings);
        let (operation_id, control) = operation.map_or_else(
            || (None, BackupLoadControl::new()),
            |(id, control)| (Some(id), control),
        );
        if self
            .source_commands
            .send(SourceCommand::Load {
                source,
                options,
                index_profile,
                operation_id,
                control,
                root_manifest,
            })
            .is_err()
        {
            if let Some(operation_id) = operation_id {
                self.finish_operation(operation_id);
            }
            self.show_error(
                "Could not read notebook",
                "The source-loading worker is not available.",
            );
        }
    }

    fn register_source_order(&self, sources: &[SourceDescriptor]) {
        self.state
            .borrow_mut()
            .source_order
            .register(sources.iter().map(|source| source.path().to_path_buf()));
    }

    fn open_notebooks_location(&self) {
        let location = self.settings.borrow().notebooks_location.clone();
        if let Err(error) = settings::ensure_notebooks_location(&location) {
            self.show_error(
                "Could not prepare default notebooks location",
                &error.to_string(),
            );
            return;
        }
        self.set_busy(&format!(
            "Scanning default notebooks location {}",
            location.display()
        ));
        let _ignored = self
            .source_commands
            .send(SourceCommand::DiscoverLibrary(location));
    }

    fn add_source(self: &Rc<Self>, descriptor: SourceDescriptor, loaded: Arc<LoadedNotebook>) {
        let backup_source = descriptor.is_backup();
        let source_id = loaded.notebook.source_id.clone();
        let mut state = self.state.borrow_mut();
        let restored_tree = state
            .pending_expansions
            .remove(&source_id)
            .map(source_tree_expansion);
        let restored_location =
            restore_location_for_source(state.restore_target.as_ref(), &loaded.notebook);
        let restoring = restored_location.is_some();
        let previous_path = state
            .sources
            .iter()
            .find(|source| source.loaded.notebook.source_id == source_id)
            .map(|source| source.descriptor.path().to_path_buf());
        register_configured_source(
            &mut state.configured_sources,
            &descriptor,
            &self.settings.borrow().notebooks_location,
            previous_path.as_deref(),
        );
        let display_position = state.upsert_source(descriptor, loaded, restored_location);
        let notebook = state
            .sources
            .iter()
            .find(|source| source.loaded.notebook.source_id == source_id)
            .map(|source| source.loaded.notebook.clone())
            .expect("inserted source");
        let active_source = state.active.as_ref().map(|active| active.source.clone());
        {
            let State {
                sources, history, ..
            } = &mut *state;
            history.retain(|location| page_location_exists(sources, location));
        }
        if restoring {
            state.restore_target = None;
        }
        drop(state);

        self.synchronize_selections(|| {
            self.notebook_tree.upsert_at(
                &notebook,
                restored_tree.clone(),
                u32::try_from(display_position).unwrap_or(u32::MAX),
            );
        });
        if restoring
            || active_source
                .as_ref()
                .is_none_or(|active| active == &source_id)
        {
            let history_update = if restoring || active_source.is_none() {
                HistoryUpdate::Replace
            } else {
                HistoryUpdate::Record
            };
            self.activate_source(&source_id, history_update);
        } else {
            self.refresh_history_actions();
        }
        if let Some(expansion) = restored_tree {
            let (source_is_active, active_section) = {
                let state = self.state.borrow();
                let active = state
                    .active
                    .as_ref()
                    .filter(|active| active.source == source_id);
                (
                    active.is_some(),
                    active
                        .and_then(|active| active.section.as_ref())
                        .map(|location| location.section_id.clone()),
                )
            };
            self.synchronize_selections(|| {
                self.notebook_tree.restore_expansion(&source_id, expansion);
                if source_is_active
                    && active_section.as_ref().is_none_or(|section_id| {
                        self.notebook_tree
                            .select_visible_section(&source_id, section_id)
                            .is_none()
                    })
                {
                    self.notebook_tree.select_notebook(&source_id);
                }
            });
        }
        self.schedule_workspace_save();
        self.refresh_source_actions();
        self.status.set_label(if backup_source {
            "Backup notebook opened with reconstructed section order"
        } else {
            "Notebook opened"
        });
    }

    fn queue_index(&self, loaded: Arc<LoadedNotebook>, profile: IndexProfile) {
        let source_id = loaded.notebook.source_id.clone();
        let (generation, cancel) = self.index_activity.borrow_mut().begin(source_id.clone());
        if self
            .index_commands
            .send(IndexCommand::Ensure {
                loaded,
                profile,
                generation,
                cancel,
            })
            .is_err()
        {
            let mut activity = self.index_activity.borrow_mut();
            if activity.take_current(&source_id, generation) {
                activity.failed += 1;
            }
            self.show_error(
                "Notebook opened, but indexing failed",
                "The search-index worker is not available.",
            );
            return;
        }
        self.refresh_index_status();
    }

    fn queue_staged_publication(
        self: &Rc<Self>,
        descriptor: SourceDescriptor,
        load: worker::SourceLoad,
        profile: IndexProfile,
        operation_id: Option<u64>,
    ) {
        let source_id = load.loaded.notebook.source_id.clone();
        let loaded = load.loaded.clone();
        let (generation, cancel) = self.index_activity.borrow_mut().begin(source_id.clone());
        if let Some(operation_id) = operation_id {
            if let Some(ForegroundOperation {
                id,
                kind: ForegroundOperationKind::BackupLoad(operation),
            }) = self.foreground_operation.borrow().as_ref()
            {
                if *id == operation_id {
                    operation.set_index_cancel(cancel.clone());
                }
            }
        }
        self.state
            .borrow_mut()
            .pending_publications
            .retain(|(pending_source, _), _| pending_source != &source_id);
        self.state.borrow_mut().pending_publications.insert(
            (source_id.clone(), generation),
            PendingPublication {
                descriptor,
                load,
                operation_id,
            },
        );
        if self
            .index_commands
            .send(IndexCommand::Ensure {
                loaded,
                profile,
                generation,
                cancel,
            })
            .is_err()
        {
            self.index_activity
                .borrow_mut()
                .take_current(&source_id, generation);
            let pending = self
                .state
                .borrow_mut()
                .pending_publications
                .remove(&(source_id, generation));
            if let Some(operation_id) = pending.and_then(|pending| pending.operation_id) {
                self.finish_operation(operation_id);
            }
            self.show_error(
                "Could not open backup folder",
                "The search-index worker is not available.",
            );
            return;
        }
        if operation_id.is_some() {
            self.operation_pulsing.set(true);
            self.operation_progress.set_fraction(0.0);
            self.operation_activity_phase
                .set_label("Updating the search index...");
        }
        self.refresh_index_status();
    }

    fn handle_backup_progress(&self, operation_id: Option<u64>, progress: BackupLoadProgress) {
        let phase = backup_progress_label(progress.phase);
        self.status.set_label(phase);
        let Some(operation_id) = operation_id else {
            return;
        };
        if !self.operation_is_active(operation_id) {
            return;
        }
        self.operation_activity_phase.set_label(phase);
        if progress.total == 0 {
            self.operation_pulsing.set(true);
        } else {
            self.operation_pulsing.set(false);
            let completed = u32::try_from(progress.completed).unwrap_or(u32::MAX);
            let total = u32::try_from(progress.total).unwrap_or(u32::MAX);
            self.operation_progress
                .set_fraction((f64::from(completed) / f64::from(total)).clamp(0.0, 1.0));
        }
    }

    fn finish_index(
        self: &Rc<Self>,
        source_id: &SourceId,
        generation: u64,
        result: std::result::Result<IndexUpdate, String>,
    ) {
        if !self
            .index_activity
            .borrow_mut()
            .take_current(source_id, generation)
        {
            return;
        }
        let pending = self
            .state
            .borrow_mut()
            .pending_publications
            .remove(&(source_id.clone(), generation));
        match result {
            Ok(IndexUpdate::Reused) => self.index_activity.borrow_mut().reused += 1,
            Ok(IndexUpdate::Rebuilt) => self.index_activity.borrow_mut().rebuilt += 1,
            Err(error) => {
                let operation_id = pending.as_ref().and_then(|pending| pending.operation_id);
                let cancelled = operation_id.is_some_and(|id| self.operation_was_cancelled(id));
                if let Some(operation_id) = operation_id {
                    self.finish_operation(operation_id);
                }
                if cancelled {
                    self.status.set_label("Backup loading cancelled");
                    return;
                }
                self.index_activity.borrow_mut().failed += 1;
                self.show_error(
                    if pending.is_some() {
                        "Could not refresh notebook"
                    } else {
                        "Notebook opened, but indexing failed"
                    },
                    &format!("{source_id}: {error}"),
                );
                return;
            }
        }
        if let Some(pending) = pending {
            let backup_source = pending.descriptor.is_backup();
            self.add_source(pending.descriptor, pending.load.loaded);
            if let Some(operation_id) = pending.operation_id {
                self.finish_operation(operation_id);
            }
            if backup_source {
                self.status
                    .set_label("Backup notebook opened with reconstructed section order");
                return;
            }
        }
        self.refresh_index_status();
    }

    fn refresh_index_status(&self) {
        let activity = self.index_activity.borrow();
        self.status.set_label(&activity.status());
    }

    fn cancel_index_source(&self, source_id: &SourceId) {
        self.index_activity.borrow_mut().cancel_source(source_id);
        self.refresh_index_status();
    }

    fn cancel_index_jobs(&self) {
        self.index_activity.borrow_mut().cancel_all();
    }

    fn finish_restore_discovery(
        self: &Rc<Self>,
        requested: &std::path::Path,
        sources: &[SourceDescriptor],
    ) {
        let paths = sources
            .iter()
            .map(|source| source.path().to_path_buf())
            .collect::<Vec<_>>();
        let mut state = self.state.borrow_mut();
        let page_unavailable =
            restore_missing_from_discovery(state.restore_target.as_ref(), requested, &paths);
        if page_unavailable {
            state.restore_target = None;
        }
        let before = state.pending_expansions.len();
        state
            .pending_expansions
            .retain(|_, source| !tree_state_missing_from_discovery(source, requested, &paths));
        let tree_unavailable = before != state.pending_expansions.len();
        drop(state);
        if page_unavailable || tree_unavailable {
            self.schedule_workspace_save();
        }
    }

    fn finish_restore_load(self: &Rc<Self>, path: &std::path::Path) {
        let mut state = self.state.borrow_mut();
        let page_unavailable = state
            .restore_target
            .as_ref()
            .is_some_and(|target| same_source_path(path, &target.source_path));
        if page_unavailable {
            state.restore_target = None;
        }
        let before = state.pending_expansions.len();
        state
            .pending_expansions
            .retain(|_, source| !same_source_path(path, &source.source_path));
        let tree_unavailable = before != state.pending_expansions.len();
        drop(state);
        if page_unavailable || tree_unavailable {
            self.schedule_workspace_save();
        }
    }

    fn cancel_pending_restore(&self) {
        self.state.borrow_mut().restore_target = None;
    }

    fn synchronize_selections<R>(&self, update: impl FnOnce() -> R) -> R {
        let previous = self.selection_syncing.replace(true);
        let result = update();
        self.selection_syncing.set(previous);
        result
    }

    fn notebook_selection_changed(self: &Rc<Self>) {
        if self.selection_syncing.get() {
            return;
        }
        match self.notebook_tree.selected_target() {
            Some(NavigationTarget::Notebook { source_id }) => {
                self.cancel_pending_restore();
                self.activate_source(&source_id, HistoryUpdate::Record);
            }
            Some(NavigationTarget::Section {
                source_id,
                section_id,
            }) => {
                self.cancel_pending_restore();
                self.activate_section(&source_id, &section_id, HistoryUpdate::Record);
            }
            Some(NavigationTarget::Group { .. }) | None => {}
        }
    }

    fn page_selection_changed(self: &Rc<Self>, position: u32) {
        if self.selection_syncing.get() || position == NO_SELECTION {
            return;
        }
        self.cancel_pending_restore();
        self.activate_page(position as usize, None, HistoryUpdate::Record);
    }

    fn result_selection_changed(self: &Rc<Self>, position: u32) {
        if self.selection_syncing.get() || position == NO_SELECTION {
            return;
        }
        self.cancel_pending_restore();
        self.activate_result(position as usize);
    }

    fn navigate_history(self: &Rc<Self>, direction: HistoryDirection) {
        self.cancel_pending_restore();
        let target = {
            let mut state = self.state.borrow_mut();
            let State {
                sources, history, ..
            } = &mut *state;
            history.retain(|location| page_location_exists(sources, location));
            history.step(direction)
        };
        self.refresh_history_actions();
        let Some(target) = target else {
            return;
        };

        self.synchronize_selections(|| self.result_selection.set_selected(NO_SELECTION));
        let activated = self.activate_location(
            &target.source,
            &SectionLocation {
                section_id: target.section.clone(),
                page_id: Some(target.page.clone()),
            },
            None,
            HistoryUpdate::Preserve,
        );
        if !activated {
            let mut state = self.state.borrow_mut();
            state.history.retain(|location| location != &target);
            drop(state);
            self.refresh_history_actions();
        }
    }

    fn refresh_history_actions(&self) {
        let state = self.state.borrow();
        self.history_back_action
            .set_enabled(state.history.can_go_back());
        self.history_forward_action
            .set_enabled(state.history.can_go_forward());
    }

    fn activate_source(self: &Rc<Self>, source_id: &SourceId, history_update: HistoryUpdate) {
        let location = {
            let mut state = self.state.borrow_mut();
            let Some(source) = state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == *source_id)
            else {
                return;
            };
            let location =
                preferred_location(&source.loaded.notebook, source.last_location.as_ref());
            state.active = Some(ActiveLocation {
                source: source_id.clone(),
                section: location.clone(),
            });
            location
        };
        if let Some(location) = location {
            self.activate_location(source_id, &location, None, history_update);
        } else {
            self.clear_page_content();
            self.synchronize_selections(|| {
                self.notebook_tree.select_notebook(source_id);
                self.page_selection.set_selected(NO_SELECTION);
            });
            self.schedule_workspace_save();
            self.refresh_source_actions();
        }
    }

    fn activate_section(
        self: &Rc<Self>,
        source_id: &SourceId,
        section_id: &SectionId,
        history_update: HistoryUpdate,
    ) {
        let location = {
            let state = self.state.borrow();
            state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == *source_id)
                .and_then(|source| {
                    location_for_section(
                        &source.loaded.notebook,
                        source.last_location.as_ref(),
                        section_id,
                    )
                })
        };
        if let Some(location) = location {
            self.activate_location(source_id, &location, None, history_update);
        }
    }

    fn activate_location(
        self: &Rc<Self>,
        source_id: &SourceId,
        requested: &SectionLocation,
        reveal: Option<RevealTarget>,
        history_update: HistoryUpdate,
    ) -> bool {
        let (pages, location) = {
            let state = self.state.borrow();
            let Some(source) = state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == *source_id)
            else {
                return false;
            };
            let Some(location) = location_for_section(
                &source.loaded.notebook,
                Some(requested),
                &requested.section_id,
            ) else {
                return false;
            };
            let pages = source
                .loaded
                .notebook
                .section(&location.section_id)
                .map(|section| section.pages.clone())
                .unwrap_or_default();
            (pages, location)
        };

        let page_position = location
            .page_id
            .as_ref()
            .and_then(|page_id| pages.iter().position(|page| page.id == *page_id));
        {
            let mut state = self.state.borrow_mut();
            let Some(source) = state
                .sources
                .iter_mut()
                .find(|source| source.loaded.notebook.source_id == *source_id)
            else {
                return false;
            };
            source.last_location = Some(location.clone());
            state.active = Some(ActiveLocation {
                source: source_id.clone(),
                section: Some(location.clone()),
            });
            state.pages = pages
                .iter()
                .map(|page| PageRow {
                    source: source_id.clone(),
                    section: location.section_id.clone(),
                    page: page.id.clone(),
                })
                .collect();
        }

        self.synchronize_selections(|| {
            self.notebook_tree
                .select_section(source_id, &location.section_id);
            clear_model(&self.page_model);
            for page in &pages {
                let indent = "  ".repeat(usize::try_from(page.level.max(0)).unwrap_or(0).min(8));
                append_model(
                    &self.page_model,
                    &format!("{indent}{}", display_title(page)),
                );
            }
            self.page_selection.set_selected(
                page_position
                    .and_then(|position| u32::try_from(position).ok())
                    .unwrap_or(NO_SELECTION),
            );
        });

        if let Some(position) = page_position {
            self.activate_page(position, reveal, history_update)
        } else {
            self.clear_rendered_page();
            self.schedule_workspace_save();
            false
        }
    }

    fn activate_page(
        self: &Rc<Self>,
        position: usize,
        reveal: Option<RevealTarget>,
        history_update: HistoryUpdate,
    ) -> bool {
        let value = (|| {
            let state = self.state.borrow();
            let row = state.pages.get(position)?;
            let source = state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == row.source)?;
            let section = source.loaded.notebook.section(&row.section)?;
            let page = section.pages.iter().find(|page| page.id == row.page)?;
            let section_path = source
                .loaded
                .notebook
                .section_path(&row.section)?
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            Some((
                row.source.clone(),
                row.section.clone(),
                row.page.clone(),
                page.clone(),
                source.loaded.clone(),
                source.loaded.notebook.name.clone(),
                section_path,
            ))
        })();
        let Some((source_id, section_id, page_id, page, loaded, notebook_name, section_path)) =
            value
        else {
            return false;
        };
        let location = SectionLocation {
            section_id: section_id.clone(),
            page_id: Some(page_id.clone()),
        };
        {
            let mut state = self.state.borrow_mut();
            if let Some(source) = state
                .sources
                .iter_mut()
                .find(|source| source.loaded.notebook.source_id == source_id)
            {
                source.last_location = Some(location.clone());
            }
            state.active = Some(ActiveLocation {
                source: source_id.clone(),
                section: Some(location),
            });
            let history_location = PageLocation {
                source: source_id.clone(),
                section: section_id.clone(),
                page: page_id,
            };
            match history_update {
                HistoryUpdate::Record => state.history.record(history_location),
                HistoryUpdate::Replace => state.history.replace_current(history_location),
                HistoryUpdate::Preserve => {}
            }
        }
        self.synchronize_selections(|| {
            self.notebook_tree.select_section(&source_id, &section_id);
            self.page_selection
                .set_selected(u32::try_from(position).unwrap_or(NO_SELECTION));
        });
        self.schedule_workspace_save();
        let display_title = display_title(&page);
        let title = gtk_text(&display_title);
        self.page_title.set_attributes(None);
        self.page_title.set_label(&title);
        let date = display_timestamp(&page.created_at);
        self.page_date.set_label(&date);
        let display_context = std::iter::once(notebook_name.as_str())
            .chain(section_path.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("  /  ");
        let context = gtk_text(&display_context);
        self.page_context.set_label(&context);
        self.page_view
            .set_resources(Some(Arc::new(loaded.resources.clone())));
        self.page_view.set_scene(None);
        self.prepare_page_find_for_page_load();
        let generation = {
            let mut state = self.state.borrow_mut();
            state.scene_generation = state.scene_generation.wrapping_add(1);
            state.pending_reveal = reveal;
            state.scene_generation
        };
        if let Some(cancel) = self.scene_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.scene_cancel.borrow_mut() = Some(cancel.clone());
        self.set_busy("Laying out page");
        worker::build_scene(generation, page, cancel, self.events.clone());
        self.refresh_history_actions();
        self.refresh_workspace_search_scope();
        self.refresh_page_find_action();
        self.refresh_source_actions();
        true
    }

    fn prepare_page_find_for_page_load(&self) {
        if self.page_find_revealer.reveals_child() {
            *self.page_find_state.borrow_mut() = PageFindState::default();
            self.page_find_count.set_label("Loading page...");
            self.page_find_previous.set_sensitive(false);
            self.page_find_next.set_sensitive(false);
        }
    }

    fn clear_page_content(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(active) = state.active.as_mut() {
            active.section = None;
        }
        state.pages.clear();
        drop(state);
        self.synchronize_selections(|| {
            clear_model(&self.page_model);
            self.page_selection.set_selected(NO_SELECTION);
        });
        self.clear_rendered_page();
        self.refresh_workspace_search_scope();
    }

    fn clear_rendered_page(&self) {
        if let Some(cancel) = self.scene_cancel.borrow_mut().take() {
            cancel.store(true, Ordering::Relaxed);
        }
        {
            let mut state = self.state.borrow_mut();
            state.scene_generation = state.scene_generation.wrapping_add(1);
            state.pending_reveal = None;
        }
        self.page_title.set_label("");
        self.page_date.set_label("");
        self.page_context.set_label("");
        self.page_view.set_scene(None);
        self.canvas_stack.set_visible_child_name("empty");
        self.close_page_find();
        self.refresh_page_find_action();
    }

    fn refresh_page_find_action(&self) {
        self.page_find_action
            .set_enabled(self.page_view.canvas().scene().is_some());
    }

    fn show_page_find(self: &Rc<Self>) {
        if !self.page_find_action.is_enabled() {
            return;
        }
        self.page_find_revealer.set_reveal_child(true);
        self.page_find_entry.grab_focus();
        self.page_find_entry.select_region(0, -1);
        if !self.page_find_entry.text().is_empty() {
            self.recompute_page_find(false);
        }
    }

    fn close_page_find(&self) {
        self.page_find_revealer.set_reveal_child(false);
        self.page_find_entry.set_text("");
        if let Some(timer) = self.page_find_timer.borrow_mut().take() {
            timer.remove();
        }
        *self.page_find_state.borrow_mut() = PageFindState::default();
        self.page_find_count.set_label("No matches");
        self.page_find_previous.set_sensitive(false);
        self.page_find_next.set_sensitive(false);
        self.page_title.set_attributes(None);
        self.page_view.set_find_highlights(Vec::new(), None, false);
        if self.page_view.canvas().scene().is_some() {
            self.page_view.canvas().grab_focus();
        }
    }

    fn schedule_page_find(self: &Rc<Self>) {
        if let Some(timer) = self.page_find_timer.borrow_mut().take() {
            timer.remove();
        }
        let weak = Rc::downgrade(self);
        *self.page_find_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(120),
            move || {
                if let Some(viewer) = weak.upgrade() {
                    viewer.page_find_timer.borrow_mut().take();
                    viewer.recompute_page_find(true);
                }
            },
        ));
    }

    fn page_find_options(&self) -> FindOptions {
        FindOptions {
            case_sensitive: self.page_find_case.is_active(),
            whole_word: self.page_find_words.is_active(),
            match_diacritics: self.page_find_diacritics.is_active(),
        }
    }

    fn recompute_page_find(&self, reset_active: bool) {
        let query = self.page_find_entry.text();
        if query.is_empty() || self.page_view.canvas().scene().is_none() {
            *self.page_find_state.borrow_mut() = PageFindState::default();
            self.apply_page_find_state(false);
            return;
        }
        let options = self.page_find_options();
        let title_matches = find_text_ranges(
            self.page_title.text().as_str(),
            query.as_str(),
            options,
            PAGE_FIND_LIMIT,
        );
        let page_matches = self.page_view.find(
            query.as_str(),
            options,
            PAGE_FIND_LIMIT.saturating_sub(title_matches.len()),
        );
        let mut state = self.page_find_state.borrow_mut();
        let previous_active = state.active;
        state.title_matches = title_matches;
        state.page_matches = page_matches;
        state.active = if state.len() == 0 {
            None
        } else if reset_active {
            Some(0)
        } else {
            Some(previous_active.unwrap_or(0).min(state.len() - 1))
        };
        drop(state);
        self.apply_page_find_state(true);
    }

    fn navigate_page_find(self: &Rc<Self>, direction: i32) {
        if !self.page_find_revealer.reveals_child() {
            self.show_page_find();
        }
        let mut state = self.page_find_state.borrow_mut();
        let count = state.len();
        if count == 0 {
            return;
        }
        let current = state.active.unwrap_or(0);
        state.active = Some(if direction < 0 {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        });
        drop(state);
        self.apply_page_find_state(true);
    }

    fn apply_page_find_state(&self, reveal_active: bool) {
        let state = self.page_find_state.borrow();
        let count = state.len();
        let highlight_all = self.page_find_highlight_all.is_active();
        let title_active = state
            .active
            .filter(|index| *index < state.title_matches.len());
        let title_attributes =
            page_find_title_attributes(&state.title_matches, title_active, highlight_all);
        self.page_title.set_attributes(title_attributes.as_ref());
        let page_active = state
            .active
            .and_then(|index| index.checked_sub(state.title_matches.len()));
        self.page_view
            .set_find_highlights(state.page_matches.clone(), page_active, highlight_all);
        match state.active {
            Some(active) => self
                .page_find_count
                .set_label(&format!("{} of {count}", active + 1)),
            None => self.page_find_count.set_label("No matches"),
        }
        self.page_find_previous.set_sensitive(count > 0);
        self.page_find_next.set_sensitive(count > 0);
        if reveal_active {
            if let Some(found) = page_active.and_then(|index| state.page_matches.get(index)) {
                self.page_view.reveal_find_match(found);
            }
        }
    }

    fn set_workspace_search_scope(self: &Rc<Self>, scope: WorkspaceSearchScope) {
        self.workspace_search_scope.set(scope);
        self.search_scope.choice(scope).set_active(true);
        self.search_scope
            .button
            .set_tooltip_text(Some(&format!("Search scope: {}", scope.label())));
        self.search_scope.button.popdown();
        self.search_entry
            .set_placeholder_text(Some(&format!("Search {}", scope.status_label())));
        self.search_entry.grab_focus();
        self.search_entry.select_region(0, -1);
        self.workspace_search_text_changed();
    }

    fn refresh_workspace_search_scope(&self) {
        let (has_notebook, has_section, has_section_group) = {
            let state = self.state.borrow();
            let active = state.active.as_ref();
            let source = active.and_then(|active| {
                state
                    .sources
                    .iter()
                    .find(|source| source.loaded.notebook.source_id == active.source)
            });
            let section_id = active
                .and_then(|active| active.section.as_ref())
                .map(|location| &location.section_id);
            let has_group = source
                .zip(section_id)
                .and_then(|(source, section_id)| {
                    source.loaded.notebook.section_group_ancestry(section_id)
                })
                .is_some_and(|groups| !groups.is_empty());
            (source.is_some(), section_id.is_some(), has_group)
        };
        self.search_scope.notebook.set_sensitive(has_notebook);
        self.search_scope.section.set_sensitive(has_section);
        self.search_scope
            .section_group
            .set_sensitive(has_section_group);
        let scope_available = match self.workspace_search_scope.get() {
            WorkspaceSearchScope::AllNotebooks => true,
            WorkspaceSearchScope::Notebook => has_notebook,
            WorkspaceSearchScope::SectionGroup => has_section_group,
            WorkspaceSearchScope::Section => has_section,
        };
        if !scope_available {
            self.workspace_search_scope
                .set(WorkspaceSearchScope::AllNotebooks);
            self.search_scope.all_notebooks.set_active(true);
            self.search_scope
                .button
                .set_tooltip_text(Some("Search scope: All notebooks"));
            self.search_entry
                .set_placeholder_text(Some("Search all notebooks"));
        }
    }

    fn workspace_search_text_changed(self: &Rc<Self>) {
        if let Some(timer) = self.search_timer.borrow_mut().take() {
            timer.remove();
        }
        {
            let mut state = self.state.borrow_mut();
            state.search_generation = state.search_generation.wrapping_add(1);
        }
        let text = self.search_entry.text().trim().to_owned();
        if text.is_empty() {
            self.spinner.stop();
            self.navigation_stack.set_visible_child_name("pages");
            self.set_navigation_width(self.page_navigation_width.get());
            self.synchronize_selections(|| self.result_selection.set_selected(NO_SELECTION));
            self.result_model.remove_all();
            self.state.borrow_mut().search_hits.clear();
            return;
        }
        let weak = Rc::downgrade(self);
        *self.search_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(250),
            move || {
                if let Some(viewer) = weak.upgrade() {
                    viewer.start_search(text);
                    viewer.search_timer.borrow_mut().take();
                }
            },
        ));
    }

    fn start_search(&self, text: String) {
        let query = {
            let state = self.state.borrow();
            let active = state.active.as_ref();
            let source = active.and_then(|active| {
                state
                    .sources
                    .iter()
                    .find(|source| source.loaded.notebook.source_id == active.source)
            });
            let section_id = active
                .and_then(|active| active.section.as_ref())
                .map(|location| &location.section_id);
            scoped_search_query(
                self.workspace_search_scope.get(),
                text,
                source.map(|source| &source.loaded.notebook),
                section_id,
            )
        };
        let Ok(query) = query else {
            self.status
                .set_label("The selected search scope is not available");
            return;
        };
        let generation = {
            let mut state = self.state.borrow_mut();
            state.search_generation = state.search_generation.wrapping_add(1);
            state.search_generation
        };
        self.set_busy(&format!(
            "Searching {}",
            self.workspace_search_scope.get().status_label()
        ));
        worker::search(
            self.index_path.clone(),
            generation,
            query,
            self.events.clone(),
        );
    }

    fn show_search_results(&self, hits: Vec<SearchHit>) {
        self.result_model.remove_all();
        for hit in &hits {
            self.result_model
                .append(&glib::BoxedAnyObject::new(search_result_row(hit)));
        }
        let count = hits.len();
        self.state.borrow_mut().search_hits = hits;
        self.navigation_stack.set_visible_child_name("results");
        self.set_navigation_width(SEARCH_RESULTS_WIDTH);
        self.status.set_label(&format!(
            "{count} search result{}",
            if count == 1 { "" } else { "s" }
        ));
    }

    fn move_search_result(&self, direction: i32) {
        let count = self.state.borrow().search_hits.len();
        if count == 0 {
            return;
        }
        let selected = self.result_selection.selected();
        let next = if selected == NO_SELECTION {
            if direction < 0 {
                count - 1
            } else {
                0
            }
        } else if direction < 0 {
            usize::try_from(selected).unwrap_or(0).saturating_sub(1)
        } else {
            (usize::try_from(selected).unwrap_or(0) + 1).min(count - 1)
        };
        self.result_selection
            .set_selected(u32::try_from(next).unwrap_or(NO_SELECTION));
    }

    fn commit_search_result(self: &Rc<Self>) {
        let selected = self.result_selection.selected();
        if selected == NO_SELECTION {
            return;
        }
        self.activate_result(usize::try_from(selected).unwrap_or(0));
        self.dismiss_workspace_search();
    }

    fn dismiss_workspace_search(&self) {
        self.search_entry.set_text("");
        self.page_view.canvas().grab_focus();
    }

    fn activate_result(self: &Rc<Self>, position: usize) {
        let hit = {
            let state = self.state.borrow();
            state.search_hits.get(position).cloned()
        };
        let Some(hit) = hit else {
            return;
        };
        let source_exists = {
            let state = self.state.borrow();
            state
                .sources
                .iter()
                .any(|source| source.loaded.notebook.source_id == hit.source_id)
        };
        if !source_exists {
            self.show_error(
                "Search result is stale",
                "The source is no longer open. Refresh the search index.",
            );
            return;
        }
        self.activate_location(
            &hit.source_id,
            &SectionLocation {
                section_id: hit.section_id,
                page_id: Some(hit.page_id),
            },
            hit.bounds.map(|bounds| RevealTarget {
                object_id: hit.object_id,
                bounds,
            }),
            HistoryUpdate::Record,
        );
    }

    fn close_active_source(self: &Rc<Self>) {
        let removed = {
            let mut state = self.state.borrow_mut();
            let Some(source_id) = state.active.as_ref().map(|active| active.source.clone()) else {
                return;
            };
            let Some(position) = state
                .sources
                .iter()
                .position(|source| source.loaded.notebook.source_id == source_id)
            else {
                return;
            };
            state.active = None;
            state.pages.clear();
            state.restore_target = None;
            let removed = state.sources.remove(position);
            state.source_order.remove(removed.descriptor.path());
            state
                .configured_sources
                .retain(|source| !same_source_path(source.path(), removed.descriptor.path()));
            state
                .pending_publications
                .retain(|(source, _), _| source != &source_id);
            {
                let State {
                    sources, history, ..
                } = &mut *state;
                history.retain(|location| page_location_exists(sources, location));
            }
            let history_target = state.history.current().cloned();
            (position, removed, history_target)
        };
        let removed_source_id = removed.1.loaded.notebook.source_id.clone();
        self.cancel_backup_operation_for_source(removed.1.descriptor.path());
        self.cancel_index_source(&removed_source_id);
        let _ignored = self
            .index_commands
            .send(IndexCommand::Remove(removed_source_id.clone()));
        self.synchronize_selections(|| {
            self.notebook_tree.remove(&removed_source_id);
            self.notebook_tree.selection.set_selected(NO_SELECTION);
            self.page_selection.set_selected(NO_SELECTION);
            clear_model(&self.page_model);
        });
        self.clear_rendered_page();
        self.refresh_history_actions();
        self.refresh_workspace_search_scope();
        self.refresh_source_actions();
        if let Some(target) = removed.2.as_ref() {
            if self.activate_location(
                &target.source,
                &SectionLocation {
                    section_id: target.section.clone(),
                    page_id: Some(target.page.clone()),
                },
                None,
                HistoryUpdate::Preserve,
            ) {
                return;
            }
        }
        let fallback = {
            let state = self.state.borrow();
            let position = removed.0.min(state.sources.len().saturating_sub(1));
            state
                .sources
                .get(position)
                .map(|source| source.loaded.notebook.source_id.clone())
        };
        if let Some(source_id) = fallback {
            self.activate_source(&source_id, HistoryUpdate::Replace);
        } else {
            self.status.set_label("No notebooks open");
            self.schedule_workspace_save();
        }
    }

    fn workspace_config(&self) -> WorkspaceConfig {
        let notebooks_location = self.settings.borrow().notebooks_location.clone();
        let state = self.state.borrow();
        let last_page = state.restore_target.clone().or_else(|| {
            let active = state.active.as_ref()?;
            let section = active.section.as_ref()?;
            let page_id = section.page_id.clone()?;
            let source = state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == active.source)?;
            Some(PersistedPageLocation {
                source_path: source.descriptor.path().to_path_buf(),
                source_id: active.source.clone(),
                section_id: section.section_id.clone(),
                page_id,
            })
        });
        let mut sources = state.configured_sources.clone();
        if let Some(target) = state.restore_target.as_ref().filter(|target| {
            !workspace::source_is_in_location(&target.source_path, &notebooks_location)
                && !sources
                    .iter()
                    .any(|source| same_source_path(source.path(), &target.source_path))
        }) {
            sources.push(SourceDescriptor::native(target.source_path.clone()));
        }
        let mut source_states = state
            .sources
            .iter()
            .filter_map(|source| {
                self.notebook_tree
                    .expansion_state(&source.loaded.notebook.source_id)
                    .map(|expansion| PersistedSourceTreeState {
                        source_path: source.descriptor.path().to_path_buf(),
                        source_id: source.loaded.notebook.source_id.clone(),
                        notebook_expanded: expansion.notebook_expanded,
                        expanded_groups: expansion.expanded_groups.into_iter().collect(),
                    })
            })
            .chain(state.pending_expansions.values().cloned())
            .collect::<Vec<_>>();
        source_states.sort_by_key(|source| {
            state
                .source_order
                .rank(&source.source_path)
                .unwrap_or(usize::MAX)
        });
        WorkspaceConfig {
            sources,
            navigation: WorkspaceNavigation { last_page },
            ui: Some(WorkspaceUiState::new(
                PersistedPaneState {
                    notebooks_collapsed: self.notebooks_collapsed.get(),
                    pages_collapsed: self.pages_collapsed.get(),
                },
                source_states,
            )),
        }
    }

    fn schedule_workspace_save(self: &Rc<Self>) {
        self.cancel_workspace_save();
        let weak = Rc::downgrade(self);
        *self.workspace_save_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(300),
            move || {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                viewer.workspace_save_timer.borrow_mut().take();
                if let Err(error) = viewer.write_workspace() {
                    viewer.show_error("Could not save workspace", &error.to_string());
                }
            },
        ));
    }

    fn cancel_workspace_save(&self) {
        if let Some(timer) = self.workspace_save_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    fn write_workspace(&self) -> Result<()> {
        workspace::save(&self.workspace_path, &self.workspace_config())
    }

    fn flush_workspace(&self) -> Result<()> {
        self.cancel_workspace_save();
        self.write_workspace()
    }

    fn choose_file(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Open OneNote file")
            .modal(true)
            .build();
        let weak = Rc::downgrade(self);
        dialog.open_multiple(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(files) => {
                        for position in 0..files.n_items() {
                            if let Some(path) = files
                                .item(position)
                                .and_then(|item| item.downcast::<gio::File>().ok())
                                .and_then(|file| file.path())
                            {
                                viewer.discover(path);
                            }
                        }
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => viewer.show_error("Could not select file", &error.to_string()),
                }
            },
        );
    }

    fn choose_folder(self: &Rc<Self>) {
        let dialog = gtk::FileDialog::builder()
            .title("Open notebook folder")
            .modal(true)
            .build();
        let weak = Rc::downgrade(self);
        dialog.select_folder(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            viewer.discover(path);
                        }
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => viewer.show_error("Could not select folder", &error.to_string()),
                }
            },
        );
    }

    fn choose_backup_folder(self: &Rc<Self>) {
        let initial = gio::File::for_path(&self.settings.borrow().notebooks_location);
        let dialog = gtk::FileDialog::builder()
            .title("Open OneNote backup folder")
            .initial_folder(&initial)
            .modal(true)
            .build();
        let weak = Rc::downgrade(self);
        dialog.select_folder(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(file) => {
                        if let Some(path) = file.path() {
                            viewer.start_backup_load(
                                SourceDescriptor::backup(
                                    path,
                                    BackupSelectionPolicy::LatestPerSection,
                                ),
                                "Opening OneNote backup folder",
                                onenote_core::RootManifestPolicy::Reject,
                            );
                        }
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => {
                        viewer.show_error("Could not select backup folder", &error.to_string());
                    }
                }
            },
        );
    }

    fn refresh_active_source(self: &Rc<Self>) {
        let descriptor = {
            let state = self.state.borrow();
            let Some(active) = state.active.as_ref() else {
                return;
            };
            state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == active.source)
                .map(|source| source.descriptor.clone())
        };
        let Some(descriptor) = descriptor.filter(SourceDescriptor::is_backup) else {
            return;
        };
        self.start_backup_load(
            descriptor,
            "Refreshing OneNote backup folder",
            onenote_core::RootManifestPolicy::Ignore,
        );
    }

    fn set_active_backup_policy(self: &Rc<Self>, policy: BackupSelectionPolicy) {
        let descriptor = {
            let state = self.state.borrow();
            let Some(active) = state.active.as_ref() else {
                return;
            };
            state
                .sources
                .iter()
                .find(|source| source.loaded.notebook.source_id == active.source)
                .map(|source| source.descriptor.with_backup_selection(policy))
        };
        let Some(descriptor) = descriptor.filter(SourceDescriptor::is_backup) else {
            return;
        };
        self.start_backup_load(
            descriptor,
            "Changing backup snapshot view",
            onenote_core::RootManifestPolicy::Ignore,
        );
    }

    fn start_backup_load(
        &self,
        descriptor: SourceDescriptor,
        title: &str,
        root_manifest: onenote_core::RootManifestPolicy,
    ) {
        if self.foreground_operation.borrow().is_some() {
            self.status
                .set_label("Another file operation is already running");
            return;
        }
        let operation_id = self.allocate_operation_id();
        let cancellation = Rc::new(BackupOperationCancellation::new(
            descriptor.path().to_path_buf(),
        ));
        let control = cancellation.load.clone();
        *self.foreground_operation.borrow_mut() = Some(ForegroundOperation {
            id: operation_id,
            kind: ForegroundOperationKind::BackupLoad(cancellation),
        });
        self.operation_pulsing.set(true);
        self.operation_cancel_button.set_sensitive(true);
        self.operation_progress.set_fraction(0.0);
        self.operation_activity_title.set_label(title);
        self.operation_activity_phase
            .set_label("Inspecting backup folder...");
        self.operation_activity.set_reveal_child(true);
        self.refresh_source_action.set_enabled(false);
        self.show_backup_copies_action.set_enabled(false);
        self.set_busy(title);
        self.load_source_with_manifest_policy(
            descriptor,
            Some((operation_id, control)),
            root_manifest,
        );
    }

    fn cancel_backup_operation_for_source(&self, source_path: &std::path::Path) {
        let operation_id = {
            let operation = self.foreground_operation.borrow();
            operation
                .as_ref()
                .and_then(|operation| match &operation.kind {
                    ForegroundOperationKind::BackupLoad(cancel)
                        if same_source_path(&cancel.source_path, source_path) =>
                    {
                        operation.cancel();
                        Some(operation.id)
                    }
                    _ => None,
                })
        };
        if let Some(operation_id) = operation_id {
            self.finish_operation(operation_id);
        }
    }

    fn refresh_source_actions(&self) {
        let selection = {
            let state = self.state.borrow();
            state.active.as_ref().and_then(|active| {
                state
                    .sources
                    .iter()
                    .find(|source| source.loaded.notebook.source_id == active.source)
                    .and_then(|source| match &source.descriptor {
                        SourceDescriptor::BackupFolder { selection, .. } => Some(*selection),
                        SourceDescriptor::NativeFile { .. } => None,
                    })
            })
        };
        let idle = self.foreground_operation.borrow().is_none();
        self.refresh_source_action
            .set_enabled(selection.is_some() && idle);
        self.show_backup_copies_action
            .set_enabled(selection.is_some() && idle);
        self.show_backup_copies_action
            .set_state(&(selection == Some(BackupSelectionPolicy::AllCopies)).to_variant());
    }

    fn choose_package(self: &Rc<Self>) {
        if self.foreground_operation.borrow().is_some() {
            self.status
                .set_label("Another file operation is already running");
            return;
        }
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("OneNote packages"));
        filter.add_suffix("onepkg");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Select OneNote package")
            .accept_label("Select")
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let weak = Rc::downgrade(self);
        dialog.open(
            Some(&self.window),
            None::<&gio::Cancellable>,
            move |result| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(file) => {
                        if let Some(package) = file.path() {
                            viewer.confirm_package_import(package);
                        }
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => viewer.show_error("Could not select package", &error.to_string()),
                }
            },
        );
    }

    fn confirm_package_import(self: &Rc<Self>, package: PathBuf) {
        let default_parent = self.settings.borrow().notebooks_location.clone();
        let weak_import = Rc::downgrade(self);
        let weak_error = weak_import.clone();
        crate::dialogs::present_package_import(
            &self.window,
            package,
            default_parent,
            move |package, destination| {
                if let Some(viewer) = weak_import.upgrade() {
                    viewer.start_package_import(package, destination);
                }
            },
            move |title, detail| {
                if let Some(viewer) = weak_error.upgrade() {
                    viewer.show_error(title, detail);
                }
            },
        );
    }

    fn show_settings(self: &Rc<Self>) {
        let current_settings = self.settings.borrow().clone();
        let default = settings::default_notebooks_location();
        let weak_save = Rc::downgrade(self);
        let weak_error = weak_save.clone();
        crate::dialogs::present_settings(
            &self.window,
            current_settings.notebooks_location,
            default,
            current_settings.theme,
            current_settings.detect_plain_text_links,
            move |location, theme, detect_plain_text_links| {
                weak_save.upgrade().is_some_and(|viewer| {
                    viewer.set_preferences(location, theme, detect_plain_text_links)
                })
            },
            move |title, detail| {
                if let Some(viewer) = weak_error.upgrade() {
                    viewer.show_error(title, detail);
                }
            },
        );
    }

    fn set_preferences(
        self: &Rc<Self>,
        location: &std::path::Path,
        theme: ThemePreference,
        detect_plain_text_links: bool,
    ) -> bool {
        if let Err(error) = settings::ensure_notebooks_location(location) {
            self.show_error(
                "Could not use default notebooks location",
                &error.to_string(),
            );
            return false;
        }
        let mut updated = self.settings.borrow().clone();
        let location_changed = updated.notebooks_location != location;
        let link_detection_changed = updated.detect_plain_text_links != detect_plain_text_links;
        updated.notebooks_location = location.to_path_buf();
        updated.theme = theme;
        updated.detect_plain_text_links = detect_plain_text_links;
        self.cancel_settings_save();
        if let Err(error) = settings::save(&self.settings_path, &updated) {
            self.show_error("Could not save settings", &error.to_string());
            return false;
        }
        *self.settings.borrow_mut() = updated;
        self.apply_theme(theme);
        self.schedule_workspace_save();
        if link_detection_changed {
            self.reload_sources_for_link_detection();
        }
        if location_changed {
            self.open_notebooks_location();
        }
        true
    }

    fn reload_sources_for_link_detection(&self) {
        let sources = self
            .state
            .borrow()
            .sources
            .iter()
            .map(|source| source.descriptor.clone())
            .collect::<Vec<_>>();
        if sources.is_empty() {
            return;
        }
        self.set_busy("Applying link detection setting");
        for source in sources {
            self.load_source(source, None);
        }
    }

    fn apply_theme(&self, preference: ThemePreference) {
        let theme = effective_theme(preference);
        self.style_provider.load_from_string(&theme_css(theme));
        self.page_view
            .set_default_text_color(&theme_default_text_color(theme));
    }

    fn start_package_import(&self, package: PathBuf, destination: PathBuf) {
        if self.foreground_operation.borrow().is_some() {
            self.status
                .set_label("Another file operation is already running");
            return;
        }
        let operation_id = self.allocate_operation_id();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.foreground_operation.borrow_mut() = Some(ForegroundOperation {
            id: operation_id,
            kind: ForegroundOperationKind::PackageImport(Arc::clone(&cancel)),
        });
        self.operation_pulsing.set(true);
        self.import_package_action.set_enabled(false);
        self.operation_cancel_button.set_sensitive(true);
        self.operation_progress.set_fraction(0.0);
        self.operation_activity_title.set_label(&format!(
            "Importing {}",
            package
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("OneNote package")
        ));
        self.operation_activity_phase
            .set_label("Preparing package import...");
        self.operation_activity.set_reveal_child(true);
        self.set_busy("Importing OneNote package");
        worker::extract(
            operation_id,
            package,
            destination,
            cancel,
            self.events.clone(),
        );
    }

    fn allocate_operation_id(&self) -> u64 {
        let id = self.next_operation_id.get();
        self.next_operation_id.set(id.wrapping_add(1).max(1));
        id
    }

    fn operation_is_active(&self, operation_id: u64) -> bool {
        self.foreground_operation
            .borrow()
            .as_ref()
            .is_some_and(|operation| operation.id == operation_id)
    }

    fn operation_was_cancelled(&self, operation_id: u64) -> bool {
        self.foreground_operation
            .borrow()
            .as_ref()
            .is_some_and(|operation| operation.id == operation_id && operation.is_cancelled())
    }

    fn finish_operation(&self, operation_id: u64) {
        let mut operation = self.foreground_operation.borrow_mut();
        if operation
            .as_ref()
            .is_none_or(|operation| operation.id != operation_id)
        {
            return;
        }
        operation.take();
        self.operation_pulsing.set(false);
        self.operation_cancel_button.set_sensitive(false);
        self.import_package_action.set_enabled(true);
        self.operation_activity.set_reveal_child(false);
        self.refresh_source_actions();
    }

    fn cancel_foreground_operation_for_shutdown(&self, timeout: Duration) {
        let operation_id = {
            let operation = self.foreground_operation.borrow();
            let Some(operation) = operation.as_ref() else {
                return;
            };
            operation.cancel();
            operation.id
        };
        let pending_index = self.state.borrow().pending_publications.iter().find_map(
            |((source_id, generation), publication)| {
                (publication.operation_id == Some(operation_id))
                    .then(|| (source_id.clone(), *generation))
            },
        );
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let wait = remaining.min(Duration::from_millis(100));
            match self.receiver.borrow().recv_timeout(wait) {
                Ok(
                    Event::Extracted {
                        operation_id: completed,
                        ..
                    }
                    | Event::AttachmentCopied {
                        operation_id: completed,
                        ..
                    }
                    | Event::Loaded {
                        operation_id: Some(completed),
                        ..
                    }
                    | Event::BackupFallbackRequired {
                        operation_id: completed,
                        ..
                    },
                ) if completed == operation_id => break,
                Ok(Event::Indexed {
                    source_id,
                    generation,
                    ..
                }) if pending_index
                    .as_ref()
                    .is_some_and(|pending| pending.0 == source_id && pending.1 == generation) =>
                {
                    break;
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.finish_operation(operation_id);
    }

    fn hide_operation_activity_after(self: &Rc<Self>, operation_id: u64, delay: Duration) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            if let Some(viewer) = weak.upgrade() {
                viewer.finish_operation(operation_id);
            }
        });
    }

    fn zoom_changed(self: &Rc<Self>, zoom: f32) {
        update_zoom_label(&self.zoom_label, zoom);
        self.settings.borrow_mut().zoom = zoom;
        self.schedule_settings_save();
    }

    fn schedule_settings_save(self: &Rc<Self>) {
        self.cancel_settings_save();
        let weak = Rc::downgrade(self);
        *self.settings_save_timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(300),
            move || {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                viewer.settings_save_timer.borrow_mut().take();
                if let Err(error) = viewer.write_settings() {
                    viewer.show_error("Could not save zoom setting", &error.to_string());
                }
            },
        ));
    }

    fn cancel_settings_save(&self) {
        if let Some(timer) = self.settings_save_timer.borrow_mut().take() {
            timer.remove();
        }
    }

    fn write_settings(&self) -> Result<()> {
        settings::save(&self.settings_path, &self.settings.borrow())
    }

    fn flush_settings(&self) -> Result<()> {
        self.cancel_settings_save();
        self.write_settings()
    }

    fn set_navigation_width(&self, width: i32) {
        self.navigation_stack.set_width_request(width);
        self.content_paned.set_position(width);
    }

    fn set_busy(&self, message: &str) {
        let message = gtk_text(message);
        self.status.set_label(&message);
        self.spinner.start();
    }

    fn show_error(&self, title: &str, detail: &str) {
        self.spinner.stop();
        let title = gtk_text(title);
        let detail = gtk_text(detail);
        self.status.set_label(&title);

        let dialog = gtk::Window::builder()
            .title(title.as_ref())
            .transient_for(&self.window)
            .modal(true)
            .resizable(true)
            .default_width(640)
            .default_height(280)
            .build();
        dialog.add_css_class("error-dialog");

        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.set_margin_top(18);
        content.set_margin_bottom(18);

        let heading = gtk::Label::builder()
            .label(title.as_ref())
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .selectable(true)
            .build();
        heading.add_css_class("error-title");
        content.append(&heading);

        let detail_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(10)
            .right_margin(10)
            .top_margin(10)
            .bottom_margin(10)
            .build();
        detail_view.add_css_class("error-detail");
        detail_view.buffer().set_text(detail.as_ref());

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(140)
            .child(&detail_view)
            .build();
        scroller.add_css_class("error-detail-frame");
        content.append(&scroller);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let copy = gtk::Button::with_label("Copy error");
        let close = gtk::Button::with_label("Close");
        close.add_css_class("suggested-action");
        actions.append(&copy);
        actions.append(&close);
        content.append(&actions);

        let clipboard_text = format!("{title}\n\n{detail}");
        copy.connect_clicked(move |_| {
            gdk_display().clipboard().set_text(&clipboard_text);
        });
        let dialog_on_close = dialog.clone();
        close.connect_clicked(move |_| dialog_on_close.close());

        dialog.set_child(Some(&content));
        close.grab_focus();
        dialog.present();
    }

    fn show_backup_fallback_warning(self: &Rc<Self>, root: PathBuf, manifest_error: &str) {
        let dialog = gtk::Window::builder()
            .title("Notebook manifest could not be read")
            .transient_for(&self.window)
            .modal(true)
            .resizable(true)
            .default_width(640)
            .default_height(300)
            .build();
        dialog.add_css_class("error-dialog");

        let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
        content.set_margin_start(18);
        content.set_margin_end(18);
        content.set_margin_top(18);
        content.set_margin_bottom(18);
        let heading = gtk::Label::builder()
            .label("The folder contains a notebook table of contents, but it could not be read.")
            .wrap(true)
            .xalign(0.0)
            .selectable(true)
            .build();
        heading.add_css_class("error-title");
        let detail = gtk::Label::builder()
            .label(format!(
                "{manifest_error}\n\nOpening the folder as a backup reconstructs section groups and order from filenames and directories. The source files will not be changed."
            ))
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .yalign(0.0)
            .selectable(true)
            .vexpand(true)
            .build();
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Cancel");
        let open = gtk::Button::with_label("Open as Backup");
        open.add_css_class("suggested-action");
        actions.append(&cancel);
        actions.append(&open);
        content.append(&heading);
        content.append(&detail);
        content.append(&actions);
        dialog.set_child(Some(&content));

        let close_dialog = dialog.clone();
        cancel.connect_clicked(move |_| close_dialog.close());
        let weak = Rc::downgrade(self);
        let open_dialog = dialog.clone();
        open.connect_clicked(move |_| {
            open_dialog.close();
            if let Some(viewer) = weak.upgrade() {
                viewer.start_backup_load(
                    SourceDescriptor::backup(root.clone(), BackupSelectionPolicy::LatestPerSection),
                    "Opening OneNote backup folder",
                    onenote_core::RootManifestPolicy::Ignore,
                );
            }
        });
        dialog.present();
    }
}

fn extraction_phase_label(phase: ExtractionPhase) -> &'static str {
    match phase {
        ExtractionPhase::Inspecting => "Inspecting package...",
        ExtractionPhase::Testing => "Testing archive integrity...",
        ExtractionPhase::Extracting => "Extracting notebook files on disk...",
        ExtractionPhase::Verifying => "Verifying extracted notebook files...",
        ExtractionPhase::Publishing => "Publishing notebook folder...",
    }
}

fn attachment_progress_fraction(copied_bytes: u64, total_bytes: u64) -> f64 {
    let total = u32::try_from(total_bytes.min(crate::attachment::MAX_ATTACHMENT_BYTES))
        .unwrap_or(u32::MAX)
        .max(1);
    let copied = u32::try_from(copied_bytes.min(u64::from(total))).unwrap_or(total);
    f64::from(copied) / f64::from(total)
}

fn list_view(model: &gtk::StringList, css_class: &str) -> (gtk::SingleSelection, gtk::ListView) {
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    let list = gtk::ListView::builder()
        .model(&selection)
        .single_click_activate(false)
        .css_classes([css_class])
        .build();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        label.set_margin_start(12);
        label.set_margin_end(10);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| bind_string(item, false));
    list.set_factory(Some(&factory));
    (selection, list)
}

fn result_list(model: &gio::ListStore) -> (gtk::SingleSelection, gtk::ListView) {
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    let list = gtk::ListView::builder()
        .model(&selection)
        .single_click_activate(false)
        .css_classes(["result-list"])
        .build();
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        title.add_css_class("result-title");
        let path = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        path.add_css_class("result-path");
        let field = gtk::Label::builder().xalign(0.0).build();
        field.add_css_class("result-field");
        let snippet = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        snippet.add_css_class("result-snippet");
        let excerpt = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        excerpt.append(&field);
        excerpt.append(&snippet);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
        content.set_margin_start(14);
        content.set_margin_end(12);
        content.set_margin_top(9);
        content.set_margin_bottom(9);
        content.append(&title);
        content.append(&path);
        content.append(&excerpt);
        item.set_child(Some(&content));
    });
    factory.connect_bind(|_, item| bind_search_result(item));
    list.set_factory(Some(&factory));
    (selection, list)
}

fn bind_search_result(item: &glib::Object) {
    let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
    let Some(value) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
        return;
    };
    let content = item
        .child()
        .and_downcast::<gtk::Box>()
        .expect("result content");
    let title = content
        .first_child()
        .and_downcast::<gtk::Label>()
        .expect("result title");
    let path = title
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("result path");
    let excerpt = path
        .next_sibling()
        .and_downcast::<gtk::Box>()
        .expect("result excerpt");
    let field = excerpt
        .first_child()
        .and_downcast::<gtk::Label>()
        .expect("result field");
    let snippet = field
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("result snippet");
    let row = value.borrow::<SearchResultRow>();
    title.set_label(&row.title);
    path.set_label(&row.path);
    let field_text = matched_field_label(row.matched_field);
    field.set_label(field_text);
    field.set_visible(!field_text.is_empty());
    snippet.set_label(&row.snippet);
    let attributes = search_highlight_attributes(&row.snippet, &row.highlights);
    snippet.set_attributes(attributes.as_ref());
}

fn search_highlight_attributes(
    text: &str,
    highlights: &[TextRange],
) -> Option<gtk::pango::AttrList> {
    let attributes = gtk::pango::AttrList::new();
    let mut applied = false;
    for range in highlights {
        if range.start_byte >= range.end_byte
            || range.end_byte > text.len()
            || !text.is_char_boundary(range.start_byte)
            || !text.is_char_boundary(range.end_byte)
        {
            continue;
        }
        let (Ok(start), Ok(end)) = (
            u32::try_from(range.start_byte),
            u32::try_from(range.end_byte),
        ) else {
            continue;
        };
        let mut weight = gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold);
        weight.set_start_index(start);
        weight.set_end_index(end);
        attributes.insert(weight);
        let mut underline = gtk::pango::AttrInt::new_underline(gtk::pango::Underline::Single);
        underline.set_start_index(start);
        underline.set_end_index(end);
        attributes.insert(underline);
        applied = true;
    }
    applied.then_some(attributes)
}

fn page_find_title_attributes(
    matches: &[FindTextRange],
    active: Option<usize>,
    highlight_all: bool,
) -> Option<gtk::pango::AttrList> {
    let attributes = gtk::pango::AttrList::new();
    let mut applied = false;
    for (index, range) in matches.iter().enumerate() {
        if !highlight_all && Some(index) != active {
            continue;
        }
        let (Ok(start), Ok(end)) = (
            u32::try_from(range.start_byte),
            u32::try_from(range.end_byte),
        ) else {
            continue;
        };
        let (red, green, blue) = if Some(index) == active {
            (u16::MAX, 34_000, 4_000)
        } else {
            (u16::MAX, 52_000, 7_000)
        };
        let mut background = gtk::pango::AttrColor::new_background(red, green, blue);
        background.set_start_index(start);
        background.set_end_index(end);
        attributes.insert(background);
        applied = true;
    }
    applied.then_some(attributes)
}

fn selectable_page_header_label(css_class: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .selectable(true)
        .build();
    label.add_css_class(css_class);
    label
}

fn bind_string(item: &glib::Object, multiline: bool) {
    let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
    let string = item
        .item()
        .and_then(|item| item.downcast::<gtk::StringObject>().ok());
    let label = item
        .child()
        .and_then(|child| child.downcast::<gtk::Label>().ok());
    if let (Some(string), Some(label)) = (string, label) {
        label.set_label(&string.string());
        if multiline {
            label.add_css_class("result-row");
        }
    }
}

struct CollapsibleNavigationBand {
    root: gtk::Box,
    header: gtk::Box,
    heading: gtk::Label,
    body: gtk::ScrolledWindow,
    toggle: gtk::Button,
    header_action: Option<gtk::Button>,
    expanded_width: i32,
    collapsed: Rc<Cell<bool>>,
}

impl CollapsibleNavigationBand {
    fn new(
        title: &str,
        list: &gtk::ListView,
        expanded_width: i32,
        header_action: Option<&gtk::Button>,
        collapsed: bool,
    ) -> Self {
        let heading = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .build();
        heading.add_css_class("nav-heading");
        let toggle = icon_button(
            "onenote-panel-collapse-symbolic",
            &format!("Collapse {}", title.to_lowercase()),
        );
        toggle.add_css_class("flat");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.set_margin_start(12);
        header.set_margin_end(6);
        header.set_margin_top(6);
        header.set_margin_bottom(4);
        header.append(&heading);
        if let Some(action) = header_action {
            header.append(action);
        }
        header.append(&toggle);

        let body = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .child(list)
            .vexpand(true)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_width_request(expanded_width);
        root.add_css_class("navigation-band");
        root.append(&header);
        root.append(&body);
        let band = Self {
            root,
            header,
            heading,
            body,
            toggle,
            header_action: header_action.cloned(),
            expanded_width,
            collapsed: Rc::new(Cell::new(false)),
        };
        band.apply_collapsed(collapsed);
        band
    }

    fn width(&self) -> i32 {
        if self.collapsed.get() {
            COLLAPSED_NAVIGATION_WIDTH
        } else {
            self.expanded_width
        }
    }

    fn apply_collapsed(&self, collapsed: bool) {
        self.body.set_visible(!collapsed);
        self.heading.set_visible(!collapsed);
        if let Some(action) = &self.header_action {
            action.set_visible(!collapsed);
        }
        self.header.set_margin_start(if collapsed { 0 } else { 12 });
        self.header.set_margin_end(if collapsed { 0 } else { 6 });
        self.header.set_halign(if collapsed {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
        self.root.set_width_request(if collapsed {
            COLLAPSED_NAVIGATION_WIDTH
        } else {
            self.expanded_width
        });
        self.toggle.set_icon_name(if collapsed {
            "onenote-panel-expand-symbolic"
        } else {
            "onenote-panel-collapse-symbolic"
        });
        self.toggle.set_tooltip_text(Some(if collapsed {
            "Expand navigation"
        } else {
            "Collapse navigation"
        }));
        self.collapsed.set(collapsed);
    }

    fn connect_width_changed<F>(&self, changed: F)
    where
        F: Fn(i32) + 'static,
    {
        let root = self.root.clone();
        let header = self.header.clone();
        let heading = self.heading.clone();
        let body = self.body.clone();
        let header_action = self.header_action.clone();
        let expanded_width = self.expanded_width;
        let collapsed = Rc::clone(&self.collapsed);
        self.toggle.connect_clicked(move |button| {
            let collapse = !collapsed.get();
            body.set_visible(!collapse);
            heading.set_visible(!collapse);
            if let Some(action) = &header_action {
                action.set_visible(!collapse);
            }
            header.set_margin_start(if collapse { 0 } else { 12 });
            header.set_margin_end(if collapse { 0 } else { 6 });
            header.set_halign(if collapse {
                gtk::Align::Center
            } else {
                gtk::Align::Fill
            });
            let width = if collapse {
                COLLAPSED_NAVIGATION_WIDTH
            } else {
                expanded_width
            };
            root.set_width_request(width);
            if collapse {
                button.set_icon_name("onenote-panel-expand-symbolic");
                button.set_tooltip_text(Some("Expand navigation"));
            } else {
                button.set_icon_name("onenote-panel-collapse-symbolic");
                button.set_tooltip_text(Some("Collapse navigation"));
            }
            collapsed.set(collapse);
            changed(width);
        });
    }
}

fn connect_navigation_band_width(
    band: &CollapsibleNavigationBand,
    width: Rc<Cell<i32>>,
    other_width: Rc<Cell<i32>>,
    total_width: Rc<Cell<i32>>,
    stack: &gtk::Stack,
    paned: &gtk::Paned,
) {
    let stack = stack.clone();
    let paned = paned.clone();
    band.connect_width_changed(move |new_width| {
        width.set(new_width);
        let total = new_width + other_width.get() + NAVIGATION_SEPARATOR_WIDTH;
        total_width.set(total);
        stack.set_width_request(total);
        paned.set_position(total);
    });
}

fn navigation_band(title: &str, list: &gtk::ListView, width: i32) -> gtk::Box {
    let heading = gtk::Label::builder().label(title).xalign(0.0).build();
    heading.add_css_class("nav-heading");
    heading.set_margin_start(12);
    heading.set_margin_end(10);
    heading.set_margin_top(11);
    heading.set_margin_bottom(7);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(list)
        .vexpand(true)
        .build();
    let band = gtk::Box::new(gtk::Orientation::Vertical, 0);
    band.set_width_request(width);
    band.add_css_class("navigation-band");
    band.append(&heading);
    band.append(&scroll);
    band
}

fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("icon-button");
    button
}

fn separator() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Vertical)
}

fn separator_horizontal() -> gtk::Separator {
    gtk::Separator::new(gtk::Orientation::Horizontal)
}

fn format_zoom(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

fn update_zoom_label(label: &gtk::Label, zoom: f32) {
    label.set_label(&format_zoom(zoom));
}

fn clear_model(model: &gtk::StringList) {
    while model.n_items() > 0 {
        model.remove(model.n_items() - 1);
    }
}

fn append_model(model: &gtk::StringList, value: &str) {
    let value = gtk_text(value);
    model.append(&value);
}

fn scoped_search_query(
    scope: WorkspaceSearchScope,
    text: String,
    notebook: Option<&Notebook>,
    section_id: Option<&SectionId>,
) -> std::result::Result<SearchQuery, ()> {
    let mut query = SearchQuery::simple(text);
    if scope == WorkspaceSearchScope::AllNotebooks {
        return Ok(query);
    }
    let notebook = notebook.ok_or(())?;
    query.filters.source_ids = vec![notebook.source_id.clone()];
    match scope {
        WorkspaceSearchScope::AllNotebooks | WorkspaceSearchScope::Notebook => {}
        WorkspaceSearchScope::Section => {
            query.filters.section_ids = vec![section_id.ok_or(())?.clone()];
        }
        WorkspaceSearchScope::SectionGroup => {
            let group = notebook
                .section_group_ancestry(section_id.ok_or(())?)
                .and_then(|groups| groups.last().copied())
                .ok_or(())?;
            query.filters.section_ids =
                group.sections().map(|section| section.id.clone()).collect();
        }
    }
    Ok(query)
}

fn search_result_row(hit: &SearchHit) -> SearchResultRow {
    let original = hit.snippet.text.replace('\n', " ");
    let snippet = gtk_text(&original).into_owned();
    let highlights = if snippet == original {
        hit.snippet.highlights.clone()
    } else {
        Vec::new()
    };
    let path = if hit.path.trim().is_empty() {
        format!("{} / {}", hit.notebook_name, hit.section_name)
    } else {
        hit.path.clone()
    };
    SearchResultRow {
        title: gtk_text(&hit.page_title).into_owned(),
        path: gtk_text(&path).into_owned(),
        snippet,
        highlights,
        matched_field: hit.matched_field,
    }
}

const fn matched_field_label(field: MatchedField) -> &'static str {
    match field {
        MatchedField::AltText => "Image text:",
        MatchedField::InkText => "Handwriting:",
        MatchedField::Attachment => "Attachment:",
        MatchedField::Link => "Link:",
        MatchedField::Path => "Location:",
        MatchedField::Title | MatchedField::Body | MatchedField::Other => "",
    }
}

fn gtk_text(value: &str) -> Cow<'_, str> {
    if value.contains('\0') {
        Cow::Owned(value.replace('\0', "\u{fffd}"))
    } else {
        Cow::Borrowed(value)
    }
}

fn display_title(page: &Page) -> String {
    if page.title.trim().is_empty() {
        "Untitled page".to_owned()
    } else {
        page.title.clone()
    }
}

fn display_timestamp(value: &str) -> String {
    display_timestamp_in_timezone(value, &glib::TimeZone::local())
        .unwrap_or_else(|| display_unparsed_timestamp(value))
}

fn display_timestamp_in_timezone(value: &str, timezone: &glib::TimeZone) -> Option<String> {
    let timestamp =
        glib::DateTime::from_iso8601(&iso8601_timestamp(value)?, Some(&glib::TimeZone::utc()))
            .ok()?;
    timestamp
        .to_timezone(timezone)
        .and_then(|local| local.format("%Y-%m-%d  %H:%M"))
        .ok()
        .map(Into::into)
}

fn iso8601_timestamp(value: &str) -> Option<String> {
    let mut parts = value.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let offset = match parts.next() {
        None | Some("Z") => "Z".to_owned(),
        Some(offset) if is_hour_offset(offset) => format!("{offset}:00"),
        Some(offset) if is_minute_offset(offset) => offset.to_owned(),
        Some(offset) if is_zero_second_offset(offset) => offset[..6].to_owned(),
        Some(_) => return None,
    };
    (parts.next().is_none()).then(|| format!("{date}T{time}{offset}"))
}

fn is_hour_offset(value: &str) -> bool {
    value.len() == 3
        && matches!(value.as_bytes()[0], b'+' | b'-')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_digit)
}

fn is_minute_offset(value: &str) -> bool {
    value.len() == 6
        && matches!(value.as_bytes()[0], b'+' | b'-')
        && value.as_bytes()[1..3].iter().all(u8::is_ascii_digit)
        && value.as_bytes()[3] == b':'
        && value.as_bytes()[4..].iter().all(u8::is_ascii_digit)
}

fn is_zero_second_offset(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 9
        && matches!(bytes[0], b'+' | b'-')
        && bytes[1..3].iter().all(u8::is_ascii_digit)
        && bytes[3] == b':'
        && bytes[4..6].iter().all(u8::is_ascii_digit)
        && &bytes[6..] == b":00"
}

fn display_unparsed_timestamp(value: &str) -> String {
    let mut parts = value.split_whitespace();
    let Some(date) = parts.next() else {
        return String::new();
    };
    let Some(time) = parts.next() else {
        return date.to_owned();
    };
    let time = time.split('.').next().unwrap_or(time);
    let time = time.get(..5).unwrap_or(time);
    format!("{date}  {time}")
}

#[derive(Clone, Copy)]
enum EffectiveTheme {
    Light,
    Dark,
}

fn effective_theme(preference: ThemePreference) -> EffectiveTheme {
    match preference {
        ThemePreference::Light => EffectiveTheme::Light,
        ThemePreference::Dark => EffectiveTheme::Dark,
        ThemePreference::System => {
            if gtk::Settings::default()
                .is_some_and(|settings| settings.is_gtk_application_prefer_dark_theme())
            {
                EffectiveTheme::Dark
            } else {
                EffectiveTheme::Light
            }
        }
    }
}

fn theme_default_text_color(theme: EffectiveTheme) -> gtk::gdk::RGBA {
    match theme {
        EffectiveTheme::Light => gtk::gdk::RGBA::new(32.0 / 255.0, 33.0 / 255.0, 36.0 / 255.0, 1.0),
        EffectiveTheme::Dark => {
            gtk::gdk::RGBA::new(241.0 / 255.0, 243.0 / 255.0, 244.0 / 255.0, 1.0)
        }
    }
}

fn theme_colors(theme: EffectiveTheme) -> &'static str {
    match theme {
        EffectiveTheme::Light => {
            "
            @define-color app_bg #f7f7f8;
            @define-color surface #ffffff;
            @define-color navigation_bg #f3f3f5;
            @define-color text #202124;
            @define-color muted #666970;
            @define-color border #d9dadd;
            @define-color control_bg #ffffff;
            @define-color control_hover #e7e8eb;
            @define-color control_disabled #e1e2e5;
            @define-color disabled_text #92959c;
            @define-color accent #6b3a96;
            @define-color accent_hover #5b2d90;
            @define-color selected_bg #e8def3;
            @define-color selected_text #2d183d;
            @define-color page_accent #b35a24;
            @define-color activity_bg #f1eaf8;
            @define-color activity_border #d6c8e4;
            @define-color activity_text #5f5268;
            @define-color warning #9b2c1f;
            "
        }
        EffectiveTheme::Dark => {
            "
            @define-color app_bg #202124;
            @define-color surface #292a2d;
            @define-color navigation_bg #242529;
            @define-color text #f1f3f4;
            @define-color muted #b4b7bd;
            @define-color border #484a50;
            @define-color control_bg #34363a;
            @define-color control_hover #414349;
            @define-color control_disabled #292b2f;
            @define-color disabled_text #777a80;
            @define-color accent #a970d5;
            @define-color accent_hover #b982df;
            @define-color selected_bg #463454;
            @define-color selected_text #ffffff;
            @define-color page_accent #e08a52;
            @define-color activity_bg #33293d;
            @define-color activity_border #584467;
            @define-color activity_text #d4c4df;
            @define-color warning #ff8a78;
            "
        }
    }
}

#[allow(clippy::too_many_lines)]
fn theme_css(theme: EffectiveTheme) -> String {
    format!(
        "{}
        window, window:backdrop {{
            background: @app_bg;
            color: @text;
        }}
        headerbar, headerbar:backdrop {{
            background: @surface;
            color: @text;
            border-bottom: 1px solid @border;
        }}
        button, button:backdrop,
        dropdown > button, dropdown > button:backdrop {{
            background: @control_bg;
            color: @text;
            border: 1px solid @border;
            box-shadow: none;
        }}
        button:hover, dropdown > button:hover {{
            background: @control_hover;
        }}
        button:disabled, button:disabled:backdrop,
        dropdown > button:disabled, dropdown > button:disabled:backdrop {{
            background: @control_disabled;
            color: @disabled_text;
        }}
        button.suggested-action, button.suggested-action:backdrop {{
            background: @accent;
            color: white;
            border-color: @accent;
        }}
        button.suggested-action:hover {{
            background: @accent_hover;
            border-color: @accent_hover;
        }}
        button.suggested-action:disabled,
        button.suggested-action:disabled:backdrop {{
            background: @control_disabled;
            color: @disabled_text;
            border-color: @border;
        }}
        windowcontrols button, windowcontrols button:backdrop {{
            background: transparent;
            color: @text;
            border-color: transparent;
        }}
        windowcontrols button image, windowcontrols button image:backdrop {{
            color: @text;
        }}
        windowcontrols button:hover {{
            background: @control_hover;
        }}
        .icon-button, .icon-button:backdrop,
        .icon-button image, .icon-button image:backdrop {{
            background: @control_bg;
            color: @text;
        }}
        .icon-button:hover {{ background: @control_hover; }}
        .main-menu {{
            min-width: 36px;
        }}
        .search-scope {{
            min-width: 42px;
            min-height: 34px;
            padding-left: 6px;
            padding-right: 6px;
        }}
        .workspace-search entry {{
            min-width: 320px;
        }}
        .brand, .brand:backdrop {{
            font-size: 16px;
            font-weight: 700;
            color: @accent;
        }}
        searchentry, searchentry:backdrop,
        entry, entry:backdrop {{
            min-height: 34px;
            background: @control_bg;
            color: @text;
            border-color: @border;
        }}
        searchentry text, searchentry text:backdrop,
        entry text, entry text:backdrop {{
            background: transparent;
            color: @text;
        }}
        searchentry image, searchentry image:backdrop,
        entry image, entry image:backdrop {{
            color: @muted;
        }}
        popover contents, popover contents:backdrop {{
            background: @surface;
            color: @text;
            border: 1px solid @border;
        }}
        popover modelbutton, popover modelbutton:backdrop {{
            background: transparent;
            color: @text;
            border-color: transparent;
        }}
        popover modelbutton:hover {{ background: @control_hover; }}
        .navigation-band, .navigation-band:backdrop {{
            background: @navigation_bg;
            color: @text;
        }}
        .navigation-band label, .navigation-band label:backdrop,
        treeexpander, treeexpander:backdrop,
        treeexpander > expander, treeexpander > expander:backdrop {{
            color: @text;
        }}
        treeexpander > expander {{
            min-width: 16px;
            min-height: 16px;
            -gtk-icon-source: -gtk-icontheme(\"onenote-chevron-right-symbolic\");
        }}
        treeexpander > expander:checked {{
            -gtk-icon-source: -gtk-icontheme(\"onenote-chevron-down-symbolic\");
        }}
        .nav-heading, .nav-heading:backdrop {{
            font-size: 11px;
            font-weight: 700;
            color: @muted;
        }}
        listview, listview:backdrop {{
            background: transparent;
            color: @text;
        }}
        listview row, listview row:backdrop {{
            border-radius: 4px;
            margin: 1px 6px;
            color: @text;
        }}
        listview row:hover, listview row:hover:backdrop {{
            background: transparent;
            color: @text;
        }}
        listview row:selected, listview row:selected:backdrop,
        listview row:selected:hover, listview row:selected:hover:backdrop {{
            background: @selected_bg;
            color: @selected_text;
        }}
        .notebook-row {{ font-weight: 600; }}
        .group-row {{ font-weight: 500; }}
        .notebook-tree row:selected {{ border-left: 3px solid @accent; }}
        .page-list row:selected {{ border-left: 3px solid @page_accent; }}
        .result-title {{
            font-weight: 650;
            color: @text;
        }}
        .result-path, .result-field {{
            font-size: 12px;
            color: @muted;
        }}
        .result-field {{ font-weight: 650; }}
        .result-snippet {{
            line-height: 1.2;
            color: @text;
        }}
        .page-find-bar {{
            background-color: @navigation_bg;
            border-top: 1px solid @border;
            color: @text;
            font-size: 12px;
        }}
        .page-find-count, .page-find-count:backdrop {{
            min-width: 60px;
            color: @muted;
        }}
        .page-find-option, .page-find-option:backdrop {{
            color: @text;
        }}
        .page-title, .page-title:backdrop {{
            font-size: 20px;
            font-weight: 650;
            color: @text;
        }}
        .page-date, .page-date:backdrop,
        .page-context, .page-context:backdrop,
        .status, .status:backdrop {{
            font-size: 12px;
            color: @muted;
        }}
        .empty-title, .empty-title:backdrop {{
            font-size: 20px;
            font-weight: 600;
            color: @text;
        }}
        .empty-icon, .empty-icon:backdrop {{ color: @muted; }}
        .operation-activity, .operation-activity:backdrop {{
            background: @activity_bg;
            color: @text;
            border-bottom: 1px solid @activity_border;
        }}
        .activity-title, .activity-title:backdrop {{
            font-size: 14px;
            font-weight: 650;
            color: @selected_text;
        }}
        .activity-phase, .activity-phase:backdrop {{
            font-size: 12px;
            color: @activity_text;
        }}
        .settings-dialog, .settings-dialog:backdrop,
        .error-dialog, .error-dialog:backdrop {{
            background: @app_bg;
            color: @text;
        }}
        .settings-dialog label, .settings-dialog label:backdrop,
        .error-dialog label, .error-dialog label:backdrop,
        .link-dialog label, .link-dialog label:backdrop {{
            color: @text;
        }}
        .dialog-title, .dialog-title:backdrop,
        .error-title, .error-title:backdrop {{
            font-size: 18px;
            font-weight: 700;
            color: @text;
        }}
        .field-label, .field-label:backdrop {{
            font-size: 12px;
            font-weight: 700;
            color: @muted;
        }}
        .dim-label, .dim-label:backdrop {{
            color: @muted;
        }}
        .path-value, .path-value:backdrop {{
            background: @surface;
            color: @text;
            border: 1px solid @border;
            border-radius: 4px;
            padding: 10px;
        }}
        .link-destination, .link-destination:backdrop {{
            background: @surface;
            color: @text;
            border: 1px solid @border;
            border-radius: 4px;
            padding: 10px;
        }}
        label selection, label selection:backdrop {{
            background: @accent;
            color: white;
        }}
        .warning-label, .warning-label:backdrop {{
            color: @warning;
            font-weight: 600;
        }}
        .error-detail-frame {{
            border: 1px solid @border;
            border-radius: 4px;
        }}
        .error-detail, .error-detail:backdrop,
        .error-detail text, .error-detail text:backdrop {{
            background: @surface;
            color: @text;
            caret-color: @text;
        }}
        .error-detail text selection,
        .error-detail text selection:backdrop {{
            background: @accent;
            color: white;
        }}
        ",
        theme_colors(theme)
    )
}

fn uri_scheme(target: &str) -> Option<String> {
    let (scheme, _) = target.split_once(':')?;
    let mut characters = scheme.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        });
    valid.then(|| scheme.to_ascii_lowercase())
}

fn is_local_link(target: &str) -> bool {
    uri_scheme(target).as_deref() == Some("file") || is_plain_local_path(target)
}

fn is_plain_local_path(target: &str) -> bool {
    target.starts_with('/')
        || target.starts_with('\\')
        || target
            .as_bytes()
            .get(1)
            .is_some_and(|character| *character == b':')
}

fn onenote_page_id(target: &str) -> Option<String> {
    let lower = target.to_ascii_lowercase();
    let start = lower.find("page-id=")? + "page-id=".len();
    let raw = target[start..].split('&').next()?.trim();
    let decoded = glib::uri_unescape_string(raw, None::<&str>)?;
    let value = decoded.trim().trim_matches(['{', '}']);
    (!value.is_empty()).then(|| value.to_owned())
}

fn native_ids_equal(left: &str, right: &str) -> bool {
    left.trim()
        .trim_matches(['{', '}'])
        .eq_ignore_ascii_case(right.trim().trim_matches(['{', '}']))
}

fn same_source_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn page_location_exists(sources: &[Source], location: &PageLocation) -> bool {
    sources
        .iter()
        .find(|source| source.loaded.notebook.source_id == location.source)
        .and_then(|source| source.loaded.notebook.section(&location.section))
        .is_some_and(|section| section.pages.iter().any(|page| page.id == location.page))
}

fn restore_location_for_source(
    target: Option<&PersistedPageLocation>,
    notebook: &Notebook,
) -> Option<SectionLocation> {
    let target = target.filter(|target| target.source_id == notebook.source_id)?;
    if notebook
        .section(&target.section_id)
        .is_some_and(|section| section.pages.iter().any(|page| page.id == target.page_id))
    {
        return Some(SectionLocation {
            section_id: target.section_id.clone(),
            page_id: Some(target.page_id.clone()),
        });
    }
    notebook.sections().find_map(|section| {
        section
            .pages
            .iter()
            .any(|page| page.id == target.page_id)
            .then(|| SectionLocation {
                section_id: section.id.clone(),
                page_id: Some(target.page_id.clone()),
            })
    })
}

fn restore_missing_from_discovery(
    target: Option<&PersistedPageLocation>,
    requested: &std::path::Path,
    discovered: &[PathBuf],
) -> bool {
    target.is_some_and(|target| {
        workspace::source_is_in_location(&target.source_path, requested)
            && !discovered
                .iter()
                .any(|path| same_source_path(path, &target.source_path))
    })
}

fn tree_state_missing_from_discovery(
    state: &PersistedSourceTreeState,
    requested: &std::path::Path,
    discovered: &[PathBuf],
) -> bool {
    workspace::source_is_in_location(&state.source_path, requested)
        && !discovered
            .iter()
            .any(|path| same_source_path(path, &state.source_path))
}

fn source_tree_expansion(state: PersistedSourceTreeState) -> SourceTreeExpansion {
    SourceTreeExpansion {
        notebook_expanded: state.notebook_expanded,
        expanded_groups: state.expanded_groups.into_iter().collect(),
    }
}

fn load_options(settings: &AppSettings) -> LoadOptions {
    LoadOptions {
        detect_plain_text_links: settings.detect_plain_text_links,
    }
}

fn index_profile(settings: &AppSettings) -> IndexProfile {
    IndexProfile::new(format!(
        "detect-plain-text-links={}",
        settings.detect_plain_text_links
    ))
}

fn prioritize_discovered_sources(
    sources: &mut Vec<SourceDescriptor>,
    target: Option<&PersistedPageLocation>,
) {
    let Some(target) = target else {
        return;
    };
    move_matching_source_first(sources, |source| {
        same_source_path(source.path(), &target.source_path)
    });
}

fn prioritize_source_roots(
    sources: &mut Vec<SourceDescriptor>,
    target: Option<&PersistedPageLocation>,
) {
    let Some(target) = target else {
        return;
    };
    move_matching_source_first(sources, |source| {
        workspace::source_is_in_location(&target.source_path, source.path())
    });
}

fn move_matching_source_first<T>(sources: &mut Vec<T>, predicate: impl Fn(&T) -> bool) {
    let Some(position) = sources.iter().position(predicate) else {
        return;
    };
    if position != 0 {
        let source = sources.remove(position);
        sources.insert(0, source);
    }
}

fn register_configured_source(
    configured: &mut Vec<SourceDescriptor>,
    descriptor: &SourceDescriptor,
    notebooks_location: &std::path::Path,
    previous_path: Option<&std::path::Path>,
) {
    if workspace::source_is_in_location(descriptor.path(), notebooks_location)
        && !descriptor.is_backup()
    {
        if let Some(previous_path) = previous_path {
            configured.retain(|source| !same_source_path(source.path(), previous_path));
        }
        return;
    }
    if let Some(existing) = configured.iter_mut().find(|source| {
        same_source_path(source.path(), descriptor.path())
            || previous_path.is_some_and(|path| same_source_path(source.path(), path))
    }) {
        *existing = descriptor.clone();
    } else {
        configured.push(descriptor.clone());
    }
}

fn apply_source_overrides(discovered: &mut [SourceDescriptor], configured: &[SourceDescriptor]) {
    for source in discovered {
        let Some(override_source) = configured.iter().find(|configured| {
            configured.is_backup()
                && source.is_backup()
                && same_source_path(configured.path(), source.path())
        }) else {
            continue;
        };
        *source = override_source.clone();
    }
}

const fn backup_progress_label(phase: BackupProgressPhase) -> &'static str {
    match phase {
        BackupProgressPhase::Classifying => "Checking backup folder",
        BackupProgressPhase::Discovering => "Discovering backup sections",
        BackupProgressPhase::Grouping => "Grouping backup snapshots",
        BackupProgressPhase::Selecting => "Selecting backup snapshots",
        BackupProgressPhase::Parsing => "Reading backup sections",
        BackupProgressPhase::Assembling => "Assembling reconstructed notebook",
        BackupProgressPhase::Verifying => "Verifying backup snapshot",
    }
}

fn install_resources(theme: ThemePreference) -> gtk::CssProvider {
    let display = gdk_display();
    gtk::IconTheme::for_display(&display).add_resource_path("/io/github/emsi/OneNoteViewer/icons");
    gtk::Window::set_default_icon_name(APP_ICON_NAME);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&theme_css(effective_theme(theme)));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    provider
}

fn gdk_display() -> gtk::gdk::Display {
    gtk::gdk::Display::default().expect("GTK display")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtk_text_replaces_interior_nuls() {
        assert_eq!(gtk_text("One\0Note"), "One�Note");
        assert_eq!(gtk_text("OneNote"), "OneNote");
    }

    #[test]
    fn classifies_link_schemes_and_local_targets() {
        assert_eq!(uri_scheme("HTTPS://example.test"), Some("https".to_owned()));
        assert_eq!(uri_scheme("not a link"), None);
        assert!(is_local_link("file:///tmp/note.txt"));
        assert!(is_local_link("FILE:///tmp/note.txt"));
        assert!(is_plain_local_path("/tmp/note.txt"));
        assert!(is_local_link("C:\\Notes\\note.txt"));
        assert!(!is_plain_local_path("file:///tmp/note.txt"));
        assert!(!is_local_link("https://example.test"));
    }

    #[test]
    fn extracts_percent_encoded_onenote_page_ids() {
        assert_eq!(
            onenote_page_id("onenote:#Page&page-id=%7BABC-123%7D&end"),
            Some("ABC-123".to_owned())
        );
        assert!(native_ids_equal("{abc-123}", "ABC-123"));
        assert_eq!(onenote_page_id("onenote:#Page&section-id={abc}"), None);
    }

    #[test]
    fn viewer_link_detection_preference_controls_loader_enrichment() {
        let mut settings = AppSettings::default();
        assert!(load_options(&settings).detect_plain_text_links);
        let enabled_profile = index_profile(&settings);

        settings.detect_plain_text_links = false;
        assert!(!load_options(&settings).detect_plain_text_links);
        assert_ne!(index_profile(&settings), enabled_profile);
    }

    #[test]
    fn startup_priority_does_not_change_notebook_presentation_order() {
        let target = persisted_page("wanted", "/notes/Wanted/Open Notebook.onetoc2");
        let presentation_order = vec![
            SourceDescriptor::native("/notes/First/Open Notebook.onetoc2"),
            SourceDescriptor::native("/notes/Wanted/Open Notebook.onetoc2"),
            SourceDescriptor::native("/notes/Last/Open Notebook.onetoc2"),
        ];
        let order = SourceDisplayOrder::new(
            presentation_order
                .iter()
                .map(|source| source.path().to_path_buf()),
        );
        let mut discovered = presentation_order.clone();
        prioritize_discovered_sources(&mut discovered, Some(&target));
        assert_eq!(
            discovered,
            vec![
                SourceDescriptor::native("/notes/Wanted/Open Notebook.onetoc2"),
                SourceDescriptor::native("/notes/First/Open Notebook.onetoc2"),
                SourceDescriptor::native("/notes/Last/Open Notebook.onetoc2"),
            ]
        );
        let mut visible = Vec::new();
        for source in discovered {
            let position =
                order.insertion_index(source.path(), visible.iter().map(SourceDescriptor::path));
            visible.insert(position, source);
        }
        assert_eq!(visible, presentation_order);

        let mut roots = vec![
            SourceDescriptor::native("/archive"),
            SourceDescriptor::native("/notes"),
        ];
        prioritize_source_roots(&mut roots, Some(&target));
        assert_eq!(
            roots,
            vec![
                SourceDescriptor::native("/notes"),
                SourceDescriptor::native("/archive")
            ]
        );
    }

    #[test]
    fn persisted_backup_policy_overrides_default_library_discovery() {
        let root = PathBuf::from("/notes/Backup");
        let mut discovered = vec![SourceDescriptor::backup(
            &root,
            BackupSelectionPolicy::LatestPerSection,
        )];
        let configured = vec![SourceDescriptor::backup(
            &root,
            BackupSelectionPolicy::AllCopies,
        )];

        apply_source_overrides(&mut discovered, &configured);

        assert_eq!(discovered, configured);
    }

    #[test]
    fn default_location_keeps_only_backup_policy_overrides() {
        let default = std::path::Path::new("/notes");
        let mut configured = Vec::new();
        register_configured_source(
            &mut configured,
            &SourceDescriptor::native("/notes/Native/Open Notebook.onetoc2"),
            default,
            None,
        );
        register_configured_source(
            &mut configured,
            &SourceDescriptor::backup("/notes/Backup", BackupSelectionPolicy::AllCopies),
            default,
            None,
        );

        assert_eq!(
            configured,
            vec![SourceDescriptor::backup(
                "/notes/Backup",
                BackupSelectionPolicy::AllCopies
            )]
        );
    }

    #[test]
    fn canonicalized_backup_path_replaces_the_original_workspace_entry() {
        let mut configured = vec![SourceDescriptor::backup(
            "/alias/Backup",
            BackupSelectionPolicy::LatestPerSection,
        )];
        register_configured_source(
            &mut configured,
            &SourceDescriptor::backup("/canonical/Backup", BackupSelectionPolicy::LatestPerSection),
            std::path::Path::new("/notes"),
            Some(std::path::Path::new("/alias/Backup")),
        );

        assert_eq!(
            configured,
            vec![SourceDescriptor::backup(
                "/canonical/Backup",
                BackupSelectionPolicy::LatestPerSection
            )]
        );
    }

    #[test]
    fn index_activity_rejects_superseded_and_removed_generations() {
        let source = SourceId::new("source");
        let mut activity = IndexActivity::default();
        let (first_generation, first_cancel) = activity.begin(source.clone());
        let (second_generation, second_cancel) = activity.begin(source.clone());

        assert!(first_cancel.load(Ordering::Acquire));
        assert!(!second_cancel.load(Ordering::Acquire));
        assert!(!activity.take_current(&source, first_generation));
        assert!(activity.take_current(&source, second_generation));

        let (_, removed_cancel) = activity.begin(source.clone());
        activity.cancel_source(&source);
        assert!(removed_cancel.load(Ordering::Acquire));
        assert!(activity.jobs.is_empty());
    }

    #[test]
    fn index_activity_cancels_every_pending_job_at_shutdown() {
        let mut activity = IndexActivity::default();
        let (_, first) = activity.begin(SourceId::new("first"));
        let (_, second) = activity.begin(SourceId::new("second"));

        activity.cancel_all();

        assert!(first.load(Ordering::Acquire));
        assert!(second.load(Ordering::Acquire));
        assert!(activity.jobs.is_empty());
    }

    #[test]
    fn backup_cancellation_carries_from_loading_into_indexing() {
        let cancellation = BackupOperationCancellation::new(PathBuf::from("/backup"));
        cancellation.cancel();
        let index = Arc::new(AtomicBool::new(false));

        cancellation.set_index_cancel(index.clone());

        assert!(index.load(Ordering::Acquire));
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn workspace_search_scopes_resolve_to_stable_source_and_section_ids() {
        let nested = SectionId::new("nested");
        let sibling = SectionId::new("sibling");
        let root = SectionId::new("root");
        let notebook = Notebook {
            source_id: SourceId::new("source"),
            fingerprint: onenote_core::SourceFingerprint::new("fingerprint"),
            name: "Notebook".to_owned(),
            color: None,
            entries: vec![
                onenote_core::NotebookEntry::Group(onenote_core::SectionGroup {
                    id: SectionId::new("group"),
                    name: "Group".to_owned(),
                    entries: vec![
                        onenote_core::NotebookEntry::Section(empty_section(nested.clone())),
                        onenote_core::NotebookEntry::Section(empty_section(sibling.clone())),
                    ],
                }),
                onenote_core::NotebookEntry::Section(empty_section(root)),
            ],
            diagnostics: Vec::new(),
        };

        let all = scoped_search_query(
            WorkspaceSearchScope::AllNotebooks,
            "term".to_owned(),
            None,
            None,
        )
        .expect("all notebooks");
        assert!(all.filters.source_ids.is_empty());
        assert!(all.filters.section_ids.is_empty());

        let notebook_query = scoped_search_query(
            WorkspaceSearchScope::Notebook,
            "term".to_owned(),
            Some(&notebook),
            Some(&nested),
        )
        .expect("notebook");
        assert_eq!(
            notebook_query.filters.source_ids,
            vec![notebook.source_id.clone()]
        );
        assert!(notebook_query.filters.section_ids.is_empty());

        let section = scoped_search_query(
            WorkspaceSearchScope::Section,
            "term".to_owned(),
            Some(&notebook),
            Some(&nested),
        )
        .expect("section");
        assert_eq!(section.filters.section_ids, vec![nested.clone()]);

        let group = scoped_search_query(
            WorkspaceSearchScope::SectionGroup,
            "term".to_owned(),
            Some(&notebook),
            Some(&nested),
        )
        .expect("section group");
        assert_eq!(group.filters.section_ids, vec![nested, sibling]);
    }

    fn empty_section(id: SectionId) -> onenote_core::Section {
        onenote_core::Section {
            id,
            name: "Section".to_owned(),
            color: None,
            pages: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn startup_restore_matches_stable_source_identity() {
        let target = persisted_page("wanted", "/notes/Wanted.onetoc2");
        let wanted = restore_notebook("wanted", "section", "page");
        let other = restore_notebook("other", "section", "page");

        assert_eq!(
            restore_location_for_source(Some(&target), &wanted),
            Some(SectionLocation {
                section_id: SectionId::new("section"),
                page_id: Some(PageId::new("page")),
            })
        );
        assert_eq!(restore_location_for_source(Some(&target), &other), None);
    }

    #[test]
    fn startup_restore_follows_a_stable_page_when_section_identity_changes() {
        let target = persisted_page("source", "/notes/Notebook.onetoc2");
        let notebook = restore_notebook("source", "new-section", "page");

        assert_eq!(
            restore_location_for_source(Some(&target), &notebook),
            Some(SectionLocation {
                section_id: SectionId::new("new-section"),
                page_id: Some(PageId::new("page")),
            })
        );
    }

    #[test]
    fn startup_restore_is_discarded_only_after_its_scope_omits_the_source() {
        let target = persisted_page("wanted", "/notes/Wanted/Open Notebook.onetoc2");
        let discovered = vec![PathBuf::from("/notes/Wanted/Open Notebook.onetoc2")];

        assert!(!restore_missing_from_discovery(
            Some(&target),
            std::path::Path::new("/notes"),
            &discovered,
        ));
        assert!(restore_missing_from_discovery(
            Some(&target),
            std::path::Path::new("/notes"),
            &[PathBuf::from("/notes/Other/Open Notebook.onetoc2")],
        ));
        assert!(!restore_missing_from_discovery(
            Some(&target),
            std::path::Path::new("/unrelated"),
            &[],
        ));
    }

    #[test]
    fn pending_tree_restore_is_pruned_only_by_its_discovery_scope() {
        let state = PersistedSourceTreeState {
            source_path: PathBuf::from("/notes/Wanted/Open Notebook.onetoc2"),
            source_id: SourceId::new("wanted"),
            notebook_expanded: false,
            expanded_groups: vec![SectionId::new("group")],
        };
        let discovered = vec![PathBuf::from("/notes/Wanted/Open Notebook.onetoc2")];

        assert!(!tree_state_missing_from_discovery(
            &state,
            std::path::Path::new("/notes"),
            &discovered,
        ));
        assert!(tree_state_missing_from_discovery(
            &state,
            std::path::Path::new("/notes"),
            &[PathBuf::from("/notes/Other/Open Notebook.onetoc2")],
        ));
        assert!(!tree_state_missing_from_discovery(
            &state,
            std::path::Path::new("/unrelated"),
            &[],
        ));
    }

    fn persisted_page(source_id: &str, source_path: &str) -> PersistedPageLocation {
        PersistedPageLocation {
            source_path: PathBuf::from(source_path),
            source_id: SourceId::new(source_id),
            section_id: SectionId::new("section"),
            page_id: PageId::new("page"),
        }
    }

    fn restore_notebook(source_id: &str, section_id: &str, page_id: &str) -> Notebook {
        let mut section = empty_section(SectionId::new(section_id));
        section.pages.push(Page {
            id: PageId::new(page_id),
            native_id: String::new(),
            title: String::new(),
            level: 0,
            created_at: String::new(),
            updated_at: String::new(),
            author: None,
            height: None,
            objects: Vec::new(),
        });
        Notebook {
            source_id: SourceId::new(source_id),
            fingerprint: onenote_core::SourceFingerprint::new("fingerprint"),
            name: "Notebook".to_owned(),
            color: None,
            entries: vec![onenote_core::NotebookEntry::Section(section)],
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn renderer_zoom_changes_update_the_footer_once() {
        crate::test_support::run_gtk_test(renderer_zoom_changes_update_the_footer_once_gtk);
    }

    fn renderer_zoom_changes_update_the_footer_once_gtk() {
        let view = PageView::new();
        let label = gtk::Label::new(Some(&format_zoom(view.zoom())));
        let notifications = Rc::new(Cell::new(0_u32));
        let callback_label = label.clone();
        let callback_notifications = Rc::clone(&notifications);
        let _handler = view.connect_zoom_changed(move |zoom| {
            update_zoom_label(&callback_label, zoom);
            callback_notifications.set(callback_notifications.get() + 1);
        });

        view.set_zoom(1.21);
        assert_eq!(label.label(), "121%");
        assert_eq!(notifications.get(), 1);

        view.set_zoom(1.21);
        assert_eq!(notifications.get(), 1);

        view.set_zoom(10.0);
        assert_eq!(label.label(), "400%");
        assert_eq!(notifications.get(), 2);
    }

    #[test]
    fn page_header_label_copies_complete_ellipsized_unicode_text() {
        crate::test_support::run_gtk_test(
            page_header_label_copies_complete_ellipsized_unicode_text_gtk,
        );
    }

    fn page_header_label_copies_complete_ellipsized_unicode_text_gtk() {
        let text = "A long Unicode page title: Matematyka, Ελληνικά, 日本語, 😀";
        let label = selectable_page_header_label("page-title");
        label.set_label(text);
        assert!(label.is_selectable());
        assert!(label.is_focusable());
        assert_eq!(label.ellipsize(), gtk::pango::EllipsizeMode::End);

        let window = gtk::Window::builder()
            .default_width(180)
            .default_height(48)
            .child(&label)
            .build();
        window.present();
        while glib::MainContext::default().iteration(false) {}

        assert!(label.grab_focus());
        label
            .activate_action("selection.select-all", None)
            .expect("select-all action");
        assert_eq!(
            label.selection_bounds(),
            Some((
                0,
                i32::try_from(text.chars().count()).expect("test text length")
            ))
        );
        label
            .activate_action("clipboard.copy", None)
            .expect("copy action");

        let clipboard = label.clipboard();
        let copied = glib::MainContext::default()
            .block_on(clipboard.read_text_future())
            .expect("read clipboard")
            .expect("clipboard text");
        assert_eq!(copied, text);

        label.set_label("Next page");
        assert_eq!(label.selection_bounds(), None);
        window.close();
    }

    #[test]
    fn both_themes_define_complete_control_and_selection_states() {
        for theme in [EffectiveTheme::Light, EffectiveTheme::Dark] {
            let css = theme_css(theme);
            for required in [
                "@define-color control_bg",
                "@define-color control_hover",
                "@define-color control_disabled",
                "button:backdrop",
                "button:disabled",
                "button.suggested-action",
                "label selection",
                ".error-detail text selection",
                ".page-find-option:backdrop",
            ] {
                assert!(css.contains(required), "theme CSS is missing {required}");
            }
        }
    }

    #[test]
    fn page_timestamp_uses_timezone_offset_for_the_note_date() {
        #[allow(deprecated)]
        let warsaw = glib::TimeZone::new(Some("Europe/Warsaw"));

        assert_eq!(
            display_timestamp_in_timezone("2025-11-18 12:54:27.0 +00", &warsaw).as_deref(),
            Some("2025-11-18  13:54")
        );
        assert_eq!(
            display_timestamp_in_timezone("2025-07-18 12:54:27.0 +00:00:00", &warsaw).as_deref(),
            Some("2025-07-18  14:54")
        );
    }

    #[test]
    fn page_timestamp_falls_back_for_unparsed_values() {
        assert_eq!(display_timestamp("unknown"), "unknown");
    }

    #[test]
    fn collapsed_bands_reclaim_the_outer_pane_width() {
        crate::test_support::run_gtk_test(collapsed_bands_reclaim_the_outer_pane_width_gtk);
    }

    fn collapsed_bands_reclaim_the_outer_pane_width_gtk() {
        let (_, notebook_list) = list_view(&gtk::StringList::new(&[]), "notebook-list");
        let (_, page_list) = list_view(&gtk::StringList::new(&[]), "page-list");
        assert!(!notebook_list.is_single_click_activate());
        assert!(!page_list.is_single_click_activate());
        let close_source = gtk::Button::new();
        let notebooks = CollapsibleNavigationBand::new(
            "NOTEBOOKS",
            &notebook_list,
            NOTEBOOK_NAVIGATION_WIDTH,
            Some(&close_source),
            false,
        );
        let pages =
            CollapsibleNavigationBand::new("PAGES", &page_list, PAGE_NAVIGATION_WIDTH, None, false);
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        navigation.append(&notebooks.root);
        navigation.append(&separator());
        navigation.append(&pages.root);
        let stack = gtk::Stack::new();
        stack.add_named(&navigation, Some("pages"));
        let canvas = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let initial_width =
            NOTEBOOK_NAVIGATION_WIDTH + PAGE_NAVIGATION_WIDTH + NAVIGATION_SEPARATOR_WIDTH;
        let paned = gtk::Paned::builder()
            .start_child(&stack)
            .end_child(&canvas)
            .position(initial_width)
            .build();
        let notebook_width = Rc::new(Cell::new(NOTEBOOK_NAVIGATION_WIDTH));
        let page_width = Rc::new(Cell::new(PAGE_NAVIGATION_WIDTH));
        let total_width = Rc::new(Cell::new(initial_width));
        connect_navigation_band_width(
            &notebooks,
            Rc::clone(&notebook_width),
            Rc::clone(&page_width),
            Rc::clone(&total_width),
            &stack,
            &paned,
        );
        connect_navigation_band_width(
            &pages,
            Rc::clone(&page_width),
            Rc::clone(&notebook_width),
            Rc::clone(&total_width),
            &stack,
            &paned,
        );

        notebooks.toggle.emit_clicked();
        assert!(!close_source.is_visible());
        assert_eq!(
            total_width.get(),
            COLLAPSED_NAVIGATION_WIDTH + PAGE_NAVIGATION_WIDTH + NAVIGATION_SEPARATOR_WIDTH
        );
        pages.toggle.emit_clicked();
        let (page_minimum, _, _, _) = pages.root.measure(gtk::Orientation::Horizontal, -1);
        assert!(
            page_minimum <= COLLAPSED_NAVIGATION_WIDTH,
            "collapsed pages require {page_minimum}px but receive {COLLAPSED_NAVIGATION_WIDTH}px"
        );
        assert_eq!(
            total_width.get(),
            COLLAPSED_NAVIGATION_WIDTH * 2 + NAVIGATION_SEPARATOR_WIDTH
        );
        assert_eq!(stack.width_request(), total_width.get());
        assert_eq!(paned.position(), total_width.get());
    }

    #[test]
    fn restored_bands_apply_all_initial_collapse_combinations() {
        crate::test_support::run_gtk_test(
            restored_bands_apply_all_initial_collapse_combinations_gtk,
        );
    }

    fn restored_bands_apply_all_initial_collapse_combinations_gtk() {
        for collapsed in [false, true] {
            let (_, list) = list_view(&gtk::StringList::new(&[]), "page-list");
            let band = CollapsibleNavigationBand::new(
                "PAGES",
                &list,
                PAGE_NAVIGATION_WIDTH,
                None,
                collapsed,
            );

            assert_eq!(band.collapsed.get(), collapsed);
            assert_eq!(band.body.is_visible(), !collapsed);
            assert_eq!(
                band.width(),
                if collapsed {
                    COLLAPSED_NAVIGATION_WIDTH
                } else {
                    PAGE_NAVIGATION_WIDTH
                }
            );
        }
    }
}

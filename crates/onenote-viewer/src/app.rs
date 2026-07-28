use crate::navigation::{NavigationTarget, NotebookTree};
use crate::settings::{self, AppSettings};
use crate::worker::{self, Command, Event};
use crate::workspace::{self, WorkspaceConfig};
use anyhow::Result;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{
    ExtractionPhase, LoadedNotebook, NotebookEntry, Page, PageId, Rect, Section, SectionId,
};
use onenote_index::SearchHit;
use onenote_render_gtk::PageView;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
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
const SYMBOLIC_ICON_NAMES: [&str; 14] = [
    "onenote-chevron-down-symbolic",
    "onenote-chevron-right-symbolic",
    "onenote-close-symbolic",
    "onenote-folder-symbolic",
    "onenote-import-package-symbolic",
    "onenote-notebook-symbolic",
    "onenote-open-file-symbolic",
    "onenote-open-folder-symbolic",
    "onenote-panel-collapse-symbolic",
    "onenote-panel-expand-symbolic",
    "onenote-settings-symbolic",
    "onenote-zoom-in-symbolic",
    "onenote-zoom-out-symbolic",
    "onenote-zoom-reset-symbolic",
];

pub(crate) fn run(requested_sources: Vec<PathBuf>) -> Result<()> {
    register_resources()?;
    let (workspace_path, index_path) = workspace::paths()?;
    workspace::ensure_index_parent(&index_path)?;
    let persisted = workspace::load(&workspace_path).unwrap_or_default();
    let settings_path = settings::path();
    let persisted_settings = settings::load(&settings_path).unwrap_or_default();
    let notebooks_location = persisted_settings.notebooks_location.clone();
    let initial_sources: Vec<_> = if requested_sources.is_empty() {
        persisted.sources
    } else {
        requested_sources
    }
    .into_iter()
    .filter(|source| !workspace::source_is_in_location(source, &notebooks_location))
    .collect();

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
        install_resources();
        let instance = Viewer::new(
            application,
            workspace_path.clone(),
            index_path.clone(),
            settings_path.clone(),
            persisted_settings.clone(),
        );
        instance.open_notebooks_location();
        for source in &initial_sources {
            instance.discover(source.clone());
        }
        instance.window.present();
        if let Some(delay) = smoke_quit_delay() {
            let application = application.clone();
            glib::timeout_add_local_once(delay, move || application.quit());
        }
        *viewer_on_activate.borrow_mut() = Some(instance);
    });

    let status = application.run_with_args::<&str>(&[]);
    if let Some(viewer) = viewer.borrow().as_ref() {
        let _ignored = viewer.commands.send(Command::Shutdown);
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
    install_resources();

    let display = gdk_display();
    let theme = gtk::IconTheme::for_display(&display);
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
    let pages = CollapsibleNavigationBand::new("PAGES", &page_list, PAGE_NAVIGATION_WIDTH, None);
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
    path: PathBuf,
    loaded: Arc<LoadedNotebook>,
}

#[derive(Clone)]
struct PageRow {
    source: usize,
    section_id: SectionId,
    page_id: PageId,
}

#[derive(Default)]
struct State {
    sources: Vec<Source>,
    pages: Vec<PageRow>,
    search_hits: Vec<SearchHit>,
    active_source: Option<usize>,
    search_generation: u64,
    scene_generation: u64,
    pending_reveal: Option<Rect>,
}

struct Viewer {
    window: gtk::ApplicationWindow,
    notebook_tree: NotebookTree,
    page_model: gtk::StringList,
    page_selection: gtk::SingleSelection,
    page_list: gtk::ListView,
    result_model: gtk::StringList,
    result_selection: gtk::SingleSelection,
    result_list: gtk::ListView,
    navigation_stack: gtk::Stack,
    content_paned: gtk::Paned,
    page_navigation_width: Rc<Cell<i32>>,
    page_view: PageView,
    canvas_stack: gtk::Stack,
    page_title: gtk::Label,
    page_date: gtk::Label,
    page_context: gtk::Label,
    search_entry: gtk::SearchEntry,
    status: gtk::Label,
    spinner: gtk::Spinner,
    zoom_label: gtk::Label,
    import_activity: gtk::Revealer,
    import_activity_title: gtk::Label,
    import_activity_phase: gtk::Label,
    import_progress: gtk::ProgressBar,
    import_cancel_button: gtk::Button,
    import_package_button: gtk::Button,
    import_cancel: RefCell<Option<Arc<AtomicBool>>>,
    state: RefCell<State>,
    workspace_path: PathBuf,
    index_path: PathBuf,
    settings_path: PathBuf,
    settings: RefCell<AppSettings>,
    commands: mpsc::Sender<Command>,
    events: mpsc::Sender<Event>,
    receiver: RefCell<mpsc::Receiver<Event>>,
    search_timer: RefCell<Option<glib::SourceId>>,
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
    ) -> Rc<Self> {
        let notebook_tree = NotebookTree::new();
        let page_model = gtk::StringList::new(&[]);
        let (page_selection, page_list) = list_view(&page_model, "page-list");
        let result_model = gtk::StringList::new(&[]);
        let (result_selection, result_list) = result_list(&result_model);
        let page_view = PageView::new();
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Search all notebooks")
            .hexpand(true)
            .width_request(320)
            .build();

        let open_file = icon_button("onenote-open-file-symbolic", "Open OneNote file");
        let open_folder = icon_button("onenote-open-folder-symbolic", "Open notebook folder");
        let import_package =
            icon_button("onenote-import-package-symbolic", "Import OneNote package");
        let open_settings = icon_button("onenote-settings-symbolic", "Settings");
        let close_source = icon_button("onenote-close-symbolic", "Close selected notebook");
        let spinner = gtk::Spinner::new();
        spinner.set_tooltip_text(Some("Background activity"));

        let header_title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let brand = gtk::Label::new(Some("OneNote Viewer"));
        brand.add_css_class("brand");
        header_title.append(&brand);
        header_title.append(&search_entry);

        let header = gtk::HeaderBar::new();
        header.set_show_title_buttons(false);
        header.pack_start(&open_file);
        header.pack_start(&open_folder);
        header.pack_start(&import_package);
        header.pack_end(&open_settings);
        header.set_title_widget(Some(&header_title));

        let notebooks = CollapsibleNavigationBand::new(
            "NOTEBOOKS",
            &notebook_tree.view,
            NOTEBOOK_NAVIGATION_WIDTH,
            Some(&close_source),
        );
        let pages =
            CollapsibleNavigationBand::new("PAGES", &page_list, PAGE_NAVIGATION_WIDTH, None);
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
            NOTEBOOK_NAVIGATION_WIDTH + PAGE_NAVIGATION_WIDTH + NAVIGATION_SEPARATOR_WIDTH;
        navigation_stack.set_width_request(initial_navigation_width);

        let page_title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        page_title.add_css_class("page-title");
        let page_date = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        page_date.add_css_class("page-date");
        let page_context = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        page_context.add_css_class("page-context");
        let title_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        title_box.set_margin_start(20);
        title_box.set_margin_end(16);
        title_box.set_margin_top(10);
        title_box.set_margin_bottom(10);
        title_box.append(&page_title);
        title_box.append(&page_date);
        title_box.append(&page_context);

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
        let zoom_label = gtk::Label::new(Some("100%"));
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

        let import_activity_title = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        import_activity_title.add_css_class("activity-title");
        let import_activity_phase = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        import_activity_phase.add_css_class("activity-phase");
        let activity_labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        activity_labels.set_hexpand(true);
        activity_labels.append(&import_activity_title);
        activity_labels.append(&import_activity_phase);
        let import_progress = gtk::ProgressBar::builder()
            .width_request(240)
            .valign(gtk::Align::Center)
            .build();
        import_progress.set_pulse_step(0.025);
        let import_cancel_button = gtk::Button::with_label("Cancel");
        let activity_content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        activity_content.set_margin_start(16);
        activity_content.set_margin_end(16);
        activity_content.set_margin_top(10);
        activity_content.set_margin_bottom(10);
        activity_content.append(&activity_labels);
        activity_content.append(&import_progress);
        activity_content.append(&import_cancel_button);
        activity_content.add_css_class("import-activity");
        let import_activity = gtk::Revealer::builder()
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
        let notebook_width = Rc::new(Cell::new(NOTEBOOK_NAVIGATION_WIDTH));
        let page_width = Rc::new(Cell::new(PAGE_NAVIGATION_WIDTH));
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
        root.append(&import_activity);
        root.append(&content_paned);
        root.append(&separator_horizontal());
        root.append(&footer);
        let window = gtk::ApplicationWindow::builder()
            .application(application)
            .title("OneNote Viewer")
            .default_width(1_500)
            .default_height(920)
            .titlebar(&header)
            .child(&root)
            .build();

        let (event_sender, event_receiver) = mpsc::channel();
        let commands = worker::start_index_worker(index_path.clone(), event_sender.clone());
        let viewer = Rc::new(Self {
            window,
            notebook_tree,
            page_model,
            page_selection,
            page_list,
            result_model,
            result_selection,
            result_list,
            navigation_stack,
            content_paned,
            page_navigation_width,
            page_view,
            canvas_stack,
            page_title,
            page_date,
            page_context,
            search_entry,
            status,
            spinner,
            zoom_label,
            import_activity,
            import_activity_title,
            import_activity_phase,
            import_progress,
            import_cancel_button,
            import_package_button: import_package.clone(),
            import_cancel: RefCell::default(),
            state: RefCell::default(),
            workspace_path,
            index_path,
            settings_path,
            settings: RefCell::new(settings),
            commands,
            events: event_sender,
            receiver: RefCell::new(event_receiver),
            search_timer: RefCell::default(),
        });
        viewer.connect_navigation();
        viewer.connect_header(
            &open_file,
            &open_folder,
            &import_package,
            &open_settings,
            &close_source,
        );
        viewer.connect_import_activity();
        viewer.connect_zoom(&zoom_out, &zoom_in, &zoom_reset);
        viewer.poll_events();
        viewer
    }

    fn connect_navigation(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.notebook_tree
            .view
            .connect_activate(move |_, position| {
                if let Some(viewer) = weak.upgrade() {
                    viewer.activate_navigation(position);
                }
            });
        let weak = Rc::downgrade(self);
        self.page_list.connect_activate(move |_, position| {
            if let Some(viewer) = weak.upgrade() {
                viewer.activate_page(position as usize, None);
            }
        });
        let weak = Rc::downgrade(self);
        self.result_list.connect_activate(move |_, position| {
            if let Some(viewer) = weak.upgrade() {
                viewer.activate_result(position as usize);
            }
        });

        let weak = Rc::downgrade(self);
        self.search_entry.connect_search_changed(move |entry| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            if let Some(timer) = viewer.search_timer.borrow_mut().take() {
                timer.remove();
            }
            let text = entry.text().trim().to_owned();
            if text.is_empty() {
                viewer.navigation_stack.set_visible_child_name("pages");
                viewer.set_navigation_width(viewer.page_navigation_width.get());
                viewer.result_selection.set_selected(NO_SELECTION);
                return;
            }
            let weak = Rc::downgrade(&viewer);
            *viewer.search_timer.borrow_mut() = Some(glib::timeout_add_local_once(
                Duration::from_millis(250),
                move || {
                    if let Some(viewer) = weak.upgrade() {
                        viewer.start_search(text);
                        viewer.search_timer.borrow_mut().take();
                    }
                },
            ));
        });
    }

    fn connect_header(
        self: &Rc<Self>,
        open_file: &gtk::Button,
        open_folder: &gtk::Button,
        import_package: &gtk::Button,
        open_settings: &gtk::Button,
        close_source: &gtk::Button,
    ) {
        let weak = Rc::downgrade(self);
        open_file.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_file();
            }
        });
        let weak = Rc::downgrade(self);
        open_folder.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_folder();
            }
        });
        let weak = Rc::downgrade(self);
        import_package.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.choose_package();
            }
        });
        let weak = Rc::downgrade(self);
        open_settings.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.show_settings();
            }
        });
        let weak = Rc::downgrade(self);
        close_source.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.close_active_source();
            }
        });
    }

    fn connect_import_activity(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.import_cancel_button.connect_clicked(move |button| {
            let Some(viewer) = weak.upgrade() else {
                return;
            };
            let Some(cancel) = viewer.import_cancel.borrow().as_ref().cloned() else {
                return;
            };
            cancel.store(true, Ordering::Relaxed);
            button.set_sensitive(false);
            viewer
                .import_activity_phase
                .set_label("Cancelling and cleaning temporary files...");
        });
    }

    fn connect_zoom(
        self: &Rc<Self>,
        zoom_out: &gtk::Button,
        zoom_in: &gtk::Button,
        zoom_reset: &gtk::Button,
    ) {
        let weak = Rc::downgrade(self);
        zoom_out.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.set_zoom(viewer.page_view.zoom() / 1.1);
            }
        });
        let weak = Rc::downgrade(self);
        zoom_in.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.set_zoom(viewer.page_view.zoom() * 1.1);
            }
        });
        let weak = Rc::downgrade(self);
        zoom_reset.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.set_zoom(1.0);
            }
        });
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
            if viewer.import_activity.reveals_child() && viewer.import_cancel.borrow().is_some() {
                viewer.import_progress.pulse();
            }
            glib::ControlFlow::Continue
        });
    }
    fn handle_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Discovered { requested, result } => match result {
                Ok(paths) => {
                    for path in paths {
                        let _ignored = self.commands.send(Command::Load(path));
                    }
                    self.set_busy(&format!("Opening {}", requested.display()));
                }
                Err(error) => self.show_error("Could not open source", &error),
            },
            Event::LibraryDiscovered { location, result } => match result {
                Ok(paths) if paths.is_empty() => {
                    self.spinner.stop();
                    self.status.set_label(&format!(
                        "Default notebooks location is empty: {}",
                        location.display()
                    ));
                }
                Ok(paths) => {
                    let count = paths.len();
                    for path in paths {
                        let _ignored = self.commands.send(Command::Load(path));
                    }
                    self.set_busy(&format!(
                        "Opening {count} notebook{} from the default location",
                        if count == 1 { "" } else { "s" }
                    ));
                }
                Err(error) => self.show_error(
                    "Could not scan default notebooks location",
                    &format!("{}\n\n{error}", location.display()),
                ),
            },
            Event::Loaded { path, result } => match result {
                Ok(loaded) => self.add_source(path, loaded),
                Err(error) => self.show_error("Could not read notebook", &error),
            },
            Event::Indexed { source_id, result } => {
                self.spinner.stop();
                match result {
                    Ok(()) => self.status.set_label("Search index is up to date"),
                    Err(error) => self.show_error(
                        "Notebook opened, but indexing failed",
                        &format!("{source_id}: {error}"),
                    ),
                }
            }
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
                self.spinner.stop();
                match result {
                    Ok(scene) => {
                        self.page_view.set_scene(Some(scene));
                        self.canvas_stack.set_visible_child_name("document");
                        if let Some(bounds) = self.state.borrow_mut().pending_reveal.take() {
                            self.page_view.reveal(bounds);
                        }
                        self.status.set_label("Page ready");
                    }
                    Err(error) => self.show_error("Could not render page", &error),
                }
            }
            Event::Extracted { result } => match result {
                Ok(destination) => {
                    self.status.set_label("Package imported; opening notebooks");
                    self.import_cancel.borrow_mut().take();
                    self.import_cancel_button.set_sensitive(false);
                    self.import_progress.set_fraction(1.0);
                    self.import_activity_phase
                        .set_label("Imported successfully; opening notebooks...");
                    self.hide_import_activity_after(Duration::from_millis(1_800));
                    self.discover(destination);
                }
                Err(error) => {
                    self.finish_import_activity();
                    if error == "OneNote package extraction was cancelled" {
                        self.status.set_label("Package import cancelled");
                    } else {
                        self.show_error("Package import failed", &error);
                    }
                }
            },
            Event::ExtractionProgress { phase } => {
                self.import_activity_phase
                    .set_label(extraction_phase_label(phase));
            }
        }
    }

    fn discover(&self, path: PathBuf) {
        self.set_busy(&format!("Discovering {}", path.display()));
        let _ignored = self.commands.send(Command::Discover(path));
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
        let _ignored = self.commands.send(Command::DiscoverLibrary(location));
    }

    fn add_source(&self, path: PathBuf, loaded: Arc<LoadedNotebook>) {
        let source_id = loaded.notebook.source_id.clone();
        let mut state = self.state.borrow_mut();
        if let Some(existing) = state
            .sources
            .iter_mut()
            .find(|source| source.loaded.notebook.source_id == source_id)
        {
            existing.path = path;
            existing.loaded = loaded;
        } else {
            state.sources.push(Source { path, loaded });
        }
        let select = state
            .sources
            .iter()
            .position(|source| source.loaded.notebook.source_id == source_id)
            .unwrap_or_else(|| state.sources.len().saturating_sub(1));
        let first_section = state
            .sources
            .get(select)
            .and_then(|source| first_section(&source.loaded.notebook.entries))
            .map(|section| section.id.clone());
        drop(state);
        self.refresh_notebooks();
        self.notebook_tree.select_notebook(select);
        if let Some(section_id) = first_section {
            self.notebook_tree.select_section(select, &section_id);
            self.activate_section(select, &section_id);
        }
        self.persist_workspace();
        self.status
            .set_label("Notebook opened; building search index");
    }

    fn refresh_notebooks(&self) {
        let state = self.state.borrow();
        self.notebook_tree.rebuild(
            state
                .sources
                .iter()
                .enumerate()
                .map(|(index, source)| (index, &source.loaded.notebook)),
        );
        if state.sources.is_empty() {
            self.canvas_stack.set_visible_child_name("empty");
            clear_model(&self.page_model);
        }
    }

    fn activate_navigation(&self, position: u32) {
        let Some(target) = self.notebook_tree.target_at(position) else {
            return;
        };
        match target {
            NavigationTarget::Notebook { source } | NavigationTarget::Group { source } => {
                self.state.borrow_mut().active_source = Some(source);
            }
            NavigationTarget::Section { source, section_id } => {
                self.activate_section(source, &section_id);
            }
        }
    }

    fn activate_section(&self, source_index: usize, section_id: &SectionId) {
        let pages = {
            let state = self.state.borrow();
            state
                .sources
                .get(source_index)
                .and_then(|source| find_section(&source.loaded.notebook.entries, section_id))
                .map(|section| section.pages.clone())
                .unwrap_or_default()
        };
        let mut state = self.state.borrow_mut();
        state.active_source = Some(source_index);
        state.pages = pages
            .iter()
            .map(|page| PageRow {
                source: source_index,
                section_id: section_id.clone(),
                page_id: page.id.clone(),
            })
            .collect();
        drop(state);
        clear_model(&self.page_model);
        for page in pages {
            let indent = "  ".repeat(usize::try_from(page.level.max(0)).unwrap_or(0).min(8));
            append_model(
                &self.page_model,
                &format!("{indent}{}", display_title(&page)),
            );
        }
        if self.page_model.n_items() > 0 {
            self.page_selection.set_selected(0);
            self.activate_page(0, None);
        }
    }

    fn activate_page(&self, position: usize, reveal: Option<Rect>) {
        let value = (|| {
            let state = self.state.borrow();
            let row = state.pages.get(position)?;
            let source = state.sources.get(row.source)?;
            let section = find_section(&source.loaded.notebook.entries, &row.section_id)?;
            let page = section.pages.iter().find(|page| page.id == row.page_id)?;
            Some((
                page.clone(),
                source.loaded.clone(),
                source.loaded.notebook.name.clone(),
                section.name.clone(),
            ))
        })();
        let Some((page, loaded, notebook_name, section_name)) = value else {
            return;
        };
        let display_title = display_title(&page);
        let title = gtk_text(&display_title);
        self.page_title.set_label(&title);
        let date = display_timestamp(&page.created_at);
        self.page_date.set_label(&date);
        let display_context = format!("{notebook_name}  /  {section_name}");
        let context = gtk_text(&display_context);
        self.page_context.set_label(&context);
        self.page_view
            .set_resources(Some(Arc::new(loaded.resources.clone())));
        self.page_view.set_scene(None);
        let generation = {
            let mut state = self.state.borrow_mut();
            state.scene_generation = state.scene_generation.wrapping_add(1);
            state.pending_reveal = reveal;
            state.scene_generation
        };
        self.set_busy("Laying out page");
        worker::build_scene(generation, page, self.events.clone());
    }

    fn start_search(&self, text: String) {
        let generation = {
            let mut state = self.state.borrow_mut();
            state.search_generation = state.search_generation.wrapping_add(1);
            state.search_generation
        };
        self.set_busy("Searching all open notebooks");
        worker::search(
            self.index_path.clone(),
            generation,
            text,
            self.events.clone(),
        );
    }

    fn show_search_results(&self, hits: Vec<SearchHit>) {
        clear_model(&self.result_model);
        for hit in &hits {
            let snippet = hit.snippet.text.replace('\n', " ");
            append_model(
                &self.result_model,
                &format!(
                    "{}\n{}  /  {}    {}",
                    hit.page_title, hit.notebook_name, hit.section_name, snippet
                ),
            );
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

    fn activate_result(&self, position: usize) {
        let hit = {
            let state = self.state.borrow();
            state.search_hits.get(position).cloned()
        };
        let Some(hit) = hit else {
            return;
        };
        let source_position = {
            let state = self.state.borrow();
            state
                .sources
                .iter()
                .position(|source| source.loaded.notebook.source_id == hit.source_id)
        };
        let Some(source_position) = source_position else {
            self.show_error(
                "Search result is stale",
                "The source is no longer open. Refresh the search index.",
            );
            return;
        };
        if self
            .notebook_tree
            .select_section(source_position, &hit.section_id)
            .is_none()
        {
            return;
        }
        self.activate_section(source_position, &hit.section_id);
        let page_position = self
            .state
            .borrow()
            .pages
            .iter()
            .position(|row| row.page_id == hit.page_id);
        let Some(page_position) = page_position else {
            return;
        };
        self.page_selection
            .set_selected(u32::try_from(page_position).unwrap_or(NO_SELECTION));
        self.activate_page(page_position, hit.bounds);
    }

    fn close_active_source(&self) {
        let removed = {
            let mut state = self.state.borrow_mut();
            let Some(position) = state.active_source else {
                return;
            };
            if position >= state.sources.len() {
                return;
            }
            state.active_source = None;
            state.pages.clear();
            (position, state.sources.remove(position))
        };
        let _ignored = self
            .commands
            .send(Command::Remove(removed.1.loaded.notebook.source_id.clone()));
        self.refresh_notebooks();
        self.persist_workspace();
        let remaining = self.state.borrow().sources.len();
        if remaining > 0 {
            let new_position = removed.0.min(remaining - 1);
            self.notebook_tree.select_notebook(new_position);
            let first_section = self
                .state
                .borrow()
                .sources
                .get(new_position)
                .and_then(|source| first_section(&source.loaded.notebook.entries))
                .map(|section| section.id.clone());
            if let Some(section_id) = first_section {
                self.notebook_tree.select_section(new_position, &section_id);
                self.activate_section(new_position, &section_id);
            }
        } else {
            self.status.set_label("No notebooks open");
        }
    }

    fn persist_workspace(&self) {
        let notebooks_location = self.settings.borrow().notebooks_location.clone();
        let config = WorkspaceConfig {
            sources: self
                .state
                .borrow()
                .sources
                .iter()
                .map(|source| source.path.clone())
                .filter(|source| !workspace::source_is_in_location(source, &notebooks_location))
                .collect(),
        };
        if let Err(error) = workspace::save(&self.workspace_path, &config) {
            self.show_error("Could not save workspace", &error.to_string());
        }
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

    fn choose_package(self: &Rc<Self>) {
        if self.import_cancel.borrow().is_some() {
            self.status
                .set_label("A OneNote package import is already running");
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
        let current = self.settings.borrow().notebooks_location.clone();
        let default = settings::default_notebooks_location();
        let weak_save = Rc::downgrade(self);
        let weak_error = weak_save.clone();
        crate::dialogs::present_settings(
            &self.window,
            current,
            default,
            move |location| {
                weak_save
                    .upgrade()
                    .is_some_and(|viewer| viewer.set_notebooks_location(location))
            },
            move |title, detail| {
                if let Some(viewer) = weak_error.upgrade() {
                    viewer.show_error(title, detail);
                }
            },
        );
    }

    fn set_notebooks_location(&self, location: &std::path::Path) -> bool {
        if let Err(error) = settings::ensure_notebooks_location(location) {
            self.show_error(
                "Could not use default notebooks location",
                &error.to_string(),
            );
            return false;
        }
        let updated = AppSettings {
            notebooks_location: location.to_path_buf(),
        };
        if let Err(error) = settings::save(&self.settings_path, &updated) {
            self.show_error("Could not save settings", &error.to_string());
            return false;
        }
        *self.settings.borrow_mut() = updated;
        self.persist_workspace();
        self.open_notebooks_location();
        true
    }

    fn start_package_import(&self, package: PathBuf, destination: PathBuf) {
        if self.import_cancel.borrow().is_some() {
            self.status
                .set_label("A OneNote package import is already running");
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *self.import_cancel.borrow_mut() = Some(Arc::clone(&cancel));
        self.import_package_button.set_sensitive(false);
        self.import_cancel_button.set_sensitive(true);
        self.import_progress.set_fraction(0.0);
        self.import_activity_title.set_label(&format!(
            "Importing {}",
            package
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("OneNote package")
        ));
        self.import_activity_phase
            .set_label("Preparing package import...");
        self.import_activity.set_reveal_child(true);
        self.set_busy("Importing OneNote package");
        worker::extract(package, destination, cancel, self.events.clone());
    }

    fn finish_import_activity(&self) {
        self.import_cancel.borrow_mut().take();
        self.import_cancel_button.set_sensitive(false);
        self.import_package_button.set_sensitive(true);
        self.import_activity.set_reveal_child(false);
    }

    fn hide_import_activity_after(self: &Rc<Self>, delay: Duration) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(delay, move || {
            if let Some(viewer) = weak.upgrade() {
                viewer.finish_import_activity();
            }
        });
    }

    fn set_zoom(&self, zoom: f32) {
        self.page_view.set_zoom(zoom);
        self.zoom_label
            .set_label(&format!("{:.0}%", self.page_view.zoom() * 100.0));
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

fn list_view(model: &gtk::StringList, css_class: &str) -> (gtk::SingleSelection, gtk::ListView) {
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
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
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .single_click_activate(true)
        .css_classes([css_class])
        .build();
    (selection, list)
}

fn result_list(model: &gtk::StringList) -> (gtk::SingleSelection, gtk::ListView) {
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
        let label = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(3)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        label.set_margin_start(14);
        label.set_margin_end(12);
        label.set_margin_top(10);
        label.set_margin_bottom(10);
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| bind_string(item, true));
    let list = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .single_click_activate(true)
        .css_classes(["result-list"])
        .build();
    (selection, list)
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
}

impl CollapsibleNavigationBand {
    fn new(
        title: &str,
        list: &gtk::ListView,
        expanded_width: i32,
        header_action: Option<&gtk::Button>,
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
        Self {
            root,
            header,
            heading,
            body,
            toggle,
            header_action: header_action.cloned(),
            expanded_width,
        }
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
        self.toggle.connect_clicked(move |button| {
            let collapse = body.is_visible();
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

fn clear_model(model: &gtk::StringList) {
    while model.n_items() > 0 {
        model.remove(model.n_items() - 1);
    }
}

fn append_model(model: &gtk::StringList, value: &str) {
    let value = gtk_text(value);
    model.append(&value);
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

fn first_section(entries: &[NotebookEntry]) -> Option<&Section> {
    entries.iter().find_map(|entry| match entry {
        NotebookEntry::Section(section) => Some(section),
        NotebookEntry::Group(group) => first_section(&group.entries),
    })
}

fn find_section<'a>(entries: &'a [NotebookEntry], id: &SectionId) -> Option<&'a Section> {
    for entry in entries {
        match entry {
            NotebookEntry::Section(section) if section.id == *id => return Some(section),
            NotebookEntry::Section(_) => {}
            NotebookEntry::Group(group) => {
                if let Some(section) = find_section(&group.entries, id) {
                    return Some(section);
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_lines)]
fn install_resources() {
    let display = gdk_display();
    gtk::IconTheme::for_display(&display).add_resource_path("/io/github/emsi/OneNoteViewer/icons");
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "
        window, window:backdrop { background: #f7f7f8; color: #202124; }
        headerbar, headerbar:backdrop {
            background: #ffffff;
            color: #202124;
            border-bottom: 1px solid #d9dadd;
        }
        .icon-button, .icon-button:backdrop,
        .icon-button image, .icon-button image:backdrop {
            background: #f7f7f8;
            color: #202124;
        }
        .icon-button:hover { background: #e7e8eb; }
        .brand, .brand:backdrop {
            font-size: 16px;
            font-weight: 700;
            color: #5b2d90;
        }
        searchentry { min-height: 34px; }
        .navigation-band, .navigation-band:backdrop {
            background: #f3f3f5;
            color: #202124;
        }
        .navigation-band label, .navigation-band label:backdrop,
        treeexpander, treeexpander:backdrop,
        treeexpander > expander, treeexpander > expander:backdrop {
            color: #202124;
        }
        treeexpander > expander {
            min-width: 16px;
            min-height: 16px;
            -gtk-icon-source: -gtk-icontheme(\"onenote-chevron-right-symbolic\");
        }
        treeexpander > expander:checked {
            -gtk-icon-source: -gtk-icontheme(\"onenote-chevron-down-symbolic\");
        }
        .nav-heading, .nav-heading:backdrop {
            font-size: 11px;
            font-weight: 700;
            color: #666970;
        }
        listview, listview:backdrop { background: transparent; color: #202124; }
        listview row, listview row:backdrop {
            border-radius: 4px;
            margin: 1px 6px;
            color: #202124;
        }
        listview row:selected, listview row:selected:backdrop {
            background: #e8def3;
            color: #2d183d;
        }
        .notebook-row { font-weight: 600; }
        .group-row { font-weight: 500; }
        .notebook-tree row:selected { border-left: 3px solid #6b3a96; }
        .page-list row:selected { border-left: 3px solid #b35a24; }
        .result-row { line-height: 1.25; }
        .page-title, .page-title:backdrop {
            font-size: 20px;
            font-weight: 650;
            color: #202124;
        }
        .page-date, .page-date:backdrop,
        .page-context, .page-context:backdrop,
        .status, .status:backdrop { font-size: 12px; color: #666970; }
        .empty-title, .empty-title:backdrop {
            font-size: 20px;
            font-weight: 600;
            color: #202124;
        }
        .empty-icon, .empty-icon:backdrop { color: #777a80; }
        .import-activity, .import-activity:backdrop {
            background: #f1eaf8;
            color: #202124;
            border-bottom: 1px solid #d6c8e4;
        }
        .activity-title, .activity-title:backdrop {
            font-size: 14px;
            font-weight: 650;
            color: #2d183d;
        }
        .activity-phase, .activity-phase:backdrop {
            font-size: 12px;
            color: #5f5268;
        }
        .settings-dialog, .settings-dialog:backdrop {
            background: #f7f7f8;
            color: #202124;
        }
        .settings-dialog label, .settings-dialog label:backdrop,
        .settings-dialog button, .settings-dialog button:backdrop {
            color: #202124;
        }
        .dialog-title, .dialog-title:backdrop {
            font-size: 18px;
            font-weight: 700;
            color: #202124;
        }
        .field-label, .field-label:backdrop {
            font-size: 12px;
            font-weight: 700;
            color: #55585f;
        }
        .path-value, .path-value:backdrop {
            background: #ffffff;
            color: #202124;
            border: 1px solid #d9dadd;
            border-radius: 4px;
            padding: 10px;
        }
        .warning-label, .warning-label:backdrop {
            color: #9b2c1f;
            font-weight: 600;
        }
        .error-dialog, .error-dialog:backdrop {
            background: #f7f7f8;
            color: #202124;
        }
        .error-dialog label, .error-dialog label:backdrop,
        .error-dialog button, .error-dialog button:backdrop {
            color: #202124;
        }
        .error-title, .error-title:backdrop {
            font-size: 18px;
            font-weight: 700;
            color: #202124;
        }
        .error-detail-frame {
            border: 1px solid #d9dadd;
            border-radius: 4px;
        }
        .error-detail, .error-detail:backdrop,
        .error-detail text, .error-detail text:backdrop {
            background: #ffffff;
            color: #202124;
            caret-color: #202124;
        }
        .error-detail text selection,
        .error-detail text selection:backdrop {
            background: #6b3a96;
            color: #ffffff;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn gdk_display() -> gtk::gdk::Display {
    gtk::gdk::Display::default().expect("GTK display")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_section_follows_group_order() {
        let section = Section {
            id: SectionId::new("section"),
            name: "Details".to_owned(),
            color: None,
            pages: Vec::new(),
            diagnostics: Vec::new(),
        };
        let entries = vec![NotebookEntry::Group(onenote_core::SectionGroup {
            id: SectionId::new("group"),
            name: "Project".to_owned(),
            entries: vec![NotebookEntry::Section(section)],
        })];

        let first = first_section(&entries).expect("nested section");

        assert_eq!(first.name, "Details");
    }

    #[test]
    fn gtk_text_replaces_interior_nuls() {
        assert_eq!(gtk_text("One\0Note"), "One�Note");
        assert_eq!(gtk_text("OneNote"), "OneNote");
    }

    #[test]
    fn page_timestamp_keeps_source_date_and_minute() {
        assert_eq!(
            display_timestamp("2015-06-12 11:06:27.0 +00"),
            "2015-06-12  11:06"
        );
        assert_eq!(display_timestamp("unknown"), "unknown");
    }

    #[test]
    fn collapsed_bands_reclaim_the_outer_pane_width() {
        if gtk::init().is_err() {
            return;
        }
        let (_, notebook_list) = list_view(&gtk::StringList::new(&[]), "notebook-list");
        let (_, page_list) = list_view(&gtk::StringList::new(&[]), "page-list");
        let close_source = gtk::Button::new();
        let notebooks = CollapsibleNavigationBand::new(
            "NOTEBOOKS",
            &notebook_list,
            NOTEBOOK_NAVIGATION_WIDTH,
            Some(&close_source),
        );
        let pages =
            CollapsibleNavigationBand::new("PAGES", &page_list, PAGE_NAVIGATION_WIDTH, None);
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
}

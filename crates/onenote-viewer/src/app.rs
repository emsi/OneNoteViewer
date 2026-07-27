use crate::navigation::{NavigationTarget, NotebookTree};
use crate::worker::{self, Command, Event};
use crate::workspace::{self, WorkspaceConfig};
use anyhow::Result;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{LoadedNotebook, NotebookEntry, Page, PageId, Rect, Section, SectionId};
use onenote_index::SearchHit;
use onenote_render_gtk::PageView;
use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const NO_SELECTION: u32 = gtk::INVALID_LIST_POSITION;

pub(crate) fn run(requested_sources: Vec<PathBuf>) -> Result<()> {
    gio::resources_register_include!("onenote-viewer.gresource")?;
    let (workspace_path, index_path) = workspace::paths()?;
    workspace::ensure_index_parent(&index_path)?;
    let persisted = workspace::load(&workspace_path).unwrap_or_default();
    let initial_sources = if requested_sources.is_empty() {
        persisted.sources
    } else {
        requested_sources
    };

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
        let instance = Viewer::new(application, workspace_path.clone(), index_path.clone());
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
    result_model: gtk::StringList,
    result_selection: gtk::SingleSelection,
    navigation_stack: gtk::Stack,
    page_view: PageView,
    canvas_stack: gtk::Stack,
    page_title: gtk::Label,
    page_date: gtk::Label,
    page_context: gtk::Label,
    search_entry: gtk::SearchEntry,
    status: gtk::Label,
    spinner: gtk::Spinner,
    zoom_label: gtk::Label,
    state: RefCell<State>,
    workspace_path: PathBuf,
    index_path: PathBuf,
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
        let close_source = icon_button("onenote-close-symbolic", "Close selected notebook");
        let spinner = gtk::Spinner::new();
        spinner.set_tooltip_text(Some("Background activity"));

        let header_title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let brand = gtk::Label::new(Some("OneNote Viewer"));
        brand.add_css_class("brand");
        header_title.append(&brand);
        header_title.append(&search_entry);

        let header = gtk::HeaderBar::new();
        header.pack_start(&open_file);
        header.pack_start(&open_folder);
        header.pack_start(&import_package);
        header.set_title_widget(Some(&header_title));
        header.pack_end(&close_source);
        header.pack_end(&spinner);

        let notebooks = collapsible_navigation_band("NOTEBOOKS", &notebook_tree.view, 320);
        let pages = collapsible_navigation_band("PAGES", &page_list, 280);
        let results = navigation_band("SEARCH RESULTS", &result_list, 520);

        let navigation_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        navigation.append(&notebooks);
        navigation.append(&separator());
        navigation.append(&pages);
        navigation_stack.add_named(&navigation, Some("pages"));
        navigation_stack.add_named(&results, Some("results"));
        navigation_stack.set_visible_child_name("pages");

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
        footer.append(&zoom_out);
        footer.append(&zoom_label);
        footer.append(&zoom_in);
        footer.append(&zoom_reset);

        let content_paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&navigation_stack)
            .end_child(&canvas_stack)
            .resize_start_child(false)
            .shrink_start_child(false)
            .position(600)
            .build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
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
            result_model,
            result_selection,
            navigation_stack,
            page_view,
            canvas_stack,
            page_title,
            page_date,
            page_context,
            search_entry,
            status,
            spinner,
            zoom_label,
            state: RefCell::default(),
            workspace_path,
            index_path,
            commands,
            events: event_sender,
            receiver: RefCell::new(event_receiver),
            search_timer: RefCell::default(),
        });
        viewer.connect_navigation();
        viewer.connect_header(&open_file, &open_folder, &import_package, &close_source);
        viewer.connect_zoom(&zoom_out, &zoom_in, &zoom_reset);
        viewer.poll_events();
        viewer
    }

    fn connect_navigation(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.notebook_tree
            .selection
            .connect_selected_notify(move |selection| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                let position = selection.selected();
                if position != NO_SELECTION {
                    viewer.activate_navigation(position);
                }
            });
        let weak = Rc::downgrade(self);
        self.page_selection
            .connect_selected_notify(move |selection| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                let position = selection.selected();
                if position != NO_SELECTION {
                    viewer.activate_page(position as usize, None);
                }
            });
        let weak = Rc::downgrade(self);
        self.result_selection
            .connect_selected_notify(move |selection| {
                let Some(viewer) = weak.upgrade() else {
                    return;
                };
                let position = selection.selected();
                if position != NO_SELECTION {
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
        close_source.connect_clicked(move |_| {
            if let Some(viewer) = weak.upgrade() {
                viewer.close_active_source();
            }
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
                    self.discover(destination);
                }
                Err(error) => self.show_error("Package import failed", &error),
            },
        }
    }

    fn discover(&self, path: PathBuf) {
        self.set_busy(&format!("Discovering {}", path.display()));
        let _ignored = self.commands.send(Command::Discover(path));
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
            }
        } else {
            self.status.set_label("No notebooks open");
        }
    }

    fn persist_workspace(&self) {
        let config = WorkspaceConfig {
            sources: self
                .state
                .borrow()
                .sources
                .iter()
                .map(|source| source.path.clone())
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
        let dialog = gtk::FileDialog::builder()
            .title("Select OneNote package")
            .modal(true)
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
                            viewer.choose_package_destination(package);
                        }
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => viewer.show_error("Could not select package", &error.to_string()),
                }
            },
        );
    }

    fn choose_package_destination(self: &Rc<Self>, package: PathBuf) {
        let dialog = gtk::FileDialog::builder()
            .title("Select package destination folder")
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
                        let Some(parent) = file.path() else {
                            return;
                        };
                        let name = package
                            .file_stem()
                            .and_then(|value| value.to_str())
                            .unwrap_or("OneNote Notebook");
                        let destination = parent.join(name);
                        viewer.set_busy("Validating and extracting package on disk");
                        worker::extract(package.clone(), destination, viewer.events.clone());
                    }
                    Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                    Err(error) => viewer
                        .show_error("Could not select package destination", &error.to_string()),
                }
            },
        );
    }

    fn set_zoom(&self, zoom: f32) {
        self.page_view.set_zoom(zoom);
        self.zoom_label
            .set_label(&format!("{:.0}%", self.page_view.zoom() * 100.0));
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
        gtk::AlertDialog::builder()
            .modal(true)
            .message(title.as_ref())
            .detail(detail.as_ref())
            .build()
            .show(Some(&self.window));
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

fn collapsible_navigation_band(title: &str, list: &gtk::ListView, expanded_width: i32) -> gtk::Box {
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
    header.append(&toggle);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(list)
        .vexpand(true)
        .build();
    let band = gtk::Box::new(gtk::Orientation::Vertical, 0);
    band.set_width_request(expanded_width);
    band.add_css_class("navigation-band");
    band.append(&header);
    band.append(&scroll);

    let band_for_toggle = band.clone();
    toggle.connect_clicked(move |button| {
        let collapse = scroll.is_visible();
        scroll.set_visible(!collapse);
        heading.set_visible(!collapse);
        band_for_toggle.set_width_request(if collapse { 42 } else { expanded_width });
        if collapse {
            button.set_icon_name("onenote-panel-expand-symbolic");
            button.set_tooltip_text(Some("Expand navigation"));
        } else {
            button.set_icon_name("onenote-panel-collapse-symbolic");
            button.set_tooltip_text(Some("Collapse navigation"));
        }
    });
    band
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
}

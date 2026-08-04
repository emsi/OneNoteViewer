use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{Color, Notebook, NotebookEntry, SectionId, SourceId};

const DEFAULT_NOTEBOOK_COLOR: Color = Color {
    red: 91,
    green: 45,
    blue: 144,
    alpha: 255,
};
const DEFAULT_SECTION_COLOR: Color = Color {
    red: 22,
    green: 131,
    blue: 111,
    alpha: 255,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NavigationTarget {
    Notebook {
        source_id: SourceId,
    },
    Group {
        source_id: SourceId,
    },
    Section {
        source_id: SourceId,
        section_id: SectionId,
    },
}

#[derive(Clone)]
struct NavigationNode {
    label: String,
    target: NavigationTarget,
    color: Option<Color>,
    children: Vec<Self>,
}

pub(crate) struct NotebookTree {
    roots: gio::ListStore,
    model: gtk::TreeListModel,
    pub(crate) selection: gtk::SingleSelection,
    pub(crate) view: gtk::ListView,
}

impl NotebookTree {
    pub(crate) fn new() -> Self {
        let roots = gio::ListStore::new::<glib::BoxedAnyObject>();
        let model = tree_model(roots.clone());
        let selection = gtk::SingleSelection::new(Some(model.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        let view = gtk::ListView::builder()
            .model(&selection)
            .single_click_activate(false)
            .css_classes(["notebook-tree"])
            .build();
        let factory = tree_factory();
        view.set_factory(Some(&factory));
        Self {
            roots,
            model,
            selection,
            view,
        }
    }

    pub(crate) fn upsert(&self, notebook: &Notebook) {
        let source_id = notebook.source_id.clone();
        let node = notebook_node(notebook);
        let item = glib::BoxedAnyObject::new(node);
        if let Some(position) = self.root_position(&source_id) {
            self.roots.splice(position, 1, &[item]);
        } else {
            self.roots.append(&item);
        }
        self.expand_notebook(&source_id);
    }

    pub(crate) fn remove(&self, source_id: &SourceId) -> bool {
        let Some(position) = self.root_position(source_id) else {
            return false;
        };
        self.roots.remove(position);
        true
    }

    pub(crate) fn target_at(&self, position: u32) -> Option<NavigationTarget> {
        let row = self.model.row(position)?;
        let item = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
        let target = item.borrow::<NavigationNode>().target.clone();
        Some(target)
    }

    pub(crate) fn select_section(
        &self,
        source_id: &SourceId,
        section_id: &SectionId,
    ) -> Option<u32> {
        loop {
            if let Some(position) = (0..self.model.n_items()).find(|position| {
                matches!(
                    self.target_at(*position),
                    Some(NavigationTarget::Section {
                        source_id: ref row_source,
                        section_id: ref row_section,
                    }) if row_source == source_id && row_section == section_id
                )
            }) {
                self.selection.set_selected(position);
                return Some(position);
            }
            let expandable = (0..self.model.n_items()).find(|position| {
                self.row_node(*position).is_some_and(|node| {
                    node.contains_section(source_id, section_id)
                        && self
                            .model
                            .row(*position)
                            .is_some_and(|row| row.is_expandable() && !row.is_expanded())
                })
            })?;
            self.model.row(expandable)?.set_expanded(true);
        }
    }

    pub(crate) fn select_notebook(&self, source_id: &SourceId) -> Option<u32> {
        let position = (0..self.model.n_items()).find(|position| {
            matches!(
                self.target_at(*position),
                Some(NavigationTarget::Notebook { source_id: ref row_source })
                    if row_source == source_id
            )
        })?;
        self.selection.set_selected(position);
        Some(position)
    }

    pub(crate) fn selected_target(&self) -> Option<NavigationTarget> {
        let position = self.selection.selected();
        (position != gtk::INVALID_LIST_POSITION)
            .then(|| self.target_at(position))
            .flatten()
    }

    fn root_position(&self, source_id: &SourceId) -> Option<u32> {
        (0..self.roots.n_items()).find(|position| {
            let Some(item) = self
                .roots
                .item(*position)
                .and_downcast::<glib::BoxedAnyObject>()
            else {
                return false;
            };
            let target = item.borrow::<NavigationNode>().target.clone();
            matches!(
                target,
                NavigationTarget::Notebook { source_id: ref row_source }
                    if row_source == source_id
            )
        })
    }

    fn expand_notebook(&self, source_id: &SourceId) {
        if let Some(position) = (0..self.model.n_items()).find(|position| {
            matches!(
                self.target_at(*position),
                Some(NavigationTarget::Notebook { source_id: ref row_source })
                    if row_source == source_id
            )
        }) {
            if let Some(row) = self.model.row(position) {
                row.set_expanded(true);
            }
        }
    }

    fn row_node(&self, position: u32) -> Option<NavigationNode> {
        let row = self.model.row(position)?;
        let item = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
        let node = item.borrow::<NavigationNode>().clone();
        Some(node)
    }
}

impl NavigationNode {
    fn contains_section(&self, source_id: &SourceId, section_id: &SectionId) -> bool {
        matches!(
            self.target,
            NavigationTarget::Section {
                source_id: ref row_source,
                section_id: ref row_section,
            } if row_source == source_id && row_section == section_id
        ) || self
            .children
            .iter()
            .any(|child| child.contains_section(source_id, section_id))
    }
}

fn notebook_node(notebook: &Notebook) -> NavigationNode {
    NavigationNode {
        label: notebook.name.clone(),
        target: NavigationTarget::Notebook {
            source_id: notebook.source_id.clone(),
        },
        color: Some(notebook.color.unwrap_or(DEFAULT_NOTEBOOK_COLOR)),
        children: entry_nodes(&notebook.source_id, &notebook.entries),
    }
}

fn entry_nodes(source_id: &SourceId, entries: &[NotebookEntry]) -> Vec<NavigationNode> {
    entries
        .iter()
        .map(|entry| match entry {
            NotebookEntry::Section(section) => NavigationNode {
                label: section.name.clone(),
                target: NavigationTarget::Section {
                    source_id: source_id.clone(),
                    section_id: section.id.clone(),
                },
                color: Some(section.color.unwrap_or(DEFAULT_SECTION_COLOR)),
                children: Vec::new(),
            },
            NotebookEntry::Group(group) => NavigationNode {
                label: group.name.clone(),
                target: NavigationTarget::Group {
                    source_id: source_id.clone(),
                },
                color: None,
                children: entry_nodes(source_id, &group.entries),
            },
        })
        .collect()
}

fn node_model(nodes: &[NavigationNode]) -> gio::ListStore {
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    for node in nodes {
        model.append(&glib::BoxedAnyObject::new(node.clone()));
    }
    model
}

fn tree_model(roots: gio::ListStore) -> gtk::TreeListModel {
    gtk::TreeListModel::new(roots, false, false, |item| {
        let item = item.downcast_ref::<glib::BoxedAnyObject>()?;
        let node = item.borrow::<NavigationNode>();
        (!node.children.is_empty()).then(|| node_model(&node.children).upcast())
    })
}

fn tree_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| setup_tree_item(item));
    factory.connect_bind(|_, item| bind_tree_item(item));
    factory
}

fn setup_tree_item(item: &glib::Object) {
    let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
    let expander = gtk::TreeExpander::new();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    content.set_margin_start(4);
    content.set_margin_end(8);
    content.set_margin_top(7);
    content.set_margin_bottom(7);

    let icon = gtk::Image::new();
    icon.set_pixel_size(17);
    let swatch = gtk::DrawingArea::new();
    swatch.set_content_width(7);
    swatch.set_content_height(20);
    let label = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .hexpand(true)
        .build();
    content.append(&icon);
    content.append(&swatch);
    content.append(&label);
    expander.set_child(Some(&content));
    item.set_child(Some(&expander));
}

fn bind_tree_item(item: &glib::Object) {
    let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
    let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
        return;
    };
    let Some(node) = row
        .item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|item| item.borrow::<NavigationNode>().clone())
    else {
        return;
    };
    let expander = item
        .child()
        .and_downcast::<gtk::TreeExpander>()
        .expect("tree expander");
    expander.set_list_row(Some(&row));
    let content = expander
        .child()
        .and_downcast::<gtk::Box>()
        .expect("tree content");
    let icon = content
        .first_child()
        .and_downcast::<gtk::Image>()
        .expect("tree icon");
    let swatch = icon
        .next_sibling()
        .and_downcast::<gtk::DrawingArea>()
        .expect("tree swatch");
    let label = swatch
        .next_sibling()
        .and_downcast::<gtk::Label>()
        .expect("tree label");
    let label_text = safe_text(&node.label);
    label.set_label(&label_text);
    label.remove_css_class("notebook-row");
    label.remove_css_class("group-row");
    item.set_selectable(!matches!(node.target, NavigationTarget::Group { .. }));

    match node.target {
        NavigationTarget::Notebook { .. } => {
            icon.set_icon_name(Some("onenote-notebook-symbolic"));
            icon.set_visible(true);
            swatch.set_visible(false);
            label.add_css_class("notebook-row");
        }
        NavigationTarget::Group { .. } => {
            icon.set_icon_name(Some("onenote-folder-symbolic"));
            icon.set_visible(true);
            swatch.set_visible(false);
            label.add_css_class("group-row");
        }
        NavigationTarget::Section { .. } => {
            icon.set_visible(false);
            swatch.set_visible(true);
            set_swatch_color(&swatch, node.color.unwrap_or(DEFAULT_SECTION_COLOR));
        }
    }
}

fn set_swatch_color(swatch: &gtk::DrawingArea, color: Color) {
    swatch.set_draw_func(move |_, cairo, width, height| {
        cairo.set_source_rgba(
            f64::from(color.red) / 255.0,
            f64::from(color.green) / 255.0,
            f64::from(color.blue) / 255.0,
            f64::from(color.alpha) / 255.0,
        );
        let radius = 3.0;
        cairo.new_sub_path();
        cairo.arc(
            f64::from(width) - radius,
            radius,
            radius,
            -std::f64::consts::FRAC_PI_2,
            0.0,
        );
        cairo.arc(
            f64::from(width) - radius,
            f64::from(height) - radius,
            radius,
            0.0,
            std::f64::consts::FRAC_PI_2,
        );
        cairo.line_to(0.0, f64::from(height));
        cairo.line_to(0.0, 0.0);
        cairo.close_path();
        let _ignored = cairo.fill();
    });
}

fn safe_text(value: &str) -> std::borrow::Cow<'_, str> {
    if value.contains('\0') {
        std::borrow::Cow::Owned(value.replace('\0', "\u{fffd}"))
    } else {
        std::borrow::Cow::Borrowed(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        entry_nodes, node_model, tree_model, NavigationNode, NavigationTarget, NotebookTree,
    };
    use gtk::prelude::*;
    use onenote_core::{
        Notebook, NotebookEntry, Section, SectionGroup, SectionId, SourceFingerprint, SourceId,
    };

    #[test]
    fn section_groups_remain_nested() {
        let section = Section {
            id: SectionId::new("section"),
            name: "Nested Section".to_owned(),
            color: None,
            pages: Vec::new(),
            diagnostics: Vec::new(),
        };
        let entries = vec![NotebookEntry::Group(SectionGroup {
            id: SectionId::new("group"),
            name: "Example Group".to_owned(),
            entries: vec![NotebookEntry::Section(section)],
        })];

        let source_id = SourceId::new("source");
        let nodes = entry_nodes(&source_id, &entries);

        assert_eq!(nodes[0].label, "Example Group");
        assert_eq!(nodes[0].children[0].label, "Nested Section");
        assert!(matches!(
            nodes[0].children[0].target,
            NavigationTarget::Section { source_id: ref row_source, .. }
                if row_source == &source_id
        ));
    }

    #[test]
    fn expansion_is_owned_by_the_tree_row() {
        crate::test_support::run_gtk_test(expansion_is_owned_by_the_tree_row_gtk);
    }

    fn expansion_is_owned_by_the_tree_row_gtk() {
        let source_id = SourceId::new("source");
        let child = NavigationNode {
            label: "Section".to_owned(),
            target: NavigationTarget::Section {
                source_id: source_id.clone(),
                section_id: SectionId::new("section"),
            },
            color: None,
            children: Vec::new(),
        };
        let group = NavigationNode {
            label: "Group".to_owned(),
            target: NavigationTarget::Group { source_id },
            color: None,
            children: vec![child],
        };
        let model = tree_model(node_model(&[group]));

        assert_eq!(model.n_items(), 1);
        model.row(0).expect("group row").set_expanded(true);
        assert_eq!(model.n_items(), 2);
        model.row(0).expect("group row").set_expanded(false);
        assert_eq!(model.n_items(), 1);
    }

    #[test]
    fn appending_a_loaded_notebook_preserves_the_active_section() {
        crate::test_support::run_gtk_test(
            appending_a_loaded_notebook_preserves_the_active_section_gtk,
        );
    }

    fn appending_a_loaded_notebook_preserves_the_active_section_gtk() {
        let tree = NotebookTree::new();
        let first = notebook("first", "first-section");
        let second = notebook("second", "second-section");
        tree.upsert(&first);
        tree.select_section(&first.source_id, &SectionId::new("first-section"));
        let selected = tree.selected_target();

        tree.upsert(&second);

        assert_eq!(tree.selected_target(), selected);
    }

    #[test]
    fn rapid_section_selection_is_immediate_and_latest_wins() {
        crate::test_support::run_gtk_test(rapid_section_selection_is_immediate_and_latest_wins_gtk);
    }

    fn rapid_section_selection_is_immediate_and_latest_wins_gtk() {
        let tree = NotebookTree::new();
        let notebook = Notebook {
            entries: vec![
                NotebookEntry::Section(section("first")),
                NotebookEntry::Section(section("second")),
            ],
            ..notebook("source", "unused")
        };
        tree.upsert(&notebook);
        let notifications = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let notifications_for_signal = std::rc::Rc::clone(&notifications);
        tree.selection.connect_selected_notify(move |selection| {
            notifications_for_signal
                .borrow_mut()
                .push(selection.selected());
        });

        let first = tree
            .select_section(&notebook.source_id, &SectionId::new("first"))
            .expect("first section");
        let second = tree
            .select_section(&notebook.source_id, &SectionId::new("second"))
            .expect("second section");

        assert_ne!(first, second);
        assert_eq!(tree.selection.selected(), second);
        assert_eq!(notifications.borrow().as_slice(), &[first, second]);
        assert!(matches!(
            tree.selected_target(),
            Some(NavigationTarget::Section { section_id, .. })
                if section_id == SectionId::new("second")
        ));
        assert!(!tree.view.is_single_click_activate());
    }

    fn notebook(source_id: &str, section_id: &str) -> Notebook {
        Notebook {
            source_id: SourceId::new(source_id),
            fingerprint: SourceFingerprint::new("fingerprint"),
            name: source_id.to_owned(),
            color: None,
            entries: vec![NotebookEntry::Section(section(section_id))],
            diagnostics: Vec::new(),
        }
    }

    fn section(section_id: &str) -> Section {
        Section {
            id: SectionId::new(section_id),
            name: section_id.to_owned(),
            color: None,
            pages: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

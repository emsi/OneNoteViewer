use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{Color, Notebook, NotebookEntry, SectionId};

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
        source: usize,
    },
    Group {
        source: usize,
    },
    Section {
        source: usize,
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
        let factory = tree_factory();
        let view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(true)
            .css_classes(["notebook-tree"])
            .build();
        let model_for_activation = model.clone();
        view.connect_activate(move |_, position| {
            toggle_expansion(&model_for_activation, position);
        });
        Self {
            roots,
            model,
            selection,
            view,
        }
    }

    pub(crate) fn rebuild<'a>(&self, notebooks: impl IntoIterator<Item = (usize, &'a Notebook)>) {
        self.selection.set_selected(gtk::INVALID_LIST_POSITION);
        self.roots.remove_all();
        for (source, notebook) in notebooks {
            let node = NavigationNode {
                label: notebook.name.clone(),
                target: NavigationTarget::Notebook { source },
                color: Some(notebook.color.unwrap_or(DEFAULT_NOTEBOOK_COLOR)),
                children: entry_nodes(source, &notebook.entries),
            };
            self.roots.append(&glib::BoxedAnyObject::new(node));
        }
        self.expand_notebooks();
    }

    pub(crate) fn target_at(&self, position: u32) -> Option<NavigationTarget> {
        let row = self.model.row(position)?;
        let item = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
        let target = item.borrow::<NavigationNode>().target.clone();
        Some(target)
    }

    pub(crate) fn select_section(&self, source: usize, section_id: &SectionId) -> Option<u32> {
        loop {
            if let Some(position) = (0..self.model.n_items()).find(|position| {
                matches!(
                    self.target_at(*position),
                    Some(NavigationTarget::Section {
                        source: row_source,
                        section_id: ref row_section,
                    }) if row_source == source && row_section == section_id
                )
            }) {
                self.selection.set_selected(position);
                return Some(position);
            }
            let expandable = (0..self.model.n_items()).find(|position| {
                self.row_node(*position).is_some_and(|node| {
                    node.contains_section(source, section_id)
                        && self
                            .model
                            .row(*position)
                            .is_some_and(|row| row.is_expandable() && !row.is_expanded())
                })
            })?;
            self.model.row(expandable)?.set_expanded(true);
        }
    }

    pub(crate) fn select_notebook(&self, source: usize) -> Option<u32> {
        let position = (0..self.model.n_items()).find(|position| {
            matches!(
                self.target_at(*position),
                Some(NavigationTarget::Notebook { source: row_source }) if row_source == source
            )
        })?;
        self.selection.set_selected(position);
        Some(position)
    }

    fn expand_notebooks(&self) {
        let mut position = 0;
        while position < self.model.n_items() {
            let Some(row) = self.model.row(position) else {
                break;
            };
            if matches!(
                self.target_at(position),
                Some(NavigationTarget::Notebook { .. })
            ) {
                row.set_expanded(true);
            }
            position += 1;
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
    fn contains_section(&self, source: usize, section_id: &SectionId) -> bool {
        matches!(
            self.target,
            NavigationTarget::Section {
                source: row_source,
                section_id: ref row_section,
            } if row_source == source && row_section == section_id
        ) || self
            .children
            .iter()
            .any(|child| child.contains_section(source, section_id))
    }
}

fn entry_nodes(source: usize, entries: &[NotebookEntry]) -> Vec<NavigationNode> {
    entries
        .iter()
        .map(|entry| match entry {
            NotebookEntry::Section(section) => NavigationNode {
                label: section.name.clone(),
                target: NavigationTarget::Section {
                    source,
                    section_id: section.id.clone(),
                },
                color: Some(section.color.unwrap_or(DEFAULT_SECTION_COLOR)),
                children: Vec::new(),
            },
            NotebookEntry::Group(group) => NavigationNode {
                label: group.name.clone(),
                target: NavigationTarget::Group { source },
                color: None,
                children: entry_nodes(source, &group.entries),
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

fn toggle_expansion(model: &gtk::TreeListModel, position: u32) -> bool {
    let Some(row) = model.row(position) else {
        return false;
    };
    if !row.is_expandable() {
        return false;
    }
    row.set_expanded(!row.is_expanded());
    true
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
        entry_nodes, node_model, toggle_expansion, tree_model, NavigationNode, NavigationTarget,
    };
    use gtk::prelude::*;
    use onenote_core::{NotebookEntry, Section, SectionGroup, SectionId};

    #[test]
    fn section_groups_remain_nested() {
        let section = Section {
            id: SectionId::new("section"),
            name: "Install Log".to_owned(),
            color: None,
            pages: Vec::new(),
            diagnostics: Vec::new(),
        };
        let entries = vec![NotebookEntry::Group(SectionGroup {
            id: SectionId::new("group"),
            name: "OpenStack".to_owned(),
            entries: vec![NotebookEntry::Section(section)],
        })];

        let nodes = entry_nodes(3, &entries);

        assert_eq!(nodes[0].label, "OpenStack");
        assert_eq!(nodes[0].children[0].label, "Install Log");
        assert!(matches!(
            nodes[0].children[0].target,
            NavigationTarget::Section { source: 3, .. }
        ));
    }

    #[test]
    fn activating_expandable_row_toggles_its_children() {
        if gtk::init().is_err() {
            return;
        }
        let child = NavigationNode {
            label: "Section".to_owned(),
            target: NavigationTarget::Section {
                source: 0,
                section_id: SectionId::new("section"),
            },
            color: None,
            children: Vec::new(),
        };
        let group = NavigationNode {
            label: "Group".to_owned(),
            target: NavigationTarget::Group { source: 0 },
            color: None,
            children: vec![child],
        };
        let model = tree_model(node_model(&[group]));

        assert_eq!(model.n_items(), 1);
        assert!(toggle_expansion(&model, 0));
        assert_eq!(model.n_items(), 2);
        assert!(toggle_expansion(&model, 0));
        assert_eq!(model.n_items(), 1);
    }
}

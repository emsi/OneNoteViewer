use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use onenote_core::{Color, Notebook, NotebookEntry, SectionId, SourceId};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

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
        group_id: SectionId,
    },
    Section {
        source_id: SourceId,
        section_id: SectionId,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SourceTreeExpansion {
    pub(crate) notebook_expanded: bool,
    pub(crate) expanded_groups: BTreeSet<SectionId>,
}

#[derive(Default)]
struct ExpansionRegistry {
    states: RefCell<BTreeMap<SourceId, SourceTreeExpansion>>,
    suppress_notifications: Cell<bool>,
    changed: RefCell<Option<Box<dyn Fn()>>>,
}

impl ExpansionRegistry {
    fn record(&self, target: &NavigationTarget, expanded: bool) {
        if self.suppress_notifications.get() {
            return;
        }
        let source_id = target.source_id().clone();
        let mut states = self.states.borrow_mut();
        let state = states.entry(source_id).or_default();
        match target {
            NavigationTarget::Notebook { .. } => state.notebook_expanded = expanded,
            NavigationTarget::Group { group_id, .. } => {
                if expanded {
                    state.expanded_groups.insert(group_id.clone());
                } else {
                    state.expanded_groups.remove(group_id);
                }
            }
            NavigationTarget::Section { .. } => return,
        }
        drop(states);
        if let Some(changed) = self.changed.borrow().as_ref() {
            changed();
        }
    }
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
    expansion: Rc<ExpansionRegistry>,
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
        let expansion = Rc::new(ExpansionRegistry::default());
        let factory = tree_factory(model.clone(), Rc::clone(&expansion));
        view.set_factory(Some(&factory));
        Self {
            roots,
            model,
            selection,
            view,
            expansion,
        }
    }

    pub(crate) fn connect_expansion_changed(&self, changed: impl Fn() + 'static) {
        *self.expansion.changed.borrow_mut() = Some(Box::new(changed));
    }

    pub(crate) fn upsert(&self, notebook: &Notebook, restored: Option<SourceTreeExpansion>) {
        let source_id = notebook.source_id.clone();
        let node = notebook_node(notebook);
        let mut desired = restored
            .or_else(|| self.expansion.states.borrow().get(&source_id).cloned())
            .unwrap_or(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: BTreeSet::new(),
            });
        desired
            .expanded_groups
            .retain(|group_id| node.contains_group(group_id));
        self.expansion
            .states
            .borrow_mut()
            .insert(source_id.clone(), desired.clone());
        let item = glib::BoxedAnyObject::new(node);
        if let Some(position) = self.root_position(&source_id) {
            self.roots.splice(position, 1, &[item]);
        } else {
            self.roots.append(&item);
        }
        self.apply_expansion(&source_id, &desired);
    }

    pub(crate) fn remove(&self, source_id: &SourceId) -> bool {
        let Some(position) = self.root_position(source_id) else {
            return false;
        };
        self.roots.remove(position);
        self.expansion.states.borrow_mut().remove(source_id);
        true
    }

    pub(crate) fn expansion_state(&self, source_id: &SourceId) -> Option<SourceTreeExpansion> {
        self.expansion.states.borrow().get(source_id).cloned()
    }

    pub(crate) fn restore_expansion(&self, source_id: &SourceId, mut state: SourceTreeExpansion) {
        if let Some(node) = self.root_node(source_id) {
            state
                .expanded_groups
                .retain(|group_id| node.contains_group(group_id));
        } else {
            return;
        }
        self.expansion
            .states
            .borrow_mut()
            .insert(source_id.clone(), state.clone());
        self.apply_expansion(source_id, &state);
    }

    pub(crate) fn target_at(&self, position: u32) -> Option<NavigationTarget> {
        model_target_at(&self.model, position)
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
            self.set_row_expanded(expandable, true)?;
        }
    }

    pub(crate) fn select_visible_section(
        &self,
        source_id: &SourceId,
        section_id: &SectionId,
    ) -> Option<u32> {
        let position = (0..self.model.n_items()).find(|position| {
            matches!(
                self.target_at(*position),
                Some(NavigationTarget::Section {
                    source_id: ref row_source,
                    section_id: ref row_section,
                }) if row_source == source_id && row_section == section_id
            )
        })?;
        self.selection.set_selected(position);
        Some(position)
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

    fn row_node(&self, position: u32) -> Option<NavigationNode> {
        let row = self.model.row(position)?;
        let item = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
        let node = item.borrow::<NavigationNode>().clone();
        Some(node)
    }

    fn root_node(&self, source_id: &SourceId) -> Option<NavigationNode> {
        let position = self.root_position(source_id)?;
        let item = self
            .roots
            .item(position)?
            .downcast::<glib::BoxedAnyObject>()
            .ok()?;
        let node = item.borrow::<NavigationNode>().clone();
        Some(node)
    }

    fn set_row_expanded(&self, position: u32, expanded: bool) -> Option<()> {
        let target = self.target_at(position)?;
        let row = self.model.row(position)?;
        let previous = self.expansion.suppress_notifications.replace(true);
        row.set_expanded(expanded);
        self.expansion.suppress_notifications.set(previous);
        self.expansion.record(&target, expanded);
        if expanded {
            if let Some(state) = self
                .expansion
                .states
                .borrow()
                .get(target.source_id())
                .cloned()
            {
                apply_expansion_to_model(&self.model, &self.expansion, target.source_id(), &state);
            }
        }
        Some(())
    }

    fn apply_expansion(&self, source_id: &SourceId, state: &SourceTreeExpansion) {
        apply_expansion_to_model(&self.model, &self.expansion, source_id, state);
    }
}

fn model_target_at(model: &gtk::TreeListModel, position: u32) -> Option<NavigationTarget> {
    let row = model.row(position)?;
    let item = row.item()?.downcast::<glib::BoxedAnyObject>().ok()?;
    let target = item.borrow::<NavigationNode>().target.clone();
    Some(target)
}

fn apply_expansion_to_model(
    model: &gtk::TreeListModel,
    expansion: &ExpansionRegistry,
    source_id: &SourceId,
    state: &SourceTreeExpansion,
) {
    let previous = expansion.suppress_notifications.replace(true);
    if let Some(position) = (0..model.n_items()).find(|position| {
        matches!(
            model_target_at(model, *position),
            Some(NavigationTarget::Notebook { source_id: ref row_source })
                if row_source == source_id
        )
    }) {
        if let Some(row) = model.row(position) {
            row.set_expanded(state.notebook_expanded);
        }
    }
    if state.notebook_expanded {
        let mut position = 0;
        while position < model.n_items() {
            if let Some(NavigationTarget::Group {
                source_id: row_source,
                group_id,
            }) = model_target_at(model, position)
            {
                if row_source == *source_id {
                    if let Some(row) = model.row(position) {
                        row.set_expanded(state.expanded_groups.contains(&group_id));
                    }
                }
            }
            position += 1;
        }
    }
    expansion.suppress_notifications.set(previous);
}

impl NavigationTarget {
    fn source_id(&self) -> &SourceId {
        match self {
            Self::Notebook { source_id }
            | Self::Group { source_id, .. }
            | Self::Section { source_id, .. } => source_id,
        }
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

    fn contains_group(&self, group_id: &SectionId) -> bool {
        matches!(
            self.target,
            NavigationTarget::Group {
                group_id: ref row_group,
                ..
            } if row_group == group_id
        ) || self
            .children
            .iter()
            .any(|child| child.contains_group(group_id))
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
                    group_id: group.id.clone(),
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

fn tree_factory(
    model: gtk::TreeListModel,
    expansion: Rc<ExpansionRegistry>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| setup_tree_item(item));
    factory.connect_bind(move |_, item| bind_tree_item(item, &model, &expansion));
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

fn bind_tree_item(
    item: &glib::Object,
    model: &gtk::TreeListModel,
    expansion: &Rc<ExpansionRegistry>,
) {
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
    if !matches!(node.target, NavigationTarget::Section { .. }) {
        let target = node.target.clone();
        let expansion = Rc::clone(expansion);
        let model = model.clone();
        row.connect_expanded_notify(move |row| {
            if expansion.suppress_notifications.get() {
                return;
            }
            let row_expanded = row.is_expanded();
            expansion.record(&target, row_expanded);
            if row_expanded {
                let state = expansion.states.borrow().get(target.source_id()).cloned();
                if let Some(state) = state {
                    apply_expansion_to_model(&model, &expansion, target.source_id(), &state);
                }
            }
        });
    }
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
        SourceTreeExpansion,
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
            target: NavigationTarget::Group {
                source_id,
                group_id: SectionId::new("group"),
            },
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
        tree.upsert(&first, None);
        tree.select_section(&first.source_id, &SectionId::new("first-section"));
        let selected = tree.selected_target();

        tree.upsert(&second, None);

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
        tree.upsert(&notebook, None);
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

    #[test]
    fn restored_groups_follow_identity_across_rename_reorder_and_duplicate_labels() {
        crate::test_support::run_gtk_test(
            restored_groups_follow_identity_across_rename_reorder_and_duplicate_labels_gtk,
        );
    }

    fn restored_groups_follow_identity_across_rename_reorder_and_duplicate_labels_gtk() {
        let tree = NotebookTree::new();
        let source_id = SourceId::new("source");
        let original =
            notebook_with_groups("source", &[("first", "Duplicate"), ("second", "Duplicate")]);
        tree.upsert(
            &original,
            Some(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: [SectionId::new("second")].into_iter().collect(),
            }),
        );

        let reordered =
            notebook_with_groups("source", &[("second", "Renamed"), ("first", "Duplicate")]);
        tree.upsert(&reordered, None);

        assert_eq!(
            tree.expansion_state(&source_id),
            Some(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: [SectionId::new("second")].into_iter().collect(),
            })
        );
        assert!(group_is_expanded(&tree, &source_id, "second"));
        assert!(!group_is_expanded(&tree, &source_id, "first"));
    }

    #[test]
    fn removed_groups_are_pruned_without_losing_latent_collapsed_state() {
        crate::test_support::run_gtk_test(
            removed_groups_are_pruned_without_losing_latent_collapsed_state_gtk,
        );
    }

    fn removed_groups_are_pruned_without_losing_latent_collapsed_state_gtk() {
        let tree = NotebookTree::new();
        let source_id = SourceId::new("source");
        let notebook = notebook_with_groups("source", &[("kept", "Kept")]);
        tree.upsert(
            &notebook,
            Some(SourceTreeExpansion {
                notebook_expanded: false,
                expanded_groups: [SectionId::new("kept"), SectionId::new("missing")]
                    .into_iter()
                    .collect(),
            }),
        );

        assert_eq!(
            tree.expansion_state(&source_id),
            Some(SourceTreeExpansion {
                notebook_expanded: false,
                expanded_groups: [SectionId::new("kept")].into_iter().collect(),
            })
        );
        assert_eq!(tree.model.n_items(), 1);

        tree.set_row_expanded(0, true).expect("notebook row");

        assert!(group_is_expanded(&tree, &source_id, "kept"));
    }

    #[test]
    fn equal_group_ids_are_scoped_to_their_source() {
        crate::test_support::run_gtk_test(equal_group_ids_are_scoped_to_their_source_gtk);
    }

    fn equal_group_ids_are_scoped_to_their_source_gtk() {
        let tree = NotebookTree::new();
        let expanded = notebook_with_groups("expanded", &[("shared", "Group")]);
        let collapsed = notebook_with_groups("collapsed", &[("shared", "Group")]);
        tree.upsert(
            &expanded,
            Some(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: [SectionId::new("shared")].into_iter().collect(),
            }),
        );
        tree.upsert(
            &collapsed,
            Some(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: std::collections::BTreeSet::new(),
            }),
        );

        assert!(group_is_expanded(
            &tree,
            &SourceId::new("expanded"),
            "shared"
        ));
        assert!(!group_is_expanded(
            &tree,
            &SourceId::new("collapsed"),
            "shared"
        ));
    }

    #[test]
    fn selecting_a_hidden_section_records_the_revealed_ancestry() {
        crate::test_support::run_gtk_test(
            selecting_a_hidden_section_records_the_revealed_ancestry_gtk,
        );
    }

    fn selecting_a_hidden_section_records_the_revealed_ancestry_gtk() {
        let tree = NotebookTree::new();
        let notebook = notebook_with_groups("source", &[("group", "Group")]);
        tree.upsert(&notebook, None);

        assert!(tree
            .select_section(&notebook.source_id, &SectionId::new("group-section"))
            .is_some());
        assert_eq!(
            tree.expansion_state(&notebook.source_id),
            Some(SourceTreeExpansion {
                notebook_expanded: true,
                expanded_groups: [SectionId::new("group")].into_iter().collect(),
            })
        );
    }

    fn group_is_expanded(tree: &NotebookTree, source_id: &SourceId, group_id: &str) -> bool {
        (0..tree.model.n_items()).any(|position| {
            matches!(
                tree.target_at(position),
                Some(NavigationTarget::Group {
                    source_id: ref row_source,
                    group_id: ref row_group,
                }) if row_source == source_id && row_group == &SectionId::new(group_id)
            ) && tree
                .model
                .row(position)
                .is_some_and(|row| row.is_expanded())
        })
    }

    fn notebook_with_groups(source_id: &str, groups: &[(&str, &str)]) -> Notebook {
        Notebook {
            source_id: SourceId::new(source_id),
            fingerprint: SourceFingerprint::new("fingerprint"),
            name: source_id.to_owned(),
            color: None,
            entries: groups
                .iter()
                .map(|(id, name)| {
                    NotebookEntry::Group(SectionGroup {
                        id: SectionId::new(*id),
                        name: (*name).to_owned(),
                        entries: vec![NotebookEntry::Section(section(&format!("{id}-section")))],
                    })
                })
                .collect(),
            diagnostics: Vec::new(),
        }
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

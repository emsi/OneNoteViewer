use crate::settings::ThemePreference;
use gtk::gio;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

type ErrorHandler = Rc<dyn Fn(&str, &str)>;

pub(crate) fn present_about(parent_window: &gtk::ApplicationWindow) {
    let system_information = format!(
        "Version: {}\nSource revision: {}\nApplication ID: io.github.emsi.OneNoteViewer",
        env!("CARGO_PKG_VERSION"),
        crate::SOURCE_REVISION,
    );
    let dialog = gtk::AboutDialog::builder()
        .transient_for(parent_window)
        .modal(true)
        .program_name("OneNote Viewer")
        .version(env!("CARGO_PKG_VERSION"))
        .comments("A native Linux viewer for local Microsoft OneNote notebooks.")
        .copyright("Copyright (c) 2026 Mariusz Woloszyn")
        .authors(["Mariusz Woloszyn"])
        .license_type(gtk::License::Gpl30)
        .logo_icon_name("io.github.emsi.OneNoteViewer")
        .website("https://github.com/emsi/OneNoteViewer")
        .website_label("Project website")
        .system_information(system_information)
        .build();
    dialog.add_css_class("settings-dialog");
    dialog.present();
}

#[allow(clippy::too_many_lines)]
pub(crate) fn present_package_import<F, E>(
    parent_window: &gtk::ApplicationWindow,
    package: PathBuf,
    default_parent: PathBuf,
    on_import: F,
    on_error: E,
) where
    F: Fn(PathBuf, PathBuf) + 'static,
    E: Fn(&str, &str) + 'static,
{
    let folder_name = package_folder_name(&package);
    let fallback_parent = default_parent.clone();
    let parent = Rc::new(RefCell::new(default_parent));
    let on_error: ErrorHandler = Rc::new(on_error);

    let dialog = gtk::Window::builder()
        .title("Import OneNote Package")
        .transient_for(parent_window)
        .modal(true)
        .resizable(false)
        .default_width(620)
        .build();
    dialog.add_css_class("settings-dialog");

    let content = dialog_content(16);
    let heading = gtk::Label::builder()
        .label(format!("Import {folder_name}"))
        .xalign(0.0)
        .selectable(true)
        .build();
    heading.add_css_class("dialog-title");
    content.append(&heading);

    let package_label = gtk::Label::builder()
        .label(package.display().to_string())
        .xalign(0.0)
        .selectable(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    package_label.add_css_class("dim-label");
    content.append(&package_label);

    let destination_heading = gtk::Label::builder()
        .label("Notebook folder")
        .xalign(0.0)
        .selectable(true)
        .build();
    destination_heading.add_css_class("field-label");
    content.append(&destination_heading);

    let destination_label = path_label(None);
    content.append(&destination_label);

    let explanation = gtk::Label::builder()
        .label(
            "A new notebook folder will be created at the path above. \
             Change Location selects a different parent for this import.",
        )
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    explanation.add_css_class("dim-label");
    content.append(&explanation);

    let conflict = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    conflict.add_css_class("warning-label");
    content.append(&conflict);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let change_location = gtk::Button::with_label("Change Location");
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let cancel = gtk::Button::with_label("Cancel");
    let import = gtk::Button::with_label("Import");
    import.add_css_class("suggested-action");
    actions.append(&change_location);
    actions.append(&spacer);
    actions.append(&cancel);
    actions.append(&import);
    content.append(&actions);

    update_import_destination(
        &parent.borrow(),
        &folder_name,
        &destination_label,
        &conflict,
        &import,
    );

    let dialog_on_cancel = dialog.clone();
    cancel.connect_clicked(move |_| dialog_on_cancel.close());

    let dialog_for_chooser = dialog.clone();
    let parent_for_chooser = Rc::clone(&parent);
    let folder_name_for_chooser = folder_name.clone();
    let destination_for_chooser = destination_label.clone();
    let conflict_for_chooser = conflict.clone();
    let import_for_chooser = import.clone();
    let error_for_chooser = Rc::clone(&on_error);
    change_location.connect_clicked(move |button| {
        let parent = Rc::clone(&parent_for_chooser);
        let folder_name = folder_name_for_chooser.clone();
        let destination = destination_for_chooser.clone();
        let conflict = conflict_for_chooser.clone();
        let import = import_for_chooser.clone();
        let on_error = Rc::clone(&error_for_chooser);
        let preferred = parent.borrow().clone();
        let title = format!("Choose where to create \"{folder_name_for_chooser}\"");
        select_folder(
            FolderDialogRequest {
                owner: &dialog_for_chooser,
                trigger: button,
                title: &title,
                accept_label: "Choose This Location",
                preferred: &preferred,
                fallback: &fallback_parent,
                error_title: "Could not select package destination",
            },
            move |path| {
                *parent.borrow_mut() = path;
                update_import_destination(
                    &parent.borrow(),
                    &folder_name,
                    &destination,
                    &conflict,
                    &import,
                );
            },
            on_error,
        );
    });

    let dialog_on_import = dialog.clone();
    import.connect_clicked(move |_| {
        let destination = parent.borrow().join(&folder_name);
        dialog_on_import.close();
        on_import(package.clone(), destination);
    });

    dialog.set_child(Some(&content));
    import.grab_focus();
    dialog.present();
}

#[allow(clippy::too_many_lines)]
pub(crate) fn present_settings<F, E>(
    parent_window: &gtk::ApplicationWindow,
    current: PathBuf,
    default: PathBuf,
    current_theme: ThemePreference,
    detect_plain_text_links: bool,
    on_save: F,
    on_error: E,
) where
    F: Fn(&Path, ThemePreference, bool) -> bool + 'static,
    E: Fn(&str, &str) + 'static,
{
    let candidate = Rc::new(RefCell::new(current));
    let on_error: ErrorHandler = Rc::new(on_error);
    let dialog = gtk::Window::builder()
        .title("Settings")
        .transient_for(parent_window)
        .modal(true)
        .resizable(false)
        .default_width(660)
        .build();
    dialog.add_css_class("settings-dialog");

    let content = dialog_content(18);
    let heading = gtk::Label::builder()
        .label("Notebook Storage")
        .xalign(0.0)
        .selectable(true)
        .build();
    heading.add_css_class("dialog-title");
    content.append(&heading);

    let description = gtk::Label::builder()
        .label(
            "Notebook folders in this location open automatically. \
             Notebooks opened elsewhere remain in their original locations.",
        )
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .build();
    description.add_css_class("dim-label");
    content.append(&description);

    let field_heading = gtk::Label::builder()
        .label("Default notebooks location")
        .xalign(0.0)
        .selectable(true)
        .build();
    field_heading.add_css_class("field-label");
    content.append(&field_heading);

    let path_label = path_label(Some(&candidate.borrow()));
    content.append(&path_label);

    let location_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let choose = gtk::Button::with_label("Choose Location");
    let reset = gtk::Button::with_label("Reset to Default");
    location_actions.append(&choose);
    location_actions.append(&reset);
    content.append(&location_actions);

    let appearance_heading = gtk::Label::builder()
        .label("Appearance")
        .xalign(0.0)
        .selectable(true)
        .build();
    appearance_heading.add_css_class("field-label");
    content.append(&appearance_heading);

    let theme = gtk::DropDown::from_strings(&["System", "Light", "Dark"]);
    theme.set_selected(current_theme.selected());
    theme.set_tooltip_text(Some("Choose the application color theme"));
    theme.set_hexpand(true);
    content.append(&theme);

    let links_heading = gtk::Label::builder()
        .label("Link Handling")
        .xalign(0.0)
        .selectable(true)
        .build();
    links_heading.add_css_class("field-label");
    content.append(&links_heading);

    let link_detection = gtk::CheckButton::with_label("Detect links in plain text");
    link_detection.set_active(detect_plain_text_links);
    link_detection.set_tooltip_text(Some(
        "Recognize visible URLs and email addresses without changing explicit OneNote links",
    ));
    content.append(&link_detection);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Cancel");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    actions.append(&cancel);
    actions.append(&save);
    content.append(&actions);

    let dialog_on_cancel = dialog.clone();
    cancel.connect_clicked(move |_| dialog_on_cancel.close());

    let candidate_for_reset = Rc::clone(&candidate);
    let path_for_reset = path_label.clone();
    let default_for_reset = default.clone();
    reset.connect_clicked(move |_| {
        path_for_reset.set_label(&default_for_reset.display().to_string());
        candidate_for_reset
            .borrow_mut()
            .clone_from(&default_for_reset);
    });

    let dialog_for_chooser = dialog.clone();
    let candidate_for_chooser = Rc::clone(&candidate);
    let path_for_chooser = path_label.clone();
    let error_for_chooser = Rc::clone(&on_error);
    choose.connect_clicked(move |button| {
        let candidate = Rc::clone(&candidate_for_chooser);
        let path_label = path_for_chooser.clone();
        let on_error = Rc::clone(&error_for_chooser);
        let preferred = candidate.borrow().clone();
        select_folder(
            FolderDialogRequest {
                owner: &dialog_for_chooser,
                trigger: button,
                title: "Choose default notebooks location",
                accept_label: "Use This Location",
                preferred: &preferred,
                fallback: &default,
                error_title: "Could not select default notebooks location",
            },
            move |path| {
                path_label.set_label(&path.display().to_string());
                *candidate.borrow_mut() = path;
            },
            on_error,
        );
    });

    let dialog_on_save = dialog.clone();
    save.connect_clicked(move |_| {
        if on_save(
            &candidate.borrow(),
            ThemePreference::from_selected(theme.selected()),
            link_detection.is_active(),
        ) {
            dialog_on_save.close();
        }
    });

    dialog.set_child(Some(&content));
    save.grab_focus();
    dialog.present();
}

fn dialog_content(spacing: i32) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, spacing);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content
}

fn path_label(path: Option<&Path>) -> gtk::Label {
    let label = gtk::Label::builder()
        .xalign(0.0)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    if let Some(path) = path {
        label.set_label(&path.display().to_string());
    }
    label.add_css_class("path-value");
    label
}

#[derive(Clone, Copy)]
struct FolderDialogRequest<'a> {
    owner: &'a gtk::Window,
    trigger: &'a gtk::Button,
    title: &'a str,
    accept_label: &'a str,
    preferred: &'a Path,
    fallback: &'a Path,
    error_title: &'static str,
}

fn select_folder<F>(request: FolderDialogRequest<'_>, on_selected: F, on_error: ErrorHandler)
where
    F: Fn(PathBuf) + 'static,
{
    let initial = gio::File::for_path(chooser_initial_folder(request.preferred, request.fallback));
    let chooser = gtk::FileDialog::builder()
        .title(request.title)
        .accept_label(request.accept_label)
        .initial_folder(&initial)
        .modal(true)
        .build();
    let original_label = request
        .trigger
        .label()
        .unwrap_or_else(|| gtk::glib::GString::from("Choose Location"));
    request.trigger.set_label("Opening...");
    request.trigger.set_sensitive(false);
    let trigger = request.trigger.clone();
    let error_title = request.error_title;
    chooser.select_folder(
        Some(request.owner),
        None::<&gio::Cancellable>,
        move |result| {
            trigger.set_label(&original_label);
            trigger.set_sensitive(true);
            match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        on_selected(path);
                    }
                }
                Err(error) if error.matches(gtk::DialogError::Dismissed) => {}
                Err(error) => on_error(error_title, &error.to_string()),
            }
        },
    );
}

fn chooser_initial_folder(preferred: &Path, fallback: &Path) -> PathBuf {
    if preferred.is_dir() {
        preferred.to_path_buf()
    } else if fallback.is_dir() {
        fallback.to_path_buf()
    } else {
        gtk::glib::home_dir()
    }
}

fn package_folder_name(package: &Path) -> String {
    package
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("OneNote Notebook")
        .to_owned()
}

fn update_import_destination(
    parent: &Path,
    folder_name: &str,
    destination_label: &gtk::Label,
    conflict: &gtk::Label,
    import: &gtk::Button,
) {
    let destination = parent.join(folder_name);
    destination_label.set_label(&destination.display().to_string());
    let exists = destination.exists();
    conflict.set_visible(exists);
    conflict.set_label(if exists {
        "This notebook folder already exists. Choose another location before importing."
    } else {
        ""
    });
    import.set_sensitive(!exists);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooser_uses_existing_preferred_folder() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let preferred = temporary.path().join("preferred");
        let fallback = temporary.path().join("fallback");
        std::fs::create_dir_all(&preferred).expect("preferred");
        std::fs::create_dir_all(&fallback).expect("fallback");

        assert_eq!(chooser_initial_folder(&preferred, &fallback), preferred);
    }

    #[test]
    fn chooser_uses_default_when_preferred_folder_is_invalid() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let preferred = temporary.path().join("missing");
        let fallback = temporary.path().join("default-notebooks");
        std::fs::create_dir_all(&fallback).expect("fallback");

        assert_eq!(chooser_initial_folder(&preferred, &fallback), fallback);
    }
}

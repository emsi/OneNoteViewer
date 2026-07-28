use onenote_core::{
    ElementContent, Image, Ink, Notebook, NotebookEntry, ObjectKind, Outline, OutlineElement, Page,
    PageObject, Section, Table,
};
use std::collections::HashMap;

pub(crate) struct PageDocument<'a> {
    pub(crate) notebook_name: &'a str,
    pub(crate) section: &'a Section,
    pub(crate) path: String,
    pub(crate) page: &'a Page,
    pub(crate) body: String,
    pub(crate) alt_text: String,
    pub(crate) ink_text: String,
    pub(crate) attachments: String,
    pub(crate) links: String,
    pub(crate) objects: Vec<ObjectDocument<'a>>,
}

pub(crate) struct ObjectDocument<'a> {
    pub(crate) object: &'a PageObject,
    pub(crate) text: String,
}

pub(crate) fn documents(notebook: &Notebook) -> Vec<PageDocument<'_>> {
    fn visit<'a>(
        notebook: &'a Notebook,
        entries: &'a [NotebookEntry],
        groups: &mut Vec<&'a str>,
        seen_pages: &mut HashMap<&'a str, usize>,
        output: &mut Vec<PageDocument<'a>>,
    ) {
        for entry in entries {
            match entry {
                NotebookEntry::Section(section) => {
                    let mut path_parts = Vec::with_capacity(groups.len() + 2);
                    path_parts.push(notebook.name.as_str());
                    path_parts.extend(groups.iter().copied());
                    path_parts.push(section.name.as_str());
                    let path = path_parts.join(" / ");
                    for page in &section.pages {
                        if let Some(index) = seen_pages.get(page.id.as_str()).copied() {
                            if timestamp_sort_key(&page.updated_at)
                                > timestamp_sort_key(&output[index].page.updated_at)
                            {
                                output[index] =
                                    project_page(&notebook.name, section, path.clone(), page);
                            }
                        } else {
                            seen_pages.insert(page.id.as_str(), output.len());
                            output.push(project_page(&notebook.name, section, path.clone(), page));
                        }
                    }
                }
                NotebookEntry::Group(group) => {
                    groups.push(&group.name);
                    visit(notebook, &group.entries, groups, seen_pages, output);
                    groups.pop();
                }
            }
        }
    }

    let mut output = Vec::new();
    visit(
        notebook,
        &notebook.entries,
        &mut Vec::new(),
        &mut HashMap::new(),
        &mut output,
    );
    output
}

fn timestamp_sort_key(value: &str) -> [u32; 7] {
    let mut key = [0; 7];
    for (slot, component) in key.iter_mut().zip(
        value
            .split(|character: char| !character.is_ascii_digit())
            .filter(|component| !component.is_empty()),
    ) {
        *slot = component.parse().unwrap_or_default();
    }
    key
}

fn project_page<'a>(
    notebook_name: &'a str,
    section: &'a Section,
    path: String,
    page: &'a Page,
) -> PageDocument<'a> {
    let mut fields = TextFields::default();
    let objects = page
        .objects
        .iter()
        .map(|object| {
            visit_object(object, &mut fields);
            ObjectDocument {
                object,
                text: object.visible_text(),
            }
        })
        .collect();
    PageDocument {
        notebook_name,
        section,
        path,
        page,
        body: fields.body.join("\n"),
        alt_text: fields.alt_text.join("\n"),
        ink_text: fields.ink_text.join("\n"),
        attachments: fields.attachments.join("\n"),
        links: fields.links.join("\n"),
        objects,
    }
}

#[derive(Default)]
struct TextFields {
    body: Vec<String>,
    alt_text: Vec<String>,
    ink_text: Vec<String>,
    attachments: Vec<String>,
    links: Vec<String>,
}

fn visit_object(object: &PageObject, fields: &mut TextFields) {
    match &object.kind {
        ObjectKind::Outline(outline) => visit_outline(outline, fields),
        ObjectKind::Image(image) => visit_image(image, fields),
        ObjectKind::Attachment(attachment) => {
            push(&mut fields.attachments, &attachment.resource.name);
            push(&mut fields.attachments, &attachment.resource.media_type);
        }
        ObjectKind::Ink(ink) => visit_ink(ink, fields),
        ObjectKind::Unknown => {}
    }
}

fn visit_outline(outline: &Outline, fields: &mut TextFields) {
    for element in &outline.elements {
        visit_element(element, fields);
    }
}

fn visit_element(element: &OutlineElement, fields: &mut TextFields) {
    for content in &element.content {
        match content {
            ElementContent::Text(text) => push(&mut fields.body, &text.visible_text()),
            ElementContent::Table(table) => visit_table(table, fields),
            ElementContent::Image(image) => visit_image(image, fields),
            ElementContent::Attachment(attachment) => {
                push(&mut fields.attachments, &attachment.resource.name);
                push(&mut fields.attachments, &attachment.resource.media_type);
            }
            ElementContent::Ink(ink) => visit_ink(ink, fields),
            ElementContent::Unknown => {}
        }
    }
    for child in &element.children {
        visit_element(child, fields);
    }
}

fn visit_table(table: &Table, fields: &mut TextFields) {
    for row in &table.rows {
        for cell in row {
            for element in &cell.elements {
                visit_element(element, fields);
            }
        }
    }
}

fn visit_image(image: &Image, fields: &mut TextFields) {
    if let Some(text) = &image.alt_text {
        push(&mut fields.alt_text, text);
    }
    if let Some(text) = &image.search_text {
        push(&mut fields.alt_text, text);
    }
    if let Some(link) = &image.hyperlink {
        push(&mut fields.links, link);
    }
}

fn visit_ink(ink: &Ink, fields: &mut TextFields) {
    if let Some(text) = &ink.recognized_text {
        push(&mut fields.ink_text, text);
    }
}

fn push(output: &mut Vec<String>, text: &str) {
    let text = text.trim();
    if !text.is_empty() {
        output.push(text.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::timestamp_sort_key;

    #[test]
    fn timestamp_sort_key_compares_unpadded_time_components() {
        assert!(
            timestamp_sort_key("2026-01-02 3:04:05.0 +00")
                < timestamp_sort_key("2026-01-02 20:04:05.0 +00")
        );
    }
}

use onenote_core::{Notebook, PageId, SectionId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SectionLocation {
    pub(crate) section_id: SectionId,
    pub(crate) page_id: Option<PageId>,
}

pub(crate) fn preferred_location(
    notebook: &Notebook,
    remembered: Option<&SectionLocation>,
) -> Option<SectionLocation> {
    if let Some(remembered) = remembered {
        if let Some(location) =
            location_for_section(notebook, Some(remembered), &remembered.section_id)
        {
            return Some(location);
        }
    }
    notebook.sections().next().map(|section| SectionLocation {
        section_id: section.id.clone(),
        page_id: section.pages.first().map(|page| page.id.clone()),
    })
}

pub(crate) fn location_for_section(
    notebook: &Notebook,
    remembered: Option<&SectionLocation>,
    section_id: &SectionId,
) -> Option<SectionLocation> {
    let section = notebook.section(section_id)?;
    let remembered_page = remembered
        .filter(|remembered| remembered.section_id == *section_id)
        .and_then(|remembered| remembered.page_id.as_ref())
        .and_then(|page_id| {
            section
                .pages
                .iter()
                .find(|page| page.id == *page_id)
                .map(|page| page.id.clone())
        });
    Some(SectionLocation {
        section_id: section_id.clone(),
        page_id: remembered_page.or_else(|| section.pages.first().map(|page| page.id.clone())),
    })
}

#[cfg(test)]
mod tests {
    use super::{location_for_section, preferred_location, SectionLocation};
    use onenote_core::{
        Notebook, NotebookEntry, Page, PageId, Section, SectionId, SourceFingerprint, SourceId,
    };

    #[test]
    fn restores_the_last_valid_page_for_a_notebook() {
        let notebook = notebook_with_pages(&["first", "second"]);
        let remembered = SectionLocation {
            section_id: SectionId::new("section"),
            page_id: Some(PageId::new("second")),
        };

        assert_eq!(
            preferred_location(&notebook, Some(&remembered)),
            Some(remembered)
        );
    }

    #[test]
    fn removed_remembered_page_falls_back_to_the_first_page() {
        let notebook = notebook_with_pages(&["first"]);
        let remembered = SectionLocation {
            section_id: SectionId::new("section"),
            page_id: Some(PageId::new("removed")),
        };

        assert_eq!(
            preferred_location(&notebook, Some(&remembered)),
            Some(SectionLocation {
                section_id: SectionId::new("section"),
                page_id: Some(PageId::new("first")),
            })
        );
    }

    #[test]
    fn removed_remembered_section_falls_back_to_the_first_section() {
        let notebook = notebook_with_pages(&["first"]);
        let remembered = SectionLocation {
            section_id: SectionId::new("removed"),
            page_id: Some(PageId::new("removed-page")),
        };

        assert_eq!(
            preferred_location(&notebook, Some(&remembered)),
            Some(SectionLocation {
                section_id: SectionId::new("section"),
                page_id: Some(PageId::new("first")),
            })
        );
    }

    #[test]
    fn switching_sections_does_not_reuse_a_page_from_the_previous_section() {
        let notebook = Notebook {
            entries: vec![
                NotebookEntry::Section(section("one", &["one-page"])),
                NotebookEntry::Section(section("two", &["two-page"])),
            ],
            ..notebook_with_pages(&[])
        };
        let remembered = SectionLocation {
            section_id: SectionId::new("one"),
            page_id: Some(PageId::new("one-page")),
        };

        assert_eq!(
            location_for_section(&notebook, Some(&remembered), &SectionId::new("two")),
            Some(SectionLocation {
                section_id: SectionId::new("two"),
                page_id: Some(PageId::new("two-page")),
            })
        );
    }

    fn notebook_with_pages(page_ids: &[&str]) -> Notebook {
        Notebook {
            source_id: SourceId::new("source"),
            fingerprint: SourceFingerprint::new("fingerprint"),
            name: "Notebook".to_owned(),
            color: None,
            entries: vec![NotebookEntry::Section(section("section", page_ids))],
            diagnostics: Vec::new(),
        }
    }

    fn section(id: &str, page_ids: &[&str]) -> Section {
        Section {
            id: SectionId::new(id),
            name: id.to_owned(),
            color: None,
            pages: page_ids.iter().map(|id| page(id)).collect(),
            diagnostics: Vec::new(),
        }
    }

    fn page(id: &str) -> Page {
        Page {
            id: PageId::new(id),
            native_id: String::new(),
            title: id.to_owned(),
            level: 0,
            created_at: String::new(),
            updated_at: String::new(),
            author: None,
            height: None,
            objects: Vec::new(),
        }
    }
}

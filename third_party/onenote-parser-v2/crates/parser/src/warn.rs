//! Non-fatal parser warnings collected while reading a OneNote file.

use uuid::Uuid;

/// A collection of non-fatal warnings produced during parsing.
///
/// A `Report` is returned alongside the parsed data by the `*_with_report`
/// methods on [`Parser`](crate::Parser). It contains any issues encountered
/// while parsing that did not prevent the file from being read.
#[derive(Clone, Debug)]
pub struct Report {
    warnings: Vec<Warning>,
}

impl Report {
    pub(crate) fn new() -> Self {
        Report {
            warnings: Vec::new(),
        }
    }

    /// Returns the warnings collected during parsing.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    pub(crate) fn push_warning(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}

/// A single non-fatal issue encountered during parsing.
///
/// Warnings carry a human-readable message describing the issue and, when
/// available, the title of the page on which the issue occurred.
#[derive(Clone, Debug)]
pub struct Warning {
    page: Option<(Uuid, String)>,
    message: String,
}

impl Warning {
    /// Returns the title of the page on which the warning was raised, if known.
    ///
    /// Returns `None` for warnings that are not associated with a specific page
    /// (e.g. warnings raised while parsing notebook- or section-level data).
    pub fn page(&self) -> Option<(Uuid, &str)> {
        self.page.as_ref().map(|(id, title)| (*id, title.as_str()))
    }

    /// Returns the human-readable warning message.
    pub fn message(&self) -> &str {
        self.message.as_str()
    }

    pub(crate) fn new(page: Option<(Uuid, String)>, message: String) -> Self {
        Self { page, message }
    }
}

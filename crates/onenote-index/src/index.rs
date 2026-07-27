use crate::document::{documents, ObjectDocument, PageDocument};
use crate::model::{
    IndexProgress, MatchedField, SearchHit, SearchQuery, SourceStatus, TextRange, TextSnippet,
};
use crate::query::{prepare_query, PreparedQuery};
use crate::{Error, Result, SCHEMA_VERSION};
use onenote_core::{Notebook, ObjectId, PageId, Rect, SectionId, SourceFingerprint, SourceId};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_RESULTS: usize = 1_000;
const MAX_SNIPPET_CHARACTERS: usize = 2_048;

/// Caller-owned, rebuildable multi-notebook search index.
pub struct SearchIndex {
    connection: Connection,
}

impl SearchIndex {
    /// Open or create an index at a caller-selected path.
    ///
    /// # Errors
    ///
    /// Returns a database error for inaccessible/corrupt storage or an
    /// incompatible-schema error when rebuilding is required.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path).map_err(Error::from)?;
        Self::initialize(connection)
    }

    /// Create an ephemeral index for independent consumers and tests.
    ///
    /// # Errors
    ///
    /// Returns a database error if `SQLite` or FTS5 initialization fails.
    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().map_err(Error::from)?;
        Self::initialize(connection)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(Error::from)?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Error::from)?;
        if version == 0 {
            create_schema(&connection)?;
        } else if version != SCHEMA_VERSION {
            return Err(Error::IncompatibleSchema {
                found: version,
                expected: SCHEMA_VERSION,
            });
        }
        Ok(Self { connection })
    }

    /// Transactionally replace one complete source generation.
    ///
    /// Progress callbacks execute on the calling thread after each page is
    /// inserted. Cancellation rolls the transaction back, preserving the last
    /// published source generation.
    ///
    /// # Errors
    ///
    /// Returns a cancellation or database error. No partial generation is
    /// visible after either failure.
    pub fn replace_source(
        &mut self,
        notebook: &Notebook,
        cancel: &AtomicBool,
        mut progress: impl FnMut(IndexProgress),
    ) -> Result<()> {
        let documents = documents(notebook);
        let total = documents.len();
        let transaction = self.connection.transaction().map_err(Error::from)?;
        transaction
            .execute(
                "DELETE FROM sources WHERE source_id = ?1",
                [notebook.source_id.as_str()],
            )
            .map_err(Error::from)?;
        transaction
            .execute(
                "INSERT INTO sources(source_id, fingerprint, notebook_name, page_count)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    notebook.source_id.as_str(),
                    notebook.fingerprint.as_str(),
                    notebook.name,
                    usize_to_i64(total),
                ],
            )
            .map_err(Error::from)?;

        for (source_order, document) in documents.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            insert_page(&transaction, notebook, document, source_order)?;
            progress(IndexProgress {
                source_id: notebook.source_id.clone(),
                pages_completed: source_order + 1,
                pages_total: total,
            });
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        transaction.commit().map_err(Error::from)
    }

    /// Remove one source and its derived documents transactionally.
    ///
    /// # Errors
    ///
    /// Returns a database error if removal cannot be committed.
    pub fn remove_source(&mut self, source_id: &SourceId) -> Result<bool> {
        let changed = self
            .connection
            .execute(
                "DELETE FROM sources WHERE source_id = ?1",
                [source_id.as_str()],
            )
            .map_err(Error::from)?;
        Ok(changed > 0)
    }

    /// List every fully published source generation.
    ///
    /// # Errors
    ///
    /// Returns a database error when status rows cannot be read.
    pub fn sources(&self) -> Result<Vec<SourceStatus>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT source_id, fingerprint, notebook_name, page_count
                 FROM sources ORDER BY notebook_name, source_id",
            )
            .map_err(Error::from)?;
        let sources = statement
            .query_map([], |row| {
                Ok(SourceStatus {
                    source_id: SourceId::new(row.get::<_, String>(0)?),
                    fingerprint: SourceFingerprint::new(row.get::<_, String>(1)?),
                    notebook_name: row.get(2)?,
                    page_count: i64_to_usize(row.get(3)?),
                })
            })
            .map_err(Error::from)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)?;
        Ok(sources)
    }

    /// Verify `SQLite`, foreign-key, and FTS index consistency.
    ///
    /// This is intended for diagnostics and release tests, not the search hot
    /// path.
    ///
    /// # Errors
    ///
    /// Returns a database error if any consistency check fails.
    pub fn verify_integrity(&self) -> Result<()> {
        let quick_check: String = self
            .connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(Error::from)?;
        if quick_check != "ok" {
            return Err(Error::Database {
                message: format!("SQLite quick check returned {quick_check}"),
            });
        }
        let foreign_key_failure: Option<String> = self
            .connection
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()
            .map_err(Error::from)?;
        if foreign_key_failure.is_some() {
            return Err(Error::Database {
                message: "foreign-key consistency check failed".to_owned(),
            });
        }
        self.connection
            .execute(
                "INSERT INTO page_fts(page_fts, rank) VALUES('integrity-check', 1)",
                [],
            )
            .map_err(Error::from)?;
        Ok(())
    }

    /// Execute a bounded structured query across selected sources.
    ///
    /// # Errors
    ///
    /// Returns a validation, cancellation, limit, or database error. Query text
    /// is never interpreted as raw SQL and supports only the documented simple
    /// syntax.
    pub fn search(&self, query: &SearchQuery, cancel: &AtomicBool) -> Result<Vec<SearchHit>> {
        if query.limit > MAX_RESULTS {
            return Err(Error::ResultLimit {
                requested: query.limit,
                maximum: MAX_RESULTS,
            });
        }
        if query.snippet_characters > MAX_SNIPPET_CHARACTERS {
            return Err(Error::InvalidQuery {
                message: format!("snippet length exceeds {MAX_SNIPPET_CHARACTERS} characters"),
            });
        }
        if query.limit == 0 {
            return Ok(Vec::new());
        }
        let prepared = prepare_query(&query.text)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let (sql, values) = search_statement(query, &prepared);
        let mut statement = self.connection.prepare(&sql).map_err(Error::from)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok(RawHit {
                    rowid: row.get(0)?,
                    rank: row.get(1)?,
                    marked_snippet: row.get(2)?,
                    source_id: row.get(3)?,
                    fingerprint: row.get(4)?,
                    notebook_name: row.get(5)?,
                    section_id: row.get(6)?,
                    section_name: row.get(7)?,
                    page_id: row.get(8)?,
                    title: row.get(9)?,
                    updated_at: row.get(10)?,
                    body: row.get(11)?,
                    alt_text: row.get(12)?,
                    ink_text: row.get(13)?,
                    attachments: row.get(14)?,
                    links: row.get(15)?,
                    path: row.get(16)?,
                })
            })
            .map_err(Error::from)?;
        let mut hits = Vec::new();
        for row in rows {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let row = row.map_err(Error::from)?;
            hits.push(self.resolve_hit(row, &prepared, query.snippet_characters)?);
        }
        Ok(hits)
    }

    fn resolve_hit(
        &self,
        hit: RawHit,
        prepared: &PreparedQuery,
        snippet_characters: usize,
    ) -> Result<SearchHit> {
        let (object_id, bounds) = self.matching_object(hit.rowid, &prepared.plain_terms)?;
        Ok(SearchHit {
            rank: -hit.rank,
            matched_field: classify_field(&hit, &prepared.plain_terms),
            snippet: parse_snippet(&hit.marked_snippet, snippet_characters),
            source_fingerprint: SourceFingerprint::new(hit.fingerprint),
            source_id: SourceId::new(hit.source_id),
            notebook_name: hit.notebook_name,
            section_id: SectionId::new(hit.section_id),
            section_name: hit.section_name,
            page_id: PageId::new(hit.page_id),
            page_title: hit.title,
            object_id,
            bounds,
            updated_at: hit.updated_at,
        })
    }

    fn matching_object(
        &self,
        page_rowid: i64,
        terms: &[String],
    ) -> Result<(Option<ObjectId>, Option<Rect>)> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT object_id, x, y, width, height, visible_text
                 FROM objects WHERE page_rowid = ?1 ORDER BY z_index",
            )
            .map_err(Error::from)?;
        let mut rows = statement.query([page_rowid]).map_err(Error::from)?;
        while let Some(row) = rows.next().map_err(Error::from)? {
            let text: String = row.get(5).map_err(Error::from)?;
            if terms_match(&text, terms) {
                return Ok((
                    Some(ObjectId::new(row.get::<_, String>(0).map_err(Error::from)?)),
                    Some(Rect {
                        x: row.get(1).map_err(Error::from)?,
                        y: row.get(2).map_err(Error::from)?,
                        width: row.get(3).map_err(Error::from)?,
                        height: row.get(4).map_err(Error::from)?,
                    }),
                ));
            }
        }
        Ok((None, None))
    }
}

fn create_schema(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "
            BEGIN;
            CREATE TABLE sources (
                source_id TEXT PRIMARY KEY NOT NULL,
                fingerprint TEXT NOT NULL,
                notebook_name TEXT NOT NULL,
                page_count INTEGER NOT NULL CHECK(page_count >= 0)
            );
            CREATE TABLE pages (
                rowid INTEGER PRIMARY KEY,
                source_id TEXT NOT NULL REFERENCES sources(source_id) ON DELETE CASCADE,
                fingerprint TEXT NOT NULL,
                notebook_name TEXT NOT NULL,
                section_id TEXT NOT NULL,
                section_name TEXT NOT NULL,
                page_id TEXT NOT NULL,
                title TEXT NOT NULL,
                body TEXT NOT NULL,
                tags TEXT NOT NULL,
                alt_text TEXT NOT NULL,
                ink_text TEXT NOT NULL,
                attachments TEXT NOT NULL,
                links TEXT NOT NULL,
                path_text TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                has_attachments INTEGER NOT NULL CHECK(has_attachments IN (0, 1)),
                source_order INTEGER NOT NULL,
                UNIQUE(source_id, page_id)
            );
            CREATE TABLE objects (
                page_rowid INTEGER NOT NULL REFERENCES pages(rowid) ON DELETE CASCADE,
                object_id TEXT NOT NULL,
                z_index INTEGER NOT NULL,
                x REAL NOT NULL,
                y REAL NOT NULL,
                width REAL NOT NULL,
                height REAL NOT NULL,
                visible_text TEXT NOT NULL,
                PRIMARY KEY(page_rowid, object_id)
            );
            CREATE VIRTUAL TABLE page_fts USING fts5(
                title, body, tags, alt_text, ink_text, attachments, links, path_text,
                content='pages', content_rowid='rowid',
                tokenize='unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER pages_after_insert AFTER INSERT ON pages BEGIN
                INSERT INTO page_fts(
                    rowid, title, body, tags, alt_text, ink_text, attachments, links, path_text
                ) VALUES (
                    new.rowid, new.title, new.body, new.tags, new.alt_text, new.ink_text,
                    new.attachments, new.links, new.path_text
                );
            END;
            CREATE TRIGGER pages_before_delete BEFORE DELETE ON pages BEGIN
                INSERT INTO page_fts(
                    page_fts, rowid, title, body, tags, alt_text, ink_text, attachments,
                    links, path_text
                ) VALUES (
                    'delete', old.rowid, old.title, old.body, old.tags, old.alt_text,
                    old.ink_text, old.attachments, old.links, old.path_text
                );
            END;
            CREATE INDEX pages_source ON pages(source_id);
            CREATE INDEX pages_section ON pages(section_id);
            PRAGMA user_version = 1;
            COMMIT;
            ",
        )
        .map_err(Error::from)
}

fn insert_page(
    transaction: &Transaction<'_>,
    notebook: &Notebook,
    document: &PageDocument<'_>,
    source_order: usize,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO pages(
                source_id, fingerprint, notebook_name, section_id, section_name, page_id,
                title, body, tags, alt_text, ink_text, attachments, links, path_text,
                updated_at, has_attachments, source_order
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
             )",
            params![
                notebook.source_id.as_str(),
                notebook.fingerprint.as_str(),
                document.notebook_name,
                document.section.id.as_str(),
                document.section.name,
                document.page.id.as_str(),
                document.page.title,
                document.body,
                document.alt_text,
                document.ink_text,
                document.attachments,
                document.links,
                document.path,
                document.page.updated_at,
                i64::from(!document.attachments.is_empty()),
                usize_to_i64(source_order),
            ],
        )
        .map_err(Error::from)?;
    let page_rowid = transaction.last_insert_rowid();
    for object in &document.objects {
        insert_object(transaction, page_rowid, object)?;
    }
    Ok(())
}

fn insert_object(
    transaction: &Transaction<'_>,
    page_rowid: i64,
    document: &ObjectDocument<'_>,
) -> Result<()> {
    let object = document.object;
    transaction
        .execute(
            "INSERT INTO objects(
                page_rowid, object_id, z_index, x, y, width, height, visible_text
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                page_rowid,
                object.id.as_str(),
                i64::from(object.z_index),
                object.bounds.x,
                object.bounds.y,
                object.bounds.width,
                object.bounds.height,
                document.text,
            ],
        )
        .map_err(Error::from)?;
    Ok(())
}

fn search_statement(query: &SearchQuery, prepared: &PreparedQuery) -> (String, Vec<Value>) {
    let mut sql = String::from(
        "SELECT p.rowid,
                bm25(page_fts, 10.0, 1.0, 5.0, 2.0, 2.0, 2.0, 1.0, 4.0),
                snippet(page_fts, -1, char(30), char(31), ' … ', 32),
                p.source_id, p.fingerprint, p.notebook_name, p.section_id, p.section_name,
                p.page_id, p.title, p.updated_at, p.body, p.alt_text, p.ink_text,
                p.attachments, p.links, p.path_text
         FROM page_fts JOIN pages p ON p.rowid = page_fts.rowid
         WHERE page_fts MATCH ?",
    );
    let mut values = vec![Value::Text(prepared.fts.clone())];
    append_in_filter(
        &mut sql,
        &mut values,
        "p.source_id",
        query.filters.source_ids.iter().map(SourceId::as_str),
    );
    append_in_filter(
        &mut sql,
        &mut values,
        "p.section_id",
        query.filters.section_ids.iter().map(SectionId::as_str),
    );
    if let Some(has_attachments) = query.filters.has_attachments {
        sql.push_str(" AND p.has_attachments = ?");
        values.push(Value::Integer(i64::from(has_attachments)));
    }
    if let Some(after) = &query.filters.updated_after {
        sql.push_str(" AND p.updated_at >= ?");
        values.push(Value::Text(after.clone()));
    }
    if let Some(before) = &query.filters.updated_before {
        sql.push_str(" AND p.updated_at <= ?");
        values.push(Value::Text(before.clone()));
    }
    sql.push_str(
        " ORDER BY bm25(page_fts, 10.0, 1.0, 5.0, 2.0, 2.0, 2.0, 1.0, 4.0) ASC,
                   p.updated_at DESC, p.source_order ASC
          LIMIT ?",
    );
    values.push(Value::Integer(usize_to_i64(query.limit)));
    (sql, values)
}

fn append_in_filter<'a>(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    items: impl Iterator<Item = &'a str>,
) {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        return;
    }
    sql.push_str(" AND ");
    sql.push_str(column);
    sql.push_str(" IN (");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            sql.push(',');
        }
        sql.push('?');
        values.push(Value::Text((*item).to_owned()));
    }
    sql.push(')');
}

struct RawHit {
    rowid: i64,
    rank: f64,
    marked_snippet: String,
    source_id: String,
    fingerprint: String,
    notebook_name: String,
    section_id: String,
    section_name: String,
    page_id: String,
    title: String,
    updated_at: String,
    body: String,
    alt_text: String,
    ink_text: String,
    attachments: String,
    links: String,
    path: String,
}

fn classify_field(hit: &RawHit, terms: &[String]) -> MatchedField {
    for (field, value) in [
        (MatchedField::Title, hit.title.as_str()),
        (MatchedField::Path, hit.path.as_str()),
        (MatchedField::Attachment, hit.attachments.as_str()),
        (MatchedField::AltText, hit.alt_text.as_str()),
        (MatchedField::InkText, hit.ink_text.as_str()),
        (MatchedField::Link, hit.links.as_str()),
        (MatchedField::Body, hit.body.as_str()),
    ] {
        if terms_match(value, terms) {
            return field;
        }
    }
    MatchedField::Other
}

fn terms_match(text: &str, terms: &[String]) -> bool {
    let text = text.to_lowercase();
    terms.iter().all(|term| text.contains(&term.to_lowercase()))
}

fn parse_snippet(marked: &str, max_characters: usize) -> TextSnippet {
    let mut text = String::with_capacity(marked.len());
    let mut highlights = Vec::new();
    let mut start = None;
    for character in marked.chars().take(max_characters) {
        match character {
            '\u{1e}' => start = Some(text.len()),
            '\u{1f}' => {
                if let Some(start_byte) = start.take() {
                    highlights.push(TextRange {
                        start_byte,
                        end_byte: text.len(),
                    });
                }
            }
            _ => text.push(character),
        }
    }
    if let Some(start_byte) = start {
        highlights.push(TextRange {
            start_byte,
            end_byte: text.len(),
        });
    }
    TextSnippet { text, highlights }
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn i64_to_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::SearchIndex;
    use crate::{Error, SearchQuery};
    use onenote_core::{
        Notebook, NotebookEntry, ObjectId, ObjectKind, Outline, OutlineElement, Page, PageId,
        PageObject, PageObjectRole, Rect, Section, SectionId, SourceFingerprint, SourceId,
        TextAlignment, TextBlock, TextStyle,
    };
    use std::sync::atomic::AtomicBool;

    #[test]
    fn indexes_queries_filters_and_removes_multiple_sources() {
        let mut index = SearchIndex::open_in_memory().expect("index");
        let first = notebook("source-1", "fingerprint-1", "Alpha notebook");
        let second = notebook("source-2", "fingerprint-2", "Beta notebook");
        let cancel = AtomicBool::new(false);
        index
            .replace_source(&first, &cancel, |_| {})
            .expect("index first");
        index
            .replace_source(&second, &cancel, |_| {})
            .expect("index second");

        let hits = index
            .search(&SearchQuery::simple("searchable"), &cancel)
            .expect("search");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|hit| hit.object_id.is_some()));
        assert!(hits.iter().all(|hit| !hit.snippet.highlights.is_empty()));

        let mut filtered = SearchQuery::simple("\"searchable phrase\"");
        filtered.filters.source_ids = vec![first.source_id.clone()];
        assert_eq!(index.search(&filtered, &cancel).expect("filtered").len(), 1);
        assert!(index.remove_source(&first.source_id).expect("remove"));
        assert_eq!(index.sources().expect("sources").len(), 1);
    }

    #[test]
    fn cancelled_replacement_preserves_published_generation() {
        let mut index = SearchIndex::open_in_memory().expect("index");
        let notebook = notebook("source", "old", "Notebook");
        index
            .replace_source(&notebook, &AtomicBool::new(false), |_| {})
            .expect("initial generation");
        let mut replacement = notebook.clone();
        replacement.fingerprint = SourceFingerprint::new("new");
        let error = index
            .replace_source(&replacement, &AtomicBool::new(true), |_| {})
            .expect_err("cancel replacement");
        assert_eq!(error, Error::Cancelled);
        assert_eq!(
            index.sources().expect("sources")[0].fingerprint.as_str(),
            "old"
        );
    }

    fn notebook(source: &str, fingerprint: &str, notebook_name: &str) -> Notebook {
        let text = TextBlock {
            text: "A searchable phrase in freeform content".to_owned(),
            base_style: TextStyle::default(),
            runs: Vec::new(),
            alignment: TextAlignment::Left,
            space_before: 0.0,
            space_after: 0.0,
            line_spacing: None,
        };
        Notebook {
            source_id: SourceId::new(source),
            fingerprint: SourceFingerprint::new(fingerprint),
            name: notebook_name.to_owned(),
            color: None,
            entries: vec![NotebookEntry::Section(Section {
                id: SectionId::new(format!("{source}-section")),
                name: "Section".to_owned(),
                color: None,
                pages: vec![Page {
                    id: PageId::new(format!("{source}-page")),
                    native_id: "native".to_owned(),
                    title: "Page title".to_owned(),
                    level: 0,
                    created_at: "2026-01-01 00:00:00.0 +00:00:00".to_owned(),
                    updated_at: "2026-01-02 00:00:00.0 +00:00:00".to_owned(),
                    author: None,
                    height: None,
                    objects: vec![PageObject {
                        id: ObjectId::new(format!("{source}-object")),
                        role: PageObjectRole::Body,
                        bounds: Rect {
                            x: 10.0,
                            y: 20.0,
                            width: 300.0,
                            height: 80.0,
                        },
                        z_index: 0,
                        kind: ObjectKind::Outline(Outline {
                            child_level: 0,
                            indents: Vec::new(),
                            user_sized: false,
                            elements: vec![OutlineElement {
                                level: 0,
                                list: None,
                                content: vec![onenote_core::ElementContent::Text(text)],
                                children: Vec::new(),
                            }],
                        }),
                    }],
                }],
                diagnostics: Vec::new(),
            })],
            diagnostics: Vec::new(),
        }
    }
}

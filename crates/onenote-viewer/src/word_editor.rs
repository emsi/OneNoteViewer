use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct LiveWordEditor {
    pub revealer: gtk::Revealer,
    #[allow(dead_code)]
    pub container: gtk::Box,
    pub view: gtk::TextView,
    pub buffer: gtk::TextBuffer,
    pub active_node_id: Rc<RefCell<Option<String>>>,
    pub status_label: gtk::Label,
    pub stats_label: gtk::Label,
    pub is_updating_internal: Rc<RefCell<bool>>,
}

impl LiveWordEditor {
    pub fn new() -> Self {
        let active_node_id = Rc::new(RefCell::new(None));
        let is_updating_internal = Rc::new(RefCell::new(false));

        let buffer = gtk::TextBuffer::builder().enable_undo(true).build();
        let tag_table = buffer.tag_table();
        let tag_h1 = gtk::TextTag::builder()
            .name("h1")
            .weight(700)
            .scale(1.4)
            .foreground("#1a73e8")
            .build();
        let tag_h2 = gtk::TextTag::builder()
            .name("h2")
            .weight(700)
            .scale(1.25)
            .foreground("#188038")
            .build();
        let tag_h3 = gtk::TextTag::builder()
            .name("h3")
            .weight(700)
            .scale(1.1)
            .foreground("#d93025")
            .build();
        let tag_bold = gtk::TextTag::builder()
            .name("bold")
            .weight(700)
            .build();
        let tag_italic = gtk::TextTag::builder()
            .name("italic")
            .style(pango::Style::Italic)
            .build();
        let tag_underline = gtk::TextTag::builder()
            .name("underline")
            .underline(pango::Underline::Single)
            .build();
        let tag_strike = gtk::TextTag::builder()
            .name("strike")
            .strikethrough(true)
            .build();
        tag_table.add(&tag_h1);
        tag_table.add(&tag_h2);
        tag_table.add(&tag_h3);
        tag_table.add(&tag_bold);
        tag_table.add(&tag_italic);
        tag_table.add(&tag_underline);
        tag_table.add(&tag_strike);
        let view = gtk::TextView::builder()
            .buffer(&buffer)
            .wrap_mode(gtk::WrapMode::WordChar)
            .vexpand(true)
            .hexpand(true)
            .top_margin(16)
            .bottom_margin(16)
            .left_margin(18)
            .right_margin(18)
            .monospace(false)
            .build();
        view.add_css_class("word-editor-view");

        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .min_content_width(380)
            .min_content_height(350)
            .build();

        // Top Header
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header.add_css_class("word-editor-header");

        let title_label = gtk::Label::builder()
            .label("📝 Word Document Editor")
            .xalign(0.0)
            .hexpand(true)
            .build();
        title_label.add_css_class("word-editor-title");

        let live_badge = gtk::Label::new(Some("● Live Sync"));
        live_badge.add_css_class("word-live-badge");

        let close_btn = gtk::Button::with_label("✕");
        close_btn.add_css_class("flat");
        close_btn.set_tooltip_text(Some("Close Word Editor"));

        header.append(&title_label);
        header.append(&live_badge);
        header.append(&close_btn);

        // Toolbar 1: Styles & Typography
        let tb1 = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tb1.add_css_class("word-toolbar-row");

        let btn_h1 = gtk::Button::with_label("H1");
        btn_h1.set_tooltip_text(Some("Heading 1 (22pt Bold)"));
        let btn_h2 = gtk::Button::with_label("H2");
        btn_h2.set_tooltip_text(Some("Heading 2 (18pt Bold)"));
        let btn_h3 = gtk::Button::with_label("H3");
        btn_h3.set_tooltip_text(Some("Heading 3 (15pt Bold)"));
        let btn_quote = gtk::Button::with_label("❞ Quote");
        btn_quote.set_tooltip_text(Some("Quote Block"));

        let sep1 = gtk::Separator::new(gtk::Orientation::Vertical);

        let btn_bold = gtk::Button::with_label("B");
        btn_bold.add_css_class("word-btn-bold");
        btn_bold.set_tooltip_text(Some("Bold (Ctrl+B)"));

        let btn_italic = gtk::Button::with_label("I");
        btn_italic.add_css_class("word-btn-italic");
        btn_italic.set_tooltip_text(Some("Italic (Ctrl+I)"));

        let btn_underline = gtk::Button::with_label("U");
        btn_underline.add_css_class("word-btn-underline");
        btn_underline.set_tooltip_text(Some("Underline (Ctrl+U)"));

        let btn_strike = gtk::Button::with_label("S");
        btn_strike.add_css_class("word-btn-strike");
        btn_strike.set_tooltip_text(Some("Strikethrough"));

        let sep2 = gtk::Separator::new(gtk::Orientation::Vertical);

        let btn_undo = gtk::Button::with_label("↶ Undo");
        btn_undo.set_tooltip_text(Some("Undo Edit (Ctrl+Z)"));
        let btn_redo = gtk::Button::with_label("↷ Redo");
        btn_redo.set_tooltip_text(Some("Redo Edit (Ctrl+Y)"));

        for b in [&btn_h1, &btn_h2, &btn_h3, &btn_quote, &btn_bold, &btn_italic, &btn_underline, &btn_strike, &btn_undo, &btn_redo] {
            b.add_css_class("word-tb-btn");
        }

        tb1.append(&btn_h1);
        tb1.append(&btn_h2);
        tb1.append(&btn_h3);
        tb1.append(&btn_quote);
        tb1.append(&sep1);
        tb1.append(&btn_bold);
        tb1.append(&btn_italic);
        tb1.append(&btn_underline);
        tb1.append(&btn_strike);
        tb1.append(&sep2);
        tb1.append(&btn_undo);
        tb1.append(&btn_redo);

        // Toolbar 2: Tasks, Lists & Inserts
        let tb2 = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tb2.add_css_class("word-toolbar-row");

        let btn_task = gtk::Button::with_label("☐ To-Do Task");
        btn_task.add_css_class("word-tb-btn-primary");
        btn_task.set_tooltip_text(Some("Toggle To-Do Task Checkbox (Ctrl+1)"));

        let btn_bullet = gtk::Button::with_label("• Bullets");
        btn_bullet.set_tooltip_text(Some("Bulleted List"));

        let btn_number = gtk::Button::with_label("1. Numbering");
        btn_number.set_tooltip_text(Some("Numbered List"));

        let sep3 = gtk::Separator::new(gtk::Orientation::Vertical);

        let btn_table = gtk::Button::with_label("📊 Table");
        btn_table.set_tooltip_text(Some("Insert 3x3 Table"));

        let btn_time = gtk::Button::with_label("📅 Date/Time");
        btn_time.set_tooltip_text(Some("Insert Current Timestamp"));

        let btn_divider = gtk::Button::with_label("── Line");
        btn_divider.set_tooltip_text(Some("Insert Horizontal Divider"));

        let btn_clear = gtk::Button::with_label("🧹 Clean");
        btn_clear.set_tooltip_text(Some("Clear Formatting"));

        for b in [&btn_task, &btn_bullet, &btn_number, &btn_table, &btn_time, &btn_divider, &btn_clear] {
            b.add_css_class("word-tb-btn");
        }

        tb2.append(&btn_task);
        tb2.append(&btn_bullet);
        tb2.append(&btn_number);
        tb2.append(&sep3);
        tb2.append(&btn_table);
        tb2.append(&btn_time);
        tb2.append(&btn_divider);
        tb2.append(&btn_clear);

        // Footer Bar
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.add_css_class("word-editor-footer");

        let stats_label = gtk::Label::builder()
            .label("Chars: 0 | Words: 0 | Lines: 1")
            .xalign(0.0)
            .hexpand(true)
            .build();
        stats_label.add_css_class("dim-label");

        let status_label = gtk::Label::builder()
            .label("Ready for live edits")
            .xalign(1.0)
            .build();
        status_label.add_css_class("dim-label");

        footer.append(&stats_label);
        footer.append(&status_label);

        // Full Editor Container
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.add_css_class("word-editor-container");
        container.append(&header);
        container.append(&tb1);
        container.append(&tb2);
        container.append(&scroller);
        container.append(&footer);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .child(&container)
            .reveal_child(false)
            .build();

        // Connect Toolbar Actions
        {
            let b = buffer.clone();
            btn_bold.connect_clicked(move |_| wrap_selection(&b, "**", "**"));
        }
        {
            let b = buffer.clone();
            btn_italic.connect_clicked(move |_| wrap_selection(&b, "*", "*"));
        }
        {
            let b = buffer.clone();
            btn_underline.connect_clicked(move |_| wrap_selection(&b, "<u>", "</u>"));
        }
        {
            let b = buffer.clone();
            btn_strike.connect_clicked(move |_| wrap_selection(&b, "~~", "~~"));
        }
        {
            let b = buffer.clone();
            btn_h1.connect_clicked(move |_| toggle_heading(&b, 1));
        }
        {
            let b = buffer.clone();
            btn_h2.connect_clicked(move |_| toggle_heading(&b, 2));
        }
        {
            let b = buffer.clone();
            btn_h3.connect_clicked(move |_| toggle_heading(&b, 3));
        }
        {
            let b = buffer.clone();
            btn_quote.connect_clicked(move |_| toggle_heading(&b, 4));
        }
        {
            let b = buffer.clone();
            btn_task.connect_clicked(move |_| toggle_task_checkbox(&b));
        }
        {
            let b = buffer.clone();
            btn_bullet.connect_clicked(move |_| toggle_bullet_list(&b));
        }
        {
            let b = buffer.clone();
            btn_number.connect_clicked(move |_| toggle_numbered_list(&b));
        }
        {
            let b = buffer.clone();
            btn_table.connect_clicked(move |_| insert_table(&b));
        }
        {
            let b = buffer.clone();
            btn_time.connect_clicked(move |_| insert_timestamp(&b));
        }
        {
            let b = buffer.clone();
            btn_divider.connect_clicked(move |_| insert_divider(&b));
        }
        {
            let b = buffer.clone();
            btn_clear.connect_clicked(move |_| clear_formatting(&b));
        }
        {
            let b = buffer.clone();
            btn_undo.connect_clicked(move |_| {
                if b.can_undo() {
                    b.undo();
                }
            });
        }
        {
            let b = buffer.clone();
            btn_redo.connect_clicked(move |_| {
                if b.can_redo() {
                    b.redo();
                }
            });
        }
        {
            let rev = revealer.clone();
            close_btn.connect_clicked(move |_| {
                rev.set_reveal_child(false);
            });
        }

        // Keyboard controller for shortcuts
        let key_controller = gtk::EventControllerKey::new();
        {
            let b = buffer.clone();
            key_controller.connect_key_pressed(move |_, key, _, modifier| {
                if modifier.contains(gdk::ModifierType::CONTROL_MASK) {
                    match key {
                        gdk::Key::b | gdk::Key::B => {
                            wrap_selection(&b, "**", "**");
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::i | gdk::Key::I => {
                            wrap_selection(&b, "*", "*");
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::u | gdk::Key::U => {
                            wrap_selection(&b, "<u>", "</u>");
                            return glib::Propagation::Stop;
                        }
                        gdk::Key::_1 => {
                            toggle_task_checkbox(&b);
                            return glib::Propagation::Stop;
                        }
                        _ => {}
                    }
                }
                glib::Propagation::Proceed
            });
        }
        view.add_controller(key_controller);

        Self {
            revealer,
            container,
            view,
            buffer,
            active_node_id,
            status_label,
            stats_label,
            is_updating_internal,
        }
    }

    pub fn load_text(&self, node_id: String, text: String) {
        *self.is_updating_internal.borrow_mut() = true;
        *self.active_node_id.borrow_mut() = Some(node_id);
        self.buffer.set_text(&text);
        apply_formatting_tags(&self.buffer);
        *self.is_updating_internal.borrow_mut() = false;

        self.update_stats(&text);
        self.status_label.set_label("Editing paragraph live");
        self.revealer.set_reveal_child(true);
        self.view.grab_focus();
    }

    pub fn update_stats(&self, text: &str) {
        let chars = text.chars().count();
        let words = text.split_whitespace().count();
        let lines = text.lines().count().max(1);
        self.stats_label
            .set_label(&format!("Chars: {chars} | Words: {words} | Lines: {lines}"));
    }
}

fn wrap_selection(buffer: &gtk::TextBuffer, prefix: &str, suffix: &str) {
    if let Some((mut start, mut end)) = buffer.selection_bounds() {
        let selected = buffer.text(&start, &end, false).to_string();
        if selected.starts_with(prefix)
            && selected.ends_with(suffix)
            && selected.len() >= prefix.len() + suffix.len()
        {
            let inner = &selected[prefix.len()..selected.len() - suffix.len()];
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, inner);
        } else {
            let wrapped = format!("{prefix}{selected}{suffix}");
            buffer.delete(&mut start, &mut end);
            buffer.insert(&mut start, &wrapped);
        }
    } else {
        buffer.insert_at_cursor(&format!("{prefix}{suffix}"));
    }
}

fn toggle_task_checkbox(buffer: &gtk::TextBuffer) {
    let cursor_mark = buffer.get_insert();
    let iter = buffer.iter_at_mark(&cursor_mark);
    let line = iter.line();
    let mut line_start = buffer.iter_at_line(line).unwrap_or(iter);
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = buffer.text(&line_start, &line_end, false).to_string();
    let new_line = if line_text.starts_with("☐ ") {
        line_text.replacen("☐ ", "☑ ", 1)
    } else if line_text.starts_with("☑ ") {
        line_text.replacen("☑ ", "☐ ", 1)
    } else if line_text.starts_with("[ ] ") {
        line_text.replacen("[ ] ", "[x] ", 1)
    } else if line_text.starts_with("[x] ") {
        line_text.replacen("[x] ", "[ ] ", 1)
    } else if line_text.starts_with("[X] ") {
        line_text.replacen("[X] ", "[ ] ", 1)
    } else {
        format!("☐ {line_text}")
    };
    buffer.delete(&mut line_start, &mut line_end);
    buffer.insert(&mut line_start, &new_line);
}

fn toggle_heading(buffer: &gtk::TextBuffer, level: u8) {
    let cursor_mark = buffer.get_insert();
    let iter = buffer.iter_at_mark(&cursor_mark);
    let line = iter.line();
    let mut line_start = buffer.iter_at_line(line).unwrap_or(iter);
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = buffer.text(&line_start, &line_end, false).to_string();
    let clean_text = line_text
        .trim_start_matches("# ")
        .trim_start_matches("## ")
        .trim_start_matches("### ")
        .trim_start_matches("> ");
    let new_line = match level {
        1 => format!("# {clean_text}"),
        2 => format!("## {clean_text}"),
        3 => format!("### {clean_text}"),
        4 => format!("> {clean_text}"),
        _ => clean_text.to_string(),
    };
    buffer.delete(&mut line_start, &mut line_end);
    buffer.insert(&mut line_start, &new_line);
}

fn toggle_bullet_list(buffer: &gtk::TextBuffer) {
    let cursor_mark = buffer.get_insert();
    let iter = buffer.iter_at_mark(&cursor_mark);
    let line = iter.line();
    let mut line_start = buffer.iter_at_line(line).unwrap_or(iter);
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = buffer.text(&line_start, &line_end, false).to_string();
    let new_line = if line_text.starts_with("• ") {
        line_text.replacen("• ", "", 1)
    } else if line_text.starts_with("- ") {
        line_text.replacen("- ", "", 1)
    } else {
        format!("• {line_text}")
    };
    buffer.delete(&mut line_start, &mut line_end);
    buffer.insert(&mut line_start, &new_line);
}

fn toggle_numbered_list(buffer: &gtk::TextBuffer) {
    let cursor_mark = buffer.get_insert();
    let iter = buffer.iter_at_mark(&cursor_mark);
    let line = iter.line();
    let mut line_start = buffer.iter_at_line(line).unwrap_or(iter);
    let mut line_end = line_start;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line_text = buffer.text(&line_start, &line_end, false).to_string();
    let new_line = if line_text
        .chars()
        .next()
        .map_or(false, |c| c.is_ascii_digit())
        && line_text.contains(". ")
    {
        if let Some(idx) = line_text.find(". ") {
            line_text[idx + 2..].to_string()
        } else {
            line_text
        }
    } else {
        format!("1. {line_text}")
    };
    buffer.delete(&mut line_start, &mut line_end);
    buffer.insert(&mut line_start, &new_line);
}

fn insert_table(buffer: &gtk::TextBuffer) {
    let table = "\n| Column 1 | Column 2 | Column 3 |\n| :--- | :--- | :--- |\n| Item A | Details B | Notes C |\n| Item X | Details Y | Notes Z |\n";
    buffer.insert_at_cursor(table);
}

fn insert_timestamp(buffer: &gtk::TextBuffer) {
    let now = glib::DateTime::now_local()
        .map(|dt| {
            dt.format("%Y-%m-%d %H:%M:%S")
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_else(|_| "2026-11-08".to_string());
    buffer.insert_at_cursor(&format!(" {now} "));
}

fn insert_divider(buffer: &gtk::TextBuffer) {
    buffer.insert_at_cursor("\n────────────────────────────────────────\n");
}

fn clear_formatting(buffer: &gtk::TextBuffer) {
    if let Some((mut start, mut end)) = buffer.selection_bounds() {
        let text = buffer.text(&start, &end, false).to_string();
        let cleaned = text
            .replace("**", "")
            .replace('*', "")
            .replace("<u>", "")
            .replace("</u>", "")
            .replace("~~", "")
            .replace("# ", "")
            .replace("## ", "")
            .replace("### ", "")
            .replace("> ", "");
        buffer.delete(&mut start, &mut end);
        buffer.insert(&mut start, &cleaned);
    }
}

pub fn apply_formatting_tags(buffer: &gtk::TextBuffer) {
    let (b_start, b_end) = buffer.bounds();
    buffer.remove_all_tags(&b_start, &b_end);

    // Apply heading styles per line
    let mut line_iter = buffer.start_iter();
    while !line_iter.is_end() {
        let mut line_end = line_iter;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let line_text = buffer.text(&line_iter, &line_end, false);
        let trimmed = line_text.trim_start();
        if trimmed.starts_with("# ") {
            buffer.apply_tag_by_name("h1", &line_iter, &line_end);
        } else if trimmed.starts_with("## ") {
            buffer.apply_tag_by_name("h2", &line_iter, &line_end);
        } else if trimmed.starts_with("### ") {
            buffer.apply_tag_by_name("h3", &line_iter, &line_end);
        }
        if !line_iter.forward_line() {
            break;
        }
    }

    // Apply inline styles for **...**, *...*, <u>...</u>, ~~...~~
    apply_pattern_tag(buffer, "**", "**", "bold");
    apply_pattern_tag(buffer, "*", "*", "italic");
    apply_pattern_tag(buffer, "<u>", "</u>", "underline");
    apply_pattern_tag(buffer, "~~", "~~", "strike");
}

fn apply_pattern_tag(buffer: &gtk::TextBuffer, open: &str, close: &str, tag_name: &str) {
    let (start, end) = buffer.bounds();
    let text = buffer.text(&start, &end, false).to_string();
    let mut search_start = 0;
    while let Some(start_idx) = text[search_start..].find(open) {
        let actual_start = search_start + start_idx;
        let content_start = actual_start + open.len();
        if let Some(close_idx) = text[content_start..].find(close) {
            let actual_end = content_start + close_idx + close.len();
            let char_start = text[..actual_start].chars().count() as i32;
            let char_end = text[..actual_end].chars().count() as i32;
            let iter_start = buffer.iter_at_offset(char_start);
            let iter_end = buffer.iter_at_offset(char_end);
            buffer.apply_tag_by_name(tag_name, &iter_start, &iter_end);
            search_start = actual_end;
        } else {
            break;
        }
    }
}

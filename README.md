# 📓 OneNoteOnly — Native Linux OneNote Workstation & Sketch Editor

<p align="center">
  <img src="crates/onenote-viewer/resources/icons/scalable/apps/onenoteonly.svg" width="128" height="128" alt="OneNoteOnly Logo" />
</p>

<p align="center">
  <b>The Modern, Feature-Complete Microsoft OneNote Workstation for Linux.</b><br/>
  <i>In-Canvas Vector Inks &amp; Sketches • Live Word-like Rich Text Editing • Arbitrary Canvas Notes &amp; Images • Interactive SVG/HTML Exporter</i>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg" alt="License: GPL-3.0" /></a>
  <a href="https://www.gtk.org/"><img src="https://img.shields.io/badge/GUI-GTK4%20%7C%20Cairo%20%7C%20Pango-red.svg" alt="GTK4" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust%202021-orange.svg?logo=rust" alt="Rust" /></a>
  <a href="https://github.com/ROHANSHAJ/OneNoteOnly"><img src="https://img.shields.io/badge/Platform-Linux%20(Universal)-brightgreen.svg?logo=linux&logoColor=white" alt="Linux" /></a>
  <a href="https://github.com/emsi/OneNoteViewer"><img src="https://img.shields.io/badge/Upstream%20Base-OneNoteViewer-purple.svg" alt="Upstream Base" /></a>
</p>

---

## 🌟 Acknowledgement & Upstream Credits

**OneNoteOnly** stands on the shoulders of giants. It is built upon the remarkable, pioneering foundation of **[OneNoteViewer](https://github.com/emsi/OneNoteViewer)** created by **Mariusz Woloszyn** (`@emsi`) and the **`onenote.rs` parser** by **Markus Siemens**, distributed under the **GPL-3.0-or-later** license.

We extend our deepest gratitude to Mariusz Woloszyn and the open-source contributors of OneNoteViewer for reverse engineering the binary Microsoft OneNote file structures (`.one`, `.onetoc2`, `.onepkg`). **OneNoteOnly** extends this foundation into a full-fledged Linux creative and document editing suite.

---

## 🚀 What Makes OneNoteOnly Unique?

While traditional viewers merely display static pages, **OneNoteOnly** turns your local OneNote notebooks into a live, interactive, freeform workstation:

```text
+---------------------------------------------------------------------------------------+
|                                    ONENOTEONLY                                        |
+---------------------------------------------------------------------------------------+
|  [Ribbon Toolbar: File | Home | Insert | Draw | View]                 [Quick Access]  |
+---------------------------------------------------------------------------------------+
|   NOTEBOOK TREE   |               INFINITE FREEFORM CANVAS             |  LIVE WORD   |
|                   |                                                    |    EDITOR    |
| 📁 My Notebook    |  [Original Note Content & Math Equations]          |              |
|   📑 General      |                                                    | H1 H2 H3 B I |
|   📑 Sketches     |  [Batched Cairo Vector Inks / Highlighting]        |              |
|   📑 Tasks        |  • Zero fragmentation, smooth pen strokes          | Live typing  |
|                   |                                                    | syncs to the |
|                   |  [➕ Added Text Boxes Anywhere on Canvas]          | selected     |
|                   |  • Double-click anywhere to type!                  | paragraph or |
|                   |                                                    | note box!    |
|                   |  [🖼️ Inserted Images & Diagrams]                   |              |
|                   |                                                    | [x] Done!    |
|                   |  [☑️ Direct-Click Task Checkbox Toggling]          | Chars: 420   |
|                   |  • Instant click to toggle without popups          | Words: 84    |
+---------------------------------------------------------------------------------------+
```

### 1. 🎨 In-Canvas Vector Ink & Sketch Engine
- **Hardware-Accelerated Cairo Batched Drawing**: Strokes are dynamically batched into single Cairo render passes, eliminating GTK4 scene-graph node explosion and guaranteeing smooth 60 FPS drawing.
- **Full Drawing Toolkit**: Freehand pen, semi-transparent highlighter, precision stroke eraser, custom line widths (1px .. 16px), and a rich color selection palette.
- **Sketch Viewing for Legacy Notes**: Renders original OneNote ink drawings and strokes found in existing `.one` files.

### 2. 📝 Arbitrary Content Placement Anywhere
- **Double-Click Anywhere to Note**: Just like the original Microsoft OneNote, double-click anywhere on empty canvas space to immediately spawn a new text note container.
- **Insert Images Anywhere**: Add diagrams, screenshots, PNG, JPEG, SVG, WebP, or GIF files anywhere with the Ribbon `🖼️ Add Image` button or native file chooser.
- **Position Persistence**: Custom notes, positioned images, and ink drawings are automatically indexed and saved with non-destructive local ink edit manifests.

### 3. 📑 Live Word Document Editor Dock
- **Slide-Out Document Sidebar**: Click any paragraph or text box to open the full-featured Word-style rich text editor.
- **Live Typing Synchronization**: Text updates live on the canvas as you type.
- **Full Formatting Ribbon**:
  - Headings: **H1** (24pt), **H2** (18pt), **H3** (15pt), Blockquote.
  - Typography: **Bold** (`Ctrl+B`), *Italic* (`Ctrl+I`), <u>Underline</u> (`Ctrl+U`), ~~Strikethrough~~.
  - Productivity: To-Do Task Checkboxes (`Ctrl+1`), Bulleted Lists, Numbered Lists, 3×3 Tables, Timestamp Inserts, and Dividers.
  - Visual Syntax Highlighting: GTK `TextTag` engine renders bold and headings directly in the editor buffer.

### 4. ☑️ In-Canvas Task Checkbox Toggling
- No frustrating modal popups! Clicking directly on any task checkbox (`☐` / `☑` or `[ ]` / `[x]`) on the canvas toggles the task immediately.

### 5. 🔄 Standalone CLI Exporter (`onenote-export`)
- Includes the `onenote-export` command-line utility.
- Batch-converts `.one` notebooks into **interactive standalone SVG sheets** or **self-contained HTML viewers** that open in any web browser without dependencies.

---

## 💻 Quick Installation for Any Linux

### Method 1: Automated Installer (Recommended)

Run the universal installer script:

```bash
git clone https://github.com/ROHANSHAJ/OneNoteOnly.git
cd OneNoteOnly
./install.sh
```

This installs:
- Executable to `~/.local/bin/onenoteonly` (and `~/.local/bin/onenote-viewer` symlink)
- CLI Exporter to `~/.local/bin/onenote-export`
- Vector application logo to `~/.local/share/icons/hicolor/scalable/apps/onenoteonly.svg`
- Desktop shortcut to `~/.local/share/applications/OneNoteOnly.desktop` with MIME associations

### Method 2: Build from Source

**Dependencies**:
- Rust 1.85+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- GTK4, Cairo, Pango, and SQLite development headers:
  ```bash
  # Ubuntu / Debian / Pop!_OS
  sudo apt install libgtk-4-dev libcairo2-dev libpango1.0-dev libsqlite3-dev build-essential pkg-config

  # Fedora
  sudo dnf install gtk4-devel cairo-devel pango-devel sqlite-devel gcc

  # Arch Linux / Manjaro
  sudo pacman -S gtk4 cairo pango sqlite base-devel
  ```

**Build & Run**:
```bash
cargo build --release -p onenote-viewer
./target/release/onenote-viewer
```

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl + O` | Open local `.one` file or notebook |
| `Ctrl + F` | Find and highlight text in current page |
| `Ctrl + B` | Toggle **Bold** text in editor |
| `Ctrl + I` | Toggle *Italic* text in editor |
| `Ctrl + U` | Toggle <u>Underline</u> in editor |
| `Ctrl + 1` | Toggle task checkbox (`☐` $\leftrightarrow$ `☑`) |
| `Ctrl + Z` / `Ctrl + Y` | Undo / Redo in Word Editor |
| `Ctrl + +` / `Ctrl + -` | Zoom In / Zoom Out page canvas |
| `Ctrl + 0` | Reset zoom to 100% |
| `Double Click` | Create and focus new note box anywhere on canvas |

---

## 📁 Repository Structure

```text
OneNoteOnly/
├── crates/
│   ├── onenote-core/           # Document object models, styles, ink strokes & primitives
│   ├── onenote-index/          # Full-text SQLite notebook search & indexing
│   ├── onenote-render/         # Freeform page layout resolution, math & layout culling
│   ├── onenote-render-gtk/     # GTK4 canvas, batched Cairo ink engine & arbitrary placement
│   └── onenote-viewer/         # Desktop application, ribbon UI, and Live Word Editor
├── packaging/
│   ├── OneNoteOnly.desktop     # Freedesktop application entry
│   └── flatpak/                # Flatpak manifest definition
├── install.sh                  # Universal Linux installer script
├── Cargo.toml                  # Cargo workspace specification
├── LICENSE                     # GNU General Public License v3.0
└── README.md                   # Technical documentation and user guide
```

---

## 📄 License & Attribution

This project is licensed under the **GNU General Public License v3.0 or later** ([GPL-3.0-or-later](LICENSE)).

- **Original Project**: [OneNoteViewer](https://github.com/emsi/OneNoteViewer) by **Mariusz Woloszyn**
- **OneNote Parser**: [onenote.rs](https://github.com/emsi/onenote.rs) by **Markus Siemens**
- **Linux Enhancements & Ink/Word Workstation**: **Rohan Shaj** (`@ROHANSHAJ`)

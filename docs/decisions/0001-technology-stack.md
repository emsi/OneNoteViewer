# ADR 0001: Rust, GTK4, and SQLite FTS5

- **Status:** Accepted for the first implementation
- **Date:** 2026-07-26
- **Decision owners:** Project maintainers
- **Revisit after:** Milestone 1 parser/canvas/search spikes

## 2026-07-26 Revisit

The first implementations of all three spikes pass their functional and
private-corpus tests, so Rust/GTK4/SQLite remains the selected stack. The GTK
renderer runs independently and in the complete viewer under Xvfb; the index
is transactional and the parser adapter isolates the revision-pinned
dependency.

This does not close the five GTK fallback gates below. Frame pacing,
mixed-script fixtures, GTK accessible canvas children, stable-memory
measurements, and GNOME/KDE appearance still need recorded evidence. Qt is
therefore a contingency, not an active parallel implementation. See
[remaining work](../REMAINING-WORK.md).

## Decision

Build a native Linux application in stable Rust using:

- `onenote_parser` behind an internal adapter for `.one` and `.onetoc2`;
- an external 7-Zip process for one-time, on-disk `.onepkg` extraction;
- GTK4 through `gtk-rs` for the desktop shell and accessible controls;
- a UI-neutral `onenote-render` crate for page layout and retained scenes;
- an embeddable `onenote-render-gtk` crate using Pango and GSK for the custom
  read-only page canvas;
- Cairo only where GSK lacks a suitable drawing primitive;
- SQLite FTS5 through `rusqlite` for a disposable local search index;
- GIO for file selection, URI launching, and registered-application handling;
- Flatpak as the primary distribution format and AppImage as an
  installation-free alternative.

No JavaScript, TypeScript, Node, npm, NVM, browser engine, or web server is
required. The selected stack is Linux-first rather than pretending that
cross-platform support is free.

## Why This Fits

The parser is the decisive constraint. As of the pinned 2026-07-23 upstream
commit, `onenote.rs` is a memory-safe Rust parser with desktop revision-store,
FSSHTTP, ink, math, warning collection, lazy attachment, and unreleased
`.onepkg` support. Reusing it keeps the entire application in one systems
language and avoids a second high-risk binary parser.

The parser's package API is an explicit exception: it reads the package and
all expanded entries into memory, so the application will not call it.
[ADR 0002](0002-onepkg-extraction.md) assigns package unwrapping to a bounded
external process and returns a native notebook directory to the normal parser.

GTK4 is not the easiest possible canvas technology, but it provides the Linux
desktop behaviors that are expensive to recreate: input methods, bidirectional
text through Pango, HiDPI scaling, accessibility, clipboard/drag integration,
portals, theming, and mature list widgets. GSK supplies a retained render-node
tree and GPU-capable renderers while still allowing Cairo fallback.

The UI-neutral scene is built from OneNote semantics and geometry, and the GTK
adapter renders it with Pango/GSK. Neither is built from HTML or converter
output. Web-based stacks were evaluated as UI technologies; that does not
permit a linear HTML/Markdown representation in the native viewer.

SQLite FTS5 provides ranked full-text queries, phrases, prefixes, proximity,
snippets, and Unicode tokenization without a service. It is appropriate for a
local index that can be rebuilt from notebooks.

These technologies are implementation details behind the reusable boundaries
accepted in [ADR 0003](0003-reusable-components.md). GTK does not leak into
the domain or scene contract, and SQLite does not leak into the public query
contract.

## Honest Costs

### Rust and `onenote_parser`

- Desktop support is currently on upstream `master` and advertised for the
  next major release, not in published `1.1.1`. Pinning a Git commit is
  acceptable for a spike, not for a stable release.
- The parser is MPL-2.0 and relatively small. Its compatibility claims require
  independent corpus validation.
- The parser exposes semantic objects but cannot supply Microsoft's
  undocumented exact layout behavior. We still own layout reconstruction.
- Rust compile times and GTK's GObject model are more complex than Python.

Mitigation: isolate the dependency in `onenote-core`, pin exact revisions,
upstream fixes, and do not stabilize our adapter API until the fixture matrix
passes.

### GTK4

- A freeform, zoomable document canvas needs a custom widget, hit testing,
  scene caching, and accessibility work.
- GTK applications can look more GNOME-like on KDE than a Qt application.
- GTK4 removed some older convenience APIs and requires main-thread discipline.
- `gtk-rs` follows GTK closely, so system-library version selection affects
  distribution support.

Mitigation: target the GTK version in the chosen Flatpak runtime, avoid
libadwaita-only navigation patterns, separate layout from rendering, and test
under GNOME and KDE on Wayland and X11.

### SQLite FTS5

- Default `unicode61` tokenization is not linguistically complete for every
  script and does not provide stemming across languages.
- Attachment body extraction introduces security and dependency risk.

Mitigation: index Unicode source text without destructive normalization, store
language metadata, keep the tokenizer replaceable at schema-version
boundaries, and defer attachment body extraction.

## Alternatives Considered

| Stack | Strengths | Material problems here | Decision |
|---|---|---|---|
| C++/Qt 6 Widgets or QML | Most mature Linux desktop toolkit; excellent KDE integration; `QGraphicsView`, text, PDF, and accessibility APIs | The strongest parser is Rust, creating FFI and two build ecosystems; C++ binary parsing has higher memory-safety risk; QML adds another language; LGPL compliance must be designed into distribution | Best fallback if GTK canvas or accessibility spike fails |
| Rust + Slint | Very small native runtime; pleasant Rust integration; custom rendering | Younger desktop widget/accessibility ecosystem; rich text, IME, document semantics, and assistive technology are more project risk than raw footprint | Reject for first release |
| Rust + Tauri 2 | Rust backend; CSS/HTML is productive for freeform layout; smaller than Electron | Still a WebView UI with HTML/CSS/JS tooling; Linux WebKit variance; larger attack surface for hostile notebook text/URLs; accessibility and print fidelity depend on WebKit | Contingency for a renderer prototype only |
| Electron | Mature DOM, canvas, and editor ecosystem | Chromium/Node footprint, JavaScript toolchain, broad attack surface, poor fit for a lightweight read-only viewer | Reject |
| Flutter/Dart | Fast custom rendering and cross-platform packaging | Non-native controls/text stack, extra language/runtime, weak benefit for Linux-only scope | Reject |
| Python + GTK | Fastest UI iteration and excellent GTK access | Rust parser FFI or subprocess boundary, packaging size/complexity, weaker static guarantees around untrusted binary data | Useful throwaway prototype, not product stack |
| C#/.NET + Avalonia | Productive, custom-drawn UI, cross-platform | Extra runtime, less native Linux integration, Rust parser interop, smaller Linux desktop ecosystem | Reject |
| Rust immediate-mode UI (`egui`, `iced`) | Single language and easy custom drawing | Native semantics, complex text, IME, accessibility, large virtualized documents, and desktop integration require substantial work | Reject |

## Why Not Qt Despite Its Maturity?

Qt is technically credible and may produce the best KDE experience. It loses
the initial decision because parsing is the project's harder and less
replaceable component. A Qt UI would either duplicate the parser in C++, expose
the Rust object graph through a large FFI surface, or serialize an intermediate
model between processes. All three add more risk than GTK's canvas work.

This is not a permanent rejection. Move to Qt if the milestone 1 GTK prototype
cannot meet all of these gates:

1. 60 fps pan/zoom on the large-page fixture at the target hardware baseline.
2. Correct Pango shaping for mixed RTL/LTR and complex scripts.
3. Keyboard navigation and screen-reader exposure for page text, links, and
   attachments.
4. Stable memory use with virtualized pages and decoded-image limits.
5. Acceptable appearance under both GNOME and KDE.

If Qt is selected later, keep `onenote-core` as Rust and use a deliberately
small, versioned C ABI or a process boundary rather than exposing parser
internals.

## Dependency Policy

- Keep the Rust 1.85.1 toolchain pinned through `rust-toolchain.toml`.
- Keep the public parser fork pinned to an immutable revision during
  pre-release work and move to an upstream tagged or crates.io release before
  beta.
- Prefer system/Flatpak GTK, Pango, and GIO libraries.
- Keep the 7-Zip dependency and sandbox behavior explicit as specified by ADR
  0002; never substitute the parser's in-memory package API.
- Build or bundle SQLite with FTS5 deterministically.
- Avoid framework wrappers until direct `gtk-rs` state management proves
  repetitive enough to justify one.
- Keep public component dependencies and compatibility policy aligned with ADR
  0003 and the public integration API specification.
- Record every new native library, license, and sandbox permission.

## Sources Checked

- Microsoft `[MS-ONE]` and `[MS-ONESTORE]` snapshots in
  `docs/references/microsoft/`
- `onenote.rs` at commit
  `f9cdc59f984bc1f7f096b54100cefaaebc892573`
- GTK4 and `gtk-rs` official documentation
- SQLite FTS5 official documentation
- Qt open-source licensing documentation
- Tauri 2 architecture documentation

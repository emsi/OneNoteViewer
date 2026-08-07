# Desktop UI Requirements

Status: normative for `onenote-viewer`.

This specification defines the viewer shell and dialog behavior. It does not
constrain the reusable page renderer or indexing APIs.

## Compact Application Shell

The primary window uses one compact header row and one compact status row.
There is no second menu bar or decorative toolbar consuming document height.

The header contains:

- one native window-menu icon at the left edge, sourced from the registered
  bundled application icon rather than a second manually packed image;
- the OneNote Viewer identity;
- global notebook search;
- one main menu beside search for file, import, settings, and application
  commands; and
- native minimize, maximize, and close controls supplied by the desktop.

Frequently repeated, context-specific controls may remain beside their
content. Infrequent application commands belong in the main menu. Every icon
is bundled with the application, is symbolic, and has an accessible tooltip;
rendering must not depend on a host icon theme.

## Complete Theme Ownership

The application supports System, Light, and Dark appearance preferences.
System follows GTK's current application dark-theme preference. The selected
preference is persisted and applies to the main window, menus, dialogs,
backdrop states, navigation, status UI, inputs, and controls.

Application CSS must use a complete semantic palette. A component that
overrides foreground color must also own its background, border, hover,
disabled, selected, and backdrop colors where those states exist. It is
invalid to combine an application-defined foreground with a host-theme
background.

Navigation rows use selection as their only highlight. Pointer hover alone
does not resemble selection; moving across sections or pages must not obscure
which page is currently active.

Selection is also the single source of navigation events. A section or page
changes only when GTK changes the corresponding selection; the application
must not add row-level click gestures, press-count filters, delayed activation,
or a second activation signal that can diverge from the visible highlight.
Programmatic selection changes use a reentrancy guard so one transition renders
one page.

The selected section and page always describe the page shown in the document
area. Clicking a notebook restores its last valid section and page, or the
first available section and page when it has no valid history. Clicking a
notebook label does not expand or collapse it. Section-group expansion belongs
only to `GtkTreeExpander`; group rows do not become content selections.

Notebook and page targets use stable source, section, and page identifiers,
never mutable row or vector positions. Loading or indexing another notebook
must not clear, move, or replace the active selection. Reloading the active
notebook preserves its last valid location and falls back deterministically if
that content was removed.

The freeform notebook page is document content rather than application chrome;
its explicit colors continue to come from OneNote page data. Text stored with
OneNote's automatic/default color uses a host-provided renderer fallback so it
remains readable on the viewer's current page surface. Embedders can supply
their own fallback without rewriting the scene or explicit source formatting.

Standard window-control fallbacks and the application icon are bundled so the
native, AppImage, and Flatpak headers do not depend on different host icon
inventories.

## Page Context Header

The page title and complete notebook/section-group/section context are
selectable application text. Pointer selection, the standard label context
menu, `Ctrl+A`, and `Ctrl+C` use GTK's native label behavior and must not
change the active notebook, section, or page. Visual end ellipsizing may
constrain the header layout, but selecting all and copying preserves the
complete underlying Unicode value.

The page date may remain display-only. Selection inside the freeform page
canvas is a separate renderer capability and is not implemented through these
header labels.

## Link Interaction

Explicit OneNote hyperlinks are always underlined. The persisted **Detect
links in plain text** setting additionally recognizes visible URLs and email
addresses and is enabled by default. Changing it reloads and reindexes every
open source so one workspace never mixes policies. Pointer hover changes only
the pointer affordance; it does not select content, activate navigation, or
alter the current page. A primary-button click activates only the linked glyph
range, including when hidden OneNote marker text, list prefixes, math, or
non-BMP characters affect the source-to-display offset mapping.

`http`, `https`, `mailto`, `ftp`, `tel`, and `sms` targets open through GTK's
portal-aware URI launcher after the click. File paths/URIs and unfamiliar
schemes show a selectable confirmation containing the exact target before the
desktop is asked to open them. OneNote page links resolve by native page ID
across all currently open notebooks, preferring the active source; unresolved
targets produce a selectable diagnostic. The renderer emits inert actions and
does not own any of these application policies.

Pointer activation does not satisfy keyboard access or general document text
selection. Those remain separate accessibility and selection work.

## Selectable Diagnostics

Every error dialog exposes its full diagnostic text in a non-editable,
keyboard-focusable text view with normal text selection. A visible `Copy
error` command copies the title and complete detail to the clipboard. Error
titles are selectable as well.

Paths, source names, destinations, warnings, and explanatory text in custom
dialogs are selectable. Selection remains visibly contrasted in active and
backdrop states. Do not use an ordinary non-selectable label for information a
user may need to include in a bug report.

## Dialog Controls

Custom dialog buttons and selectors use the same semantic palette as the main
window. Primary actions use the accent color; ordinary and disabled actions
remain legible in both themes and when the window loses focus. Dialog content
must not inherit an uncontrolled mix of host-theme control backgrounds and
application text colors.

## File And Folder Choosers

File and folder access uses `GtkFileDialog` so sandboxed builds receive access
through the desktop portal. Folder requests suggest the current valid
destination, fall back to the configured default notebooks location when it is
invalid, and finally fall back to Home only when neither directory exists.

Desktop portal implementations are permitted to ignore the suggested initial
folder. KDE versions or portal configurations that ignore `current_folder`
can consequently open at Home even though OneNote Viewer supplied the validated
destination. The initiating control must therefore show immediate pending
feedback until the out-of-process chooser appears or returns; the application
must not look unresponsive while a desktop backend starts.

## Regression That Established These Rules

Earlier dialog CSS forced a dark text color on labels and buttons while
leaving button backgrounds to the host GTK theme. A dark KDE theme therefore
rendered dark text on dark buttons, and focus/backdrop transitions changed
which controls were visible. The same class of mistake had already affected
navigation text and bundled icons.

The failure was not a missing per-button override. It was incomplete theme
ownership and insufficient validation across active/backdrop states and
packaging environments. Future UI work must not repair this by adding another
isolated foreground rule.

## Required Visual Matrix

Before publishing UI changes, exercise:

| Package | System light | System dark | Explicit light | Explicit dark |
| --- | --- | --- | --- | --- |
| Native development build | required | required | required | required |
| AppImage | required | required | smoke | smoke |
| Flatpak | required | required | smoke | smoke |

For each required case, inspect the main window, open main menu, Settings,
package-import confirmation, an error dialog, active window, and inactive
window. Verify native window controls, bundled icons, text selection,
selection contrast, ordinary buttons, primary buttons, and disabled buttons.

Automated icon checks and GTK smoke tests are necessary gates but do not
replace this visual matrix.

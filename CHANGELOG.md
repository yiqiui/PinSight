# Changelog

All notable changes to PinSight are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- License changed from MIT to **Apache-2.0** (adds an explicit patent grant and
  trademark/attribution terms). A `NOTICE` file records the copyright.
- Settings got a visual overhaul. Controls are right-aligned in every row
  instead of trailing a fixed-width label (which left a dead 300 px gap), the
  eight tabs are an underline strip rather than a plate of pills, cards are
  capped at a readable width and centred, row spacing is tighter, and the
  scrollbar is a thin neutral handle instead of a near-black stripe.
- Every checkbox is now a switch (`ui::toggle`): a 42×24 track with an animated
  knob, accent-filled when on, with a hover and focus state.
- Sliders are drawn by the app (`ui::slider`): thin track, accent fill, round
  knob, value in a quiet badge. `egui::Slider` mirrors itself inside a
  right-to-left row, which made the knob grow the wrong way — the custom one
  always reads left-to-right.
- Colour pickers show their hex value next to a bordered swatch
  (`ui::color_field`), so white is visible on a white card.
- Packaging can now UPX-compress every artefact from one switch:
  `scripts/package.ps1 -Upx` compresses the staged exe *and* makes the Inno
  Setup installer bundle that compressed copy (the `.iss` takes the exe path via
  `/DPinsightExe=`), so the installed binary is ~2.5 MiB rather than ~7.2 MiB.
  `tools/_repackage_portable.py` always emits both a plain and a `-upx` zip;
  `PINSIGHT_UPX_ONLY=1` limits it to the UPX one. The release workflow gained a
  `upx` dispatch input that installs UPX per OS and compresses before packaging.
- Long-capture stitching no longer copies the whole image on every frame. The
  stitched output is now a growable row buffer (`Stitch`) appended in place
  (amortised O(n) instead of the old O(n²) reallocate-and-copy), which matters
  for tall captures of hundreds of frames; the single `RgbaImage` conversion
  happens once at the end. The overlap search also runs coarse-to-fine: a
  strided scan locates the few candidate peaks and only their neighbourhoods
  are compared at full resolution, cutting the per-frame alignment work without
  changing the resulting offset.

### Added

- **Upload**: a configurable share step (`src/upload.rs`). An HTTP backend posts
  the PNG as multipart (POST) or a raw body (PUT) to any endpoint — image host,
  OSS pre-signed URL, WebDAV — and extracts the link from a JSON field; a local
  backend writes a copy to a folder and returns its path. Wired to the editor
  toolbar (`OverlayAction::Upload`, run off the UI thread) and to
  `pinsight shot --upload`, which prints the URL on stdout and copies it to the
  clipboard in the GUI.
- **Custom notification sound**: `capture_sound_copy`/`capture_sound_save` can
  now play a user-chosen WAV/MP3 (settings: browse/reset) instead of the default
  system beep.
- **Record with audio**: `record_audio` + `audio_device` feed the microphone
  (or loopback) into the recorder via ffmpeg — Windows DirectShow (auto-detects
  the first input device when the name is blank), macOS AVFoundation, Linux
  PulseAudio — muxed as AAC. Falls back to silent video with a warning when no
  device or ffmpeg is available.
- **Voice notes**: `pinsight voice --out file.wav --seconds N` records the
  default microphone to a mono 44.1 kHz WAV via ffmpeg, and the tray gained
  "Record voice note" (duration from `voice_seconds`, saved to the capture
  folder). In-app playback is not included.
- **GIF editor** (`src/gifedit.rs`): pure-Rust decode/encode on top of the `gif`
  crate — **trim** a frame range, **crop** every frame to a rectangle, and burn
  **text** (CJK-friendly, five anchor positions, transparent overlay via
  `text_font::render_text_overlay`); **convert to MP4** (H.264) via ffmpeg. A new
  in-app "GIF" page (file picker, operation, parameters, run off the UI thread,
  open result) and tray "Edit GIF" action drive it; the CLI gained
  `pinsight gif <trim|crop|text|mp4> <INPUT> [OPTIONS]`.
- **Capture delay**: a 0–30 s countdown before a capture, so menus, tooltips and
  hover states can be caught. The main window stays visible during the countdown
  (the toast is readable) and only parks right before the grab.
- **Window capture**: `capture::list_windows` / `grab_window` enumerate and grab
  a top-level window cross-platform (`xcap::Window`). The capture page has a
  "Capture window" button that opens a picker; the CLI gained
  `pinsight shot --window <id>` and `--list-windows`.
- **Line tool** in the annotation editor (`L`), alongside the existing pen,
  highlighter, mosaic and numbered-step tools; the recording overlay also got it.
- **Capture history / gallery**: a new page lists every image in the capture
  folder as a thumbnail grid (newest first, one-second scan cache), with
  click-to-pin and a right-click menu to pin or delete.
- **Save decorations** (Output tab): optional border frame, soft drop shadow
  (separable box blur, cast by the framed image) and a text/image watermark
  (tile / centre / four corners, adjustable opacity, size and margin). Applied
  inside `util::save_image`, so every save path — region, window, clipboard-as-
  file — gets the same look.
- **Export to PDF**: a self-contained PDF 1.4 writer (`src/pdf.rs`, no extra
  dependency — images are stored losslessly with `FlateDecode`). One image per
  page, A4, landscape for wide captures; tall/long captures are split across
  several pages so text stays readable. Transparency is flattened over white.
  CLI: `pinsight pdf <img...> --out book.pdf`. Gallery page: tick items and
  "Export PDF" writes them via a save dialog.
- Overlay magnifier: a 5–31 px zoomed grid next to the cursor with the sampled
  colour and its coordinates. The colour value format is configurable (HEX /
  RGB / HSL) and the panel flips to stay on screen near an edge.
- "Capture the mouse cursor": the cursor is composited into area and full-screen
  captures. Windows has no API that returns a cursor as RGBA, so the shape is
  recovered by drawing it once on black and once on white and solving for
  alpha — which keeps monochrome, colour and anti-aliased cursors intact.
- Restorable pins: closing a pin keeps its bitmap and geometry, and the last
  N (0–50, default 5) can be brought back from the pins page, the tray menu, or
  by closing everything at once.
- Tray icon style: automatic (follows the taskbar theme), white glyph, colour,
  or a custom PNG. Switching rebuilds the icon in place, no restart.
- "Restart as administrator" in the settings: relaunches through the UAC prompt
  via `ShellExecuteW`/`runas`.

- Text annotation tool (`T`): click, type, Enter places it, Escape cancels.
  The rasteriser outlines glyphs with `ab_glyph` from the same font file the UI
  renders with, so the saved PNG matches the preview; the toolbar's stroke
  slider doubles as the font-size control.
- Pin right-click menu (Windows): copy image, save as, zoom presets, always on
  top, click-through, close and destroy, plus the live pixel size — the pin is
  no longer a window you can only get rid of with the middle mouse button.
- `pinsight shot` headless capture, scrot/maim style: `--region`, `--monitor`,
  `--out`, `--clipboard`, `--delay`, `--list-monitors`. It never creates a
  window or a GL context, so it works in scripts and remote sessions.
- **Text recognition (OCR)**: the capture editor's toolbar gained an "OCR"
  button that recognises the text inside the current selection (or the whole
  capture) with the OS-native offline engine and shows the result in a floating
  panel with a one-click "Copy text". The CLI gained `pinsight shot --ocr`, which
  prints the recognised text to stdout and the saved image path to stderr (so the
  text stream stays machine-readable). Recognition runs on a worker thread, and
  the flat word list is regrouped into lines by baseline for readable output.
  The engine is platform-specific and always offline:
  - **Windows**: `Windows.Media.Ocr` (ships with every Windows 10/11 install and
    works with the user's display language and installed language packs). The
    WinRT call runs on an MTA apartment because blocking the STA GUI thread would
    deadlock.
  - **macOS**: Apple Vision `VNRecognizeTextRequest` — offline, ships with macOS,
    nothing to bundle.
  - **Linux**: Tesseract via the system `libtesseract` (option "L1" — linked, not
    bundled); language packs such as `tesseract-ocr-<lang>` must be installed, and
    the feature degrades to a clear install hint when they are missing.
- Recording a hotkey now refuses a combination another action already owns,
  with a localised message instead of a phantom "already in use" conflict.
- ARM64 release builds: `aarch64-pc-windows-msvc` and
  `aarch64-unknown-linux-gnu` joined the release matrix.
- Localised tooltips for the annotation colour swatches and the stroke slider;
  every tool button already carried its name and shortcut.
- Idle memory: the working set is handed back to the OS eight seconds after
  startup and whenever the window is hidden to the tray — the Task Manager
  number drops from ~89 MB to ~5 MB while idle.
- Undo/redo for overlay annotations: `Ctrl+Z` / `Ctrl+Y` (plus `Ctrl+Shift+Z`,
  which some input methods claim first — hence the documented `Ctrl+Y`).
  A snapshot-based `History` keeps append, delete-last and move consistent;
  the toolbar gained matching Undo/Redo buttons that dim when empty.
- Numbered annotation tool (`N`): click places an auto-incrementing step
  marker. The digits are drawn as seven-segment vector strokes, so preview and
  rasteriser share one code path and no font is shipped to the rasteriser.
- Single-instance guard: a Windows named mutex, or a PID-checked lock file on
  macOS/Linux. A second launch raises the running instance (Windows) and exits
  instead of registering a second set of global hotkeys. `--mcp` deliberately
  never takes the slot so the MCP server can run beside the GUI.
- Windows notification-area icon with a localised context menu (capture area /
  full screen / pin clipboard / long capture / record / save last / show /
  quit). Every row shows its shortcut, and the icon re-arms itself when
  Explorer restarts.
- Custom background image for the main window: native file picker, Gaussian
  blur radius, dimming veil and cover / contain / stretch fit. The choice is
  persisted and restored on the next launch.
- Settings page split into Appearance / Output / Hotkeys / About slices; the
  open slice is remembered across restarts (`settings_tab`).
- In-UI hotkey recording: click "Record" on a row and press the new
  combination (Esc cancels). Bindings are re-registered live — no restart —
  and conflicts are still reported per row with a localised reason.
- "Run at sign-in" support: Windows `HKCU\...\Run`, Linux
  `~/.config/autostart/*.desktop`, macOS launch agent. Best effort per
  platform, with the reason logged when it fails.
- Portable mode: `--portable`, `PINSIGHT_PORTABLE=1`, or a `pinsight.portable`
  marker file next to the executable keeps `config.json` and captures inside
  the install folder. No registry entries and no `%APPDATA%` writes.
- Packaging: `scripts/package.ps1` builds a portable zip and an Inno Setup
  installer (`packaging/windows/pinsight.iss`) whose directory page lets the
  user choose the install location; the release workflow builds Windows,
  macOS and Linux artefacts.
- Cross-platform pin windows: non-Windows platforms render pins through an
  egui viewport (`pin_viewport.rs`) with the same drag / zoom / opacity /
  click-through behaviour as the Win32 pins.
- Redrawn application icon set (PNG, ICO and a multi-resolution tray ICO),
  generated from vectors so it stays sharp at every size.
- Release profile tuned for footprint: fat LTO, one codegen unit and stripped
  symbols (`[profile.release]`), and system fonts are memory-mapped instead of
  copied into the heap, so a 20 MB CJK face costs no private commit.
- Interface overhaul: icon navigation rail, capture tiles with shortcut badges,
  pin cards, settings cards, status bar, and a hand-painted 24×24 vector icon
  set. Every control is drawn with `egui::Painter`, so it stays sharp at any DPI.
- Appearance settings: follow-system / light / dark theme, five accent colours,
  text scale from 85% to 135%, applied instantly and persisted.
- Localisation into seven languages (Simplified Chinese, Traditional Chinese,
  English, Japanese, Korean, French, German) with automatic detection of the
  Windows UI language. All strings live in a single table, so a missing
  translation is a compile error.
- Font selection: automatic language-aware system font with a CJK fallback, plus
  sans / serif / mono / custom-file options. The settings page reports the font
  pair that was actually resolved.
- Redesigned capture toolbar: dark glass panel with tool icons, colour swatches,
  stroke slider, a size readout and primary/secondary action pills. It docks
  below the selection and keeps itself on screen.
- Hotkey failures are now classified (`in use` / `invalid`) and surfaced as a
  localised reason, with a per-row status dot in settings.
- HDR capture fix: a `hdr_fix` preference routes captures through the SDR (GDI)
  path on displays running in HDR mode, where Windows.Graphics.Capture returns
  washed-out (over-bright) frames. Detection is by DXGI colour-space overlap, so
  only regions over an actual HDR output switch; SDR monitors are untouched.
- Long-screenshot tuning, all in Capture settings: `long_scroll_ticks`
  (auto-scroll notches per step), `long_scroll_delay_ms` (settle time after a
  scroll — longer for smooth-scroll pages), `long_diff_threshold` (how still a
  frame must be before it counts as "did not move"), `long_idle_limit` and
  `long_manual_idle_limit` (still-sample patience in auto vs. manual mode), and
  `long_max_frames` (hard sampling cap so a stuck scroll cannot loop forever).
- Recording annotation layer: live vector marks — arrow, rectangle, ellipse,
  text and emoji — drawn on a click-through canvas covering exactly the recorded
  region. The desktop compositor burns them into every frame the recorder grabs,
  so the output carries the marks without the recorder knowing anything about
  them. The toolbar has colour swatches, a stroke/size slider, undo/redo and
  clear.
- Background-job HUD moved into its own always-on-top, transparent pill window.
  The recording pill shows a pulsing dot, elapsed `mm:ss`, an optional limit, and
  Pause/Resume + Annotate + Stop; the long-capture pill shows stitched frame
  count, current height and a Stop. Living outside the main window keeps them
  visible while the main window is parked off-screen during a capture.
- `Esc` now stops an active recording — the same key that cancels a capture or
  selection, so ending a session never depends on remembering a separate
  shortcut.

### Changed

- Closing the main window now keeps PinSight running in the tray instead of
  quitting, so the global hotkeys stay live. The behaviour is configurable
  (`close_to_tray`), and "Quit" in the tray menu always exits the process.
- Rebinding hotkeys reuses the live `GlobalHotKeyManager` — the old set is
  unregistered, then the new one registered — instead of building a second
  manager, which used to fail with "already registered" for most rows.
- The selection outline and handles follow the accent colour instead of a fixed
  red.
- Pin cards are painted from a single rectangle instead of nested layouts, which
  makes the metadata, zoom readout and action strip line up regardless of label
  length.

### Fixed

- The tray menu did nothing at all — not even Quit. The app runs with
  `ControlFlow::Wait`, so an idle window never repaints, and `poll_tray` only
  ran inside the update pass: commands sat in the queue until the user happened
  to move the mouse over the window. The tray now wakes the UI after queueing a
  command, and "Open PinSight" restores the window from the tray thread itself.
- The release exe had no embedded icon or version block (generic blank icon in
  Explorer and Task Manager). `build.rs` now compiles a small `.rc` with the
  SDK's `rc.exe` — located by scanning Windows Kits, so no registry lookup.
- Language names in the picker are translated into the current UI language
  instead of being listed in their own script. That removes the last tofu case
  (Korean behind a Chinese/Latin face) *and* saves ~16 MB: egui copies font
  bytes, so the CJK/Korean fallbacks are now loaded only for the languages that
  need them.
- Text annotations no longer parse the 16 MB font at startup — the rasteriser
  face is parsed on the first annotation that actually needs it.
- Switching the UI language showed tofu boxes for every CJK language name in
  the picker: the CJK fallback face was only installed when the *current*
  language needed it, but the picker lists every language in its own script.
  The fallback is now always installed (memory-mapped, so it costs no private
  commit).
- The tray menu's "Quit" item was intercepted by the close-to-tray handler and
  only hid the window; the process never exited. A `quitting` flag now marks
  the intent so the close is allowed through.
- xcap's Windows backend (Windows.Graphics.Capture) can hand back an all-black
  frame — observed on some remote sessions and driver states. A uniformly black
  capture is now retried with a GDI screen blit, and the frame with content
  wins; a genuinely black screen is unaffected.
- Pressing Esc while recording a hotkey showed a red "needs at least one
  modifier" error; it now shows a neutral "recording cancelled" message.
- The "last capture" card stayed empty after pinning or saving; it is now
  populated for every capture outcome.
- The output folder field clipped long paths; it now grows with the card.
- Hotkey conflicts leaked the backend's `Debug` output into the status bar; they
  are reported as a localised reason instead.
- Removed the temporary `tracing::info!` scaffolding from the capture path.
  `RUST_LOG=pinsight=debug,eframe=debug` shows both PinSight and backend records.
- Long screenshots duplicated the captured content every frame (the stitched
  image repeated the same slice over and over). `find_overlap` indexed the
  accumulated output by the shared `min` height, which for a tall accumulated
  image read a *middle* row instead of its bottom edge, so the overlap band
  never matched and the whole frame was appended. It now reads the band from the
  bottom of the accumulated image and the top of the new frame (regression
  tests cover the tall-`prev` case).
- PinSight's own always-on-top progress pill (scrolling capture, area capture
  **and** recording) was baked into captures, and on stitched slices / area
  grabs it appeared as a solid black rectangle. Root cause: the `xcap`
  dependency was declared without its `wgc` feature, so `xcap` silently fell
  back to **GDI BitBlt** for every capture — and GDI cannot composite a layered
  (transparent) window, so the pill's alpha landed as 0,0,0 and surfaced as pure
  black. `WDA_EXCLUDEFROMCAPTURE` (set every UI frame) was therefore a no-op:
  only Windows.Graphics.Capture honours it. The `wgc` feature is now enabled, so
  the default capture path runs on WGC, which drops the excluded pill from the
  frame entirely — no black rectangle and no hide/show flicker (recordings get
  the same fix for free, with a one-time affinity re-assert at worker start so
  the first frame cannot race the first UI frame). The GDI path is now only used
  for HDR displays and as the black-frame retry; there the pill is hidden around
  the grab (see below).
- Long screenshots still looked like several grabs glued together after the
  pill fix: the overlap detector ranked candidates by the *number* of matching
  rows, so when the page scrolled a long way (leaving only a small remaining
  overlap of 100–400 px) a coincidental wrong offset could out-vote the real
  one and the worker would append a whole duplicate slice. The detector now
  ranks by *coverage* — the longest unbroken run of matching rows as a share of
  the informative rows — which is ≈1.0 for the true offset and short for any
  wrong one no matter how many scattered votes it collects. Blank rows are
  still skipped (they match everywhere) and a foreign window over the region
  only costs a local run break, not the whole alignment. Verified against real
  screen pixels (11/11 offsets, including 600/900 px scrolls) and eight Rust
  regression tests.
- Long screenshots left a solid black rectangle where the scrolling-capture
  progress pill was, on HDR displays (where the GDI / SDR capture path is used).
  GDI's `BitBlt` cannot composite a layered (transparent) window and surfaces it
  as pure black, and the previous hide-then-grab wrapper trusted
  `IsWindowVisible` — which flips false the instant `ShowWindow(SW_HIDE)` is
  issued — so it grabbed before DWM had recomposited the desktop without the
  window. It now waits out a couple of desktop frames after hiding and
  re-asserts the hide every few milliseconds to counter egui's viewport repaint
  loop re-showing the pill, so the window is gone from the captured frame.
- Long screenshots dropped slices and showed blank seams when the user scrolled
  faster than the worker sampled: in manual mode the 420 ms settle delay was
  dead time during which content scrolled past uncaptured. Manual mode now
  samples quickly (~30 ms) so consecutive frames always overlap the previous
  one, and a duplicate-overlap guard compares each candidate's appended band
  against the stitched seam and tries the next-larger overlap when an
  under-estimated offset would re-paste already-captured rows (the "repeated
  content" report). The guard ignores blank bands so it never false-positives on
  mostly-empty pages.
- Long screenshots pasted a duplicate copy of the page when the user paused to
  read: the aligner compared the new frame against the *tall accumulated
  output*, whose bottom is no longer a full frame after a partial append, so a
  static frame could not reach a full-height overlap and matched a spurious
  offset instead. Alignment now uses the previous **full frame** as the
  reference, and a frame that is essentially unchanged is detected before
  aligning, so a still page contributes nothing (and, in manual mode, the job
  now tolerates ~30 s of stillness before ending instead of 4.5 s).
- Long screenshots pasted a second copy when the user scrolled back up: the
  aligner only searched downward, and a reverse scroll can also produce a weak,
  coincidental forward match, so the frame was stitched as new content.
  Forward and reverse matches are now both measured and compared by quality
  (`coverage × votes`); a reverse scroll that is stronger and covers at least
  half the frame is skipped, while a genuine downward scroll keeps its solid
  forward match. (Regression tests cover static, reverse and forward scrolls.)
- Long screenshots repeated sticky chrome (browser nav bar, editor tab strip,
  terminal status bar) down the stitched image on large scrolls: the overlap
  could fall inside the header, so the appended band started with it. Fixed
  bands at the top **and bottom** of the region are now detected by comparing
  consecutive frames at the same rows; blank rows are ignored so mostly-white
  pages are not mistaken for chrome. The header is stripped from the appended
  band (and a frame whose overlap lands inside it is dropped, since that
  alignment is untrustworthy); a fixed footer is lifted out of the stitched body
  and re-attached once at the very end. This only runs inside the long-capture
  loop — single-shot captures are untouched.
- Long screenshots waited out the full manual idle timeout (~30 s) after
  reaching the bottom. A best-effort scrollbar reader now detects a thumb at the
  end of its track and ends the capture after a few still frames. It only ever
  shortens an already-idle wait (never cuts a mid-page pause) and returns false
  for overlay/auto-hidden scrollbars, which fall back to the normal timeout.
- Release profile now uses `panic = "abort"` (drops the unwind runtime and
  `.eh_frame` tables) and `opt-level = "s"` (optimise for size — this is an
  I/O-bound GUI tool, so the few percent of runtime it costs is invisible). The
  release exe is now **7.2 MiB** (was 11.2 MiB). The remaining bulk is the
  egui/eframe GUI stack, the ~1.4 MiB bundled default fonts (Latin + mono +
  emoji), and the image codec set for the multi-format save — all kept because
  removing them would drop features. `tools/_repackage_portable.py` now also
  emits `PinSight-0.1.0-portable-upx.zip` (2.5 MiB exe) for people who want the
  smallest download; UPX-packed binaries can trip antivirus heuristics, so the
  plain package stays the default.
- Upload failed out of the box with "upload is disabled in the configuration":
  the config defaulted to disabled and there was no settings UI. It now defaults
  to enabled with the local backend (copies into `<save_dir>/uploaded`), falls
  back to that folder when none is set, and the Output tab gained an Upload card
  (enable, HTTP/local backend, URL, method, form field, raw/JSON response and
  JSON key, local folder picker).
- Recording stuttered the live cursor: the capture loop called
  `xcap::Monitor::all()` on every frame (15–30×/s), which starved the
  compositor. The monitor is now resolved once before the loop.
- The recording annotation layer's emoji palette drew blank buttons: the build
  only enabled `default_fonts`, so egui's font chain had no emoji glyphs. It now
  enables `monochrome_emoji_fonts` (NotoEmoji + emoji-icon-font).
- Text annotations in the recording layer could never be committed: the
  multiline `TextEdit` swallowed Enter as a newline. Plain Enter now commits and
  Shift+Enter inserts a newline.

## [0.1.0] - 2026-10-22

### Added

- Native region capture with a frozen-screen overlay, eight resize handles and
  drag-to-move selection.
- Annotation tools: rectangle, ellipse, arrow, pen, highlighter and mosaic.
  Rasterised with `tiny-skia` at source resolution.
- Native Win32 pin windows: borderless, always-on-top, draggable, wheel zoom,
  `Ctrl+wheel` opacity, middle-click/Esc close and click-through mode.
- Clipboard image pinning (`Ctrl+Shift+V`).
- Experimental scrolling long screenshots with overlap detection and stitching.
- Region recording to GIF, or H.264/MP4 when `ffmpeg` is available.
- Six global hotkeys, conflict reporting and JSON configuration.
- MCP stdio server (`--mcp`) exposing capture, pin, recording, long-screenshot
  and status tools.
- Dark/light theme, output folder shortcuts, pin management panel.
- Unit tests for hotkey parsing, overlap detection and annotation rasterisation.
- Architecture documentation and CI workflow.

### Known limitations

- Pin windows are Windows-only in this release.
- Text annotations, OCR, tray icon and single-instance mode are not implemented.
- Long screenshots are sensitive to fixed headers, sticky elements and animation.

[0.1.0]: https://github.com/your-org/PinSight/releases/tag/v0.1.0

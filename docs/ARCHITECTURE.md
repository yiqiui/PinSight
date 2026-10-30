# PinSight Architecture

This document explains how PinSight is put together, which thread owns what, how
coordinates flow between Win32, egui and captured images, and where the sharp
edges are.

## 1. Goals and non-goals

**Goals**

- Native desktop tool: no WebView, no browser runtime.
- Fast capture → annotate → copy/pin loop.
- Predictable always-on-top pin windows that behave like OS windows.
- A scriptable surface (MCP) for AI agents and automation.
- Small, inspectable modules; unsafe Win32 code isolated in `pin.rs` and `tray.rs`.

**Non-goals (for v0.1.0)**

- Pixel-perfect parity with Snipaste/PixPin.
- Windows-only behaviour: the same tree builds and runs on macOS and Linux
  (pins fall back to an egui viewport, tray is Windows-only).
- OCR, cloud sync, accounts, telemetry.
- Plugin system.

## 2. Process and threads

PinSight is a single process with a handful of threads:

| Thread | Owner | Responsibility |
| --- | --- | --- |
| UI / event loop | winit + eframe | egui passes, overlay interaction, main window, hotkey polling, tray window messages |
| MCP stdio | `mcp.rs` | reads JSON-RPC from stdin, writes responses to stdout |
| Long screenshot worker | `longshot.rs` | sends wheel events, captures frames, stitches |
| Recorder worker | `record.rs` | captures frames, encodes GIF or pipes raw video to ffmpeg |
| One message loop per pin (Windows) | `pin.rs` | owns a native `WS_POPUP` window, paints with GDI, handles drag/zoom/close |
| One viewport per pin (other platforms) | `pin_viewport.rs` | egui viewport with the same interaction model |

The UI thread is never blocked on capture, encoding or MCP I/O. MCP tool calls
that need the UI (capture, pin, recording) send a `UiCommand` over an mpsc
channel and block on a per-request reply channel with a timeout.

The tray icon does **not** get a thread of its own: its message-only window is
created on the UI thread, so the messages winit already pumps for the main
window also reach the tray window procedure. Menu selections travel back over
an mpsc channel (`TrayCommand`) that the UI loop drains every frame — see §14.

```
MCP thread ──UiCommand──▶ UI thread
     ▲                      │
     └──── reply sender ◀───┘
```

## 3. State machine

The interesting part of `app.rs` is the capture state machine:

```
Idle
 ├── hotkey / MCP capture ──▶ ParkingRoot
 │                              │  move root to (-10000,-10000)
 │                              ▼
 │                           Capturing (after ~140 ms)
 │                              │
 │              ┌───────────────┴────────────────┐
 │              ▼                                ▼
 │        CaptureSession                  return image / reply
 │        (overlay open)                  (MCP, full-screen copy)
 │              │
 │     Enter / toolbar action
 │              │
 │     ┌────────┼──────────┬──────────────┐
 │     ▼        ▼          ▼              ▼
 │   Copy     Pin        Save      Long / Record
 │                                      │
 │                                 worker starts
 │                                      │
 └────────────────────────────── job finished / failed
```

`root_restore` stores the window position before parking. The restore path
sends both `OuterPosition` and `Focus`, so the user lands back in PinSight after
the overlay closes.

### Why not hide the root window?

`ViewportCommand::Visible(false)` stops eframe from running UI passes for the
root. Immediate viewports (including the capture overlay) are rendered from the
root pass, so hiding the root would prevent the overlay from appearing at all.
Parking the window outside the virtual desktop keeps the UI alive while removing
it from the screenshot.

Windows treats positions near `-32000` as "minimized" and then stops delivering
UI passes, which makes the window impossible to restore. PinSight parks at
`-10000`, which is outside a normal desktop but still a valid window position.

## 4. Coordinate spaces

Three spaces are in play and mixing them is the most common source of bugs:

| Space | Unit / origin | Used by |
| --- | --- | --- |
| Physical desktop | pixels, virtual-screen origin | `xcap`, `enigo`, MCP region arguments, native pin windows |
| Logical points | physical / monitor scale, virtual-screen origin | egui viewport positions and sizes |
| Image pixels | pixels relative to a captured monitor image | selection, annotations, cropping |

Rules:

- `capture.rs` returns physical rectangles and physical images.
- `overlay.rs` converts pointer points to image pixels exactly once
  (`to_img`), and keeps selection/annotations in image pixels.
- Rasterisation (`annotate::rasterize`) copies the crop at source resolution;
  no preview scaling leaks into the output.
- Pins are created with **physical** `x/y/w/h` (`monitor.rect + selection`), so
  the native window lands exactly over the captured area.
- MCP `x/y/width/height` are physical desktop pixels.

On a 1920×1200 display at 125% scaling, egui works in 1536×960 points while
xcap captures 1920×1200 pixels. A 500-point overlay selection therefore becomes
a 625-pixel image crop. The overlay draws the image scaled to fit, so visual
selection and output crop match.

## 5. Capture flow

1. **Trigger** — global hotkey or MCP `capture_*`.
2. **Park** — save root position, move root off-screen, request repaint.
3. **Wait** — ~140 ms for DWM to apply the move.
4. **Grab** — `xcap::Monitor::capture_image()` (or `capture_region`).
5. **Restore** — move root back, focus it.
6. **Overlay** — create a `CaptureSession` and an immediate viewport covering
   the monitor. The screenshot is uploaded as a texture once.
7. **Interact** — drag selection, resize handles, move, annotate.
8. **Finish** — crop + rasterise annotations, then copy/pin/save/start job.

Overlay colours: dim is `black @ 120 alpha`; the selection is revealed by
clipping the screenshot texture to the selection rect; annotations are painted
above the revealed image.

### Capture fallbacks

xcap's Windows backend is Windows.Graphics.Capture, which can return an
all-black frame where that pipeline is unavailable (observed on some remote
sessions and driver states). `capture.rs` therefore samples every capture; a
uniformly black one is retried with a GDI `BitBlt` of the virtual screen and
the frame with content wins. A genuinely black screen is unaffected — the
fallback returns the same black image and the original is kept. The GDI path
cannot see secure desktops (UAC prompts, the lock screen), which is why it is
a fallback and not the primary.

## 6. Annotation model

`annotate::Annotation` is an enum of image-space shapes:

- `Rect`, `Ellipse`, `Arrow`, `Pen`, `Highlight`, `Mosaic`.

The same model is used twice:

- `preview()` paints with egui for immediate feedback.
- `rasterize()` paints with tiny-skia into a `Pixmap` at source resolution.

Mosaics are applied after vector shapes on the cropped base image, block by
block (mean RGB over a 12-pixel cell). Vector annotations are replayed after
mosaics so a rectangle drawn inside a redacted area remains visible.

Annotations are translated when the selection is moved, so the composition
sticks together. Resizing does not scale annotations in v0.1.0.

## 7. Pins (Win32 windows on Windows, egui viewports elsewhere)

Windows pins each run their own thread:

```
thread ── RegisterClassW("PinSightNativePin")
       ── CreateWindowExW(WS_POPUP | WS_VISIBLE,
                          WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                          lpParam = PinCtx)
       ── SetLayeredWindowAttributes(alpha = 255)
       ── GetMessageW / DispatchMessageW
```

`PinCtx` owns the BGRA buffer, zoom/opacity, pin id and an mpsc sender used to
notify the UI when the window is destroyed. The pointer is stored in
`GWLP_USERDATA` and freed on `WM_DESTROY`.

Window procedure behaviour:

| Message | Behaviour |
| --- | --- |
| `WM_PAINT` | `StretchDIBits` the BGRA buffer into the client rect; 1px border |
| `WM_LBUTTONDOWN` | `ReleaseCapture` + `WM_NCLBUTTONDOWN`/`HTCAPTION` → OS drag |
| `WM_MOUSEWHEEL` | zoom (0.2–4.0); with Ctrl, adjust layered alpha (0.15–1.0) |
| `WM_MBUTTONDOWN`, `WM_KEYDOWN(VK_ESCAPE)` | close |
| `WM_APP+1` | toggle `WS_EX_TRANSPARENT` (click-through) |
| `WM_CLOSE` | `DestroyWindow` |
| `WM_DESTROY` | notify UI, `PostQuitMessage` |

The main window lists pins and provides Copy, Save, Focus, Click-through and
Close. Once click-through is enabled the pin no longer receives mouse input, so
the main window is the only way to turn it off.

Why not use an egui viewport everywhere? Immediate viewports are designed for
short-lived child windows. Long-lived borderless windows with textures were
unreliable across deferred/immediate rendering paths in eframe 0.37, and
click-through / per-window alpha are simpler with Win32. The tradeoff is
Windows-only native pins: `pin_viewport.rs` provides the fallback used on
macOS and Linux — same drag / zoom / opacity / close semantics, drawn by egui,
minus OS-level click-through.

## 8. Long screenshots

`longshot.rs` runs a worker:

1. Locate the monitor by id; clamp the region.
2. Wait for the overlay to disappear.
3. Capture the initial region.
4. Move the cursor to the region centre and `enigo.scroll(4, Vertical)`.
5. Wait ~420 ms for smooth scrolling.
6. Capture the same region.
7. Find the overlap between the bottom of the stitched image and the top of the
   new frame.
8. Append rows below the overlap; repeat until `max_height`, 80 frames, or no
   movement.

Overlap search:

```
for overlap in [h*0.9 .. h/8]:
    diff = mean(|prev[y + h - overlap] - next[y]|)
    choose smallest diff
accept if diff < 22
```

Sampling uses strides (`x_step = w/64`, `row_step = h/64`) to keep the search
fast on 4K captures. Exact matches break early.

Failure modes: fixed headers/sidebars make the overlap ambiguous; video,
animations and sticky elements change pixels between frames; infinite scrollers
never stop. Cancellation sets an atomic flag and reports `Failed("cancelled")`.

## 9. Recording

`record.rs` chooses a sink at start:

- **MP4** if `ffmpeg` is on `PATH`: `ffmpeg -f rawvideo -pixel_format rgba
  -video_size WxH -framerate FPS -i - ... -pix_fmt yuv420p`.
- **GIF** otherwise: `gif::Encoder`, per-frame palette, 10 ms delay units.

Frames are captured with `xcap` at the configured FPS. Capture and encoding
happen on the worker; the UI only displays counters and a stop button. Frames
are written incrementally so memory stays bounded. Recording auto-stops after
five minutes.

## 10. MCP server

`mcp.rs` implements a minimal MCP server by hand:

- newline-delimited JSON-RPC 2.0 on stdin/stdout;
- `initialize`, `ping`, `tools/list`, `tools/call`;
- empty `resources/list` and `prompts/list`;
- tool errors are returned as `isError: true` content, not JSON-RPC errors;
- logs go to stderr only.

Capture/recording tools needing the UI are forwarded via `UiCommand` and block
on a bounded reply channel. `long_screenshot` runs directly on the MCP thread
because it only needs `xcap`/`enigo`.

The protocol version is echoed from the client's `initialize` request, which
keeps compatibility with clients expecting different revisions.

## 11. Error handling and logging

- `anyhow` for application-level errors, strings at thread/channel boundaries.
- Failed hotkey registrations are kept as `(Action, BindingError)` pairs, shown
  as a red dot plus a localised reason on the settings page and summarised in
  the status bar. `AlreadyRegistered` is reported as "already used by another
  app" instead of leaking the backend's `Debug` output.
- Capture failures restore the root window and surface a toast.
- Worker failures are returned through the job channel and displayed, never
  panicked.
- `RUST_LOG=pinsight=debug` enables diagnostics. `tracing-subscriber` also
  installs a `log` bridge, so `eframe`/`winit`/`glow` records appear in the same
  stream. MCP mode must never print to stdout.

## 12. Extension points

- **New annotation** — add to `Annotation`, `preview`, `rasterize`, and the
  toolbar in `overlay.rs`.
- **New capture backend** — implement the helpers in `capture.rs`; the rest of
  the app only sees `RgbaImage` + physical `PhysRect`.
- **New MCP tool** — add a schema entry and a `call_tool` arm; forward through
  `UiCommand` if it touches windows.
- **New output format** — extend `record::Sink` or add a post-processing step.
- **New language** — add a variant to `i18n::Lang`, a column to the `strings!`
  table, a branch in `Lang::detect`/`from_locale`, and a font candidate in
  `fonts::language_font`. A missing column is a compile error.

## 13. Presentation layer

The visual layer is four small modules with a one-way dependency chain:

```
i18n.rs  ──┐
theme.rs ──┼──▶ ui.rs ──▶ app.rs / overlay.rs
fonts.rs ──┘
```

- **`theme.rs`** owns the palette. `Palette::for_theme(mode, accent)` derives
  every surface colour (rail, card, stroke, text, accent, semantic) from a
  `(dark, accent)` pair, so widgets never hard-code a colour. `theme::apply`
  pushes the palette into `egui::Visuals` *and* writes the text-style scale, so
  `ComboBox`, `Slider` and `TextEdit` follow the same design as the hand-painted
  widgets. Both the light and dark style are configured up front, which is what
  makes "follow the system" work without a restart.
- **`fonts.rs`** scans the platform font directories once (`OnceLock`), then
  builds a `FontDefinitions` whose `Proportional` family starts with a
  language-appropriate primary font and continues with a CJK fallback before the
  built-in emoji/icon fonts. Everything downstream keeps using
  `FontId::proportional`, so no call site needs to know which file was chosen.
  The resolved pair is reported back to the settings page.
- **`i18n.rs`** expands the `strings!` table into a `struct Strings` of
  `&'static str` fields plus one `static` per language. Lookups are free of
  allocation, and a missing translation fails the build. `Lang::detect` reads
  `GetUserDefaultUILanguage` and maps the primary/sub language ids.
- **`ui.rs`** paints tiles, cards, pills, segmented controls, swatches and the
  24×24 vector icon set directly with `egui::Painter`. Because nothing is
  rasterised ahead of time, the whole interface stays crisp under any DPI scale,
  accent colour or text scale. Layout helpers that need exact geometry
  (`action_tile`, the pin cards in `app.rs`) allocate a rectangle and paint into
  it rather than composing nested `egui` layouts, which keeps widths
  deterministic across languages.

The overlay is the one place that ignores the user's theme: `Palette::overlay`
always returns the dark glass palette, because the frozen screenshot behind it
can be any colour.

## 14. Tray icon (Windows)

`tray.rs` is Win32 glue around three pieces:

```
UI thread
 ├── Shell_NotifyIconW(NIM_ADD)          icon + tip + callback message
 ├── message-only window (HWND_MESSAGE)  class "PinSightTray", wndproc on the UI thread
 │     WM_TRAY_CALLBACK ──▶ show_menu()  CreatePopupMenu + TrackPopupMenu
 │     WM_COMMAND        ──▶ run_command(id) ──▶ mpsc ──▶ poll_tray()
 └── poll_tray() every frame             drains TrayCommand like a hotkey
```

Design notes:

- **No extra thread.** The message-only window is created on the UI thread, so
  winit's pump dispatches its messages. `TrayState` (channel sender, menu rows,
  icon, hwnd) lives in a `Mutex<Option<…>>` because a `wndproc` is a plain
  `extern "system"` fn and cannot capture app state.
- **Menu rows are built once** from the current language and the current
  hotkeys, so each row shows its shortcut (`区域截图\tCtrl+Shift+A`). The
  separator is a row with `None` as its command.
- **Explorer restarts** are handled by re-arming the icon when the shell
  broadcasts `TaskbarCreated` (registered via `RegisterWindowMessageW`).
- **"Quit" vs "close to tray"**: closing the window hides it when
  `close_to_tray` is set (hotkeys must stay live), so the tray's Quit item sets
  a `quitting` flag *before* asking the viewport to close; `handle_close_request`
  lets that close through. Without the flag the two paths are
  indistinguishable and Quit would only hide the window.
- Everything is best effort: if the shell refuses the icon, `Tray::install`
  returns `None`, the app logs a warning and continues with window + hotkeys.

## 15. Custom background image

`background.rs` turns a user-picked image into a window backdrop:

1. `rfd` picks the file (native dialog; no WebView).
2. The image is decoded once, blurred (`image::imageops::blur`, radius =
   `bg_blur`) and darkened (`bg_dim`) in `image`-space, then uploaded as an
   egui texture sized to the window.
3. `bg_fit` (`cover` / `contain` / `stretch`) picks the destination rectangle;
   the texture is regenerated only when the size or the parameters change.
4. The path and parameters live in `config.json` (`bg_image`, `bg_blur`,
   `bg_dim`, `bg_fit`) and are restored on the next launch.

The image is decoded at most once per session — the blurred/darkened result is
cached, so dragging the window does not re-run the filter chain.

## 16. Config locations and portable mode

`config::Paths::resolve()` decides where `config.json` and user assets live:

| Trigger | Config | Data | Notes |
| --- | --- | --- | --- |
| `--portable` / `-p`, `PINSIGHT_PORTABLE=1`, or a `pinsight.portable` file next to the exe | `<exe>\data\config.json` | `<exe>\data` | captures default to `<exe>\captures`; nothing outside the folder is touched |
| otherwise | `%APPDATA%\PinSight\config.json` (platform equivalent) | same directory | standard per-user install |

The marker file ships inside the portable zip (`scripts/package.ps1` writes it),
which is what makes a copied folder behave portably without command-line flags.

## 17. Hotkey rebinding

`hotkeys.rs` keeps one `GlobalHotKeyManager` for the process lifetime.
Rebinding drains the live bindings (`unregister`), clears the failure list and
registers the new set on the **same** manager. Building a second manager fails
with `AlreadyRegistered` against the bindings the first one still owns, which
used to make most rows report a phantom conflict. The settings page records a
new combination in place (click `Record`, press keys, `Esc` cancels) and calls
`Hotkeys::rebind`, so changes apply without a restart.

## 18. Run at startup

`startup.rs` maps the `run_at_startup` flag onto the platform autostart slot:

- Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the installer
  can also create this via its `startup` task).
- Linux: `~/.config/autostart/pinsight.desktop`.
- macOS: a LaunchAgent plist in `~/Library/LaunchAgents`.

Every backend is best effort and reports success back to the settings page; a
read-only registry or missing autostart directory leaves the checkbox where
the user put it and logs why.

## 19. Single instance

`single_instance.rs` keeps one GUI per session, because a second process
registers a second set of global hotkeys and reports phantom conflicts:

| Platform | Mechanism | Release |
| --- | --- | --- |
| Windows | named mutex `Local\PinSight-SingleInstance` | `ReleaseMutex` + `CloseHandle` in `Drop`; the kernel also frees it on crash |
| macOS / Linux | lock file `pinsight.lock` next to the config, created with `create_new` | removed in `Drop` only if the recorded PID is still ours |

A stale lock file cannot block startup forever: the recorded PID is probed for
liveness (`kill(pid, 0)` on Unix) before the lock is believed.

A second launch hands over and exits. Raising the running instance is
Windows-only — `FindWindowW` on the main viewport's title, then the
`AttachThreadInput` + `SetForegroundWindow` dance that the OS requires before
one process may focus another's window. On macOS/Linux the second launch exits
quietly: the duplicate hotkey set is what actually hurts, and that is
prevented everywhere.

`--mcp` never takes the slot, so the MCP server can run beside the GUI.

## 20. Annotation history and numbered markers

- **`History`** (in `annotate.rs`) is snapshot-based: every mutating action
  clones the annotation list before touching it, capped at 64 entries.
  Snapshots rather than inverse operations because the overlay's mutations are
  append, delete-last and "translate everything" — one uniform mechanism keeps
  all three correct. The overlay wires `Ctrl+Z` / `Ctrl+Y` (and
  `Ctrl+Shift+Z`) to it, and the toolbar buttons reflect `can_undo`/`can_redo`.
- **Numbered markers** render their digits as seven-segment vector strokes
  (`number_segments`), shared by the egui preview and the tiny-skia
  rasteriser, so the two paths cannot drift apart and no font has to be
  shipped to the rasteriser. The index is assigned at placement time from the
  markers already drawn, and the marker answers a plain click — a press
  without movement never becomes an egui drag.

---

If you change any of the invariants above, update this document in the same PR.

## 21. Text annotation rasterisation

egui draws the preview with its own text stack; `tiny-skia` has no text
support, so a committed text annotation is rasterised by `text_font.rs`:

1. `fonts::apply` records the face that ended up primary (`PRIMARY_PATH`).
2. `text_font::font()` parses that same file with `ab_glyph` (falling back to
   any Latin face) and caches it behind an `RwLock`, so a font change can swap
   it without restarting.
3. Each glyph's outline curves are mapped from font units (y-up) into pixmap
   pixels (y-down) and filled as a `tiny-skia` path.

The subtle part is contours: a glyph is a flat list of curves where each
contour's last curve returns to its own start. `move_to` may only fire where
the pen jumps — starting a sub-path per curve leaves every segment open and
`fill_path` then paints almost nothing (which is exactly how the first
implementation failed).

## 22. Headless CLI

`cli.rs` handles `pinsight shot …` before eframe is touched: no window, no GL
context, no single-instance slot. It reuses `capture::grab_monitor` /
`grab_physical_rect` (so the GDI black-frame fallback applies) and
`util::save_png_to`. Output contract matches `maim`: one absolute path on
stdout, errors on stderr with a non-zero exit code.

## 23. Pin context menu

The pin window proc answers `WM_RBUTTONUP` with a `TrackPopupMenu` built from
the strings stored in `PinCtx` (so the menu follows the UI language) plus the
live pixel size as a greyed row. Commands split by ownership:

- copy / save-as go back to the UI thread (`PinEvent::Copy` / `SaveAs`) —
  clipboard and file dialogs belong to the app, not a pin thread;
- zoom, always-on-top, click-through, close and destroy are applied in place.

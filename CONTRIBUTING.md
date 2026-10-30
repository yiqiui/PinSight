# Contributing to PinSight

Thanks for taking the time to improve PinSight. This project is a native Rust
desktop tool, so small changes can affect windowing, capture and input in ways
that are hard to test automatically. Please include enough environment detail
in issues and pull requests.

## Before you start

- Target: Windows 10/11, Rust 1.85+ (edition 2024).
- Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing capture,
  overlay or pin code.
- For large features, open an issue first so the approach can be discussed.

## Development checks

```powershell
cargo fmt --all --check
cargo clippy --all-targets
cargo test
cargo build
```

The CI workflow runs the same commands on `windows-latest`.

## Manual test checklist

For changes touching capture or overlay:

1. `Ctrl+Shift+A`, select a region, annotate with every tool, press `Enter`.
2. Confirm the clipboard image matches the annotated selection.
3. Press `P` and verify the pin appears at the selection position.
4. Drag the pin, wheel-zoom it, `Ctrl+wheel` its opacity, close it with the
   middle mouse button.
5. Repeat on a secondary monitor if available.

For recording changes:

1. `Ctrl+Shift+R`, select a region, press `Enter`.
2. Wait a few seconds, press `Ctrl+Shift+R` again.
3. Open the generated GIF/MP4 and verify it plays and has the expected size.

For MCP changes:

```powershell
# See README for a full JSONL example
.\target\debug\pinsight.exe --mcp
```

At minimum verify `initialize`, `tools/list`, `get_status` and one capture tool.

## Code style

- Prefer small modules with explicit data flow; avoid global mutable state.
- Keep all blocking work off the UI thread.
- Document coordinate spaces (`physical`, `logical/points`, `image pixels`) in
  comments when a function crosses them.
- Do not print to stdout in MCP mode; stdout is reserved for JSON-RPC.
- Avoid adding a WebView or an async runtime without prior discussion.

## Commit messages

Use a short imperative subject, for example:

```text
capture: keep press origin when drag threshold is crossed
pin: replace egui viewport with native Win32 window
mcp: add capture_region tool
```

## Reporting bugs

Please include:

- PinSight version / commit
- Windows version
- Display layout and scaling (e.g. 1920×1200 @125%, second 2560×1440 @100%)
- Steps to reproduce, expected result and actual result
- Relevant `RUST_LOG=pinsight=debug` output (stderr)

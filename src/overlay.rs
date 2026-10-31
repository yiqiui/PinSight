//! The frozen-screen capture overlay: selection, annotation and toolbar.

#![allow(deprecated)] // egui 0.37 deprecates `Id::new`, but it remains the most direct API here.

use eframe::egui;
use image::RgbaImage;

use crate::annotate::{self, ACCENT, Annotation, History, ImgRect, Tool};
use crate::capture::MonitorInfo;
use crate::i18n::Strings;
use crate::theme::Palette;
use crate::ui::{self, Icon};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Copy,
    LongShot,
    Record,
    /// Sample one pixel and hand the colour back; used by the colour dialog's
    /// "pick from screen" button.
    PickColor,
}

impl Intent {
    pub fn default_action(self) -> OverlayAction {
        match self {
            Intent::Copy | Intent::PickColor => OverlayAction::Copy,
            Intent::LongShot | Intent::Record => OverlayAction::Start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayAction {
    Copy,
    Pin,
    Save,
    Upload,
    Start,
    /// Long-shot in manual mode: no auto-scroll, the user rolls the wheel.
    StartManual,
    Cancel,
}

pub enum OverlayOutcome {
    Image {
        action: OverlayAction,
        image: RgbaImage,
    },
    StartLongShot {
        monitor_id: u32,
        region: ImgRect,
        /// `true` = manual mode: PinSight does not scroll, the user rolls the
        /// wheel and decides how long the capture is.
        manual: bool,
    },
    StartRecording {
        monitor_id: u32,
        region: ImgRect,
    },
    Cancel,
    /// A single sampled colour (the screen picker).
    Color([u8; 3]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    N,
    S,
    W,
    E,
    Nw,
    Ne,
    Sw,
    Se,
}

#[derive(Debug, Clone, Copy)]
enum DragMode {
    NewSelection {
        anchor: egui::Pos2,
    },
    Move {
        original: ImgRect,
        anchor: egui::Pos2,
        last: egui::Pos2,
    },
    Resize {
        handle: Handle,
        original: ImgRect,
    },
    Draw {
        start: egui::Pos2,
    },
}

/// A text annotation being typed: anchor point plus the live buffer.
#[derive(Debug, Clone)]
struct TextDraft {
    at: egui::Pos2,
    buffer: String,
}

/// Overlay behaviour that comes from the settings page.
#[derive(Debug, Clone)]
pub struct OverlayPrefs {
    pub border_width: f32,
    pub mask_alpha: u8,
    /// RGB of the dimming veil.
    pub mask_color: [u8; 3],
    pub anchors: bool,
    /// RGB of the resize-handle outline.
    pub anchor_color: [u8; 3],
    pub crosshair: bool,
    pub hints: bool,
    /// Ask before Escape throws the capture away.
    pub esc_confirm: bool,
    /// What a double-click inside the overlay does.
    pub double_click: crate::config::ClickAction,
    /// What a middle-click inside the overlay does.
    pub middle_click: crate::config::ClickAction,
    pub keys: crate::config::AppKeys,
    /// Zoomed pixel preview beside the cursor.
    pub magnifier: bool,
    /// Source pixels covered by the magnifier, per axis.
    pub magnifier_size: u32,
    /// How the sampled colour is printed.
    pub color_format: crate::config::ColorFormat,
}

impl Default for OverlayPrefs {
    fn default() -> Self {
        Self {
            border_width: 1.5,
            mask_alpha: 120,
            mask_color: [0, 0, 0],
            anchors: true,
            anchor_color: [255, 255, 255],
            crosshair: true,
            hints: true,
            esc_confirm: false,
            double_click: crate::config::ClickAction::Copy,
            middle_click: crate::config::ClickAction::Pin,
            keys: crate::config::AppKeys::default(),
            magnifier: true,
            magnifier_size: 15,
            color_format: crate::config::ColorFormat::Hex,
        }
    }
}

pub struct CaptureSession {
    pub monitor: MonitorInfo,
    /// Appearance and shortcuts from the settings page.
    pub prefs: OverlayPrefs,
    pub image: RgbaImage,
    pub monitor_pos: egui::Pos2,
    pub monitor_size: egui::Vec2,
    pub texture: Option<egui::TextureHandle>,
    pub selection: Option<ImgRect>,
    pub tool: Tool,
    pub color: [u8; 4],
    pub width: f32,
    pub annotations: Vec<Annotation>,
    /// Snapshot history behind [`CaptureSession::undo`] / [`CaptureSession::redo`].
    history: History,
    /// In-flight text annotation: the click anchor plus the buffer being typed.
    editing: Option<TextDraft>,
    /// Measured size of the toolbar from the previous frame, so the panel can be
    /// kept inside the screen when it is docked near an edge.
    toolbar_size: Option<egui::Vec2>,
    active: Option<Annotation>,
    drag: Option<DragMode>,
    press_origin: Option<egui::Pos2>,
    pub intent: Intent,
    focused: bool,
    /// Escape was pressed and the "discard this capture?" prompt is showing.
    pending_cancel: bool,
    /// Cached magnifier texture plus the pixel it was built for.
    magnifier: Option<(i32, i32, u32, egui::TextureHandle)>,
    /// Live OCR job: a receiver for the result of the recognition running off
    /// the GUI thread, plus where that result should be painted back.
    ocr_rx: Option<std::sync::mpsc::Receiver<Result<Vec<crate::ocr::OcrWord>, String>>>,
    /// Last OCR result (or error) for the region below.
    ocr_words: Option<Result<Vec<crate::ocr::OcrWord>, String>>,
    /// Region (image pixels) the pending/finished OCR ran on.
    ocr_rect: Option<ImgRect>,
    /// Whether the OCR results panel is open.
    ocr_open: bool,
    tr: &'static Strings,
    palette: Palette,
}

impl CaptureSession {
    pub fn new(
        monitor: MonitorInfo,
        image: RgbaImage,
        intent: Intent,
        tr: &'static Strings,
        palette: Palette,
    ) -> Self {
        let logical = monitor.logical_rect();
        Self {
            monitor,
            image,
            monitor_pos: logical.min,
            monitor_size: logical.size(),
            texture: None,
            selection: None,
            tool: Tool::Select,
            color: ACCENT,
            width: 3.0,
            prefs: OverlayPrefs::default(),
            annotations: Vec::new(),
            history: History::default(),
            editing: None,
            toolbar_size: None,
            active: None,
            drag: None,
            press_origin: None,
            intent,
            focused: false,
            pending_cancel: false,
            magnifier: None,
            ocr_rx: None,
            ocr_words: None,
            ocr_rect: None,
            ocr_open: false,
            tr,
            palette,
        }
    }

    pub fn viewport_builder(&self) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title(self.tr.ov_title)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size(self.monitor_size)
            .with_position(self.monitor_pos)
            .with_active(true)
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [self.image.width() as usize, self.image.height() as usize],
                self.image.as_raw(),
            );
            self.texture = Some(ctx.load_texture(
                format!("pinsight-capture-{}", self.monitor.id),
                color,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<OverlayOutcome> {
        let ctx = ui.ctx().clone();
        if ui.input(|i| i.viewport().close_requested()) {
            return Some(OverlayOutcome::Cancel);
        }
        if !self.focused {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focused = true;
        }

        self.ensure_texture(&ctx);
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, egui::Color32::BLACK);

        let texture_id = self.texture.as_ref().expect("texture created").id();
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter()
            .image(texture_id, screen, uv, egui::Color32::WHITE);

        let (image_w, image_h) = (self.image.width() as f32, self.image.height() as f32);
        let scale = if image_w > 0.0 {
            screen.width() / image_w
        } else {
            1.0
        };
        let to_img = move |p: egui::Pos2| -> egui::Pos2 {
            egui::pos2(
                ((p.x - screen.min.x) / scale).clamp(0.0, image_w),
                ((p.y - screen.min.y) / scale).clamp(0.0, image_h),
            )
        };

        if self.intent == Intent::PickColor {
            return self.show_color_pick(ui, screen, scale);
        }

        // Captured before handling: committing on Enter clears `editing`, but
        // that same Enter must not also confirm the whole capture.
        let editing_text = self.editing.is_some();
        self.handle_keyboard(ui);
        if !editing_text {
            if let Some(action) = self.keyboard_action(ui) {
                return Some(self.finish(action));
            }
        }

        // Collect any finished OCR job before we paint this frame.
        self.poll_ocr();

        // Top-level paste: Ctrl/Cmd+V (or the OS paste event) drops the
        // clipboard text onto the screenshot as a new text annotation, so OCR
        // output or any copied text can be pasted straight onto the image. The
        // inline text editor handles its own paste when focused, so we skip
        // while `editing` is active to avoid fighting egui's TextEdit.
        if self.editing.is_none() {
            let mut paste_text: Option<String> = None;
            ui.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Paste(text) = ev {
                        let t = text.trim().to_string();
                        if !t.is_empty() {
                            paste_text = Some(t);
                            break;
                        }
                    }
                }
            });
            // Fallback for integrations that don't emit `Event::Paste` when no
            // text widget is focused: read the clipboard directly on the
            // Ctrl/Cmd+V chord. `Event::Paste` already set above wins, so this
            // never double-pastes.
            if paste_text.is_none()
                && ui.input(|i| {
                    i.key_pressed(egui::Key::V) && (i.modifiers.ctrl || i.modifiers.command)
                })
            {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(t) = cb.get_text() {
                        let t = t.trim().to_string();
                        if !t.is_empty() {
                            paste_text = Some(t);
                        }
                    }
                }
            }
            if let Some(text) = paste_text {
                let hover = ui
                    .input(|i| i.pointer.hover_pos())
                    .filter(|p| screen.contains(*p))
                    .unwrap_or_else(|| screen.center());
                let at = to_img(hover);
                self.history.record(&self.annotations);
                self.annotations.push(Annotation::Text {
                    at,
                    text,
                    color: self.color,
                    size: self.text_size(),
                });
            }
        }

        // Dim everything, then reveal the selection by clipping the screenshot.
        let [mr, mg, mb] = self.prefs.mask_color;
        ui.painter().rect_filled(
            screen,
            0.0,
            egui::Color32::from_rgba_unmultiplied(mr, mg, mb, self.prefs.mask_alpha),
        );
        // Crosshair: two full-width lines through the cursor, Snipaste style.
        if self.prefs.crosshair {
            if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
                let stroke = egui::Stroke::new(1.0, self.palette.accent.gamma_multiply(0.55));
                ui.painter().line_segment(
                    [
                        egui::pos2(screen.left(), cursor.y),
                        egui::pos2(screen.right(), cursor.y),
                    ],
                    stroke,
                );
                ui.painter().line_segment(
                    [
                        egui::pos2(cursor.x, screen.top()),
                        egui::pos2(cursor.x, screen.bottom()),
                    ],
                    stroke,
                );
            }
        }
        if let Some(sel) = self.selection {
            let sel_screen = image_to_screen(sel, screen, scale);
            ui.painter().with_clip_rect(sel_screen).image(
                texture_id,
                screen,
                uv,
                egui::Color32::WHITE,
            );
            ui.painter().rect_stroke(
                sel_screen,
                0.0,
                egui::Stroke::new(self.prefs.border_width, self.palette.accent),
                egui::StrokeKind::Outside,
            );
            if self.prefs.anchors {
                self.paint_handles(ui, sel_screen);
            }
            paint_size_label(ui, &self.palette, sel, sel_screen);
        }

        // Annotations.
        for annotation in self.annotations.iter().chain(self.active.iter()) {
            annotate::preview(ui.painter(), annotation, screen, scale, &self.image);
        }

        self.show_magnifier(ui, screen, scale);

        // Inline text editor, floating over the overlay at the click anchor.
        if self.editing.is_some() {
            self.show_text_editor(ui, screen, scale);
        }

        // Mouse interaction.
        let response = ui.interact(
            screen,
            egui::Id::new(("pinsight-overlay", self.monitor.id)),
            egui::Sense::click_and_drag(),
        );
        let pointer = response
            .interact_pointer_pos()
            .or_else(|| ui.input(|i| i.pointer.hover_pos()));

        if let Some(pos) = pointer {
            if ui.input(|i| i.pointer.any_pressed()) {
                self.press_origin = Some(to_img(pos));
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            // Keep a press origin only for the duration of a gesture.
            if !response.dragged() {
                self.press_origin = None;
            }
        }

        if response.drag_started() {
            if let Some(pos) = pointer {
                let start = self.press_origin.take().unwrap_or_else(|| to_img(pos));
                self.begin_drag(start);
            }
        } else if response.dragged() {
            if let Some(pos) = pointer {
                let img = to_img(pos);
                self.update_drag(img, ui);
            }
        } else if response.drag_stopped() {
            self.finish_drag(ui);
            self.press_origin = None;
        }

        // Double-click and middle-click actions from the settings. Only fire
        // with a live selection, so a stray click on the desktop does nothing.
        if self.selection.is_some() && !self.pending_cancel {
            if response.double_clicked() {
                if let Some(action) = click_action(self.prefs.double_click) {
                    return Some(self.finish(action));
                }
            }
            if response.clicked_by(egui::PointerButton::Middle) {
                if let Some(action) = click_action(self.prefs.middle_click) {
                    return Some(self.finish(action));
                }
            }
        }

        // The numbered tool answers a plain click: a press without movement
        // never becomes a drag, so the marker is placed here instead.
        if response.clicked() && self.tool == Tool::Numbered {
            if let Some(pos) = pointer {
                let img = to_img(pos);
                let annotation = self.make_annotation(img, img);
                if annotation_is_visible(&annotation) {
                    self.history.record(&self.annotations);
                    self.annotations.push(annotation);
                }
            }
        }
        // The text tool opens an inline editor at the click point; Enter
        // commits, Escape cancels.
        if response.clicked() && self.tool == Tool::Text {
            if let Some(pos) = pointer {
                self.editing = Some(TextDraft {
                    at: to_img(pos),
                    buffer: String::new(),
                });
            }
        }

        if let Some(pos) = pointer {
            let img = to_img(pos);
            self.set_cursor(ui, img);
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::Default);
        }

        // Toolbar / confirmation.
        let toolbar_outcome = self.show_toolbar(ui, screen, scale);
        if let Some(action) = toolbar_outcome {
            return Some(self.finish(action));
        }

        // Floating OCR results panel (if open).
        self.show_ocr_panel(ui, screen, scale);

        // Hint line at the top center while no selection exists.
        if self.selection.is_none() && self.prefs.hints {
            let text = match self.intent {
                Intent::LongShot => self.tr.ov_hint_long,
                Intent::Record => self.tr.ov_hint_record,
                _ => self.tr.ov_hint_copy,
            };
            let pos = egui::pos2(screen.center().x, screen.top() + 28.0);
            let galley = ui.painter().layout_no_wrap(
                text.to_owned(),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
            let rect = egui::Rect::from_center_size(pos, galley.size() + egui::vec2(28.0, 14.0));
            ui.painter().rect_filled(
                rect,
                rect.height() * 0.5,
                egui::Color32::from_black_alpha(200),
            );
            ui.painter().galley(
                rect.center() - galley.size() * 0.5,
                galley,
                egui::Color32::WHITE,
            );
        }

        // "Discard this capture?" prompt raised by Escape.
        if self.pending_cancel {
            self.show_cancel_confirm(ui, screen);
        }

        None
    }

    /// Centred yes/no prompt shown when Escape is configured to confirm.
    fn show_cancel_confirm(&mut self, ui: &mut egui::Ui, screen: egui::Rect) {
        let mut discard = false;
        let mut keep = false;
        egui::Area::new(egui::Id::new("pinsight-cancel-confirm"))
            .fixed_pos(screen.center() - egui::vec2(170.0, 46.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(320.0);
                    ui.label(self.tr.ov_cancel_confirm);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(self.tr.ov_discard).clicked() {
                            discard = true;
                        }
                        if ui.button(self.tr.ov_keep).clicked() {
                            keep = true;
                        }
                    });
                });
            });
        if discard {
            self.pending_cancel = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else if keep {
            self.pending_cancel = false;
        }
    }

    /// Font size for text annotations, in source-image pixels. Follows the
    /// toolbar's stroke slider so users already have a size control in hand.
    fn text_size(&self) -> f32 {
        10.0 + self.width * 2.5
    }

    /// Float the inline text editor at the click anchor. A single-line edit is
    /// used so Enter does not insert a newline — the overlay's keyboard handler
    /// commits on Enter instead.
    fn show_text_editor(&mut self, ui: &mut egui::Ui, screen: egui::Rect, scale: f32) {
        let hint = self.tr.ov_text_hint;
        let font = egui::FontId::proportional(self.text_size() * scale);
        let Some(draft) = self.editing.as_mut() else {
            return;
        };
        let pos = screen.min + draft.at.to_vec2() * scale;
        let mut commit = false;
        egui::Area::new(egui::Id::new("pinsight-text-editor"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut draft.buffer)
                        .hint_text(hint)
                        .desired_width(260.0)
                        .font(font),
                );
                response.request_focus();
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = true;
                }
            });
        if commit {
            self.commit_text();
        }
    }

    /// Turn the in-flight text draft into a committed annotation.
    fn commit_text(&mut self) {
        let Some(draft) = self.editing.take() else {
            return;
        };
        if draft.buffer.trim().is_empty() {
            return;
        }
        self.history.record(&self.annotations);
        self.annotations.push(Annotation::Text {
            at: draft.at,
            text: draft.buffer,
            color: self.color,
            size: self.text_size(),
        });
    }

    /// Snapshot the annotations, then undo the last change.
    pub fn undo(&mut self) {
        if let Some(previous) = self.history.undo(&self.annotations) {
            self.annotations = previous;
        }
    }

    /// Re-apply the change undone by [`CaptureSession::undo`].
    pub fn redo(&mut self) {
        if let Some(next) = self.history.redo(&self.annotations) {
            self.annotations = next;
        }
    }

    /// True when the configured binding was pressed this frame.
    fn bound(ui: &egui::Ui, text: &str) -> bool {
        let Some(binding) = crate::app_keys::parse(text) else {
            return false;
        };
        ui.input(|i| i.key_pressed(binding.key) && binding.matches(binding.key, &i.modifiers))
    }

    fn handle_keyboard(&mut self, ui: &mut egui::Ui) {
        let pressed = |key: egui::Key| ui.input(|i| i.key_pressed(key));

        // While a text annotation is being typed, every key belongs to the
        // editor except Escape (cancel) and Enter (commit).
        if self.editing.is_some() {
            if pressed(egui::Key::Escape) {
                self.editing = None;
                return;
            }
            if pressed(egui::Key::Enter) {
                self.commit_text();
                return;
            }
            return;
        }

        let keys = self.prefs.keys.clone();
        if Self::bound(ui, &keys.cancel) {
            self.drag = None;
            if self.prefs.esc_confirm && !self.pending_cancel {
                // First Escape asks; a second one (or the button) discards.
                self.pending_cancel = true;
            } else {
                // The caller watches `cancelled` via the normal close path.
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        if self.pending_cancel {
            // While the prompt is up, the rest of the shortcuts are paused.
            return;
        }
        for (binding, tool) in [
            (&keys.tool_select, Tool::Select),
            (&keys.tool_rect, Tool::Rect),
            (&keys.tool_ellipse, Tool::Ellipse),
            (&keys.tool_arrow, Tool::Arrow),
            (&keys.tool_pen, Tool::Pen),
            (&keys.tool_highlight, Tool::Highlight),
            (&keys.tool_mosaic, Tool::Mosaic),
            (&keys.tool_numbered, Tool::Numbered),
            (&keys.tool_text, Tool::Text),
        ] {
            if Self::bound(ui, binding) {
                self.tool = tool;
            }
        }
        if (pressed(egui::Key::Backspace) || pressed(egui::Key::Delete))
            && !self.annotations.is_empty()
        {
            self.history.record(&self.annotations);
            self.annotations.pop();
        }
        // Ctrl+Z undoes; Ctrl+Y redoes. Ctrl+Shift+Z is honoured too, but note
        // that on systems where the input method binds Ctrl+Shift (the classic
        // Chinese/English mode switch) the IME swallows the combination before
        // the window ever sees it — which is why Ctrl+Y is the documented redo.
        if Self::bound(ui, &keys.undo) {
            self.undo();
        }
        if Self::bound(ui, &keys.redo) {
            self.redo();
        }
        // Ctrl+Shift+Z as an extra redo path: it is muscle memory, even though
        // some input methods claim the combination first.
        if ui.input(|i| i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z)) {
            self.redo();
        }
    }

    fn keyboard_action(&self, ui: &egui::Ui) -> Option<OverlayAction> {
        let keys = &self.prefs.keys;
        let enter = Self::bound(ui, &keys.confirm);
        let copy = Self::bound(ui, &keys.copy);
        let save = Self::bound(ui, &keys.save);
        let pin = Self::bound(ui, &keys.pin);
        if enter {
            return Some(self.intent.default_action());
        }
        if copy {
            return Some(OverlayAction::Copy);
        }
        if save {
            return Some(OverlayAction::Save);
        }
        if pin && self.intent == Intent::Copy {
            return Some(OverlayAction::Pin);
        }
        None
    }

    fn begin_drag(&mut self, img: egui::Pos2) {
        if let Some(sel) = self.selection {
            if self.tool == Tool::Select {
                if let Some(handle) = hit_handle(sel, img, self.image.width(), 10.0) {
                    self.drag = Some(DragMode::Resize {
                        handle,
                        original: sel,
                    });
                    return;
                }
                if sel.contains(img) {
                    // Moving the selection translates every annotation, so the
                    // whole list is snapshotted once at drag start.
                    self.history.record(&self.annotations);
                    self.drag = Some(DragMode::Move {
                        original: sel,
                        anchor: img,
                        last: img,
                    });
                    return;
                }
            } else if sel.contains(img) {
                self.drag = Some(DragMode::Draw { start: img });
                self.active = Some(self.make_annotation(img, img));
                return;
            }
        }
        self.drag = Some(DragMode::NewSelection { anchor: img });
        self.selection = Some(ImgRect::from_points(img, img));
        self.annotations.clear();
        // A fresh selection invalidates everything drawn before it.
        self.history.clear();
        self.active = None;
    }

    fn update_drag(&mut self, img: egui::Pos2, ui: &egui::Ui) {
        match self.drag {
            Some(DragMode::NewSelection { anchor }) => {
                self.selection = Some(ImgRect::from_points(anchor, img));
            }
            Some(DragMode::Move {
                original,
                anchor,
                last,
            }) => {
                let delta = img - anchor;
                let mut moved = original;
                moved.x = (original.x + delta.x).clamp(0.0, self.image.width() as f32 - original.w);
                moved.y =
                    (original.y + delta.y).clamp(0.0, self.image.height() as f32 - original.h);
                let step = img - last;
                for annotation in &mut self.annotations {
                    annotation.translate(step);
                }
                if let Some(active) = &mut self.active {
                    active.translate(step);
                }
                self.selection = Some(moved);
                self.drag = Some(DragMode::Move {
                    original,
                    anchor,
                    last: img,
                });
            }
            Some(DragMode::Resize { handle, original }) => {
                let mut r = original;
                match handle {
                    Handle::N => {
                        let bottom = r.bottom();
                        r.y = img.y.min(bottom - 4.0);
                        r.h = bottom - r.y;
                    }
                    Handle::S => {
                        r.h = (img.y - r.y).max(4.0);
                    }
                    Handle::W => {
                        let right = r.right();
                        r.x = img.x.min(right - 4.0);
                        r.w = right - r.x;
                    }
                    Handle::E => {
                        r.w = (img.x - r.x).max(4.0);
                    }
                    Handle::Nw => {
                        let bottom = r.bottom();
                        let right = r.right();
                        r.x = img.x.min(right - 4.0);
                        r.y = img.y.min(bottom - 4.0);
                        r.w = right - r.x;
                        r.h = bottom - r.y;
                    }
                    Handle::Ne => {
                        let bottom = r.bottom();
                        r.y = img.y.min(bottom - 4.0);
                        r.h = bottom - r.y;
                        r.w = (img.x - r.x).max(4.0);
                    }
                    Handle::Sw => {
                        let right = r.right();
                        r.x = img.x.min(right - 4.0);
                        r.w = right - r.x;
                        r.h = (img.y - r.y).max(4.0);
                    }
                    Handle::Se => {
                        r.w = (img.x - r.x).max(4.0);
                        r.h = (img.y - r.y).max(4.0);
                    }
                }
                self.selection = Some(r.clamped(self.image.width(), self.image.height()));
            }
            Some(DragMode::Draw { start }) => match self.active.as_mut() {
                Some(Annotation::Pen { points, .. }) => {
                    if let Some(last) = points.last() {
                        if last.distance(img) >= 1.0 {
                            points.push(img);
                        }
                    } else {
                        points.push(img);
                    }
                }
                Some(annotation) => {
                    *annotation = Self::make_annotation_for(annotation, start, img);
                }
                None => {}
            },
            None => {}
        }
        ui.ctx().request_repaint();
    }

    fn finish_drag(&mut self, ui: &mut egui::Ui) {
        match self.drag.take() {
            Some(DragMode::NewSelection { .. }) => {
                if let Some(sel) = self.selection {
                    if sel.w < 4.0 || sel.h < 4.0 {
                        self.selection = None;
                    }
                }
            }
            Some(DragMode::Draw { .. }) => {
                if let Some(annotation) = self.active.take() {
                    if annotation_is_visible(&annotation) {
                        self.history.record(&self.annotations);
                        self.annotations.push(annotation);
                    }
                }
            }
            _ => {}
        }
        ui.ctx().request_repaint();
    }

    fn make_annotation(&self, start: egui::Pos2, end: egui::Pos2) -> Annotation {
        let rect = ImgRect::from_points(start, end);
        match self.tool {
            Tool::Rect | Tool::Select => Annotation::Rect {
                rect,
                color: self.color,
                width: self.width,
            },
            Tool::Ellipse => Annotation::Ellipse {
                rect,
                color: self.color,
                width: self.width,
            },
            Tool::Arrow => Annotation::Arrow {
                from: start,
                to: end,
                color: self.color,
                width: self.width,
            },
            Tool::Line => Annotation::Line {
                from: start,
                to: end,
                color: self.color,
                width: self.width,
            },
            Tool::Pen => Annotation::Pen {
                points: vec![start, end],
                color: self.color,
                width: self.width,
            },
            Tool::Highlight => Annotation::Highlight { rect },
            Tool::Mosaic => Annotation::Mosaic { rect, cell: 12 },
            Tool::Text => Annotation::Text {
                at: egui::pos2(start.x.midpoint(end.x), start.y.midpoint(end.y)),
                text: String::new(),
                color: self.color,
                size: self.text_size(),
            },
            Tool::Numbered => {
                // A click places the marker at the gesture midpoint; the index
                // is one past the markers already drawn, so a sequence of
                // clicks numbers itself 1, 2, 3 …
                let index = self
                    .annotations
                    .iter()
                    .filter(|a| matches!(a, Annotation::Numbered { .. }))
                    .count() as u32
                    + 1;
                Annotation::Numbered {
                    point: egui::pos2(start.x.midpoint(end.x), start.y.midpoint(end.y)),
                    index,
                    color: self.color,
                }
            }
        }
    }

    fn make_annotation_for(
        previous: &Annotation,
        start: egui::Pos2,
        end: egui::Pos2,
    ) -> Annotation {
        match previous {
            Annotation::Arrow {
                from, color, width, ..
            } => Annotation::Arrow {
                from: *from,
                to: end,
                color: *color,
                width: *width,
            },
            Annotation::Line {
                from, color, width, ..
            } => Annotation::Line {
                from: *from,
                to: end,
                color: *color,
                width: *width,
            },
            Annotation::Pen {
                points,
                color,
                width,
            } => Annotation::Pen {
                points: points.clone(),
                color: *color,
                width: *width,
            },
            Annotation::Rect { color, width, .. } => Annotation::Rect {
                rect: ImgRect::from_points(start, end),
                color: *color,
                width: *width,
            },
            Annotation::Ellipse { color, width, .. } => Annotation::Ellipse {
                rect: ImgRect::from_points(start, end),
                color: *color,
                width: *width,
            },
            Annotation::Highlight { .. } => Annotation::Highlight {
                rect: ImgRect::from_points(start, end),
            },
            Annotation::Mosaic { cell, .. } => Annotation::Mosaic {
                rect: ImgRect::from_points(start, end),
                cell: *cell,
            },
            // Text is placed through the inline editor, never through a drag.
            Annotation::Text { .. } => previous.clone(),
            // A numbered marker follows the gesture midpoint; dragging just
            // repositions it before the click is released.
            Annotation::Numbered { index, color, .. } => Annotation::Numbered {
                point: egui::pos2(start.x.midpoint(end.x), start.y.midpoint(end.y)),
                index: *index,
                color: *color,
            },
        }
    }

    fn set_cursor(&self, ui: &egui::Ui, img: egui::Pos2) {
        let icon = if let Some(sel) = self.selection {
            if self.tool == Tool::Select {
                match hit_handle(sel, img, self.image.width(), 10.0) {
                    Some(Handle::Nw) | Some(Handle::Se) => egui::CursorIcon::ResizeNwSe,
                    Some(Handle::Ne) | Some(Handle::Sw) => egui::CursorIcon::ResizeNeSw,
                    Some(Handle::N) | Some(Handle::S) => egui::CursorIcon::ResizeVertical,
                    Some(Handle::W) | Some(Handle::E) => egui::CursorIcon::ResizeHorizontal,
                    None if sel.contains(img) => egui::CursorIcon::Move,
                    None => egui::CursorIcon::Crosshair,
                }
            } else {
                egui::CursorIcon::Crosshair
            }
        } else {
            egui::CursorIcon::Crosshair
        };
        ui.ctx().set_cursor_icon(icon);
    }

    fn paint_handles(&self, ui: &egui::Ui, rect: egui::Rect) {
        let points = [
            rect.left_top(),
            rect.center_top(),
            rect.right_top(),
            rect.left_center(),
            rect.right_center(),
            rect.left_bottom(),
            rect.center_bottom(),
            rect.right_bottom(),
        ];
        let [ar, ag, ab] = self.prefs.anchor_color;
        let anchor_stroke = egui::Color32::from_rgb(ar, ag, ab);
        for point in points {
            let r = egui::Rect::from_center_size(point, egui::vec2(7.5, 7.5));
            ui.painter().rect_filled(r, 1.5, self.palette.card);
            ui.painter().rect_stroke(
                r,
                1.5,
                egui::Stroke::new(1.0, anchor_stroke),
                egui::StrokeKind::Inside,
            );
        }
    }

    fn show_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        screen: egui::Rect,
        scale: f32,
    ) -> Option<OverlayAction> {
        let sel = self.selection.unwrap_or(ImgRect::from_min_size(
            0.0,
            0.0,
            self.image.width() as f32,
            self.image.height() as f32,
        ));
        let sel_screen = image_to_screen(sel, screen, scale);
        let area_size = self.toolbar_size.unwrap_or(egui::vec2(720.0, 46.0));
        let mut pos = if self.selection.is_some() {
            // Dock under the selection, or above it when there is no room.
            let mut pos = egui::pos2(sel_screen.left(), sel_screen.bottom() + 12.0);
            if pos.y + area_size.y > screen.bottom() - 8.0 {
                pos.y = (sel_screen.top() - area_size.y - 12.0).max(screen.top() + 8.0);
            }
            pos
        } else {
            // No selection yet: dock at the bottom centre so the hint on top
            // stays readable.
            egui::pos2(
                screen.center().x - area_size.x * 0.5,
                screen.bottom() - area_size.y - 16.0,
            )
        };
        pos.x = pos.x.clamp(
            screen.left() + 8.0,
            (screen.right() - area_size.x - 8.0).max(screen.left() + 8.0),
        );

        // Follow the session palette: it already reflects the configured theme
        // (a hardcoded dark glass bar ignored the light appearance entirely).
        let p = self.palette;
        let t = self.tr;
        let mut action = None;

        let area = egui::Area::new(egui::Id::new(("pinsight-toolbar", self.monitor.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    // The bar follows the theme: a hardcoded dark glass fill
                    // left dark text on a dark panel once light mode was on.
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0, p.stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(8, 7))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(3.0, 0.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            for tool in Tool::ALL {
                                let selected = self.tool == tool;
                                let label =
                                    format!("{} ({})", tool_label(tool, t), tool.shortcut());
                                if ui::icon_button(ui, &p, tool_icon(tool), selected, &label)
                                    .clicked()
                                {
                                    self.tool = tool;
                                }
                            }

                            vertical_separator(ui, &p);

                            for (color, label) in [
                                ([255u8, 255, 255, 255], t.col_white),
                                ([255, 84, 96, 255], t.col_red),
                                ([255, 190, 60, 255], t.col_amber),
                                ([60, 214, 140, 255], t.col_green),
                                ([92, 156, 255, 255], t.col_blue),
                            ] {
                                if color_swatch(ui, &p, color, self.color == color, label) {
                                    self.color = color;
                                }
                            }

                            vertical_separator(ui, &p);
                            ui.add_sized(
                                egui::vec2(52.0, 18.0),
                                egui::Slider::new(&mut self.width, 1.0..=14.0).show_value(false),
                            )
                            .on_hover_text(t.ov_width);
                            if ui::icon_button_enabled(
                                ui,
                                &p,
                                Icon::Undo,
                                false,
                                t.ov_undo,
                                self.history.can_undo(),
                            )
                            .clicked()
                            {
                                self.undo();
                            }
                            if ui::icon_button_enabled(
                                ui,
                                &p,
                                Icon::Redo,
                                false,
                                t.ov_redo,
                                self.history.can_redo(),
                            )
                            .clicked()
                            {
                                self.redo();
                            }
                            if ui::icon_button(ui, &p, Icon::Trash, false, t.ov_clear).clicked()
                                && !self.annotations.is_empty()
                            {
                                self.history.record(&self.annotations);
                                self.annotations.clear();
                            }

                            // Actions follow the editing controls in the same
                            // row (the selection size is painted on the
                            // selection itself, so no second row is needed).
                            vertical_separator(ui, &p);
                            if self.intent == Intent::LongShot || self.intent == Intent::Record {
                                let label = if self.intent == Intent::LongShot {
                                    t.ov_start_long
                                } else {
                                    t.ov_start_record
                                };
                                let icon = if self.intent == Intent::LongShot {
                                    Icon::Scroll
                                } else {
                                    Icon::Record
                                };
                                if pill_button(ui, &p, label, Some(icon), true).clicked() {
                                    action = Some(OverlayAction::Start);
                                }
                                // Long-shot also offers a manual mode: PinSight
                                // does not scroll, the user rolls the wheel and
                                // decides how far to go. This is the reliable
                                // fallback when auto-scroll will not move a
                                // given window.
                                if self.intent == Intent::LongShot
                                    && pill_button(ui, &p, t.ov_start_long_manual, None, false)
                                        .clicked()
                                {
                                    action = Some(OverlayAction::StartManual);
                                }
                            } else {
                                if pill_button(ui, &p, t.ov_ocr, Some(Icon::Ocr), false)
                                    .on_hover_text(t.ov_ocr_unavailable)
                                    .clicked()
                                {
                                    self.start_ocr();
                                }
                                if pill_button(ui, &p, t.ov_save, Some(Icon::Save), false).clicked()
                                {
                                    action = Some(OverlayAction::Save);
                                }
                                if pill_button(ui, &p, t.ov_pin, Some(Icon::Pin), false).clicked() {
                                    action = Some(OverlayAction::Pin);
                                }
                                if pill_button(ui, &p, t.ov_copy, Some(Icon::Copy), true).clicked()
                                {
                                    action = Some(OverlayAction::Copy);
                                }
                                if pill_button(ui, &p, t.ov_upload, None, false).clicked() {
                                    action = Some(OverlayAction::Upload);
                                }
                            }
                            if pill_button(ui, &p, t.ov_cancel, None, false).clicked() {
                                action = Some(OverlayAction::Cancel);
                            }
                        });
                    });
            });
        self.toolbar_size = Some(area.response.rect.size());

        if action == Some(OverlayAction::Cancel) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return Some(OverlayAction::Cancel);
        }
        action
    }

    fn finish(&mut self, action: OverlayAction) -> OverlayOutcome {
        if action == OverlayAction::Cancel {
            return OverlayOutcome::Cancel;
        }
        let crop = self
            .selection
            .unwrap_or_else(|| {
                ImgRect::from_min_size(
                    0.0,
                    0.0,
                    self.image.width() as f32,
                    self.image.height() as f32,
                )
            })
            .clamped(self.image.width(), self.image.height());
        if crop.w < 2.0 || crop.h < 2.0 {
            return OverlayOutcome::Cancel;
        }

        if action == OverlayAction::Start || action == OverlayAction::StartManual {
            return match self.intent {
                Intent::LongShot => OverlayOutcome::StartLongShot {
                    monitor_id: self.monitor.id,
                    region: crop,
                    manual: action == OverlayAction::StartManual,
                },
                _ => OverlayOutcome::StartRecording {
                    monitor_id: self.monitor.id,
                    region: crop,
                },
            };
        }

        let image = annotate::rasterize(&self.image, crop, &self.annotations);
        OverlayOutcome::Image { action, image }
    }

    /// Kick off text recognition on the current selection (or the whole image
    /// when nothing is selected). The work runs on a worker thread so the GUI
    /// never blocks; [`CaptureSession::poll_ocr`] collects the result.
    fn start_ocr(&mut self) {
        // A job is already in flight — ignore the extra click.
        if self.ocr_rx.is_some() {
            return;
        }
        let (x, y, w, h) = match self.selection {
            Some(sel) => {
                let c = sel.clamped(self.image.width(), self.image.height());
                (c.x, c.y, c.w, c.h)
            }
            None => (
                0.0,
                0.0,
                self.image.width() as f32,
                self.image.height() as f32,
            ),
        };
        let x = x.round().max(0.0) as u32;
        let y = y.round().max(0.0) as u32;
        let w = w.round().max(1.0) as u32;
        let h = h.round().max(1.0) as u32;
        let w = w.min(self.image.width().saturating_sub(x));
        let h = h.min(self.image.height().saturating_sub(y));
        if w == 0 || h == 0 {
            return;
        }
        // Crop a fresh, small copy so the worker owns its pixels and the overlay
        // keeps the full image for display.
        let crop = image::imageops::crop_imm(&self.image, x, y, w, h).to_image();
        self.ocr_rect = Some(ImgRect::from_min_size(
            x as f32, y as f32, w as f32, h as f32,
        ));
        self.ocr_words = None;
        self.ocr_open = true;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = crate::ocr::recognize(crop).map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        self.ocr_rx = Some(rx);
    }

    /// Move a finished OCR job's result out of the channel into
    /// [`CaptureSession::ocr_words`], or surface a crash as an error.
    fn poll_ocr(&mut self) {
        let Some(rx) = self.ocr_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.ocr_words = Some(result);
                self.ocr_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ocr_words = Some(Err("OCR thread crashed".to_string()));
                self.ocr_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    /// Floating panel that shows the recognised text (or an error / spinner)
    /// and offers "Copy text". Drawn as an `Area` so it matches the toolbar's
    /// glass styling rather than a heavy OS window.
    fn show_ocr_panel(&mut self, ui: &mut egui::Ui, screen: egui::Rect, scale: f32) {
        if !self.ocr_open {
            return;
        }
        let p = self.palette;
        let t = self.tr;
        let width = 320.0_f32;
        let anchor = match self.ocr_rect {
            Some(r) => {
                let rs = image_to_screen(r, screen, scale);
                egui::pos2(rs.left(), (rs.top() - 12.0).max(screen.top() + 8.0))
            }
            None => screen.center(),
        };
        let pos = egui::pos2(
            anchor.x.clamp(
                screen.left() + 8.0,
                (screen.right() - width - 8.0).max(screen.left() + 8.0),
            ),
            anchor.y.clamp(
                screen.top() + 8.0,
                (screen.bottom() - 220.0).max(screen.top() + 8.0),
            ),
        );

        // Snapshot the result out of `self` so the buttons below can mutate
        // `self` without fighting the borrow held by the `match`.
        let (running, words_opt, err_opt, count) = match &self.ocr_words {
            None => (true, None, None, 0),
            Some(Err(e)) => (false, None, Some(e.clone()), 0),
            Some(Ok(w)) => (false, Some(crate::ocr::words_to_text(w)), None, w.len()),
        };

        egui::Area::new(egui::Id::new(("pinsight-ocr", self.monitor.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0, p.stroke))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(width);
                        ui.set_max_width(width);
                        ui.horizontal(|ui| {
                            ui.heading(t.ov_ocr_title);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button(t.ov_ocr_close).clicked() {
                                        self.ocr_open = false;
                                    }
                                },
                            );
                        });
                        ui.separator();
                        if running {
                            ui.label(t.ov_ocr_running);
                        } else if let Some(e) = err_opt {
                            ui.colored_label(egui::Color32::from_rgb(255, 120, 120), e);
                            ui.separator();
                            if ui.button(t.ov_ocr_close).clicked() {
                                self.ocr_open = false;
                            }
                        } else if let Some(text) = words_opt {
                            if text.is_empty() {
                                ui.label(t.ov_ocr_empty);
                            } else {
                                ui.label(format!("{} {}", count, t.ov_ocr_words));
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .max_height(220.0)
                                    .show(ui, |ui| {
                                        ui.monospace(text.as_str());
                                    });
                                ui.separator();
                                if ui.button(t.ov_ocr_copy).clicked() {
                                    let _ = crate::clipboard::copy_text(&text);
                                }
                            }
                        }
                    });
            });
    }
}

/// Map a configured click action onto the overlay's own action, if any.
fn click_action(action: crate::config::ClickAction) -> Option<OverlayAction> {
    use crate::config::ClickAction;
    match action {
        ClickAction::Copy => Some(OverlayAction::Copy),
        ClickAction::Pin => Some(OverlayAction::Pin),
        ClickAction::Save => Some(OverlayAction::Save),
        ClickAction::Nothing => None,
    }
}

fn tool_label(tool: Tool, t: &Strings) -> &'static str {
    match tool {
        Tool::Select => t.tool_select,
        Tool::Rect => t.tool_rect,
        Tool::Ellipse => t.tool_ellipse,
        Tool::Arrow => t.tool_arrow,
        Tool::Line => t.tool_line,
        Tool::Pen => t.tool_pen,
        Tool::Highlight => t.tool_highlight,
        Tool::Mosaic => t.tool_mosaic,
        Tool::Numbered => t.tool_numbered,
        Tool::Text => t.tool_text,
    }
}

fn tool_icon(tool: Tool) -> Icon {
    match tool {
        Tool::Select => Icon::Cursor,
        Tool::Rect => Icon::Square,
        Tool::Ellipse => Icon::Circle,
        Tool::Arrow => Icon::ArrowUpRight,
        Tool::Line => Icon::Line,
        Tool::Pen => Icon::Pencil,
        Tool::Highlight => Icon::Marker,
        Tool::Mosaic => Icon::Mosaic,
        Tool::Numbered => Icon::Numbered,
        Tool::Text => Icon::Type,
    }
}

fn vertical_separator(ui: &mut egui::Ui, p: &Palette) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.5, p.stroke);
    ui.add_space(4.0);
}

fn color_swatch(
    ui: &mut egui::Ui,
    p: &Palette,
    color: [u8; 4],
    selected: bool,
    label: &str,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label);
    let c = annotate::color32(color);
    ui.painter().rect_filled(rect, 5.0, c);
    if selected {
        // Two rings so the marker stays legible on both white and dark swatches.
        ui.painter().rect_stroke(
            rect.expand(3.5),
            7.0,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(120)),
            egui::StrokeKind::Outside,
        );
        ui.painter().rect_stroke(
            rect.expand(2.0),
            6.0,
            egui::Stroke::new(1.5, egui::Color32::WHITE),
            egui::StrokeKind::Outside,
        );
    } else if response.hovered() {
        ui.painter().rect_stroke(
            rect.expand(1.5),
            6.0,
            egui::Stroke::new(1.0, p.text_weak),
            egui::StrokeKind::Outside,
        );
    }
    response.clicked()
}

fn pill_button(
    ui: &mut egui::Ui,
    p: &Palette,
    label: &str,
    icon: Option<Icon>,
    primary: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE);
    let pad = 15.0;
    let icon_w = if icon.is_some() { 17.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + pad * 2.0 + icon_w, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();

    // On the dark overlay glass, secondary actions read better as a faint
    // translucent wash than as an opaque card.
    let (fill, stroke, text) = if primary {
        (
            if hovered {
                p.accent.gamma_multiply(1.15)
            } else {
                p.accent
            },
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)),
            p.on_accent,
        )
    } else {
        (
            if hovered { p.card_hover } else { p.card },
            egui::Stroke::new(1.0, p.stroke),
            p.text,
        )
    };
    ui.painter().rect_filled(rect, 9.0, fill);
    ui.painter()
        .rect_stroke(rect, 9.0, stroke, egui::StrokeKind::Inside);

    let mut x = rect.left() + pad;
    if let Some(icon) = icon {
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - 7.0),
            egui::Vec2::splat(14.0),
        );
        ui::paint_icon(ui.painter(), icon_rect, icon, text, 2.0);
        x += icon_w;
    }
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, text);
    ui.painter().galley(
        egui::pos2(x, rect.center().y - galley.size().y * 0.5),
        galley,
        text,
    );
    response
}

impl CaptureSession {
    /// The "pick a colour from the screen" mode: frozen desktop, crosshair,
    /// magnifier, one click to sample.
    fn show_color_pick(
        &mut self,
        ui: &mut egui::Ui,
        screen: egui::Rect,
        scale: f32,
    ) -> Option<OverlayOutcome> {
        let (image_w, image_h) = (self.image.width() as f32, self.image.height() as f32);
        let to_img = move |p: egui::Pos2| -> egui::Pos2 {
            egui::pos2(
                ((p.x - screen.min.x) / scale).clamp(0.0, image_w),
                ((p.y - screen.min.y) / scale).clamp(0.0, image_h),
            )
        };

        // A light veil tells the user the screen is frozen without hiding the
        // colours they are about to sample.
        ui.painter().rect_filled(
            screen,
            0.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 48),
        );

        if let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) {
            let stroke = egui::Stroke::new(1.0, self.palette.accent.gamma_multiply(0.8));
            ui.painter().line_segment(
                [
                    egui::pos2(screen.left(), cursor.y),
                    egui::pos2(screen.right(), cursor.y),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(cursor.x, screen.top()),
                    egui::pos2(cursor.x, screen.bottom()),
                ],
                stroke,
            );
        }

        self.show_magnifier(ui, screen, scale);

        // Hint pill, top centre.
        let text = self.tr.ov_hint_pick;
        let galley = ui.painter().layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(12.5),
            self.palette.text,
        );
        let size = galley.size() + egui::vec2(22.0, 10.0);
        let rect =
            egui::Rect::from_center_size(egui::pos2(screen.center().x, screen.top() + 26.0), size);
        ui.painter().rect_filled(rect, 9.0, self.palette.card);
        ui.painter().rect_stroke(
            rect,
            9.0,
            egui::Stroke::new(1.0, self.palette.stroke_strong),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x * 0.5,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            self.palette.text,
        );

        let response = ui.interact(
            screen,
            egui::Id::new(("pinsight-pick-color", self.monitor.id)),
            egui::Sense::click(),
        );
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return Some(OverlayOutcome::Cancel);
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let img = to_img(pos);
                let x = (img.x as u32).min(self.image.width().saturating_sub(1));
                let y = (img.y as u32).min(self.image.height().saturating_sub(1));
                let pixel = self.image.get_pixel(x, y);
                return Some(OverlayOutcome::Color([pixel[0], pixel[1], pixel[2]]));
            }
        }
        None
    }

    /// Zoomed pixel preview beside the cursor, Snipaste style: a grid of source
    /// pixels at 1:1 per cell, the sampled colour and its coordinates.
    ///
    /// The texture is rebuilt only when the cursor crosses a pixel boundary —
    /// a fresh `load_texture` per frame would churn the atlas for no visual
    /// gain, since the sampling grid is quantised to whole pixels anyway.
    fn show_magnifier(&mut self, ui: &mut egui::Ui, screen: egui::Rect, scale: f32) {
        if !self.prefs.magnifier {
            return;
        }
        let Some(cursor) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !screen.contains(cursor) || self.image.width() == 0 || self.image.height() == 0 {
            return;
        }
        let (image_w, image_h) = (self.image.width() as i32, self.image.height() as i32);
        let cx = (((cursor.x - screen.min.x) / scale) as i32).clamp(0, image_w - 1);
        let cy = (((cursor.y - screen.min.y) / scale) as i32).clamp(0, image_h - 1);

        // Odd cell count so the cursor sits on a whole cell.
        let cells = (self.prefs.magnifier_size.clamp(5, 31) | 1) as i32;
        let half = cells / 2;
        let cell = 11.0_f32;
        let zoom_size = cells as f32 * cell;

        if self
            .magnifier
            .as_ref()
            .map(|(mx, my, mn, _)| *mx != cx || *my != cy || *mn as i32 != cells)
            .unwrap_or(true)
        {
            let mut pixels = Vec::with_capacity((cells * cells * 4) as usize);
            for j in 0..cells {
                for i in 0..cells {
                    let px = (cx + i - half).clamp(0, image_w - 1) as u32;
                    let py = (cy + j - half).clamp(0, image_h - 1) as u32;
                    let p = self.image.get_pixel(px, py);
                    pixels.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
            let color =
                egui::ColorImage::from_rgba_unmultiplied([cells as usize, cells as usize], &pixels);
            let texture =
                ui.ctx()
                    .load_texture("pinsight-magnifier", color, egui::TextureOptions::NEAREST);
            self.magnifier = Some((cx, cy, cells as u32, texture));
        }
        let Some((_, _, _, texture)) = self.magnifier.as_ref() else {
            return;
        };
        let texture_id = texture.id();

        let sample = self.image.get_pixel(cx as u32, cy as u32);
        let rgb = [sample[0], sample[1], sample[2]];
        let value = self.prefs.color_format.render(rgb);

        let info_h = 26.0;
        let pad = 6.0;
        let panel_size = egui::vec2(zoom_size + pad * 2.0, zoom_size + info_h + pad * 2.0 + 4.0);
        // Prefer below-right of the cursor, flip when that would leave the
        // screen. The offset keeps the panel clear of the crosshair centre.
        let mut origin = cursor + egui::vec2(24.0, 24.0);
        if origin.x + panel_size.x > screen.right() {
            origin.x = cursor.x - 24.0 - panel_size.x;
        }
        if origin.y + panel_size.y > screen.bottom() {
            origin.y = cursor.y - 24.0 - panel_size.y;
        }
        origin.x = origin
            .x
            .clamp(screen.left() + 2.0, screen.right() - panel_size.x - 2.0);
        origin.y = origin
            .y
            .clamp(screen.top() + 2.0, screen.bottom() - panel_size.y - 2.0);
        let panel = egui::Rect::from_min_size(origin, panel_size);

        let painter = ui.painter();
        painter.rect_filled(
            panel.translate(egui::vec2(0.0, 3.0)),
            8.0,
            egui::Color32::from_black_alpha(70),
        );
        painter.rect_filled(panel, 8.0, egui::Color32::from_rgb(0x16, 0x1A, 0x21));
        painter.rect_stroke(
            panel,
            8.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(38)),
            egui::StrokeKind::Inside,
        );

        let zoom = egui::Rect::from_min_size(
            panel.min + egui::vec2(pad, pad),
            egui::vec2(zoom_size, zoom_size),
        );
        painter.image(
            texture_id,
            zoom,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );

        // Grid: a black hairline followed by a white one, so the cells stay
        // visible on both dark and light pixels (a single colour vanishes on
        // whatever it matches).
        for i in 1..cells {
            let x = zoom.left() + i as f32 * cell;
            painter.line_segment(
                [egui::pos2(x, zoom.top()), egui::pos2(x, zoom.bottom())],
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(48)),
            );
            painter.line_segment(
                [
                    egui::pos2(x + 1.0, zoom.top()),
                    egui::pos2(x + 1.0, zoom.bottom()),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(48)),
            );
            let y = zoom.top() + i as f32 * cell;
            painter.line_segment(
                [egui::pos2(zoom.left(), y), egui::pos2(zoom.right(), y)],
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(48)),
            );
            painter.line_segment(
                [
                    egui::pos2(zoom.left(), y + 1.0),
                    egui::pos2(zoom.right(), y + 1.0),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_white_alpha(48)),
            );
        }
        let center_cell = egui::Rect::from_min_size(
            zoom.min + egui::vec2(half as f32 * cell, half as f32 * cell),
            egui::vec2(cell, cell),
        );
        painter.rect_stroke(
            center_cell.expand(1.0),
            1.0,
            egui::Stroke::new(2.0, self.palette.accent),
            egui::StrokeKind::Outside,
        );

        // Colour chip + value, then the coordinates.
        let chip = egui::Rect::from_min_size(
            egui::pos2(panel.left() + pad, zoom.bottom() + pad),
            egui::vec2(18.0, 18.0),
        );
        painter.rect_filled(chip, 4.0, egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        painter.rect_stroke(
            chip,
            4.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(60)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            egui::pos2(chip.right() + 8.0, chip.center().y),
            egui::Align2::LEFT_CENTER,
            value,
            egui::FontId::monospace(11.5),
            egui::Color32::from_rgb(0xE6, 0xEA, 0xF2),
        );
        painter.text(
            egui::pos2(panel.right() - pad, chip.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{cx}, {cy}"),
            egui::FontId::monospace(10.5),
            egui::Color32::from_white_alpha(120),
        );
    }
}

fn image_to_screen(rect: ImgRect, screen: egui::Rect, scale: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        screen.min + egui::vec2(rect.x * scale, rect.y * scale),
        egui::vec2(rect.w * scale, rect.h * scale),
    )
}

fn paint_size_label(ui: &egui::Ui, p: &Palette, sel: ImgRect, sel_screen: egui::Rect) {
    let text = format!("{} × {}", sel.w.round() as u32, sel.h.round() as u32);
    let galley = ui
        .painter()
        .layout_no_wrap(text, egui::FontId::monospace(12.0), p.text);
    let pos = egui::pos2(sel_screen.left(), (sel_screen.top() - 22.0).max(4.0));
    let bg = egui::Rect::from_min_size(pos, galley.size() + egui::vec2(12.0, 6.0));
    ui.painter().rect_filled(bg, 4.0, p.card);
    ui.painter()
        .galley(pos + egui::vec2(6.0, 3.0), galley, egui::Color32::WHITE);
}

fn hit_handle(sel: ImgRect, p: egui::Pos2, image_width: u32, tolerance: f32) -> Option<Handle> {
    let tol = (tolerance / image_width.max(1) as f32 * image_width as f32).max(6.0);
    let near_left = (p.x - sel.x).abs() <= tol;
    let near_right = (p.x - sel.right()).abs() <= tol;
    let near_top = (p.y - sel.y).abs() <= tol;
    let near_bottom = (p.y - sel.bottom()).abs() <= tol;
    let inside_x = p.x >= sel.x - tol && p.x <= sel.right() + tol;
    let inside_y = p.y >= sel.y - tol && p.y <= sel.bottom() + tol;
    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(Handle::Nw),
        (_, true, true, _) => Some(Handle::Ne),
        (true, _, _, true) => Some(Handle::Sw),
        (_, true, _, true) => Some(Handle::Se),
        (true, _, _, _) if inside_y => Some(Handle::W),
        (_, true, _, _) if inside_y => Some(Handle::E),
        (_, _, true, _) if inside_x => Some(Handle::N),
        (_, _, _, true) if inside_x => Some(Handle::S),
        _ => None,
    }
}

fn annotation_is_visible(annotation: &Annotation) -> bool {
    match annotation {
        Annotation::Rect { rect, .. }
        | Annotation::Ellipse { rect, .. }
        | Annotation::Highlight { rect }
        | Annotation::Mosaic { rect, .. } => rect.w > 2.0 && rect.h > 2.0,
        Annotation::Arrow { from, to, .. } => from.distance(*to) > 3.0,
        Annotation::Line { from, to, .. } => from.distance(*to) > 3.0,
        Annotation::Pen { points, .. } => points.len() > 1,
        // A click is a complete marker; there is no minimum drag.
        Annotation::Numbered { .. } => true,
        // Drags with the text tool only open the editor, so an empty buffer
        // must never survive.
        Annotation::Text { text, .. } => !text.trim().is_empty(),
    }
}

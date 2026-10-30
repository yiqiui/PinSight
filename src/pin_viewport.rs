//! Pins for every platform other than Windows.
//!
//! Windows uses the native `WS_POPUP` implementation in `imp`; everywhere else
//! a pin is an eframe *deferred* viewport. egui cannot offer real
//! click-through on all platforms, so click-through is emulated by ignoring
//! input, and documented as a limitation.
//!
//! `show_viewport_deferred` requires a `Fn + Send + Sync + 'static` callback,
//! so per-pin mutable state lives in an `Arc<Mutex<PinState>>` that the
//! closure captures by clone. Interaction results are written back into that
//! state and turned into viewport commands after the closure returns.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use eframe::egui;
use image::RgbaImage;

use crate::pin::PinEvent;

/// Everything a live pin viewport needs, mutated from inside the viewport
/// callback. Guarded by a `Mutex` because the callback is `Fn`.
struct PinState {
    image: Arc<RgbaImage>,
    texture: Option<egui::TextureHandle>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    zoom: f32,
    opacity: f32,
    passthrough: bool,
    closed: bool,
    /// The pin is hidden — the viewport is not shown at all.
    visible: bool,
}

impl PinState {
    fn size(&self) -> egui::Vec2 {
        egui::vec2(self.w as f32 * self.zoom, self.h as f32 * self.zoom)
    }
}

pub struct ViewportPin {
    pub id: u64,
    state: Arc<Mutex<PinState>>,
    viewport_id: egui::ViewportId,
    tx: Sender<PinEvent>,
}

impl ViewportPin {
    pub fn new(
        id: u64,
        image: Arc<RgbaImage>,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        tx: Sender<PinEvent>,
    ) -> Self {
        let state = PinState {
            image,
            texture: None,
            x,
            y,
            w,
            h,
            zoom: 1.0,
            opacity: 1.0,
            passthrough: false,
            closed: false,
            visible: true,
        };
        Self {
            id,
            state: Arc::new(Mutex::new(state)),
            viewport_id: egui::ViewportId::from_hash_of(("pinsight-pin", id)),
            tx,
        }
    }

    /// Ask this pin to disappear. The viewport stops being registered on the
    /// next frame, which tears it down.
    pub fn close(&mut self) {
        if let Ok(mut s) = self.state.lock() {
            s.closed = true;
        }
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().map(|s| s.closed).unwrap_or(true)
    }

    pub fn set_passthrough(&mut self, on: bool) {
        if let Ok(mut s) = self.state.lock() {
            s.passthrough = on;
        }
    }

    /// Show or hide the pin.
    pub fn set_visible(&mut self, visible: bool) {
        if let Ok(mut s) = self.state.lock() {
            s.visible = visible;
        }
    }

    /// Must be called from `App::ui` on every frame while the pin is alive.
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.is_closed() {
            return;
        }

        // Snapshot the geometry used to build the window for this frame.
        let (pos, size, transparent, visible) = match self.state.lock() {
            Ok(s) => (
                egui::pos2(s.x as f32, s.y as f32),
                s.size(),
                s.opacity >= 0.999,
                s.visible,
            ),
            Err(_) => return,
        };

        let builder = egui::ViewportBuilder::default()
            .with_title("PinSight")
            .with_decorations(false)
            .with_transparent(transparent)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_visible(visible)
            .with_position(pos)
            .with_inner_size(size);

        let state = Arc::clone(&self.state);
        let tx = self.tx.clone();
        let viewport_id = self.viewport_id;
        let id = self.id;

        // `close` is resolved after the closure so we never hold the lock
        // while sending on the channel (the receiver may want the lock).
        let mut wants_close = false;

        ctx.show_viewport_deferred(viewport_id, builder, move |ui, _class| {
            let mut s = match state.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            if s.closed {
                return;
            }
            if ui.input(|i| i.viewport().close_requested()) {
                s.closed = true;
                return;
            }

            if s.texture.is_none() {
                let (w, h) = (s.w as usize, s.h as usize);
                let color = egui::ColorImage::from_rgba_unmultiplied([w, h], s.image.as_raw());
                let tex = ui.ctx().load_texture(
                    format!("pinsight-pin-{id}"),
                    color,
                    egui::TextureOptions::LINEAR,
                );
                s.texture = Some(tex);
            }
            let Some(texture) = s.texture.clone() else {
                return;
            };

            let passthrough = s.passthrough;
            let opacity = s.opacity;
            let zoom = s.zoom;
            let mut moved = egui::Vec2::ZERO;
            let mut new_zoom: Option<f32> = None;
            let mut new_alpha: Option<f32> = None;

            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(egui::Color32::TRANSPARENT))
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    let sense = if passthrough {
                        egui::Sense::hover()
                    } else {
                        egui::Sense::click_and_drag()
                    };
                    let response = ui.interact(rect, ui.scope_id().with(("pin", id)), sense);
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::from_black_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8),
                    );

                    if !passthrough {
                        if response.dragged() {
                            moved = response.drag_delta();
                        }
                        if response.middle_clicked() {
                            s.closed = true;
                        }
                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll != 0.0 {
                            if ui.input(|i| i.modifiers.ctrl) {
                                new_alpha = Some((opacity - scroll * 0.002).clamp(0.15, 1.0));
                            } else {
                                new_zoom = Some((zoom * (1.0 + scroll * 0.0015)).clamp(0.1, 8.0));
                            }
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            s.closed = true;
                        }
                    }
                });

            if let Some(z) = new_zoom {
                s.zoom = z;
            }
            if let Some(a) = new_alpha {
                s.opacity = a;
            }
            if moved != egui::Vec2::ZERO {
                s.x += moved.x as i32;
                s.y += moved.y as i32;
            }
        });

        // Translate this frame's state changes into viewport commands and,
        // if needed, notify the main loop that the pin is gone.
        match self.state.lock() {
            Ok(s) => {
                if s.closed {
                    wants_close = true;
                } else {
                    let (x, y, zoom) = (s.x, s.y, s.zoom);
                    if (pos.x - x as f32).abs() > 0.5 || (pos.y - y as f32).abs() > 0.5 {
                        ctx.send_viewport_cmd_to(
                            viewport_id,
                            egui::ViewportCommand::OuterPosition(egui::pos2(x as f32, y as f32)),
                        );
                    }
                    if (size.x - s.w as f32 * zoom).abs() > 0.5
                        || (size.y - s.h as f32 * zoom).abs() > 0.5
                    {
                        ctx.send_viewport_cmd_to(
                            viewport_id,
                            egui::ViewportCommand::InnerSize(egui::vec2(
                                s.w as f32 * zoom,
                                s.h as f32 * zoom,
                            )),
                        );
                    }
                }
            }
            Err(_) => wants_close = true,
        }

        if wants_close {
            self.close();
            let _ = tx.send(PinEvent::Close(id));
        }
    }
}

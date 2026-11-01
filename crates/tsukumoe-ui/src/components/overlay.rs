// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::time::Duration;

use egui::{Align2, Area, Context, Frame, Id, Margin, Order, Response, RichText, Stroke, Ui, vec2};

use crate::icons::Icon;
use crate::lang::tr;
use crate::theme::Theme;

use super::button::{Variant, icon_button};
use super::display::Tone;

/// How long a toast stays, in seconds.
const TOAST_SECONDS: f64 = 4.0;

struct Toast {
    tone: Tone,
    text: String,
    until: f64,
}

/// Short messages stacked at the bottom right. Keep one in the app, `push`
/// to it, and call `show` once per frame.
#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, ctx: &Context, tone: Tone, text: impl Into<String>) {
        let now = ctx.input(|i| i.time);
        self.items.push(Toast {
            tone,
            text: text.into(),
            until: now + TOAST_SECONDS,
        });
        ctx.request_repaint();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drops the toasts whose time is up at `now` (seconds).
    fn expire(&mut self, now: f64) {
        self.items.retain(|t| t.until > now);
    }

    pub fn show(&mut self, ctx: &Context) {
        self.expire(ctx.input(|i| i.time));
        if self.items.is_empty() {
            return;
        }
        let t = Theme::get(ctx);
        Area::new(Id::unique("tsukumoe-ui-toasts"))
            .order(Order::Tooltip)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-t.metrics.pad, -t.metrics.pad))
            .interactable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = t.metrics.gap;
                for toast in &self.items {
                    let color = toast.tone.color(&t);
                    Frame::new()
                        .fill(t.colors.surface_raised)
                        .stroke(Stroke::new(1.0, t.colors.border))
                        .corner_radius(t.metrics.radius)
                        .inner_margin(Margin::symmetric(t.metrics.pad as i8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(toast.tone.icon().image(color, t.metrics.icon));
                                ui.label(RichText::new(&toast.text).color(t.colors.text));
                            });
                        });
                }
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/// The content width of [`dialog`], in points.
pub const DIALOG_WIDTH: f32 = 480.0;

/// A modal dialog with a title and a close button. It shows while `*open`;
/// closing it (the button, Escape, a click outside) sets `*open = false`.
pub fn dialog<R>(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    open: &mut bool,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    dialog_sized(ctx, id, title, DIALOG_WIDTH, open, add_contents)
}

/// [`dialog`] with a given content width. The width is fixed: an auto-sized
/// window whose fields fill the available width would otherwise grow by its
/// margins on every frame.
pub fn dialog_sized<R>(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    width: f32,
    open: &mut bool,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    if !*open {
        return None;
    }
    let t = Theme::get(ctx);
    let frame = Frame::new()
        .fill(t.colors.surface_raised)
        .stroke(Stroke::new(1.0, t.colors.border))
        .corner_radius(t.metrics.radius + 2.0)
        .inner_margin(Margin::same(t.metrics.pad as i8 + 4));
    let mut close = false;
    let modal = egui::Modal::new(Id::unique(id))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).heading().color(t.colors.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, &tr(ctx, "dialog-close"), Variant::Ghost)
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            ui.add_space(t.metrics.gap);
            add_contents(ui)
        });
    if close || modal.should_close() {
        *open = false;
    }
    Some(modal.inner)
}

/// A menu entry with an optional shortcut text on the right. Use inside
/// `ui.menu_button`, a context menu or a popup.
pub fn menu_item(ui: &mut Ui, text: &str, shortcut: Option<&str>) -> Response {
    let mut button =
        egui::Button::new(text).min_size(vec2(160.0, Theme::get(ui.ctx()).metrics.control));
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut);
    }
    ui.add(button)
}

/// A menu entry that is a choice: `checked` is `None` for an ordinary entry,
/// else a check mark (an empty space of the same width when false) comes first.
pub fn menu_item_checked(
    ui: &mut Ui,
    text: &str,
    shortcut: Option<&str>,
    checked: Option<bool>,
) -> Response {
    let Some(on) = checked else {
        return menu_item(ui, text, shortcut);
    };
    let t = Theme::get(ui.ctx());
    let color = if on {
        t.colors.accent
    } else {
        egui::Color32::TRANSPARENT
    };
    let image = crate::icons::Icon::Check.image(color, t.metrics.icon);
    let mut button =
        egui::Button::image_and_text(image, text).min_size(vec2(160.0, t.metrics.control));
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(shortcut);
    }
    let response = ui.add(button);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItem, true, on, text));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_expire_after_their_time() {
        let mut toasts = Toasts::default();
        for (text, until) in [("a", 1.0), ("b", 5.0)] {
            toasts.items.push(Toast {
                tone: Tone::Neutral,
                text: text.into(),
                until,
            });
        }
        toasts.expire(2.0);
        assert_eq!(toasts.items.len(), 1);
        assert_eq!(toasts.items[0].text, "b");
        toasts.expire(5.0);
        assert!(toasts.is_empty());
    }

    /// The content width of `frames` consecutive frames of a dialog with an
    /// input field (which fills the available width) and a header.
    fn dialog_widths(frames: usize) -> Vec<f32> {
        let ctx = Context::default();
        let mut text = String::from("path");
        let mut open = true;
        let mut widths = Vec::new();
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 900.0),
                )),
                ..Default::default()
            };
            let mut width = 0.0;
            let mut out = ctx.run_ui(input, |ui| {
                dialog(ui.ctx(), "growth", "Title", &mut open, |ui| {
                    crate::text_input(ui, &mut text, "path", "path");
                    width = ui.min_rect().width();
                });
            });
            out.textures_delta.clear();
            widths.push(width);
        }
        widths
    }

    #[test]
    fn a_dialog_does_not_grow_from_frame_to_frame() {
        let widths = dialog_widths(24);
        let settled = widths[4];
        for (i, w) in widths.iter().enumerate().skip(4) {
            assert!(
                (w - settled).abs() < 0.5,
                "frame {i}: width {w} differs from {settled} ({widths:?})"
            );
        }
        assert!(settled < 700.0, "dialog is {settled} points wide");
    }
}

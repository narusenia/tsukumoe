// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The tool bar floating over the viewport: one icon button per edit tool and
//! one for proportional edit. A button only names an action; the editor runs
//! it like a key or a menu item, so the pressed state follows the view state.

use egui::{Align2, Rect, Stroke, vec2};
use tsukumoe_ui::{Icon, Keymap, Theme, Variant, icon_button, tr};

use crate::actions::{Tool, label_key};

/// Below the help line and the proportional options box.
const TOP: f32 = 72.0;

/// The buttons, top to bottom: the action, its icon, and whether it is pressed.
pub fn buttons(tool: Tool, proportional: bool) -> [(&'static str, Icon, bool); 4] {
    [
        (
            Tool::Select.action(),
            Icon::MousePointer,
            tool == Tool::Select,
        ),
        (Tool::Mesh.action(), Icon::Image, tool == Tool::Mesh),
        (Tool::Deformer.action(), Icon::Grid, tool == Tool::Deformer),
        ("proportional.toggle", Icon::Locate, proportional),
    ]
}

/// Draws the bar at the top left of `rect`; returns the action clicked.
pub fn show(
    ctx: &egui::Context,
    rect: Rect,
    tool: Tool,
    proportional: bool,
    keymap: &Keymap,
) -> Option<&'static str> {
    let theme = Theme::get(ctx);
    let mut chosen = None;
    egui::Area::new(egui::Id::unique("tsukumoe-toolbar"))
        .fixed_pos(rect.left_top() + vec2(8.0, TOP))
        .pivot(Align2::LEFT_TOP)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme.colors.surface_raised)
                .stroke(Stroke::new(1.0, theme.colors.border))
                .corner_radius(theme.metrics.radius)
                .inner_margin(egui::Margin::same(3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                    for (action, icon, pressed) in buttons(tool, proportional) {
                        let mut name = tr(ctx, &label_key(action));
                        if let Some(keys) = keymap.shortcut_text(ctx, action) {
                            name = format!("{name} ({keys})");
                        }
                        let variant = if pressed {
                            Variant::Primary
                        } else {
                            Variant::Ghost
                        };
                        if icon_button(ui, icon, &name, variant).clicked() {
                            chosen = Some(action);
                        }
                    }
                });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ACTIONS, cubism_compatible};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn the_pressed_buttons_follow_the_tool_and_the_option() {
        for tool in [Tool::Select, Tool::Mesh, Tool::Deformer] {
            let b = buttons(tool, false);
            assert_eq!(b.iter().filter(|(_, _, p)| *p).count(), 1);
            assert!(b.iter().any(|(a, _, p)| *p && *a == tool.action()));
        }
        assert!(buttons(Tool::Select, true)[3].2);
        assert!(
            buttons(Tool::Select, true)
                .iter()
                .all(|(a, ..)| ACTIONS.contains(a))
        );
    }

    #[test]
    fn each_button_sends_its_action() {
        let keymap = cubism_compatible();
        let mut harness = Harness::builder()
            .with_size(vec2(300.0, 300.0))
            .build_ui_state(
                move |ui, chosen: &mut Option<&'static str>| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    if let Some(a) = show(ui.ctx(), ui.max_rect(), Tool::Select, false, &keymap) {
                        *chosen = Some(a);
                    }
                },
                None,
            );
        harness.run();
        for (label, action) in [
            ("Mesh edit tool (M)", "tool.mesh-edit"),
            ("Deformer tool (D)", "tool.deformer"),
            ("Select tool (V)", "tool.select"),
            ("Toggle proportional edit (O)", "proportional.toggle"),
        ] {
            *harness.state_mut() = None;
            harness.get_by_label(label).click();
            harness.run();
            assert_eq!(*harness.state(), Some(action), "{label}");
        }
    }
}

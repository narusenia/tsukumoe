// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A pie (radial) menu. It opens at a point while a key or button is held;
//! the item is picked by the direction of the pointer from that point and
//! chosen when the key is released. Releasing near the center cancels. An item
//! can have children (one level): moving the pointer past the outer ring over
//! such an item opens them, centered where the pointer is.
//!
//! The selection geometry is pure ([`pie_hit`]); [`PieMenu`] is the state and
//! [`PieMenu::show`] draws it.

use std::f32::consts::TAU;

use egui::{Align2, Color32, Context, FontId, Id, Order, Pos2, Stroke, vec2};

use crate::theme::Theme;

/// Pointer distance below which nothing is selected (releasing there cancels).
pub const DEAD_RADIUS: f32 = 24.0;
/// Radius of the ring the items sit on; past it a submenu opens.
pub const OUTER_RADIUS: f32 = 84.0;

#[derive(Clone, Debug, PartialEq)]
pub struct PieItem {
    /// What the app maps to an action (a keymap action name).
    pub id: String,
    pub label: String,
    pub children: Vec<PieItem>,
}

impl PieItem {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<PieItem>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieHit {
    /// Inside the dead zone.
    Center,
    /// Sector `index` of `count`, the first one straight up, clockwise.
    /// `beyond` is true past the outer ring.
    Item { index: usize, beyond: bool },
}

/// The sector under `pointer` for a menu of `count` items centered at `center`.
pub fn pie_hit(center: Pos2, pointer: Pos2, count: usize) -> PieHit {
    let d = pointer - center;
    if count == 0 || d.length() < DEAD_RADIUS {
        return PieHit::Center;
    }
    // Angle clockwise from straight up, in [0, TAU).
    let angle = d.x.atan2(-d.y).rem_euclid(TAU);
    let step = TAU / count as f32;
    // Sector i is centered at i * step, so shift by half a step.
    let index = (((angle + step / 2.0) / step) as usize) % count;
    PieHit::Item {
        index,
        beyond: d.length() > OUTER_RADIUS,
    }
}

/// What a frame of the menu produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieEvent {
    Open,
    Chosen(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct PieMenu {
    center: Pos2,
    items: Vec<PieItem>,
    /// The parent level, when a submenu is showing.
    parent: Option<Vec<PieItem>>,
}

impl PieMenu {
    pub fn open(center: Pos2, items: Vec<PieItem>) -> Self {
        Self {
            center,
            items,
            parent: None,
        }
    }

    pub fn items(&self) -> &[PieItem] {
        &self.items
    }

    pub fn in_submenu(&self) -> bool {
        self.parent.is_some()
    }

    /// Advances the menu with the pointer position and whether the key was
    /// released (or `cancel`, e.g. Escape, was pressed) this frame.
    pub fn update(&mut self, pointer: Pos2, released: bool, cancel: bool) -> PieEvent {
        if cancel {
            return PieEvent::Cancelled;
        }
        let hit = pie_hit(self.center, pointer, self.items.len());
        if released {
            return match hit {
                PieHit::Center => PieEvent::Cancelled,
                PieHit::Item { index, .. } => {
                    let item = &self.items[index];
                    if item.children.is_empty() {
                        PieEvent::Chosen(item.id.clone())
                    } else {
                        // A parent is not a command; releasing on one cancels.
                        PieEvent::Cancelled
                    }
                }
            };
        }
        if self.parent.is_none()
            && let PieHit::Item {
                index,
                beyond: true,
            } = hit
            && !self.items[index].children.is_empty()
        {
            let children = self.items[index].children.clone();
            self.parent = Some(std::mem::replace(&mut self.items, children));
            self.center = pointer;
        }
        PieEvent::Open
    }

    /// Reads the pointer, updates and draws. `released` is whether the
    /// triggering key or button went up this frame.
    pub fn show(&mut self, ctx: &Context, released: bool) -> PieEvent {
        let pointer = ctx.pointer_latest_pos().unwrap_or(self.center);
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let event = self.update(pointer, released, cancel);
        if event == PieEvent::Open {
            self.paint(ctx, pointer);
            ctx.request_repaint();
        }
        event
    }

    fn paint(&self, ctx: &Context, pointer: Pos2) {
        let t = Theme::get(ctx);
        let painter = ctx.layer_painter(egui::LayerId::new(
            Order::Tooltip,
            Id::unique("tsukumoe-pie-menu"),
        ));
        painter.circle_filled(self.center, OUTER_RADIUS + 28.0, t.colors.surface_raised);
        painter.circle_stroke(
            self.center,
            OUTER_RADIUS + 28.0,
            Stroke::new(1.0, t.colors.border),
        );
        painter.circle_filled(self.center, DEAD_RADIUS, t.colors.background);
        let hit = pie_hit(self.center, pointer, self.items.len());
        let step = TAU / self.items.len().max(1) as f32;
        for (i, item) in self.items.iter().enumerate() {
            let a = i as f32 * step;
            let at = self.center + vec2(a.sin(), -a.cos()) * (OUTER_RADIUS * 0.7);
            let on = matches!(hit, PieHit::Item { index, .. } if index == i);
            if on {
                painter.circle_filled(at, 26.0, t.colors.accent);
            }
            let color = if on { Color32::WHITE } else { t.colors.text };
            let mut text = item.label.clone();
            if !item.children.is_empty() {
                text.push_str(" ›");
            }
            painter.text(
                at,
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(t.metrics.text),
                color,
            );
        }
        painter.line_segment(
            [
                self.center,
                self.center + {
                    let d = pointer - self.center;
                    if d.length() > OUTER_RADIUS {
                        d * (OUTER_RADIUS / d.length())
                    } else {
                        d
                    }
                },
            ],
            Stroke::new(1.0, t.colors.border_strong),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const C: Pos2 = pos2(100.0, 100.0);

    fn hit(dx: f32, dy: f32, n: usize) -> PieHit {
        pie_hit(C, C + vec2(dx, dy), n)
    }

    #[test]
    fn the_center_is_a_dead_zone() {
        assert_eq!(hit(0.0, 0.0, 4), PieHit::Center);
        assert_eq!(hit(10.0, -10.0, 4), PieHit::Center);
        assert_eq!(hit(50.0, 0.0, 0), PieHit::Center);
    }

    #[test]
    fn four_items_sit_up_right_down_left() {
        let at = |dx, dy| match hit(dx, dy, 4) {
            PieHit::Item { index, .. } => index,
            other => panic!("{other:?}"),
        };
        assert_eq!(at(0.0, -50.0), 0);
        assert_eq!(at(50.0, 0.0), 1);
        assert_eq!(at(0.0, 50.0), 2);
        assert_eq!(at(-50.0, 0.0), 3);
        // sector boundaries are at 45 degrees
        assert_eq!(at(30.0, -50.0), 0);
        assert_eq!(at(50.0, -30.0), 1);
        assert_eq!(at(-30.0, -50.0), 0);
    }

    #[test]
    fn sectors_wrap_for_odd_counts() {
        let idx = |dx, dy| match hit(dx, dy, 3) {
            PieHit::Item { index, .. } => index,
            other => panic!("{other:?}"),
        };
        assert_eq!(idx(0.0, -50.0), 0);
        assert_eq!(idx(40.0, 30.0), 1);
        assert_eq!(idx(-40.0, 30.0), 2);
        // just left of straight up is still item 0
        assert_eq!(idx(-3.0, -50.0), 0);
    }

    #[test]
    fn beyond_the_ring_still_picks_the_sector() {
        assert_eq!(
            hit(0.0, -200.0, 4),
            PieHit::Item {
                index: 0,
                beyond: true
            }
        );
        assert_eq!(
            hit(0.0, -50.0, 4),
            PieHit::Item {
                index: 0,
                beyond: false
            }
        );
    }

    fn menu() -> PieMenu {
        PieMenu::open(
            C,
            vec![
                PieItem::new("a", "A"),
                PieItem::new("b", "B")
                    .with_children(vec![PieItem::new("b1", "B1"), PieItem::new("b2", "B2")]),
                PieItem::new("c", "C"),
                PieItem::new("d", "D"),
            ],
        )
    }

    #[test]
    fn releasing_chooses_or_cancels() {
        let mut m = menu();
        assert_eq!(
            m.update(C + vec2(0.0, -50.0), true, false),
            PieEvent::Chosen("a".into())
        );
        assert_eq!(
            m.update(C + vec2(5.0, 5.0), true, false),
            PieEvent::Cancelled
        );
        assert_eq!(
            m.update(C + vec2(0.0, -50.0), false, true),
            PieEvent::Cancelled
        );
        assert_eq!(m.update(C + vec2(0.0, -50.0), false, false), PieEvent::Open);
        // a parent item is not a command
        assert_eq!(
            m.update(C + vec2(50.0, 0.0), true, false),
            PieEvent::Cancelled
        );
    }

    #[test]
    fn a_submenu_opens_past_the_ring_and_recenters() {
        let mut m = menu();
        assert_eq!(m.update(C + vec2(50.0, 0.0), false, false), PieEvent::Open);
        assert!(!m.in_submenu());
        let far = C + vec2(120.0, 0.0);
        assert_eq!(m.update(far, false, false), PieEvent::Open);
        assert!(m.in_submenu());
        assert_eq!(m.items().len(), 2);
        // up from the new center picks the first child
        assert_eq!(
            m.update(far + vec2(0.0, -50.0), true, false),
            PieEvent::Chosen("b1".into())
        );
        // only one level
        let mut m = menu();
        m.update(far, false, false);
        m.update(far + vec2(0.0, 200.0), false, false);
        assert!(m.in_submenu());
    }
}

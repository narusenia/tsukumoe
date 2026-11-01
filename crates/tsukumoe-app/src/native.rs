// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The macOS menu bar, drawn from the same [`Entry`] tree as the egui menu.
//! Shortcuts are part of the label text only: the egui keymap stays the single
//! place that fires them, so a native key equivalent would run an action twice.

use std::sync::mpsc::{Receiver, channel};

use muda::{IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::menu::Entry;

/// Keys of the labels muda cannot get from [`Entry`] (the app and window menus).
const FIXED_KEYS: [&str; 6] = [
    "menu-about",
    "menu-hide",
    "menu-quit",
    "menu-window",
    "menu-minimize",
    "menu-zoom",
];

pub struct NativeMenu {
    /// Kept alive; dropping it would remove the menu bar.
    menu: Menu,
    shown: Option<(Vec<Entry>, Vec<String>)>,
    events: Receiver<String>,
}

impl NativeMenu {
    /// Must run on the main thread once the event loop is running (eframe's app
    /// creator does).
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, events) = channel();
        let ctx = ctx.clone();
        // A click must wake the UI even when nothing else repaints.
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e.id.0);
            ctx.request_repaint();
        }));
        Self {
            menu: Menu::new(),
            shown: None,
            events,
        }
    }

    /// The actions chosen since the last call.
    pub fn drain(&self) -> Vec<String> {
        self.events.try_iter().collect()
    }

    /// Rebuilds the menu bar when `entries` or the fixed labels changed.
    pub fn sync(&mut self, entries: Vec<Entry>, label: &dyn Fn(&str) -> String) {
        let fixed: Vec<String> = FIXED_KEYS.iter().map(|k| label(k)).collect();
        let now = (entries, fixed);
        if self.shown.as_ref() == Some(&now) {
            return;
        }
        let (entries, fixed) = &now;
        let menu = Menu::new();
        let text = |i: usize| Some(fixed[i].as_str());
        let app = Submenu::new("Tsukumoe", true);
        let window = Submenu::new(&fixed[3], true);
        let items: Vec<Box<dyn IsMenuItem>> = vec![
            Box::new(PredefinedMenuItem::about(text(0), None)),
            Box::new(PredefinedMenuItem::separator()),
            Box::new(PredefinedMenuItem::hide(text(1))),
            Box::new(PredefinedMenuItem::separator()),
            Box::new(PredefinedMenuItem::quit(text(2))),
        ];
        let _ = app.append_items(&refs(&items));
        let _ = window.append_items(&[
            &PredefinedMenuItem::minimize(text(4)),
            &PredefinedMenuItem::zoom(text(5)),
        ]);
        let _ = menu.append(&app);
        let _ = menu.append_items(&refs(&build(entries)));
        let _ = menu.append(&window);
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        self.menu = menu;
        self.shown = Some(now);
    }
}

fn refs(items: &[Box<dyn IsMenuItem>]) -> Vec<&dyn IsMenuItem> {
    items.iter().map(|i| i.as_ref()).collect()
}

fn build(entries: &[Entry]) -> Vec<Box<dyn IsMenuItem>> {
    entries
        .iter()
        .map(|entry| -> Box<dyn IsMenuItem> {
            match entry {
                Entry::Separator => Box::new(PredefinedMenuItem::separator()),
                Entry::Sub { label, entries } => {
                    let sub = Submenu::new(label, true);
                    let _ = sub.append_items(&refs(&build(entries)));
                    Box::new(sub)
                }
                Entry::Item {
                    action,
                    label,
                    enabled,
                    shortcut,
                } => {
                    let text = match shortcut {
                        Some(keys) => format!("{label}    {keys}"),
                        None => label.clone(),
                    };
                    Box::new(MenuItem::with_id(MenuId::new(action), text, *enabled, None))
                }
            }
        })
        .collect()
}

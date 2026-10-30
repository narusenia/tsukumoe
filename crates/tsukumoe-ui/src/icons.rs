// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Lucide icons (ISC, see `assets/icons/LICENSE`), embedded as SVG and drawn
//! tinted with a theme color. To add one, see `assets/icons/README.md`.

use egui::{Color32, Image, Response, Ui, Vec2, load::Bytes};

use crate::theme::Theme;

macro_rules! icons {
    ($($variant:ident => $name:literal),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Icon {
            $($variant,)*
        }

        impl Icon {
            pub const ALL: &'static [Icon] = &[$(Icon::$variant,)*];

            /// The Lucide name, such as `chevron-down`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Icon::$variant => $name,)*
                }
            }

            fn svg(self) -> &'static [u8] {
                match self {
                    $(Icon::$variant => include_bytes!(concat!("../assets/icons/", $name, ".svg")),)*
                }
            }
        }
    };
}

icons! {
    ChevronDown => "chevron-down",
    ChevronUp => "chevron-up",
    ChevronRight => "chevron-right",
    ChevronLeft => "chevron-left",
    Check => "check",
    Close => "x",
    Plus => "plus",
    Minus => "minus",
    Search => "search",
    Settings => "settings",
    Eye => "eye",
    EyeOff => "eye-off",
    Lock => "lock",
    Unlock => "lock-open",
    Trash => "trash",
    Copy => "copy",
    Undo => "undo-2",
    Redo => "redo-2",
    Play => "play",
    Pause => "pause",
    Folder => "folder",
    File => "file",
    Image => "image",
    Layers => "layers",
    Hand => "hand",
    Rotate => "rotate-cw",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    Info => "info",
    Warning => "triangle-alert",
    Error => "circle-alert",
    Success => "circle-check",
    More => "ellipsis",
    Grip => "grip-vertical",
    Sliders => "sliders-horizontal",
    Save => "save",
    Download => "download",
    Upload => "upload",
    Menu => "menu",
}

impl Icon {
    /// The icon as an image tinted `color`, `size` points square.
    pub fn image(self, color: Color32, size: f32) -> Image<'static> {
        let uri = format!("bytes://tsukumoe-ui/icons/{}.svg", self.name());
        Image::from_bytes(uri, Bytes::Static(self.svg()))
            .tint(color)
            .fit_to_exact_size(Vec2::splat(size))
    }
}

/// An icon at the theme's icon size.
pub fn icon(ui: &mut Ui, icon: Icon, color: Color32) -> Response {
    let size = Theme::get(ui.ctx()).metrics.icon;
    ui.add(icon.image(color, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_tinted_by_a_white_stroke() {
        for icon in Icon::ALL {
            let svg = std::str::from_utf8(icon.svg()).unwrap();
            assert!(svg.contains("<svg"), "{}", icon.name());
            assert!(
                svg.contains("#ffffff"),
                "{}: stroke must be white",
                icon.name()
            );
            assert!(!svg.contains("currentColor"), "{}", icon.name());
        }
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Icon::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Icon::ALL.len());
    }
}

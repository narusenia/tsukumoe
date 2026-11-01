// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The dock layout as plain data: a binary tree of splits and tab groups in
//! the middle, plus a strip of collapsed panels on the left and right edges.
//! Every operation here is a pure function of the layout, so it is testable
//! without a window; `view` draws it. The whole thing is serde JSON.

use serde::{Deserialize, Serialize};

pub const MIN_RATIO: f32 = 0.1;
pub const MAX_RATIO: f32 = 0.9;
pub const MIN_WIDTH: f32 = 120.0;
pub const MAX_WIDTH: f32 = 1200.0;
pub const DEFAULT_WIDTH: f32 = 280.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// `a` on the left, `b` on the right.
    Horizontal,
    /// `a` on top, `b` below.
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Node {
    Split {
        axis: Axis,
        /// The share of `a`, between [`MIN_RATIO`] and [`MAX_RATIO`].
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
    Tabs {
        tabs: Vec<String>,
        #[serde(default)]
        active: usize,
    },
}

/// Panels collapsed to an edge. `open` is the one slid out; `pinned` makes it
/// take layout space, otherwise it floats over the center and closes when the
/// pointer clicks elsewhere.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Strip {
    pub panels: Vec<String>,
    #[serde(default)]
    pub open: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default = "default_width")]
    pub width: f32,
}

fn default_width() -> f32 {
    DEFAULT_WIDTH
}

impl Default for Strip {
    fn default() -> Self {
        Self {
            panels: Vec::new(),
            open: None,
            pinned: false,
            width: DEFAULT_WIDTH,
        }
    }
}

/// Where a panel is dropped. `anchor` names a panel already in the tab group
/// that receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Tabs { anchor: String },
    Split { anchor: String, edge: Edge },
    Strip(Side),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Layout {
    pub root: Node,
    #[serde(default)]
    pub left: Strip,
    #[serde(default)]
    pub right: Strip,
}

impl Node {
    pub fn tabs(ids: &[&str]) -> Self {
        Node::Tabs {
            tabs: ids.iter().map(|s| (*s).to_owned()).collect(),
            active: 0,
        }
    }

    pub fn split(axis: Axis, ratio: f32, a: Node, b: Node) -> Self {
        Node::Split {
            axis,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
            Node::Tabs { tabs, .. } => out.extend(tabs.iter().map(String::as_str)),
        }
    }

    /// Removes `id`; returns the pruned node (`None` when nothing is left).
    fn without(self, id: &str) -> Option<Node> {
        match self {
            Node::Tabs { mut tabs, active } => {
                let Some(at) = tabs.iter().position(|t| t == id) else {
                    return Some(Node::Tabs { tabs, active });
                };
                tabs.remove(at);
                if tabs.is_empty() {
                    return None;
                }
                let active = if active > at || active >= tabs.len() {
                    active.saturating_sub(1).min(tabs.len() - 1)
                } else {
                    active
                };
                Some(Node::Tabs { tabs, active })
            }
            Node::Split { axis, ratio, a, b } => match (a.without(id), b.without(id)) {
                (Some(a), Some(b)) => Some(Node::split(axis, ratio, a, b)),
                (one, other) => one.or(other),
            },
        }
    }

    /// The panel showing in each tab group.
    fn visible_tabs(&self) -> Vec<String> {
        match self {
            Node::Tabs { tabs, active } => tabs.get(*active).cloned().into_iter().collect(),
            Node::Split { a, b, .. } => {
                let mut v = a.visible_tabs();
                v.extend(b.visible_tabs());
                v
            }
        }
    }

    fn find_tabs_mut(&mut self, anchor: &str) -> Option<&mut Node> {
        match self {
            Node::Tabs { tabs, .. } => tabs.iter().any(|t| t == anchor).then_some(self),
            Node::Split { a, b, .. } => {
                if a.find_tabs_mut(anchor).is_some() {
                    a.find_tabs_mut(anchor)
                } else {
                    b.find_tabs_mut(anchor)
                }
            }
        }
    }

    fn node_mut(&mut self, path: &[bool]) -> Option<&mut Node> {
        let Some((first, rest)) = path.split_first() else {
            return Some(self);
        };
        match self {
            Node::Split { a, b, .. } => if *first { b } else { a }.node_mut(rest),
            Node::Tabs { .. } => None,
        }
    }
}

impl Layout {
    pub fn new(root: Node) -> Self {
        Self {
            root,
            left: Strip::default(),
            right: Strip::default(),
        }
    }

    /// Every panel, in the tree and the strips.
    pub fn panels(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.root.collect(&mut out);
        for strip in [&self.left, &self.right] {
            out.extend(strip.panels.iter().map(String::as_str));
        }
        out
    }

    pub fn contains(&self, id: &str) -> bool {
        self.panels().contains(&id)
    }

    pub fn strip(&self, side: Side) -> &Strip {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    pub fn strip_mut(&mut self, side: Side) -> &mut Strip {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// Takes `id` out of wherever it is. An empty tree stays as an empty tab
    /// group so panels can still be dropped into it.
    pub fn remove(&mut self, id: &str) -> bool {
        if !self.contains(id) {
            return false;
        }
        let root = std::mem::replace(&mut self.root, Node::tabs(&[]));
        self.root = root.without(id).unwrap_or_else(|| Node::tabs(&[]));
        for strip in [&mut self.left, &mut self.right] {
            strip.panels.retain(|p| p != id);
            if strip.open.as_deref() == Some(id) {
                strip.open = None;
            }
        }
        true
    }

    /// Moves `id` to `target`. False (and no change) when `id` is unknown, the
    /// anchor is unknown or is `id` itself alone in its group.
    pub fn move_panel(&mut self, id: &str, target: Target) -> bool {
        if !self.contains(id) {
            return false;
        }
        if let Target::Tabs { anchor } | Target::Split { anchor, .. } = &target {
            let empty = self.root == Node::tabs(&[]);
            if !empty && (anchor == id && self.group_len(id) == 1 || !self.contains(anchor)) {
                return false;
            }
        }
        let before = self.clone();
        self.remove(id);
        let placed = match target {
            Target::Strip(side) => {
                self.strip_mut(side).panels.push(id.to_owned());
                true
            }
            Target::Tabs { anchor } => self.place_in_tabs(id, &anchor),
            Target::Split { anchor, edge } => self.place_in_split(id, &anchor, edge),
        };
        if !placed {
            *self = before;
        }
        placed
    }

    fn group_len(&self, id: &str) -> usize {
        fn go(n: &Node, id: &str) -> Option<usize> {
            match n {
                Node::Tabs { tabs, .. } => tabs.iter().any(|t| t == id).then_some(tabs.len()),
                Node::Split { a, b, .. } => go(a, id).or_else(|| go(b, id)),
            }
        }
        go(&self.root, id).unwrap_or(0)
    }

    fn place_in_tabs(&mut self, id: &str, anchor: &str) -> bool {
        if self.root == Node::tabs(&[]) {
            self.root = Node::tabs(&[id]);
            return true;
        }
        match self.root.find_tabs_mut(anchor) {
            Some(Node::Tabs { tabs, active }) => {
                tabs.push(id.to_owned());
                *active = tabs.len() - 1;
                true
            }
            _ => false,
        }
    }

    fn place_in_split(&mut self, id: &str, anchor: &str, edge: Edge) -> bool {
        if self.root == Node::tabs(&[]) {
            self.root = Node::tabs(&[id]);
            return true;
        }
        let Some(node) = self.root.find_tabs_mut(anchor) else {
            return false;
        };
        let old = std::mem::replace(node, Node::tabs(&[]));
        let new = Node::tabs(&[id]);
        let (axis, new_first) = match edge {
            Edge::Left => (Axis::Horizontal, true),
            Edge::Right => (Axis::Horizontal, false),
            Edge::Top => (Axis::Vertical, true),
            Edge::Bottom => (Axis::Vertical, false),
        };
        *node = if new_first {
            Node::split(axis, 0.5, new, old)
        } else {
            Node::split(axis, 0.5, old, new)
        };
        true
    }

    /// Makes `id` the visible tab of its group (or the open panel of its strip).
    pub fn activate(&mut self, id: &str) {
        if let Some(Node::Tabs { tabs, active }) = self.root.find_tabs_mut(id)
            && let Some(at) = tabs.iter().position(|t| t == id)
        {
            *active = at;
        }
    }

    /// Brings `id` into view: the visible tab of its group, or the open panel
    /// of its strip.
    pub fn reveal(&mut self, id: &str) {
        for side in [Side::Left, Side::Right] {
            let strip = self.strip_mut(side);
            if strip.panels.iter().any(|p| p == id) {
                strip.open = Some(id.to_owned());
                return;
            }
        }
        self.activate(id);
    }

    /// Whether `id` is showing: the visible tab of its group, or the open
    /// panel of its strip.
    pub fn is_visible(&self, id: &str) -> bool {
        if [Side::Left, Side::Right]
            .into_iter()
            .any(|side| self.strip(side).open.as_deref() == Some(id))
        {
            return true;
        }
        self.root
            .visible_tabs()
            .iter()
            .any(|visible| visible.as_str() == id)
    }

    /// Brings `id` to the front: the tab is selected, a strip panel is opened,
    /// and a panel missing from the layout is added to the right strip and opened.
    pub fn show_panel(&mut self, id: &str) {
        if !self.contains(id) {
            self.strip_mut(Side::Right).panels.push(id.to_owned());
        }
        self.reveal(id);
    }

    /// Sets the share of the split at `path` (`false` = `a`, `true` = `b`),
    /// clamped to the allowed range.
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) -> bool {
        match self.root.node_mut(path) {
            Some(Node::Split { ratio: r, .. }) if ratio.is_finite() => {
                *r = ratio.clamp(MIN_RATIO, MAX_RATIO);
                true
            }
            _ => false,
        }
    }

    /// Slides `id` out of its strip, or back in when it is the open one. Only
    /// one panel per strip is open.
    pub fn toggle_strip_panel(&mut self, side: Side, id: &str) {
        let strip = self.strip_mut(side);
        if !strip.panels.iter().any(|p| p == id) {
            return;
        }
        strip.open = if strip.open.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_owned())
        };
    }

    pub fn set_pinned(&mut self, side: Side, pinned: bool) {
        self.strip_mut(side).pinned = pinned;
    }

    pub fn set_strip_width(&mut self, side: Side, width: f32) {
        if width.is_finite() {
            self.strip_mut(side).width = width.clamp(MIN_WIDTH, MAX_WIDTH);
        }
    }

    /// Closes the floating (unpinned) panels: a click landed elsewhere.
    pub fn close_overlays(&mut self) {
        for strip in [&mut self.left, &mut self.right] {
            if !strip.pinned {
                strip.open = None;
            }
        }
    }

    /// Checks a layout read from outside: panels exist in `known` and appear
    /// once, ratios, widths and indices are sane.
    pub fn validate(&self, known: &[&str]) -> Result<(), String> {
        let panels = self.panels();
        for (i, p) in panels.iter().enumerate() {
            if !known.contains(p) {
                return Err(format!("unknown panel {p:?}"));
            }
            if panels[..i].contains(p) {
                return Err(format!("panel {p:?} appears twice"));
            }
        }
        fn node(n: &Node) -> Result<(), String> {
            match n {
                Node::Tabs { tabs, active } => {
                    if tabs.is_empty() && *active != 0 || !tabs.is_empty() && *active >= tabs.len()
                    {
                        return Err("active tab out of range".into());
                    }
                    Ok(())
                }
                Node::Split { ratio, a, b, .. } => {
                    if !ratio.is_finite() || !(MIN_RATIO..=MAX_RATIO).contains(ratio) {
                        return Err(format!("split ratio {ratio} out of range"));
                    }
                    node(a)?;
                    node(b)
                }
            }
        }
        node(&self.root)?;
        for strip in [&self.left, &self.right] {
            if !strip.width.is_finite() || !(MIN_WIDTH..=MAX_WIDTH).contains(&strip.width) {
                return Err("strip width out of range".into());
            }
            if let Some(open) = &strip.open
                && !strip.panels.contains(open)
            {
                return Err(format!("open panel {open:?} is not in its strip"));
            }
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("layout serializes")
    }

    pub fn from_json(json: &str, known: &[&str]) -> Result<Self, String> {
        let layout: Layout = serde_json::from_str(json).map_err(|e| e.to_string())?;
        layout.validate(known)?;
        Ok(layout)
    }

    /// The layout in `json`, or `fallback` when it is corrupt or names a panel
    /// that does not exist.
    pub fn from_json_or(json: &str, known: &[&str], fallback: Layout) -> Layout {
        Self::from_json(json, known).unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [&str; 5] = ["a", "b", "c", "d", "e"];

    fn sample() -> Layout {
        let mut l = Layout::new(Node::split(
            Axis::Horizontal,
            0.3,
            Node::tabs(&["a", "b"]),
            Node::split(Axis::Vertical, 0.6, Node::tabs(&["c"]), Node::tabs(&["d"])),
        ));
        l.right.panels.push("e".into());
        l
    }

    #[test]
    fn json_round_trips_with_strips() {
        let mut l = sample();
        l.toggle_strip_panel(Side::Right, "e");
        l.set_pinned(Side::Right, true);
        l.set_strip_width(Side::Right, 333.0);
        assert_eq!(Layout::from_json(&l.to_json(), &KNOWN).unwrap(), l);
    }

    #[test]
    fn corrupt_or_unknown_data_falls_back() {
        let fallback = Layout::new(Node::tabs(&["a"]));
        for bad in [
            "",
            "not json",
            "{}",
            r#"{"root":{"type":"tabs","tabs":["zzz"]}}"#,
            r#"{"root":{"type":"tabs","tabs":["a","a"]}}"#,
            r#"{"root":{"type":"tabs","tabs":["a"],"active":5}}"#,
            r#"{"root":{"type":"split","axis":"horizontal","ratio":2.0,"a":{"type":"tabs","tabs":["a"]},"b":{"type":"tabs","tabs":["b"]}}}"#,
            r#"{"root":{"type":"tabs","tabs":["a"]},"left":{"panels":["b"],"open":"c"}}"#,
        ] {
            assert_eq!(
                Layout::from_json_or(bad, &KNOWN, fallback.clone()),
                fallback,
                "{bad}"
            );
        }
    }

    #[test]
    fn a_layout_without_strips_still_loads() {
        let l = Layout::from_json(r#"{"root":{"type":"tabs","tabs":["a"]}}"#, &KNOWN).unwrap();
        assert!(l.left.panels.is_empty() && l.right.panels.is_empty());
    }

    #[test]
    fn removing_the_last_tab_collapses_the_split() {
        let mut l = sample();
        assert!(l.remove("c"));
        assert_eq!(
            l.root,
            Node::split(
                Axis::Horizontal,
                0.3,
                Node::tabs(&["a", "b"]),
                Node::tabs(&["d"])
            )
        );
        assert!(!l.remove("c"));
        l.remove("a");
        l.remove("b");
        l.remove("d");
        assert_eq!(l.root, Node::tabs(&[]));
        assert_eq!(l.panels(), ["e"]);
    }

    #[test]
    fn moving_between_tab_groups_and_splits() {
        let mut l = sample();
        assert!(l.move_panel("b", Target::Tabs { anchor: "c".into() }));
        assert_eq!(l.group_len("c"), 2);
        assert!(l.move_panel(
            "d",
            Target::Split {
                anchor: "a".into(),
                edge: Edge::Bottom
            }
        ));
        assert_eq!(l.panels().len(), 5);
        assert!(!l.move_panel(
            "a",
            Target::Tabs {
                anchor: "nope".into()
            }
        ));
        // dropping a lone panel on itself changes nothing
        let mut l = sample();
        let before = l.clone();
        assert!(!l.move_panel("c", Target::Tabs { anchor: "c".into() }));
        assert_eq!(l, before);
    }

    #[test]
    fn panels_move_between_the_dock_and_the_strips() {
        let mut l = sample();
        assert!(l.move_panel("a", Target::Strip(Side::Left)));
        assert_eq!(l.left.panels, ["a"]);
        l.toggle_strip_panel(Side::Left, "a");
        assert_eq!(l.left.open.as_deref(), Some("a"));
        assert!(l.move_panel("a", Target::Tabs { anchor: "d".into() }));
        assert!(l.left.panels.is_empty() && l.left.open.is_none());
        assert!(l.move_panel(
            "e",
            Target::Split {
                anchor: "b".into(),
                edge: Edge::Left
            }
        ));
        assert!(l.right.panels.is_empty());
        assert_eq!(l.panels().len(), 5);
    }

    #[test]
    fn pin_and_overlay_state() {
        let mut l = sample();
        l.toggle_strip_panel(Side::Right, "e");
        assert_eq!(l.right.open.as_deref(), Some("e"));
        l.close_overlays();
        assert!(l.right.open.is_none(), "unpinned closes");
        l.toggle_strip_panel(Side::Right, "e");
        l.set_pinned(Side::Right, true);
        l.close_overlays();
        assert!(l.right.open.is_some(), "pinned stays");
        l.toggle_strip_panel(Side::Right, "e");
        assert!(l.right.open.is_none());
        l.toggle_strip_panel(Side::Right, "a"); // not in the strip
        assert!(l.right.open.is_none());
        l.set_strip_width(Side::Right, 1.0);
        assert_eq!(l.right.width, MIN_WIDTH);
    }

    #[test]
    fn reveal_opens_strip_panels_and_activates_tabs() {
        let mut l = sample();
        l.reveal("e");
        assert_eq!(l.right.open.as_deref(), Some("e"));
        l.reveal("b");
        let Node::Split { a, .. } = &l.root else {
            panic!()
        };
        assert_eq!(
            **a,
            Node::Tabs {
                tabs: vec!["a".into(), "b".into()],
                active: 1
            }
        );
    }

    #[test]
    fn ratio_is_clamped_and_activation_works() {
        let mut l = sample();
        assert!(l.set_ratio(&[], 0.99));
        assert!(l.set_ratio(&[true], 0.0));
        assert!(!l.set_ratio(&[false], 0.5), "a tab group has no ratio");
        assert!(!l.set_ratio(&[], f32::NAN));
        l.activate("b");
        let Node::Split { ratio, a, .. } = &l.root else {
            panic!()
        };
        assert_eq!(*ratio, MAX_RATIO);
        assert_eq!(
            **a,
            Node::Tabs {
                tabs: vec!["a".into(), "b".into()],
                active: 1
            }
        );
    }

    #[test]
    fn moving_into_an_emptied_tree_works() {
        let mut l = Layout::new(Node::tabs(&[]));
        l.left.panels.push("a".into());
        assert!(l.move_panel(
            "a",
            Target::Tabs {
                anchor: String::new()
            }
        ));
        assert_eq!(l.root, Node::tabs(&["a"]));
    }

    #[test]
    fn a_panel_is_visible_as_the_front_tab_or_the_open_strip_panel() {
        let mut layout = Layout::new(Node::split(
            Axis::Horizontal,
            0.5,
            Node::tabs(&["a", "b"]),
            Node::tabs(&["c"]),
        ));
        layout.left.panels.push("d".into());
        layout.left.panels.push("e".into());
        assert!(layout.is_visible("a") && layout.is_visible("c"));
        assert!(!layout.is_visible("b") && !layout.is_visible("d"));
        // Selecting a tab shows it and hides the one it replaces.
        layout.show_panel("b");
        assert!(layout.is_visible("b") && !layout.is_visible("a"));
        // A strip panel opens; opening another closes the first.
        layout.show_panel("d");
        assert!(layout.is_visible("d"));
        layout.show_panel("e");
        assert!(layout.is_visible("e") && !layout.is_visible("d"));
        // A panel the layout lacks goes to the right strip, open.
        assert!(!layout.contains("f"));
        layout.show_panel("f");
        assert!(layout.right.panels.contains(&"f".to_owned()));
        assert!(layout.is_visible("f"));
        // Showing it again changes nothing.
        let before = layout.clone();
        layout.show_panel("f");
        assert_eq!(layout, before);
    }
}

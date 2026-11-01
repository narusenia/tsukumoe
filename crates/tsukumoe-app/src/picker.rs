// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Choosing a file or folder. The editor asks a [`FilePicker`]; on macOS and
//! Windows it is the OS dialog (`rfd`), elsewhere there is none and the editor
//! opens its path-input dialogs instead. Tests use a scripted picker.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// A `.tsukumoe` archive.
    OpenProject,
    /// A project folder.
    OpenFolder,
    /// Where to save a `.tsukumoe` archive.
    SaveProject,
    /// Where to save a project folder.
    SaveFolder,
    /// A `.psd` file.
    Psd,
    /// A new or empty folder for a new project.
    NewFolder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickRequest {
    pub kind: PickKind,
    pub title: String,
    /// Where the dialog starts: the last folder used.
    pub start_dir: Option<PathBuf>,
}

pub trait FilePicker {
    /// Whether this is a real dialog. When false the editor shows its own
    /// path-input dialogs and never calls [`FilePicker::pick`].
    fn is_native(&self) -> bool;

    /// Opens the dialog and waits; `None` is a cancel. It runs on the main
    /// thread. `parent` is the editor's window, to make the dialog modal.
    fn pick(&mut self, request: &PickRequest, parent: Option<&eframe::Frame>) -> Option<PathBuf>;
}

/// No OS dialog: the editor's path-input dialogs do the work.
pub struct PathInputPicker;

impl FilePicker for PathInputPicker {
    fn is_native(&self) -> bool {
        false
    }

    fn pick(&mut self, _: &PickRequest, _: Option<&eframe::Frame>) -> Option<PathBuf> {
        None
    }
}

/// The OS file dialogs of macOS and Windows.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct RfdPicker;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl FilePicker for RfdPicker {
    fn is_native(&self) -> bool {
        true
    }

    fn pick(&mut self, request: &PickRequest, parent: Option<&eframe::Frame>) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new().set_title(request.title.as_str());
        if let Some(dir) = &request.start_dir {
            dialog = dialog.set_directory(dir);
        }
        if let Some(frame) = parent {
            dialog = dialog.set_parent(frame);
        }
        match request.kind {
            PickKind::OpenProject => dialog
                .add_filter("Tsukumoe project", &["tsukumoe"])
                .pick_file(),
            PickKind::SaveProject => dialog
                .add_filter("Tsukumoe project", &["tsukumoe"])
                .set_file_name("model.tsukumoe")
                .save_file(),
            PickKind::Psd => dialog.add_filter("PSD", &["psd"]).pick_file(),
            PickKind::OpenFolder | PickKind::SaveFolder | PickKind::NewFolder => {
                dialog.pick_folder()
            }
        }
    }
}

/// The picker the editor starts with on this OS.
pub fn default_picker() -> Box<dyn FilePicker> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    return Box::new(RfdPicker);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    Box::new(PathInputPicker)
}

#[cfg(test)]
pub use fake::{Fake, FakeState};

#[cfg(test)]
mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    #[derive(Default)]
    pub struct FakeState {
        /// The answers to give, in order; an empty queue cancels.
        pub answers: VecDeque<PathBuf>,
        pub requests: Vec<PickRequest>,
    }

    /// A scripted picker; keep the handle to script it and read its requests.
    #[derive(Clone, Default)]
    pub struct Fake(pub Rc<RefCell<FakeState>>);

    impl Fake {
        pub fn answer(&self, path: impl Into<PathBuf>) {
            self.0.borrow_mut().answers.push_back(path.into());
        }
    }

    impl FilePicker for Fake {
        fn is_native(&self) -> bool {
            true
        }

        fn pick(&mut self, request: &PickRequest, _: Option<&eframe::Frame>) -> Option<PathBuf> {
            let mut s = self.0.borrow_mut();
            s.requests.push(request.clone());
            s.answers.pop_front()
        }
    }
}

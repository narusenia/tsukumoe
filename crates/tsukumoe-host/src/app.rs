// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The window: a parameter list on the left, the model on the right.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use tsukumoe_gpu::OUTPUT_FORMAT;
use tsukumoe_ui::{
    SliderEvent, SliderState, Theme, Variant, button, hint, label, panel, slider, slider_state,
};
use wgpui::{
    App, AppContext, Application, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, TitlebarOptions, WgpuSurfaceHandle, Window,
    WindowOptions, div, px, wgpu_surface,
};

use crate::viewer::{Viewer, open_project};

/// Longest step the physics is advanced in one frame, so a stall does not
/// make the model jump.
const MAX_DT: f32 = 0.1;
const SIDEBAR_WIDTH: f32 = 300.0;
const SURFACE_SIZE: u32 = 512;

/// The window's content: the viewer, or why there is none.
struct Host {
    name: String,
    running: Result<Running, String>,
}

struct Running {
    viewer: Viewer,
    surface: WgpuSurfaceHandle,
    sliders: Vec<Entity<SliderState>>,
    last_frame: Instant,
    error: Option<String>,
}

impl Running {
    fn new(viewer: Viewer, surface: WgpuSurfaceHandle, cx: &mut Context<Host>) -> Self {
        let sliders = viewer
            .scene
            .parameters()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let state = slider_state(p.min, p.max, (p.max - p.min) / 200.0, p.default, cx);
                cx.subscribe(
                    &state,
                    move |this: &mut Host, _, event: &SliderEvent, cx| {
                        if let (SliderEvent::Change(v), Ok(running)) = (event, &mut this.running) {
                            running.viewer.scene.set_parameter(i, v.start());
                            cx.notify();
                        }
                    },
                )
                .detach();
                state
            })
            .collect();
        Self {
            viewer,
            surface,
            sliders,
            last_frame: Instant::now(),
            error: None,
        }
    }

    fn draw(&mut self) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_frame)
            .as_secs_f32()
            .min(MAX_DT);
        self.last_frame = now;
        let Some((target, size)) = self.surface.back_view_with_size() else {
            return;
        };
        match self.viewer.render(
            self.surface.device(),
            self.surface.queue(),
            &target,
            size,
            dt,
        ) {
            Ok(()) => self.surface.present(),
            Err(e) => self.error = Some(e),
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::get(cx);
        let this = cx.entity();
        let name = self.name.clone();
        let running = match &mut self.running {
            Ok(running) => running,
            Err(e) => {
                return div()
                    .size_full()
                    .p(t.metrics.pad)
                    .bg(t.colors.background)
                    .child(panel(cx).child(label(e.clone(), cx)));
            }
        };
        // Physics keeps moving, so draw every frame.
        window.request_animation_frame();
        running.draw();
        let running = &*running;

        let rows: Vec<_> = running
            .viewer
            .scene
            .parameters()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                div()
                    .flex()
                    .items_center()
                    .gap(t.metrics.gap)
                    .h(t.metrics.control)
                    .child(
                        div()
                            .w(px(110.0))
                            .flex_none()
                            .truncate()
                            .child(label(p.name.clone(), cx)),
                    )
                    .child(div().flex_1().child(slider(&running.sliders[i], false, cx)))
                    .child(
                        div()
                            .w(px(40.0))
                            .flex_none()
                            .child(hint(format!("{:.2}", running.viewer.scene.value(i)), cx)),
                    )
            })
            .collect();

        let reset =
            button("reset", "Reset", Variant::Secondary, cx).on_click(move |_, window, cx| {
                this.update(cx, |host, cx| {
                    let Ok(running) = &mut host.running else {
                        return;
                    };
                    running.viewer.scene.reset();
                    for (state, p) in running
                        .sliders
                        .iter()
                        .zip(running.viewer.scene.parameters())
                    {
                        let default = p.default;
                        state.update(cx, |s, cx| s.set_value(default, window, cx));
                    }
                    cx.notify();
                });
            });

        let mut sidebar = panel(cx)
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(name, cx))
                    .child(reset),
            )
            .child(hint(format!("{} parameters", rows.len()), cx));
        if let Some(e) = &running.error {
            sidebar = sidebar.child(hint(e.clone(), cx));
        }
        let sidebar = sidebar.child(
            div()
                .id("parameters")
                .flex_1()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .children(rows),
        );

        div()
            .size_full()
            .flex()
            .gap(t.metrics.pad)
            .p(t.metrics.pad)
            .bg(t.colors.background)
            .child(sidebar)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .rounded(t.metrics.radius)
                    .overflow_hidden()
                    .child(wgpu_surface(running.surface.clone()).absolute().inset_0()),
            )
    }
}

/// Opens `path` and shows it until the window closes.
pub fn run(path: PathBuf) -> Result<(), String> {
    let project = open_project(&path)?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let failure = Rc::new(RefCell::new(None));
    let reported = failure.clone();
    Application::new().run(move |cx: &mut App| {
        tsukumoe_ui::init(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(format!("Tsukumoe - {name}").into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let running = window
                    .create_wgpu_surface(SURFACE_SIZE, SURFACE_SIZE, OUTPUT_FORMAT)
                    .ok_or_else(|| "this platform has no wgpu surface".to_string())
                    .and_then(|surface| {
                        let viewer = Viewer::new(surface.device(), surface.queue(), &project)?;
                        Ok(Running::new(viewer, surface, cx))
                    });
                if let Err(e) = &running {
                    *reported.borrow_mut() = Some(e.clone());
                }
                Host { name, running }
            })
        });
        if let Err(e) = opened {
            *reported.borrow_mut() = Some(e.to_string());
            cx.quit();
        }
    });
    let failure = failure.borrow_mut().take();
    failure.map_or(Ok(()), Err)
}

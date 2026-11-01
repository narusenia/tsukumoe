// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The window: a parameter list on the left, the model on the right.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Vec2};
use tsukumoe_tracking::VmcReceiver;
use tsukumoe_ui::{
    Arg, Density, Language, Mode, NamedTheme, Preferences, Theme, Tone, Variant, badge, button,
    checkbox, hint, label, panel, section_header, select, slider, switch, text_input, tr, tr_args,
};

use crate::live::{Live, Status};
use crate::options::Options;
use crate::output::Output;
#[cfg(any(
    all(feature = "syphon", target_os = "macos"),
    all(feature = "spout", windows)
))]
use crate::output::SinkStatus;
#[cfg(all(feature = "spout", windows))]
use crate::output::SpoutSink;
#[cfg(all(feature = "syphon", target_os = "macos"))]
use crate::output::SyphonSink;
use crate::settings;
use crate::viewer::{Viewer, open_project};

/// Pointer travel, in points, that moves a parameter across its whole range.
const DRAG_POINTS: f32 = 400.0;
/// How long to keep drawing after a change, so physics can settle. Drawing
/// stops after that: the host redraws only while something moves.
const SETTLE: Duration = Duration::from_secs(3);
const SIDEBAR_WIDTH: f32 = 300.0;
const READOUT_WIDTH: f32 = 40.0;
/// Smoothing time constants, in seconds, and the blendshape gain.
const SMOOTHING: std::ops::RangeInclusive<f32> = 0.0..=0.3;
const SENSITIVITY: std::ops::RangeInclusive<f32> = 0.5..=2.0;

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    id: egui::TextureId,
    size: (u32, u32),
}

struct Host {
    name: String,
    viewer: Viewer,
    target: Option<Target>,
    last_frame: Instant,
    /// Draw until this time.
    active_until: Instant,
    error: Option<String>,
    prefs: Preferences,
    themes: Vec<NamedTheme>,
    /// Where preferences are saved; `None` when the environment has no home.
    config: Option<PathBuf>,
    live: Live,
    started: Instant,
    port: String,
    window: WindowMode,
    /// What the window currently is, to send commands only on a change.
    applied: WindowMode,
    show_panel: bool,
    output: Output,
    /// The Syphon server name; editable while the output is off.
    #[cfg(all(feature = "syphon", target_os = "macos"))]
    syphon_name: String,
    /// The Spout sender name; editable while the output is off.
    #[cfg(all(feature = "spout", windows))]
    spout_name: String,
}

/// The window's look. `transparent` clears the background to alpha 0 and drops
/// the opaque panel behind the model; the other two apply only then.
#[derive(Clone, Copy, PartialEq)]
struct WindowMode {
    transparent: bool,
    borderless: bool,
    on_top: bool,
}

impl Host {
    fn wake(&mut self) {
        self.active_until = Instant::now() + SETTLE;
    }

    /// The theme, density and language selectors; a change applies at once and
    /// is saved.
    fn appearance(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let text = |key: &str| tr(&ctx, key);
        let modes = [
            ("mode:dark", text("theme-dark")),
            ("mode:light", text("theme-light")),
            ("mode:system", text("theme-system")),
        ];
        let mut options: Vec<(String, &str)> = modes
            .iter()
            .map(|(key, name)| ((*key).to_owned(), name.as_str()))
            .collect();
        options.extend(
            self.themes
                .iter()
                .map(|t| (format!("theme:{}", t.name), t.name.as_str())),
        );
        let named = self
            .prefs
            .theme
            .as_ref()
            .filter(|name| self.themes.iter().any(|t| &t.name == *name));
        let mut choice = match (named, self.prefs.mode) {
            (Some(name), _) => format!("theme:{name}"),
            (None, Mode::Dark) => "mode:dark".to_owned(),
            (None, Mode::Light) => "mode:light".to_owned(),
            (None, Mode::System) => "mode:system".to_owned(),
        };
        let densities = [
            (Density::Compact, text("density-compact")),
            (Density::Regular, text("density-regular")),
        ];
        let density_options: Vec<(Density, &str)> =
            densities.iter().map(|(d, n)| (*d, n.as_str())).collect();
        let auto = text("language-auto");
        let mut language_options = vec![(None, auto.as_str())];
        language_options.extend(Language::ALL.map(|l| (Some(l), l.native_name())));
        let mut density = self.prefs.density;
        let mut language = self.prefs.language;
        let (theme_changed, density_changed, language_changed) = ui
            .horizontal_wrapped(|ui| {
                let theme = select(ui, "theme", &text("host-theme"), &mut choice, &options);
                let dens = select(
                    ui,
                    "density",
                    &text("host-density"),
                    &mut density,
                    &density_options,
                );
                let lang = select(
                    ui,
                    "language",
                    &text("host-language"),
                    &mut language,
                    &language_options,
                );
                (theme.changed(), dens.changed(), lang.changed())
            })
            .inner;
        if theme_changed {
            match choice.split_once(':') {
                Some(("theme", name)) => self.prefs.theme = Some(name.to_owned()),
                Some((_, "light")) => (self.prefs.theme, self.prefs.mode) = (None, Mode::Light),
                Some((_, "system")) => (self.prefs.theme, self.prefs.mode) = (None, Mode::System),
                _ => (self.prefs.theme, self.prefs.mode) = (None, Mode::Dark),
            }
        }
        if density_changed {
            self.prefs.density = density;
        }
        if language_changed {
            self.prefs.language = language;
        }
        if theme_changed || density_changed || language_changed {
            self.prefs.apply(&ctx, &self.themes);
            if let Some(dir) = &self.config
                && let Err(e) = settings::write_preferences(dir, &self.prefs)
            {
                self.error = Some(e);
            }
        }
    }

    /// Sends the window commands for a changed `window`.
    fn apply_window(&mut self, ctx: &egui::Context) {
        use egui::{ViewportCommand as Cmd, WindowLevel};
        let (old, new) = (self.applied, self.window);
        if old == new {
            return;
        }
        let borderless = new.transparent && new.borderless;
        if borderless != (old.transparent && old.borderless) {
            ctx.send_viewport_cmd(Cmd::Decorations(!borderless));
        }
        let top = new.transparent && new.on_top;
        if top != (old.transparent && old.on_top) {
            ctx.send_viewport_cmd(Cmd::WindowLevel(if top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            }));
        }
        self.applied = new;
        self.wake();
    }

    /// Starts or stops the VMC receiver; a bind failure is shown in the panel.
    fn set_tracking(&mut self, ctx: &egui::Context, on: bool) {
        if !on {
            self.live.detach();
            return;
        }
        match self.port.trim().parse::<u16>() {
            Ok(port) if port != 0 => match VmcReceiver::bind(("0.0.0.0", port)) {
                Ok(rx) => {
                    self.live.attach(Box::new(rx));
                    self.error = None;
                }
                Err(e) => self.error = Some(format!("UDP {port}: {e}")),
            },
            _ => self.error = Some(tr(ctx, "live-port-invalid")),
        }
    }

    /// Starts or stops the Syphon output; a failure is shown in the panel.
    #[cfg(all(feature = "syphon", target_os = "macos"))]
    fn set_syphon(&mut self, on: bool) {
        if !on {
            self.output.detach();
            return;
        }
        match SyphonSink::new(self.syphon_name.trim()) {
            Ok(sink) => {
                self.output.attach(Box::new(sink));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    #[cfg(all(feature = "syphon", target_os = "macos"))]
    fn syphon_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let text = |key: &str| tr(&ctx, key);
        section_header(ui, &text("syphon-title"));
        ui.horizontal(|ui| {
            let mut on = self.output.enabled();
            if switch(ui, &mut on, &text("syphon-title")).changed() {
                self.set_syphon(on);
            }
            if let Some(sink) = self.output.sink() {
                let (key, tone) = match sink.status() {
                    SinkStatus::NoClients => ("syphon-status-no-clients", Tone::Warning),
                    SinkStatus::Publishing => ("syphon-status-publishing", Tone::Success),
                };
                badge(ui, &text(key), tone);
            }
        });
        hint(ui, text("syphon-name"));
        ui.add_enabled_ui(!self.output.enabled(), |ui| {
            text_input(ui, &mut self.syphon_name, &text("syphon-name"), "Tsukumoe");
        });
    }

    /// Starts or stops the Spout output; a failure is shown in the panel.
    #[cfg(all(feature = "spout", windows))]
    fn set_spout(&mut self, on: bool) {
        if !on {
            self.output.detach();
            return;
        }
        match SpoutSink::new(self.spout_name.trim()) {
            Ok(sink) => {
                self.output.attach(Box::new(sink));
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    #[cfg(all(feature = "spout", windows))]
    fn spout_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let text = |key: &str| tr(&ctx, key);
        section_header(ui, &text("spout-title"));
        ui.horizontal(|ui| {
            let mut on = self.output.enabled();
            if switch(ui, &mut on, &text("spout-title")).changed() {
                self.set_spout(on);
            }
            if let Some(sink) = self.output.sink() {
                let (key, tone) = match sink.status() {
                    SinkStatus::NoClients => ("spout-status-no-clients", Tone::Warning),
                    SinkStatus::Publishing => ("spout-status-publishing", Tone::Success),
                };
                badge(ui, &text(key), tone);
            }
        });
        hint(ui, text("spout-name"));
        ui.add_enabled_ui(!self.output.enabled(), |ui| {
            text_input(ui, &mut self.spout_name, &text("spout-name"), "Tsukumoe");
        });
    }

    fn live_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let text = |key: &str| tr(&ctx, key);
        section_header(ui, &text("live-tracking"));
        ui.horizontal(|ui| {
            let mut on = self.live.enabled();
            if switch(ui, &mut on, &text("live-tracking")).changed() {
                self.set_tracking(&ctx, on);
            }
            let (key, tone) = match self.live.status() {
                Status::Off => ("live-status-off", Tone::Neutral),
                Status::Waiting => ("live-status-waiting", Tone::Warning),
                Status::Receiving => ("live-status-receiving", Tone::Success),
                Status::Dropped => ("live-status-dropped", Tone::Error),
            };
            badge(ui, &text(key), tone);
        });
        hint(ui, text("live-port"));
        ui.add_enabled_ui(!self.live.enabled(), |ui| {
            text_input(ui, &mut self.port, &text("live-port"), "39539");
        });
        for (key, range, get, set) in [
            (
                "live-smoothing",
                SMOOTHING,
                Live::smoothing as fn(&Live) -> f32,
                Live::set_smoothing as fn(&mut Live, f32),
            ),
            (
                "live-sensitivity",
                SENSITIVITY,
                Live::sensitivity,
                Live::set_sensitivity,
            ),
        ] {
            let mut value = get(&self.live);
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(110.0, 20.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| label(ui, text(key)),
                );
                let width = (ui.available_width() - READOUT_WIDTH).max(40.0);
                let changed = ui
                    .allocate_ui(Vec2::new(width, 20.0), |ui| {
                        slider(ui, &mut value, range, &text(key)).changed()
                    })
                    .inner;
                hint(ui, format!("{value:.2}"));
                if changed {
                    set(&mut self.live, value);
                }
            });
        }
        hint(
            ui,
            tr_args(
                &ctx,
                "live-mapping",
                &[
                    ("bound", Arg::from(self.live.bound())),
                    ("skipped", Arg::from(self.live.skipped())),
                ],
            ),
        );
        checkbox(ui, &mut self.window.transparent, &text("live-transparent"));
        if self.window.transparent {
            checkbox(ui, &mut self.window.borderless, &text("live-borderless"));
            checkbox(ui, &mut self.window.on_top, &text("live-on-top"));
        }
        hint(ui, text("live-hint-panel"));
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = Theme::get(ui.ctx());
        panel(ui, |ui| {
            ui.set_width(SIDEBAR_WIDTH - 2.0 * t.metrics.pad);
            ui.horizontal(|ui| {
                label(ui, self.name.clone());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, &tr(ui.ctx(), "host-reset"), Variant::Secondary).clicked() {
                        self.viewer.scene.reset();
                        self.wake();
                    }
                });
            });
            let count = self.viewer.scene.parameters().len();
            hint(
                ui,
                tr_args(ui.ctx(), "host-parameters", &[("count", Arg::from(count))]),
            );
            hint(ui, tr(ui.ctx(), "host-hint-drive"));
            self.live_panel(ui);
            #[cfg(all(feature = "syphon", target_os = "macos"))]
            self.syphon_panel(ui);
            #[cfg(all(feature = "spout", windows))]
            self.spout_panel(ui);
            self.appearance(ui);
            if let Some(e) = &self.error {
                hint(ui, e.clone());
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for i in 0..self.viewer.scene.parameters().len() {
                        let p = self.viewer.scene.parameters()[i].clone();
                        let mut value = self.viewer.scene.value(i);
                        let driven = self.live.is_driven(i);
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(110.0, t.metrics.control),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| ui.add(egui::Label::new(&p.name).truncate()),
                            );
                            let width = (ui.available_width() - READOUT_WIDTH).max(40.0);
                            let changed = ui
                                .allocate_ui(Vec2::new(width, t.metrics.control), |ui| {
                                    // Read-only while the tracking drives it.
                                    ui.add_enabled_ui(!driven, |ui| {
                                        slider(ui, &mut value, p.min..=p.max, &p.name).changed()
                                    })
                                    .inner
                                })
                                .inner;
                            hint(ui, format!("{value:.2}"));
                            if changed {
                                self.viewer.scene.set_parameter(i, value);
                                self.wake();
                            }
                        });
                    }
                });
        });
    }

    /// Makes the render target match `size` (pixels), registering it with egui.
    fn ensure_target(&mut self, frame: &eframe::Frame, size: (u32, u32)) {
        if self.target.as_ref().map(|t| t.size) == Some(size) {
            return;
        }
        let rs = frame
            .wgpu_render_state()
            .expect("the wgpu renderer is selected");
        let mut renderer = rs.renderer.write();
        if let Some(old) = self.target.take() {
            renderer.free_texture(&old.id);
        }
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: tsukumoe_gpu::OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear);
        self.target = Some(Target {
            texture,
            view,
            id,
            size,
        });
        // A new target is empty: draw it.
        self.active_until = self.active_until.max(Instant::now());
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let avail = ui.available_size();
        let ppp = ui.ctx().pixels_per_point();
        let size = (
            ((avail.x * ppp) as u32).max(1),
            ((avail.y * ppp) as u32).max(1),
        );
        let resized = self.target.as_ref().map(|t| t.size) != Some(size);
        self.ensure_target(frame, size);
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::hover());

        if response.hovered() && ui.input(|i| i.key_down(egui::Key::Space)) {
            let delta = ui.input(|i| i.pointer.delta());
            // Right is +x and up is +y, as on a slider and in the model.
            if self
                .viewer
                .scene
                .drive(delta.x / DRAG_POINTS, -delta.y / DRAG_POINTS)
            {
                self.wake();
            }
        }

        let now = Instant::now();
        if resized || now < self.active_until {
            let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
            let rs = frame
                .wgpu_render_state()
                .expect("the wgpu renderer is selected");
            let target = self.target.as_ref().expect("target was just ensured");
            match self
                .viewer
                .render(&rs.device, &rs.queue, &target.view, size, dt)
            {
                Ok(()) => {
                    self.error = None;
                    // The output gets the same frame the window just drew.
                    let viewer = &mut self.viewer;
                    if let Err(e) =
                        self.output
                            .publish(&rs.device, &rs.queue, &target.texture, |view, size| {
                                viewer.draw(&rs.device, &rs.queue, view, size)
                            })
                    {
                        self.error = Some(e);
                        self.output.detach();
                    }
                }
                Err(e) => self.error = Some(e),
            }
            ui.ctx().request_repaint();
        }
        self.last_frame = now;
        if let Some(target) = &self.target {
            ui.painter().image(
                target.id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

impl eframe::App for Host {
    /// Transparent: the surface is cleared to alpha 0 and nothing paints behind
    /// the model. The renderer's output is premultiplied alpha, which is also
    /// what egui composites, so the image goes through unchanged.
    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.window.transparent {
            [0.0; 4]
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Theme {
            colors, metrics, ..
        } = Theme::get(ui.ctx());
        let (background, pad) = (colors.background, metrics.pad);
        self.apply_window(ui.ctx());
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_panel = !self.show_panel;
        }
        if self
            .live
            .step(self.started.elapsed().as_secs_f64(), &mut self.viewer.scene)
        {
            self.wake();
        }
        // Waiting for input also needs frames, to notice the first packet.
        // A receiver of the output needs a steady stream of frames too.
        if self.live.enabled() || self.output.enabled() {
            self.wake();
        }
        if self.show_panel {
            egui::Panel::left("parameters")
                .resizable(false)
                .exact_size(SIDEBAR_WIDTH)
                .frame(egui::Frame::NONE.fill(background).inner_margin(pad))
                .show(ui, |ui| self.sidebar(ui));
        }
        let fill = if self.window.transparent {
            egui::Color32::TRANSPARENT
        } else {
            background
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(fill))
            .show(ui, |ui| self.viewport(ui, frame));
    }
}

/// Opens `path` and shows it until the window closes.
pub fn run(mut options: Options) -> Result<(), String> {
    let path = options.project.clone();
    let project = open_project(&path)?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Tsukumoe - {name}"))
            .with_inner_size([1200.0, 800.0])
            // The surface must be created with alpha to be made transparent later.
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "tsukumoe-host",
        native,
        Box::new(move |cc| {
            tsukumoe_ui::init(&cc.egui_ctx);
            let config = settings::config_dir();
            let (themes, theme_errors) = config.as_deref().map_or_else(
                || (tsukumoe_ui::builtin_themes(), Vec::new()),
                settings::load_themes,
            );
            let prefs = config
                .as_deref()
                .map(settings::read_preferences)
                .unwrap_or_default();
            prefs.apply(&cc.egui_ctx, &themes);
            let rs = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("no wgpu render state")?;
            let viewer = Viewer::new(&rs.device, &rs.queue, &project)?;
            let live = Live::new(&viewer.scene, options.mapping.take());
            let port = options
                .vmc_port
                .map_or_else(|| "39539".to_owned(), |p| p.to_string());
            let window = WindowMode {
                transparent: options.transparent,
                borderless: false,
                on_top: false,
            };
            let mut host = Host {
                name,
                viewer,
                target: None,
                last_frame: Instant::now(),
                active_until: Instant::now() + SETTLE,
                error: (!theme_errors.is_empty()).then(|| theme_errors.join("\n")),
                prefs,
                themes,
                config,
                live,
                started: Instant::now(),
                port,
                window,
                applied: window,
                show_panel: true,
                output: Output::new(options.syphon_size.or(options.spout_size)),
                #[cfg(all(feature = "syphon", target_os = "macos"))]
                syphon_name: options.syphon_name.clone(),
                #[cfg(all(feature = "spout", windows))]
                spout_name: options.spout_name.clone(),
            };
            #[cfg(all(feature = "syphon", target_os = "macos"))]
            if options.syphon {
                host.set_syphon(true);
                if let Some(e) = host.error.take() {
                    return Err(e.into());
                }
            }
            #[cfg(all(feature = "spout", windows))]
            if options.spout {
                host.set_spout(true);
                if let Some(e) = host.error.take() {
                    return Err(e.into());
                }
            }
            if options.vmc_port.is_some() {
                host.set_tracking(&cc.egui_ctx, true);
                if let Some(e) = host.error.take() {
                    return Err(e.into());
                }
            }
            Ok(Box::new(host))
        }),
    )
    .map_err(|e| e.to_string())
}

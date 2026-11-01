// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tracking wired to the model: adapter -> postprocess -> mapper ->
//! `Scene::set_parameter`. Knows nothing about the UI toolkit; the adapter is a
//! trait object, so tests run with a fake one.

use tsukumoe_tracking::{
    Mapper, PostConfig, Postprocess, Rule, TrackingAdapter, TrackingFrame, default_rules,
};

use crate::viewer::Scene;

/// What the receiver is doing, for the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Tracking is off.
    Off,
    /// On, but nothing has arrived yet.
    Waiting,
    Receiving,
    /// Input stopped; the pose is returning to default.
    Dropped,
}

pub struct Live {
    adapter: Option<Box<dyn TrackingAdapter>>,
    post: Postprocess,
    mapper: Mapper,
    skipped: usize,
    out: Vec<(usize, f32)>,
    /// Parameters the tracking has written, so the UI can lock their sliders.
    driven: Vec<bool>,
    received: bool,
}

impl Live {
    /// Binds `rules` (the defaults when `None`) to the scene's parameter names.
    pub fn new(scene: &Scene, rules: Option<Vec<Rule>>) -> Self {
        let names: Vec<&str> = scene.parameters().iter().map(|p| p.name.as_str()).collect();
        let (mapper, skipped) = Mapper::bind(&rules.unwrap_or_else(default_rules), &names);
        Self {
            adapter: None,
            post: Postprocess::new(PostConfig::default()),
            out: Vec::with_capacity(mapper.len()),
            driven: vec![false; names.len()],
            mapper,
            skipped,
            received: false,
        }
    }

    /// Starts tracking from `adapter`, replacing any previous one. The pose
    /// starts fresh; the smoothing and sensitivity settings are kept.
    pub fn attach(&mut self, adapter: Box<dyn TrackingAdapter>) {
        let config = self.post.config.clone();
        self.post = Postprocess::new(config);
        self.adapter = Some(adapter);
        self.received = false;
    }

    /// Stops tracking. The scene keeps its last pose and the sliders work again.
    pub fn detach(&mut self) {
        self.adapter = None;
        self.driven.fill(false);
    }

    pub fn enabled(&self) -> bool {
        self.adapter.is_some()
    }

    pub fn status(&self) -> Status {
        match (self.enabled(), self.received, self.post.is_dropped()) {
            (false, ..) => Status::Off,
            (_, false, _) => Status::Waiting,
            (_, true, false) => Status::Receiving,
            (_, true, true) => Status::Dropped,
        }
    }

    /// Parameters the rules bound / rules that matched nothing in this model.
    pub fn bound(&self) -> usize {
        self.mapper.len()
    }

    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// Whether tracking currently owns parameter `index`.
    pub fn is_driven(&self, index: usize) -> bool {
        self.driven.get(index).copied().unwrap_or(false)
    }

    /// Smoothing time constant in seconds, for blendshapes and the head. Takes
    /// effect at the next `step`.
    pub fn set_smoothing(&mut self, tau: f32) {
        self.post.config.blend_tau.fill(tau);
        self.post.config.head_tau = tau;
    }

    /// Blendshape gain. Takes effect on the next received frame.
    pub fn set_sensitivity(&mut self, gain: f32) {
        self.post.config.gain.fill(gain);
    }

    pub fn smoothing(&self) -> f32 {
        self.post.config.head_tau
    }

    pub fn sensitivity(&self) -> f32 {
        self.post.config.gain[0]
    }

    /// Once per frame, `now` in seconds on any monotonic clock. Writes the
    /// mapped parameters into `scene`; returns whether it did. Does nothing
    /// when tracking is off or before the first input.
    pub fn step(&mut self, now: f64, scene: &mut Scene) -> bool {
        let Some(adapter) = &self.adapter else {
            return false;
        };
        let latest: Option<TrackingFrame> = adapter.latest();
        if let Some(frame) = latest {
            self.post.push(&frame, now);
            self.received = true;
        }
        if !self.received {
            return false;
        }
        let frame = self.post.sample(now);
        self.mapper.apply(&frame, &mut self.out);
        for &(index, value) in &self.out {
            scene.set_parameter(index, value);
            self.driven[index] = true;
        }
        !self.out.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use tsukumoe_tracking::arkit_index;

    use super::*;
    use crate::viewer::open_project;

    /// Hands out the frame set on it, once.
    #[derive(Clone, Default)]
    struct Fake(Rc<Cell<Option<TrackingFrame>>>);

    impl TrackingAdapter for Fake {
        fn latest(&self) -> Option<TrackingFrame> {
            self.0.take()
        }
    }

    fn scene() -> Scene {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tsukumoe-format/tests/fixtures/minimal");
        let mut project = open_project(&dir).unwrap();
        let model = r#"{"parameters":[
            {"name":"ParamAngleX","min":-30.0,"max":30.0,"default":0.0},
            {"name":"ParamMouthOpenY","min":0.0,"max":1.0,"default":0.0},
            {"name":"Unmapped","min":0.0,"max":1.0,"default":0.5}],
            "parts":[{"name":"Face"}]}"#;
        project.files.insert("model.json".into(), model.into());
        project
            .files
            .retain(|path, _| !path.starts_with("motions/") && !path.starts_with("expressions/"));
        Scene::new(&project).unwrap()
    }

    fn jaw(v: f32) -> TrackingFrame {
        let mut f = TrackingFrame::default();
        f.blend[arkit_index("jawOpen").unwrap()] = v;
        f
    }

    fn live(scene: &Scene) -> (Live, Fake) {
        let fake = Fake::default();
        let mut live = Live::new(scene, None);
        live.attach(Box::new(fake.clone()));
        live.set_smoothing(0.0);
        (live, fake)
    }

    #[test]
    fn jaw_open_raises_the_mouth_parameter() {
        let mut scene = scene();
        let (mut live, fake) = live(&scene);
        assert_eq!(live.status(), Status::Waiting);
        assert!(!live.step(0.0, &mut scene), "nothing received yet");
        fake.0.set(Some(jaw(1.0)));
        assert!(live.step(0.1, &mut scene));
        assert_eq!(scene.value(1), 1.0);
        assert_eq!(scene.value(2), 0.5, "unmapped parameter is untouched");
        assert!(live.is_driven(1) && !live.is_driven(2));
        assert_eq!(live.status(), Status::Receiving);
    }

    #[test]
    fn dropout_returns_to_default_and_input_resumes_following() {
        let mut scene = scene();
        let (mut live, fake) = live(&scene);
        fake.0.set(Some(jaw(1.0)));
        live.step(0.0, &mut scene);
        assert_eq!(scene.value(1), 1.0);
        // Past the timeout (0.5 s) and the recovery ramp (0.3 s).
        live.step(0.6, &mut scene);
        assert_eq!(live.status(), Status::Dropped);
        live.step(2.0, &mut scene);
        assert_eq!(scene.value(1), 0.0);
        fake.0.set(Some(jaw(0.5)));
        live.step(2.1, &mut scene);
        assert_eq!(live.status(), Status::Receiving);
        assert_eq!(scene.value(1), 0.5);
    }

    #[test]
    fn disabled_tracking_does_not_touch_the_scene() {
        let mut scene = scene();
        let (mut live, fake) = live(&scene);
        live.detach();
        fake.0.set(Some(jaw(1.0)));
        assert!(!live.step(0.0, &mut scene));
        assert_eq!(scene.value(1), 0.0);
        assert_eq!(live.status(), Status::Off);
        assert!(!live.is_driven(1));
        let mut never = Live::new(&scene, None);
        assert!(!never.step(0.0, &mut scene));
    }

    #[test]
    fn smoothing_and_sensitivity_apply_from_the_next_frame() {
        let mut scene = scene();
        let (mut live, fake) = live(&scene);
        fake.0.set(Some(jaw(0.0)));
        live.step(0.0, &mut scene);
        live.set_smoothing(0.5);
        fake.0.set(Some(jaw(1.0)));
        live.step(0.05, &mut scene);
        let smoothed = scene.value(1);
        assert!(smoothed > 0.0 && smoothed < 0.5, "{smoothed}");
        live.set_smoothing(0.0);
        live.set_sensitivity(0.5);
        fake.0.set(Some(jaw(1.0)));
        live.step(0.1, &mut scene);
        assert_eq!(scene.value(1), 0.5);
    }

    #[test]
    fn unbound_rules_are_counted() {
        let scene = scene();
        let live = Live::new(&scene, None);
        assert_eq!(live.bound(), 2);
        assert_eq!(live.skipped(), default_rules().len() - 2);
    }

    #[test]
    fn a_vmc_packet_over_local_udp_moves_the_scene() {
        use std::net::UdpSocket;
        use std::time::Duration;
        use tsukumoe_tracking::VmcReceiver;

        let mut scene = scene();
        let rx = VmcReceiver::bind("127.0.0.1:0").unwrap();
        let dst = rx.local_addr();
        let mut live = Live::new(&scene, None);
        live.attach(Box::new(rx));
        live.set_smoothing(0.0);
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let pad = |s: &str| {
            let mut b = s.as_bytes().to_vec();
            b.resize((b.len() / 4 + 1) * 4, 0);
            b
        };
        let message = |addr: &str, tags: &str, args: &[u8]| {
            let mut m = [pad(addr), pad(tags)].concat();
            m.extend_from_slice(args);
            m
        };
        let mut args = pad("jawOpen");
        args.extend_from_slice(&1.0f32.to_be_bytes());
        let mut packet = [pad("#bundle"), vec![0; 8]].concat();
        for m in [
            message("/VMC/Ext/Blend/Val", ",sf", &args),
            message("/VMC/Ext/Blend/Apply", ",", &[]),
        ] {
            packet.extend_from_slice(&(m.len() as i32).to_be_bytes());
            packet.extend(m);
        }
        tx.send_to(&packet, dst).unwrap();
        for i in 0..200 {
            if live.step(f64::from(i) * 0.01, &mut scene) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(scene.value(1), 1.0);
    }
}

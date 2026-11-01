// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Spout2 sink. spout-rs sends OpenGL textures or CPU pixels, and wgpu's
//! default Windows backend (DX12) has no GL textures, so each frame is read
//! back and sent as RGBA pixels (see `host-spout.md`). Needs `Spout.dll` at
//! run time.

use spout_rs::SpoutSender;

use super::readback::Readback;
use super::{FrameSink, SinkStatus};

pub struct SpoutSink {
    name: String,
    sender: SpoutSender,
    readback: Readback,
}

impl SpoutSink {
    pub fn new(name: &str) -> Result<Self, String> {
        Ok(Self {
            name: name.to_owned(),
            sender: SpoutSender::new(name),
            readback: Readback::default(),
        })
    }
}

impl FrameSink for SpoutSink {
    fn name(&self) -> &str {
        &self.name
    }

    /// shortcut: spout-rs cannot count receivers, so this is `Publishing` as
    /// soon as the sender exists, never `NoClients`.
    fn status(&self) -> SinkStatus {
        SinkStatus::Publishing
    }

    fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::Texture,
    ) -> Result<(), String> {
        let (w, h) = (frame.width(), frame.height());
        let pixels = self.readback.read(device, queue, frame)?;
        if self.sender.send_image_rgba(pixels, w, h) {
            Ok(())
        } else {
            Err(format!("Spout: could not send a {w}x{h} frame"))
        }
    }
}

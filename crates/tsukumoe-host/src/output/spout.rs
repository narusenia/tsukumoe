// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The Spout2 sink. spout-rs's `SpoutSender` needs an OpenGL context, which
//! the host (wgpu on DX12) does not have, so frames take another route: each
//! one is read back to the CPU, converted to BGRA, uploaded into a legacy
//! shared D3D11 texture, and that texture's share handle is registered with
//! Spout through `SpoutDirectSender` (see `host-spout.md`).

use std::os::windows::io::{FromRawHandle, OwnedHandle};

use spout_rs::{DirectSharedTexture, SpoutDirectSender};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_RESOURCE_MISC_SHARED,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIResource;
use windows::core::Interface;

use super::readback::Readback;
use super::{FrameSink, SinkStatus};

/// Spout's own default format, which every receiver handles.
const DXGI_FORMAT: u32 = DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32;

pub struct SpoutSink {
    name: String,
    readback: Readback,
    bgra: Vec<u8>,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// The shared texture, the size it was made at, and the sender publishing it.
    published: Option<(ID3D11Texture2D, (u32, u32), SpoutDirectSender)>,
}

impl SpoutSink {
    /// Fails when no D3D11 device can be created. The sender itself is
    /// registered at the first frame, once the size is known.
    pub fn new(name: &str) -> Result<Self, String> {
        let (mut device, mut context) = (None, None);
        // SAFETY: plain device creation; the out-pointers outlive the call.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .map_err(|e| format!("Spout: could not create a D3D11 device: {e}"))?;
        let (Some(device), Some(context)) = (device, context) else {
            return Err("Spout: D3D11 returned no device".into());
        };
        Ok(Self {
            name: name.to_owned(),
            readback: Readback::default(),
            bgra: Vec::new(),
            device,
            context,
            published: None,
        })
    }
}

/// A shared BGRA texture of this size and the handle Spout publishes.
fn create_texture(
    device: &ID3D11Device,
    (w, h): (u32, u32),
) -> Result<(ID3D11Texture2D, DirectSharedTexture), String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_SHARED.0 as u32,
    };
    let fail = |e: windows::core::Error| format!("Spout: could not make a {w}x{h} texture: {e}");
    let mut texture = None;
    // SAFETY: `desc` is fully initialised and the out-pointer outlives the call.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(fail)?;
    let texture = texture.ok_or("Spout: D3D11 returned no texture")?;
    // SAFETY: a texture made with MISC_SHARED supports `GetSharedHandle`.
    let handle = unsafe {
        texture
            .cast::<IDXGIResource>()
            .map_err(fail)?
            .GetSharedHandle()
    }
    .map_err(fail)?;
    // shortcut: this is a legacy (KMT) share handle, which is not a real kernel
    // handle, but DirectSharedTexture only takes an OwnedHandle, so dropping it
    // runs a CloseHandle that fails harmlessly (it raises only under a debugger
    // or Application Verifier); switch if spout-rs gains a raw-handle constructor.
    // SAFETY: the value is never used as an owned handle beyond that close.
    let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let shared = DirectSharedTexture::from_owned_handle(owned, w, h, DXGI_FORMAT)
        .ok_or("Spout: the shared texture has no handle")?;
    Ok((texture, shared))
}

impl FrameSink for SpoutSink {
    fn name(&self) -> &str {
        &self.name
    }

    /// shortcut: spout-rs cannot count receivers, so this is `Publishing` as
    /// soon as the sink exists, never `NoClients`.
    fn status(&self) -> SinkStatus {
        SinkStatus::Publishing
    }

    fn publish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &wgpu::Texture,
    ) -> Result<(), String> {
        let size = (frame.width(), frame.height());
        let rgba = self.readback.read(device, queue, frame)?;
        // shortcut: a plain per-pixel swap on the CPU; fine at stream sizes, move
        // it to a GPU pass if it shows in profiles.
        self.bgra.clear();
        self.bgra
            .extend(rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]));
        if self.published.as_ref().is_none_or(|p| p.1 != size) {
            let (texture, shared) = create_texture(&self.device, size)?;
            let updated = match self.published.take() {
                Some((_, _, mut sender)) => sender.update(shared).then_some(sender),
                None => None,
            };
            let (texture, sender) = match updated {
                Some(sender) => (texture, sender),
                // The old sender is gone or refused the update: register anew
                // with a fresh texture (`update` consumed the first one).
                None => {
                    let (texture, shared) = create_texture(&self.device, size)?;
                    let sender = SpoutDirectSender::new(&self.name, shared).ok_or_else(|| {
                        format!("Spout: could not register the sender '{}'", self.name)
                    })?;
                    (texture, sender)
                }
            };
            self.published = Some((texture, size, sender));
        }
        let (texture, _, _) = self.published.as_ref().expect("made above");
        // SAFETY: `bgra` holds `width * 4` bytes for each of `height` rows, and
        // the texture is `width x height` BGRA.
        unsafe {
            self.context.UpdateSubresource(
                texture,
                0,
                None,
                self.bgra.as_ptr().cast(),
                size.0 * 4,
                0,
            );
            self.context.Flush();
        }
        Ok(())
    }
}

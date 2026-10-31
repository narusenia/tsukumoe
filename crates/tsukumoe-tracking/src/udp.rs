// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Socket layer: a receiver thread decodes VMC datagrams and publishes only
//! the newest frame. Readers poll; there is no queue, late input is dropped.

use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::frame::TrackingFrame;
use crate::osc::MAX_PACKET_LEN;
use crate::vmc::{DecodeStats, VmcDecoder};

/// A source of tracking frames (VMC today; iFacialMocap / MediaPipe later).
pub trait TrackingAdapter {
    /// Takes the newest frame published since the last call, if any.
    fn latest(&self) -> Option<TrackingFrame>;
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Option<TrackingFrame>>,
    stats: Mutex<DecodeStats>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// VMC receiver over UDP. Dropping it stops and joins the thread.
pub struct VmcReceiver {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    addr: SocketAddr,
    thread: Option<JoinHandle<()>>,
}

impl VmcReceiver {
    /// Binds `addr` (use port 0 for an OS-assigned port) and starts receiving.
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_read_timeout(Some(Duration::from_millis(50)))?;
        let addr = socket.local_addr()?;
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            std::thread::Builder::new()
                .name("tsukumoe-vmc-rx".into())
                .spawn(move || run(&socket, &shared, &stop))?
        };
        Ok(Self {
            shared,
            stop,
            addr,
            thread: Some(thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn stats(&self) -> DecodeStats {
        *lock(&self.shared.stats)
    }
}

impl TrackingAdapter for VmcReceiver {
    fn latest(&self) -> Option<TrackingFrame> {
        lock(&self.shared.slot).take()
    }
}

impl Drop for VmcReceiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(socket: &UdpSocket, shared: &Shared, stop: &AtomicBool) {
    // One byte over the limit so oversize datagrams are detectable.
    let mut buf = vec![0u8; MAX_PACKET_LEN + 1];
    let mut dec = VmcDecoder::new();
    let start = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        match socket.recv(&mut buf) {
            Ok(n) => {
                let changed = dec.decode_packet(&buf[..n], start.elapsed().as_secs_f64());
                *lock(&shared.stats) = dec.stats;
                if changed {
                    *lock(&shared.slot) = Some(*dec.frame());
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            // e.g. ICMP "port unreachable" surfacing on Windows; keep going.
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::arkit_index;
    use crate::osc::Arg;
    use crate::osc::enc::{bundle, message};

    fn wait_for(rx: &VmcReceiver) -> Option<TrackingFrame> {
        for _ in 0..200 {
            if let Some(f) = rx.latest() {
                return Some(f);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn loopback_roundtrip_survives_garbage() {
        let rx = VmcReceiver::bind("127.0.0.1:0").unwrap();
        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let dst = rx.local_addr();
        assert_ne!(dst.port(), 0);

        // Garbage first: the receiver must keep going.
        tx.send_to(b"\x00\x01garbage", dst).unwrap();
        tx.send_to(&[b'/'; 8000], dst).unwrap();
        let pkt = bundle(&[
            message(
                "/VMC/Ext/Blend/Val",
                &[Arg::Str("jawOpen".into()), Arg::Float(0.5)],
            ),
            message("/VMC/Ext/Blend/Apply", &[]),
        ]);
        tx.send_to(&pkt, dst).unwrap();

        let f = wait_for(&rx).expect("frame");
        assert_eq!(f.blend[arkit_index("jawOpen").unwrap()], 0.5);
        assert!(rx.latest().is_none(), "frame is taken once");
        assert!(rx.stats().malformed >= 1);
    }
}

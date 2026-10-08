//! A byte-pipe transport for lightyear links, with traffic counters.
//!
//! Each end of a pipe is a [`PipeIo`] component on a lightyear link entity.
//! Bytes written into the far end (another app, a Web Worker message, a WebRTC
//! data channel) come out of this end and the other way round. Native bots and
//! tests connect apps with [`PipeIo::pair`]. The browser glue holds the far end
//! as a [`PipeEnd`] and moves bytes to and from `postMessage` or WebRTC.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::prelude::*;
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use lightyear::link::{Link, LinkPlugin, LinkReceiveSystems, LinkStart, LinkSystems, Linked, recv_payload_from_bytes};
use lightyear::prelude::{Unlink, UnlinkReason};

/// Bytes and packets moved through one pipe end.
#[derive(Default, Debug)]
pub struct PipeStats {
    pub sent_bytes: AtomicU64,
    pub recv_bytes: AtomicU64,
    pub sent_packets: AtomicU64,
    pub recv_packets: AtomicU64,
}

impl PipeStats {
    pub fn snapshot(&self) -> [u64; 4] {
        [
            self.sent_bytes.load(Ordering::Relaxed),
            self.recv_bytes.load(Ordering::Relaxed),
            self.sent_packets.load(Ordering::Relaxed),
            self.recv_packets.load(Ordering::Relaxed),
        ]
    }
}

/// The lightyear-side end of a pipe.
#[derive(Component, Clone)]
#[require(Link::default())]
pub struct PipeIo {
    tx: Sender<Bytes>,
    rx: Receiver<Bytes>,
    pub stats: Arc<PipeStats>,
}

/// The outside end of a pipe, held by glue code.
#[derive(Clone)]
pub struct PipeEnd {
    pub tx: Sender<Bytes>,
    pub rx: Receiver<Bytes>,
}

impl PipeEnd {
    /// Push bytes that arrived from the network.
    pub fn deliver(&self, bytes: &[u8]) -> bool {
        self.tx.send(Bytes::copy_from_slice(bytes)).is_ok()
    }

    /// Take bytes lightyear wants sent.
    pub fn outgoing(&self) -> impl Iterator<Item = Bytes> + '_ {
        self.rx.try_iter()
    }
}

impl PipeIo {
    /// A lightyear end plus its outside end.
    pub fn new() -> (PipeIo, PipeEnd) {
        let (to_link, from_outside) = crossbeam_channel::unbounded();
        let (to_outside, from_link) = crossbeam_channel::unbounded();
        (PipeIo { tx: to_outside, rx: from_outside, stats: Arc::default() }, PipeEnd { tx: to_link, rx: from_link })
    }

    /// Two lightyear ends wired to each other (client and host in one process).
    pub fn pair() -> (PipeIo, PipeIo) {
        let (a_tx, b_rx) = crossbeam_channel::unbounded();
        let (b_tx, a_rx) = crossbeam_channel::unbounded();
        (PipeIo { tx: a_tx, rx: a_rx, stats: Arc::default() }, PipeIo { tx: b_tx, rx: b_rx, stats: Arc::default() })
    }
}

pub struct PipePlugin;

impl PipePlugin {
    fn link(trigger: On<LinkStart>, query: Query<(), With<PipeIo>>, mut commands: Commands) {
        if query.contains(trigger.entity) {
            commands.entity(trigger.entity).insert(Linked);
        }
    }

    fn send(mut query: Query<(Entity, &mut Link, &PipeIo), With<Linked>>, mut commands: Commands) {
        for (entity, mut link, io) in &mut query {
            while let Some(payload) = link.send.pop() {
                let len = payload.len() as u64;
                if io.tx.send(payload).is_err() {
                    let _ = link.send.drain();
                    commands.trigger(Unlink { entity, reason: UnlinkReason::TransportError("pipe closed".into()) });
                    break;
                }
                io.stats.sent_bytes.fetch_add(len, Ordering::Relaxed);
                io.stats.sent_packets.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn receive(mut query: Query<(Entity, &mut Link, &PipeIo), With<Linked>>, mut commands: Commands) {
        for (entity, mut link, io) in &mut query {
            loop {
                match io.rx.try_recv() {
                    Ok(data) => {
                        io.stats.recv_bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                        io.stats.recv_packets.fetch_add(1, Ordering::Relaxed);
                        link.recv.push(recv_payload_from_bytes(data), lightyear::core::time::Instant::now());
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        commands.trigger(Unlink { entity, reason: UnlinkReason::TransportError("pipe closed".into()) });
                        break;
                    }
                }
            }
        }
    }
}

impl Plugin for PipePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<LinkPlugin>() {
            app.add_plugins(LinkPlugin);
        }
        app.add_observer(Self::link);
        app.add_systems(PreUpdate, Self::receive.in_set(LinkReceiveSystems::BufferToLink));
        app.add_systems(PostUpdate, Self::send.in_set(LinkSystems::Send));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_end_round_trip() {
        let (io, end) = PipeIo::new();
        assert!(end.deliver(b"hello"));
        assert_eq!(io.rx.try_recv().unwrap().as_ref(), b"hello");
        io.tx.send(Bytes::from_static(b"world")).unwrap();
        assert_eq!(end.outgoing().next().unwrap().as_ref(), b"world");
    }
}

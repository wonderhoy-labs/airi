//! Accepts connections from Android and moves messages on and off the wire on their own threads, so
//! a slow phone can't hold up the compositor. Frames are compressed here for the same reason.

use std::{
    io::{BufReader, BufWriter, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};

use smithay::reexports::calloop::channel::Sender;
use tracing::{debug, info, warn};

use crate::proto::{self, Incoming, Payload, read_message, server_handshake, to_android, write_message};

pub type ConnId = u64;

/// What the network threads tell the compositor.
pub enum NetEvent {
    Connected { conn: ConnId, role: u8, out: Outgoing },
    Message { conn: ConnId, message: Incoming },
    Disconnected { conn: ConnId },
}

/// One rectangle of a frame, as raw RGBA.
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct Frame {
    pub seq: u32,
    pub width: u32,
    pub height: u32,
    pub rects: Vec<FrameRect>,
}

enum Out {
    Message(u8, Vec<u8>),
    Frame(Frame),
}

/// Sends to one connection. Cloning it is cheap.
#[derive(Clone)]
pub struct Outgoing {
    tx: SyncSender<Out>,
}

impl Outgoing {
    pub fn send(&self, kind: u8, payload: Vec<u8>) {
        // A full queue means the phone has stopped reading. Control messages are small and few, so
        // block only until the writer thread sees the broken socket.
        let _ = self.tx.send(Out::Message(kind, payload));
    }

    pub fn send_frame(&self, frame: Frame) {
        let _ = self.tx.send(Out::Frame(frame));
    }
}

fn encode_frame(frame: &Frame, compressed: &mut Vec<u8>) {
    let mut header = Payload::default()
        .u32(frame.seq)
        .u32(frame.width)
        .u32(frame.height)
        .u32(frame.rects.len() as u32);
    compressed.clear();
    compressed.append(&mut header.0);
    for rect in &frame.rects {
        let start = compressed.len();
        let bound = lz4_flex::block::get_maximum_output_size(rect.pixels.len());
        compressed.resize(start + 20 + bound, 0);
        let written = lz4_flex::block::compress_into(&rect.pixels, &mut compressed[start + 20..])
            .expect("the buffer is the maximum output size");
        let meta = Payload::default().u32(rect.x).u32(rect.y).u32(rect.width).u32(rect.height).u32(written as u32);
        compressed[start..start + 20].copy_from_slice(&meta.0);
        compressed.truncate(start + 20 + written);
    }
}

fn writer_thread(stream: TcpStream, rx: Receiver<Out>) {
    let mut writer = BufWriter::with_capacity(256 * 1024, &stream);
    let mut compressed = Vec::new();
    while let Ok(message) = rx.recv() {
        let result = match message {
            Out::Message(kind, payload) => write_message(&mut writer, kind, &payload),
            Out::Frame(frame) => {
                encode_frame(&frame, &mut compressed);
                write_message(&mut writer, to_android::FRAME, &compressed)
            }
        };
        if result.is_err() {
            break;
        }
        if let Err(err) = writer.flush() {
            debug!(%err, "write failed");
            break;
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn serve(conn: ConnId, mut stream: TcpStream, token: Arc<Vec<u8>>, events: Sender<NetEvent>) {
    let _ = stream.set_nodelay(true);
    let role = match server_handshake(&mut stream, &token) {
        Ok(role) => role,
        Err(err) => {
            warn!(%err, "refused a connection");
            return;
        }
    };
    let (tx, rx) = mpsc::sync_channel(64);
    let Ok(write_half) = stream.try_clone() else { return };
    thread::Builder::new()
        .name("airi-send".into())
        .spawn(move || writer_thread(write_half, rx))
        .expect("thread creation");

    if events.send(NetEvent::Connected { conn, role, out: Outgoing { tx } }).is_err() {
        return;
    }
    let mut reader = BufReader::with_capacity(64 * 1024, &stream);
    loop {
        let message = read_message(&mut reader).and_then(|(kind, payload)| proto::decode(kind, &payload));
        match message {
            Ok(message) => {
                if events.send(NetEvent::Message { conn, message }).is_err() {
                    break;
                }
            }
            Err(err) => {
                debug!(%err, conn, "connection ended");
                break;
            }
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let _ = events.send(NetEvent::Disconnected { conn });
}

/// Starts listening on loopback. The Terminal app forwards this port to Android.
pub fn listen(port: u16, token: Vec<u8>, events: Sender<NetEvent>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    info!(port, "listening for Android");
    let token = Arc::new(token);
    static NEXT: AtomicU64 = AtomicU64::new(1);
    thread::Builder::new().name("airi-listen".into()).spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let token = token.clone();
            let events = events.clone();
            let conn = NEXT.fetch_add(1, Ordering::Relaxed);
            let _ = thread::Builder::new()
                .name("airi-recv".into())
                .spawn(move || serve(conn, stream, token, events));
        }
    })?;
    Ok(())
}


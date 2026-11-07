//! Accepts connections from Android and moves messages on and off the wire on their own threads, so
//! a slow phone can't hold up the compositor. Frames are compressed here for the same reason.

use std::{
    fs,
    io::{BufReader, BufWriter, ErrorKind, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use smithay::reexports::calloop::channel::Sender;
use tracing::{debug, info, warn};

use crate::proto::{self, Incoming, Payload, read_message, server_handshake, to_android, write_message};

pub type ConnId = u64;

/// This has to be longer than the Terminal app's ten-second wait, or it would refresh in the middle of it.
const REFRESH_EVERY: Duration = Duration::from_secs(12);
const REFRESH_WINDOW: Duration = Duration::from_secs(120);

/// What the network threads tell the compositor.
pub enum NetEvent {
    Connected { conn: ConnId, out: Outgoing },
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
    /// Queues a message, or drops it if the queue is full. Android freezes apps in the background, so a
    /// connection can stop reading for a long time, and waiting here would hang every Linux app.
    pub fn send(&self, kind: u8, payload: Vec<u8>) {
        let _ = self.tx.try_send(Out::Message(kind, payload));
    }

    pub fn send_frame(&self, frame: Frame) {
        let _ = self.tx.try_send(Out::Frame(frame));
    }
}

fn encode_frame(frame: &Frame, compressed: &mut Vec<u8>) {
    let mut header = Payload::default().u32(frame.seq).u32(frame.width).u32(frame.height).u32(frame.rects.len() as u32);
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

fn serve(conn: ConnId, mut stream: TcpStream, token: Arc<Vec<u8>>, events: Sender<NetEvent>, reached: Arc<AtomicBool>) {
    let _ = stream.set_nodelay(true);
    if let Err(err) = server_handshake(&mut stream, &token) {
        warn!(%err, "refused a connection");
        return;
    }
    reached.store(true, Ordering::Relaxed);
    // Room for an icon for every app, which all get asked for at once.
    let (tx, rx) = mpsc::sync_channel(1024);
    let Ok(write_half) = stream.try_clone() else { return };
    thread::Builder::new()
        .name("airi-send".into())
        .spawn(move || writer_thread(write_half, rx))
        .expect("thread creation");

    if events.send(NetEvent::Connected { conn, out: Outgoing { tx } }).is_err() {
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

/// Binds the port, and keeps accepting connections on a thread of its own.
pub fn listen(port: u16, token: Vec<u8>, events: Sender<NetEvent>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    info!(port, "listening for Android");
    let token = Arc::new(token);
    let reached = Arc::new(AtomicBool::new(false));
    thread::Builder::new().name("airi-listen".into()).spawn(move || {
        let serve_one = |stream: TcpStream| {
            let (token, events, reached) = (token.clone(), events.clone(), reached.clone());
            let conn = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
            let _ = thread::Builder::new()
                .name("airi-recv".into())
                .spawn(move || serve(conn, stream, token, events, reached));
        };
        let listener = refresh_until_reached(port, listener, &reached, serve_one);
        accept_forever(listener, serve_one);
    })?;
    Ok(())
}

static NEXT_CONN: AtomicU64 = AtomicU64::new(1);

fn accept_forever(listener: TcpListener, serve_one: impl Fn(TcpStream)) {
    for stream in listener.incoming().flatten() {
        serve_one(stream);
    }
}

/// How long Linux has been running, going by the kernel.
fn uptime() -> Option<Duration> {
    let text = fs::read_to_string("/proc/uptime").ok()?;
    let seconds: f64 = text.split_whitespace().next()?.parse().ok()?;
    Some(Duration::from_secs_f64(seconds))
}

/// Works around how the Terminal app forwards ports. It starts forwarding an allowed port early in boot,
/// before anything is listening. If Airi connects in that gap, which it will if it's what started Linux,
/// the Terminal waits ten seconds and then stops forwarding the port, until a new listener makes it look
/// again. So until someone gets through, this keeps listening afresh. It only does that just after boot,
/// because outside of that the Terminal shows a notification for each new listener on a port not yet
/// allowed.
fn refresh_until_reached(
    port: u16,
    mut listener: TcpListener,
    reached: &AtomicBool,
    serve_one: impl Fn(TcpStream),
) -> TcpListener {
    let Some(uptime) = uptime().filter(|uptime| *uptime < REFRESH_WINDOW) else { return listener };
    let stop = Instant::now() + (REFRESH_WINDOW - uptime);
    let mut next_refresh = Instant::now() + REFRESH_EVERY;
    let _ = listener.set_nonblocking(true);
    while !reached.load(Ordering::Relaxed) && Instant::now() < stop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                serve_one(stream);
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(50)),
            Err(err) => debug!(%err, "accept failed"),
        }
        if Instant::now() >= next_refresh {
            next_refresh = Instant::now() + REFRESH_EVERY;
            drop(listener);
            debug!("listening afresh");
            listener = loop {
                match TcpListener::bind(("127.0.0.1", port)) {
                    Ok(listener) => break listener,
                    Err(err) => {
                        warn!(%err, "couldn't listen again");
                        thread::sleep(Duration::from_millis(200));
                    }
                }
            };
            let _ = listener.set_nonblocking(true);
        }
    }
    let _ = listener.set_nonblocking(false);
    listener
}

//! The wire protocol between Airi's Android apps and this agent. `docs/protocol.md` describes it.

use std::io::{self, Read, Write};

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEFAULT_PORT: u16 = 47821;

/// No ordinary message comes close to this. Only an agent upgrade does.
pub const MAX_MESSAGE: usize = 128 * 1024 * 1024;

pub mod from_android {
    pub const LIST_APPS: u8 = 1;
    pub const GET_ICON: u8 = 2;
    pub const LIST_WINDOWS: u8 = 3;
    pub const CLOSE_WINDOW: u8 = 4;
    pub const UPGRADE: u8 = 5;

    pub const OPEN: u8 = 16;
    pub const RESIZE: u8 = 17;
    pub const VISIBLE: u8 = 18;
    pub const FOCUS: u8 = 19;
    pub const FRAME_ACK: u8 = 20;
    pub const POINTER: u8 = 21;
    pub const TOUCH: u8 = 22;
    pub const KEY: u8 = 23;
    pub const TEXT: u8 = 24;
    pub const CLIPBOARD: u8 = 25;
    pub const CLOSE: u8 = 26;
}

pub mod to_android {
    pub const INFO: u8 = 128;
    pub const APPS: u8 = 129;
    pub const ICON: u8 = 130;
    pub const WINDOWS: u8 = 131;
    pub const UPGRADED: u8 = 132;

    pub const ATTACHED: u8 = 144;
    pub const TITLE: u8 = 145;
    pub const FRAME: u8 = 146;
    pub const IME: u8 = 147;
    pub const CLIPBOARD: u8 = 148;
    pub const NEW_WINDOW: u8 = 149;
    pub const CLOSED: u8 = 150;
    pub const CURSOR: u8 = 151;
    pub const ERROR: u8 = 152;
}

pub const POINTER_MOTION: u8 = 0;
pub const POINTER_BUTTON_DOWN: u8 = 1;
pub const POINTER_BUTTON_UP: u8 = 2;
pub const POINTER_SCROLL: u8 = 3;

pub const TOUCH_DOWN: u8 = 0;
pub const TOUCH_MOVE: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const TOUCH_CANCEL: u8 = 3;

/// A message from Android, decoded.
#[derive(Debug)]
pub enum Incoming {
    ListApps,
    GetIcon { id: String },
    ListWindows,
    CloseWindow { window: u32 },
    Upgrade { binary: Vec<u8> },
    Open { desktop_id: String, window: u32, width: u32, height: u32, scale: f32 },
    Resize { width: u32, height: u32, scale: f32 },
    Visible(bool),
    Focus(bool),
    FrameAck,
    Pointer { kind: u8, x: f32, y: f32, button: u32, scroll_x: f32, scroll_y: f32 },
    Touch { kind: u8, id: u32, x: f32, y: f32 },
    Key { code: u32, pressed: bool },
    Text(String),
    Clipboard(String),
    Close,
}

struct Cursor<'a>(&'a [u8]);

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "message too short"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "text isn't UTF-8"))
    }
}

pub fn decode(kind: u8, payload: &[u8]) -> io::Result<Incoming> {
    use from_android::*;
    let mut c = Cursor(payload);
    Ok(match kind {
        LIST_APPS => Incoming::ListApps,
        GET_ICON => Incoming::GetIcon { id: c.string()? },
        LIST_WINDOWS => Incoming::ListWindows,
        CLOSE_WINDOW => Incoming::CloseWindow { window: c.u32()? },
        UPGRADE => Incoming::Upgrade { binary: payload.to_vec() },
        OPEN => Incoming::Open {
            desktop_id: c.string()?,
            window: c.u32()?,
            width: c.u32()?,
            height: c.u32()?,
            scale: c.f32()?,
        },
        RESIZE => Incoming::Resize { width: c.u32()?, height: c.u32()?, scale: c.f32()? },
        VISIBLE => Incoming::Visible(c.u8()? != 0),
        FOCUS => Incoming::Focus(c.u8()? != 0),
        FRAME_ACK => Incoming::FrameAck,
        POINTER => Incoming::Pointer {
            kind: c.u8()?,
            x: c.f32()?,
            y: c.f32()?,
            button: c.u32()?,
            scroll_x: c.f32()?,
            scroll_y: c.f32()?,
        },
        TOUCH => Incoming::Touch { kind: c.u8()?, id: c.u32()?, x: c.f32()?, y: c.f32()? },
        KEY => Incoming::Key { code: c.u32()?, pressed: c.u8()? != 0 },
        TEXT => Incoming::Text(c.string()?),
        CLIPBOARD => Incoming::Clipboard(c.string()?),
        CLOSE => Incoming::Close,
        other => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown message {other}")));
        }
    })
}

/// Reads one message, returning its type and payload.
pub fn read_message(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too long"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok((header[4], payload))
}

pub fn write_message(w: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(&[kind])?;
    w.write_all(payload)
}

/// Builds a message payload.
#[derive(Default)]
pub struct Payload(pub Vec<u8>);

impl Payload {
    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    pub fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(mut self, v: i32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn string(mut self, v: &str) -> Self {
        self = self.u32(v.len() as u32);
        self.0.extend_from_slice(v.as_bytes());
        self
    }
    pub fn bytes(mut self, v: &[u8]) -> Self {
        self.0.extend_from_slice(v);
        self
    }
}

type HmacSha256 = Hmac<Sha256>;

fn proof(token: &[u8], label: &[u8], server_nonce: &[u8; 32], client_nonce: &[u8; 32]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(token).expect("HMAC takes any key length");
    mac.update(label);
    mac.update(server_nonce);
    mac.update(client_nonce);
    mac.finalize().into_bytes().into()
}

/// Runs the agent's side of the handshake. Each side proves it knows the token without sending it,
/// because any Android app can connect to a forwarded port, or listen on one while the agent is down.
pub fn server_handshake(stream: &mut (impl Read + Write), token: &[u8]) -> io::Result<()> {
    let server_nonce: [u8; 32] = rand::random();
    stream.write_all(b"AIRI")?;
    stream.write_all(&PROTOCOL_VERSION.to_le_bytes())?;
    stream.write_all(&server_nonce)?;
    stream.flush()?;

    let mut reply = [0u8; 64];
    stream.read_exact(&mut reply)?;
    let client_nonce: [u8; 32] = reply[..32].try_into().unwrap();
    let expected = proof(token, b"airi client", &server_nonce, &client_nonce);
    let mut difference = 0u8;
    for (a, b) in expected.iter().zip(&reply[32..64]) {
        difference |= a ^ b;
    }
    if difference != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "wrong token"));
    }
    stream.write_all(&proof(token, b"airi server", &server_nonce, &client_nonce))?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor as IoCursor;

    #[test]
    fn messages_round_trip() {
        let mut wire = Vec::new();
        write_message(&mut wire, 7, b"hello").unwrap();
        let (kind, payload) = read_message(&mut IoCursor::new(wire)).unwrap();
        assert_eq!((kind, payload.as_slice()), (7, b"hello".as_slice()));
    }

    #[test]
    fn open_decodes() {
        let p = Payload::default().string("foot.desktop").u32(0).u32(1080).u32(2000);
        let mut bytes = p.0;
        bytes.extend_from_slice(&2.0f32.to_le_bytes());
        match decode(from_android::OPEN, &bytes).unwrap() {
            Incoming::Open { desktop_id, width, height, scale, .. } => {
                assert_eq!(desktop_id, "foot.desktop");
                assert_eq!((width, height), (1080, 2000));
                assert_eq!(scale, 2.0);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn short_messages_are_rejected() {
        assert!(decode(from_android::CLOSE_WINDOW, &[1, 2]).is_err());
        assert!(decode(250, &[]).is_err());
    }

    #[test]
    fn handshake_rejects_a_wrong_token() {
        struct Pipe {
            input: IoCursor<Vec<u8>>,
            output: Vec<u8>,
        }
        impl Read for Pipe {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.input.read(buf)
            }
        }
        impl Write for Pipe {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.output.write(buf)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut pipe = Pipe { input: IoCursor::new(vec![0u8; 64]), output: Vec::new() };
        let err = server_handshake(&mut pipe, b"secret").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(&pipe.output[..4], b"AIRI");
    }
}

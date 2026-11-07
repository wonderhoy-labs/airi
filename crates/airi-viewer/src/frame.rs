//! Decodes the frames the agent sends. `docs/protocol.md` describes the layout.

use std::fmt;

const FRAME_HEADER: usize = 16;
const RECT_HEADER: usize = 20;
const BYTES_PER_PIXEL: usize = 4;
/// The agent limits windows to this, so anything larger is a corrupt frame.
const MAX_SIDE: u32 = 16384 * 2;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Truncated,
    BadSize,
    RectOutOfBounds,
    Corrupt,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Truncated => "the frame ended early",
            Self::BadSize => "the frame's size isn't valid",
            Self::RectOutOfBounds => "a rectangle lies outside the frame",
            Self::Corrupt => "a rectangle didn't decompress",
        })
    }
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, FrameError> {
    let bytes = data.get(at..at + 4).ok_or(FrameError::Truncated)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

/// The window's current picture, as RGBA.
#[derive(Default)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    scratch: Vec<u8>,
}

impl Canvas {
    /// Applies a frame. A size change starts from black, and the agent always follows one with a
    /// frame that covers everything.
    pub fn apply(&mut self, frame: &[u8]) -> Result<(), FrameError> {
        let width = read_u32(frame, 4)?;
        let height = read_u32(frame, 8)?;
        let count = read_u32(frame, 12)?;
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(FrameError::BadSize);
        }
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.pixels.clear();
            self.pixels.resize(width as usize * height as usize * BYTES_PER_PIXEL, 0);
        }

        let mut at = FRAME_HEADER;
        for _ in 0..count {
            let x = read_u32(frame, at)?;
            let y = read_u32(frame, at + 4)?;
            let rect_width = read_u32(frame, at + 8)?;
            let rect_height = read_u32(frame, at + 12)?;
            let compressed = read_u32(frame, at + 16)? as usize;
            at += RECT_HEADER;
            let data = frame.get(at..at + compressed).ok_or(FrameError::Truncated)?;
            at += compressed;
            self.apply_rect(x, y, rect_width, rect_height, data)?;
        }
        Ok(())
    }

    fn apply_rect(&mut self, x: u32, y: u32, width: u32, height: u32, data: &[u8]) -> Result<(), FrameError> {
        let fits = x.checked_add(width).is_some_and(|right| right <= self.width)
            && y.checked_add(height).is_some_and(|bottom| bottom <= self.height);
        if !fits {
            return Err(FrameError::RectOutOfBounds);
        }
        let row = width as usize * BYTES_PER_PIXEL;
        let size = row * height as usize;
        self.scratch.resize(size, 0);
        let written =
            lz4_flex::block::decompress_into(data, &mut self.scratch).map_err(|_| FrameError::Corrupt)?;
        if written != size {
            return Err(FrameError::Corrupt);
        }
        let stride = self.width as usize * BYTES_PER_PIXEL;
        let left = x as usize * BYTES_PER_PIXEL;
        for (i, source) in self.scratch.chunks_exact(row.max(1)).enumerate() {
            let start = (y as usize + i) * stride + left;
            self.pixels[start..start + row].copy_from_slice(source);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, rects: &[(u32, u32, u32, u32, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [1, width, height, rects.len() as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for &(x, y, w, h, fill) in rects {
            let data = lz4_flex::block::compress(&vec![fill; (w * h * 4) as usize]);
            for value in [x, y, w, h, data.len() as u32] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&data);
        }
        out
    }

    #[test]
    fn rectangles_land_where_they_should() {
        let mut canvas = Canvas::default();
        canvas.apply(&frame(4, 4, &[(0, 0, 4, 4, 1)])).unwrap();
        canvas.apply(&frame(4, 4, &[(1, 2, 2, 1, 9)])).unwrap();
        let pixel = |x: usize, y: usize| canvas.pixels[(y * 4 + x) * 4];
        assert_eq!((pixel(0, 0), pixel(1, 2), pixel(2, 2), pixel(3, 2), pixel(1, 3)), (1, 9, 9, 1, 1));
    }

    #[test]
    fn a_new_size_starts_clean() {
        let mut canvas = Canvas::default();
        canvas.apply(&frame(2, 2, &[(0, 0, 2, 2, 7)])).unwrap();
        canvas.apply(&frame(3, 1, &[])).unwrap();
        assert_eq!((canvas.width, canvas.height), (3, 1));
        assert_eq!(canvas.pixels, vec![0; 12]);
    }

    #[test]
    fn bad_frames_are_refused_without_panicking() {
        let mut canvas = Canvas::default();
        assert_eq!(canvas.apply(&[0; 3]), Err(FrameError::Truncated));
        assert_eq!(canvas.apply(&frame(0, 4, &[])), Err(FrameError::BadSize));
        assert_eq!(canvas.apply(&frame(4, 4, &[(3, 3, 2, 2, 1)])), Err(FrameError::RectOutOfBounds));
        assert_eq!(canvas.apply(&frame(4, 4, &[(u32::MAX, 0, 2, 2, 1)])), Err(FrameError::RectOutOfBounds));

        let mut cut = frame(4, 4, &[(0, 0, 4, 4, 1)]);
        cut.truncate(cut.len() - 1);
        assert_eq!(canvas.apply(&cut), Err(FrameError::Truncated));

        let mut garbled = frame(4, 4, &[(0, 0, 4, 4, 1)]);
        let end = garbled.len();
        garbled[36..end].fill(0xff);
        assert_eq!(canvas.apply(&garbled), Err(FrameError::Corrupt));
    }
}

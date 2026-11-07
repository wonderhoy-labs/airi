//! Airi's Android side of the picture: keeps a copy of a Linux window and draws it to a surface.
//! It's native so that a full frame costs a memory copy instead of a pass through the Java heap.

mod frame;
#[cfg(target_os = "android")]
mod jni_api;

pub use frame::{Canvas, FrameError};

//! The functions `NativeView.kt` calls.

use std::{
    ffi::c_void,
    ptr,
    sync::Mutex,
};

use jni::{
    JNIEnv,
    objects::{JByteBuffer, JClass, JObject},
    sys::{jint, jlong},
};

use crate::Canvas;

#[repr(C)]
struct NativeWindow {
    _private: [u8; 0],
}

#[repr(C)]
struct WindowBuffer {
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    bits: *mut c_void,
    reserved: [u32; 6],
}

/// `WINDOW_FORMAT_RGBX_8888`. Everything Airi draws is opaque.
const FORMAT_RGBX: i32 = 2;

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut NativeWindow;
    fn ANativeWindow_release(window: *mut NativeWindow);
    fn ANativeWindow_setBuffersGeometry(window: *mut NativeWindow, width: i32, height: i32, format: i32) -> i32;
    fn ANativeWindow_lock(window: *mut NativeWindow, buffer: *mut WindowBuffer, dirty: *mut c_void) -> i32;
    fn ANativeWindow_unlockAndPost(window: *mut NativeWindow) -> i32;
}

struct Viewer {
    canvas: Canvas,
    window: *mut NativeWindow,
    window_size: (u32, u32),
}

// The window pointer is only used with the mutex held, and Android's windows can move between threads.
unsafe impl Send for Viewer {}

impl Viewer {
    fn set_window(&mut self, window: *mut NativeWindow) {
        if !self.window.is_null() {
            unsafe { ANativeWindow_release(self.window) };
        }
        self.window = window;
        self.window_size = (0, 0);
    }

    fn draw(&mut self) {
        let (width, height) = (self.canvas.width, self.canvas.height);
        if self.window.is_null() || width == 0 {
            return;
        }
        if self.window_size != (width, height) {
            let result =
                unsafe { ANativeWindow_setBuffersGeometry(self.window, width as i32, height as i32, FORMAT_RGBX) };
            if result != 0 {
                return;
            }
            self.window_size = (width, height);
        }
        let mut buffer = WindowBuffer {
            width: 0,
            height: 0,
            stride: 0,
            format: 0,
            bits: ptr::null_mut(),
            reserved: [0; 6],
        };
        if unsafe { ANativeWindow_lock(self.window, &mut buffer, ptr::null_mut()) } != 0 {
            return;
        }
        // Android can hand back a buffer of the old size just after a change. Copy what fits.
        let row = (width.min(buffer.width as u32) * 4) as usize;
        let rows = height.min(buffer.height as u32) as usize;
        let source_stride = width as usize * 4;
        let target_stride = buffer.stride as usize * 4;
        for y in 0..rows {
            let source = &self.canvas.pixels[y * source_stride..y * source_stride + row];
            // Safety: Android gives us `height` rows of `stride` pixels, and this stays within both.
            unsafe {
                ptr::copy_nonoverlapping(
                    source.as_ptr(),
                    (buffer.bits as *mut u8).add(y * target_stride),
                    row,
                );
            }
        }
        unsafe { ANativeWindow_unlockAndPost(self.window) };
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.set_window(ptr::null_mut());
    }
}

fn viewer(handle: jlong) -> &'static Mutex<Viewer> {
    // Safety: the handle comes from `create` and Kotlin stops using it before `destroy`.
    unsafe { &*(handle as *const Mutex<Viewer>) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wonderhoylabs_airi_viewer_NativeView_create(_env: JNIEnv, _class: JClass) -> jlong {
    let viewer = Viewer { canvas: Canvas::default(), window: ptr::null_mut(), window_size: (0, 0) };
    Box::into_raw(Box::new(Mutex::new(viewer))) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wonderhoylabs_airi_viewer_NativeView_destroy(_env: JNIEnv, _class: JClass, handle: jlong) {
    // Safety: see `viewer`. This is the last use of the handle.
    drop(unsafe { Box::from_raw(handle as *mut Mutex<Viewer>) });
}

/// Attaches a surface, or detaches with null, and shows the current picture straight away.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wonderhoylabs_airi_viewer_NativeView_setSurface(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    surface: JObject,
) {
    let window = if surface.is_null() {
        ptr::null_mut()
    } else {
        // Safety: both pointers come straight from the JVM for this call.
        unsafe { ANativeWindow_fromSurface(env.get_raw() as *mut c_void, surface.as_raw() as *mut c_void) }
    };
    let mut viewer = viewer(handle).lock().unwrap();
    viewer.set_window(window);
    viewer.draw();
}

/// Applies a frame held in a direct buffer. Returns the picture's size as `width << 32 | height`,
/// or -1 if the frame was bad.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_wonderhoylabs_airi_viewer_NativeView_frame(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    buffer: JByteBuffer,
    length: jint,
) -> jlong {
    let Ok(address) = env.get_direct_buffer_address(&buffer) else { return -1 };
    // Safety: Kotlin passes the length it read into this direct buffer, and holds it during the call.
    let frame = unsafe { std::slice::from_raw_parts(address, length as usize) };
    let mut viewer = viewer(handle).lock().unwrap();
    if viewer.canvas.apply(frame).is_err() {
        return -1;
    }
    viewer.draw();
    ((viewer.canvas.width as jlong) << 32) | viewer.canvas.height as jlong
}

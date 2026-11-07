package com.wonderhoylabs.airi.viewer

import android.view.Surface
import java.nio.ByteBuffer

/** The Rust half of the viewer, in `crates/airi-viewer`. */
object NativeView {
    init {
        System.loadLibrary("airi_viewer")
    }

    external fun create(): Long
    external fun destroy(handle: Long)
    external fun setSurface(handle: Long, surface: Surface?)

    /** Applies a frame from a direct buffer. Returns `width << 32 | height`, or -1 if it was bad. */
    external fun frame(handle: Long, buffer: ByteBuffer, length: Int): Long
}

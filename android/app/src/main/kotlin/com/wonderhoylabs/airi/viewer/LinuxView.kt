package com.wonderhoylabs.airi.viewer

import android.content.Context
import android.text.InputType
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceView
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import com.wonderhoylabs.airi.agent.Protocol

/** Shows a Linux window, and turns everything you do to it into events for the agent. */
class LinuxView(context: Context, private val input: Input) : SurfaceView(context) {
    interface Input {
        fun pointer(kind: Int, x: Float, y: Float, button: Int = 0, scrollX: Float = 0f, scrollY: Float = 0f)
        fun touch(kind: Int, id: Int, x: Float, y: Float)
        fun key(code: Int, pressed: Boolean)
        fun text(text: String)
    }

    /** How many of the picture's pixels each of this view's stands for. It's 1 unless an app won't fit. */
    @Volatile var pictureScale = 1f

    init {
        isFocusable = true
        isFocusableInTouchMode = true
    }

    private fun toFrame(value: Float): Float = value * pictureScale

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.isFromSource(InputDevice.SOURCE_MOUSE)) {
            // Buttons arrive separately, as BUTTON_PRESS and BUTTON_RELEASE.
            if (event.actionMasked == MotionEvent.ACTION_MOVE) {
                input.pointer(Protocol.POINTER_MOTION, toFrame(event.x), toFrame(event.y))
            }
            return true
        }
        requestFocus()
        val index = event.actionIndex
        val id = event.getPointerId(index)
        val x = toFrame(event.getX(index))
        val y = toFrame(event.getY(index))
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> input.touch(Protocol.TOUCH_DOWN, id, x, y)
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> input.touch(Protocol.TOUCH_UP, id, x, y)
            MotionEvent.ACTION_CANCEL -> input.touch(Protocol.TOUCH_CANCEL, id, x, y)
            MotionEvent.ACTION_MOVE -> for (i in 0 until event.pointerCount) {
                input.touch(Protocol.TOUCH_MOVE, event.getPointerId(i), toFrame(event.getX(i)), toFrame(event.getY(i)))
            }
        }
        return true
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (!event.isFromSource(InputDevice.SOURCE_MOUSE)) return super.onGenericMotionEvent(event)
        val x = toFrame(event.x)
        val y = toFrame(event.y)
        when (event.actionMasked) {
            MotionEvent.ACTION_HOVER_ENTER, MotionEvent.ACTION_HOVER_MOVE -> input.pointer(Protocol.POINTER_MOTION, x, y)
            MotionEvent.ACTION_BUTTON_PRESS -> input.pointer(Protocol.POINTER_BUTTON_DOWN, x, y, linuxButton(event.actionButton))
            MotionEvent.ACTION_BUTTON_RELEASE -> input.pointer(Protocol.POINTER_BUTTON_UP, x, y, linuxButton(event.actionButton))
            // Android counts up as positive, and Wayland counts down.
            MotionEvent.ACTION_SCROLL -> input.pointer(
                Protocol.POINTER_SCROLL, x, y,
                scrollX = event.getAxisValue(MotionEvent.AXIS_HSCROLL),
                scrollY = -event.getAxisValue(MotionEvent.AXIS_VSCROLL),
            )
            else -> return false
        }
        return true
    }

    private fun linuxButton(button: Int): Int = when (button) {
        MotionEvent.BUTTON_SECONDARY -> BTN_RIGHT
        MotionEvent.BUTTON_TERTIARY -> BTN_MIDDLE
        MotionEvent.BUTTON_BACK -> BTN_SIDE
        MotionEvent.BUTTON_FORWARD -> BTN_EXTRA
        else -> BTN_LEFT
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean = onKey(event, true) || super.onKeyDown(keyCode, event)

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean = onKey(event, false) || super.onKeyUp(keyCode, event)

    private fun onKey(event: KeyEvent, pressed: Boolean): Boolean {
        if (KeyMap.isForAndroid(event.keyCode)) return false
        // A real keyboard reports the kernel's own code, which knows about keys Android doesn't.
        val code = event.scanCode.takeIf { it != 0 && event.device?.isVirtual == false }
            ?: KeyMap.fromKeyCode(event.keyCode)
        if (code != null) {
            // Linux apps don't repeat keys here, so a held key has to come through as more presses.
            if (event.repeatCount > 0) input.key(code, false)
            input.key(code, pressed)
            return true
        }
        val character = event.unicodeChar
        if (pressed && character != 0) {
            input.text(String(Character.toChars(character)))
            return true
        }
        return false
    }

    override fun onCheckIsTextEditor() = true

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        // Suggestions and autocorrect rewrite what's already typed, which Linux apps can't take part in.
        outAttrs.inputType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_ACTION_NONE
        return Connection()
    }

    /** Takes what the on-screen keyboard types, with nothing kept on this side to edit. */
    private inner class Connection : BaseInputConnection(this, false) {
        private var composing = ""

        private fun tap(code: Int) {
            input.key(code, true)
            input.key(code, false)
        }

        private fun send(text: String) {
            text.split('\n').forEachIndexed { index, line ->
                if (index > 0) tap(KeyMap.ENTER)
                if (line.isNotEmpty()) input.text(line)
            }
        }

        /** Changes what was composed so far into this, touching only the part that differs. */
        private fun replaceComposing(text: String) {
            val same = composing.commonPrefixWith(text).length
            repeat(composing.length - same) { tap(KeyMap.BACKSPACE) }
            send(text.substring(same))
            composing = text
        }

        override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
            replaceComposing(text.toString())
            composing = ""
            return true
        }

        override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
            replaceComposing(text.toString())
            return true
        }

        override fun finishComposingText(): Boolean {
            composing = ""
            return true
        }

        override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
            repeat(beforeLength) { tap(KeyMap.BACKSPACE) }
            repeat(afterLength) { tap(KeyMap.DELETE) }
            return true
        }

        override fun performEditorAction(actionCode: Int): Boolean {
            tap(KeyMap.ENTER)
            return true
        }
    }

    private companion object {
        const val BTN_LEFT = 0x110
        const val BTN_RIGHT = 0x111
        const val BTN_MIDDLE = 0x112
        const val BTN_SIDE = 0x113
        const val BTN_EXTRA = 0x114
    }
}

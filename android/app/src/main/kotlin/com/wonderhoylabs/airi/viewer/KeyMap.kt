package com.wonderhoylabs.airi.viewer

import android.view.KeyEvent

/** Turns Android's key codes into the Linux kernel's, which is what Wayland uses. */
object KeyMap {
    const val ENTER = 28
    const val BACKSPACE = 14
    const val DELETE = 111

    private val letters = intArrayOf(
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
    )

    private val others = mapOf(
        KeyEvent.KEYCODE_ENTER to ENTER,
        KeyEvent.KEYCODE_NUMPAD_ENTER to 96,
        KeyEvent.KEYCODE_DEL to BACKSPACE,
        KeyEvent.KEYCODE_FORWARD_DEL to DELETE,
        KeyEvent.KEYCODE_TAB to 15,
        KeyEvent.KEYCODE_ESCAPE to 1,
        KeyEvent.KEYCODE_SPACE to 57,
        KeyEvent.KEYCODE_DPAD_UP to 103,
        KeyEvent.KEYCODE_DPAD_DOWN to 108,
        KeyEvent.KEYCODE_DPAD_LEFT to 105,
        KeyEvent.KEYCODE_DPAD_RIGHT to 106,
        KeyEvent.KEYCODE_MOVE_HOME to 102,
        KeyEvent.KEYCODE_MOVE_END to 107,
        KeyEvent.KEYCODE_PAGE_UP to 104,
        KeyEvent.KEYCODE_PAGE_DOWN to 109,
        KeyEvent.KEYCODE_INSERT to 110,
        KeyEvent.KEYCODE_F11 to 87,
        KeyEvent.KEYCODE_F12 to 88,
        KeyEvent.KEYCODE_SHIFT_LEFT to 42,
        KeyEvent.KEYCODE_SHIFT_RIGHT to 54,
        KeyEvent.KEYCODE_CTRL_LEFT to 29,
        KeyEvent.KEYCODE_CTRL_RIGHT to 97,
        KeyEvent.KEYCODE_ALT_LEFT to 56,
        KeyEvent.KEYCODE_ALT_RIGHT to 100,
        KeyEvent.KEYCODE_META_LEFT to 125,
        KeyEvent.KEYCODE_META_RIGHT to 126,
        KeyEvent.KEYCODE_CAPS_LOCK to 58,
        KeyEvent.KEYCODE_MINUS to 12,
        KeyEvent.KEYCODE_EQUALS to 13,
        KeyEvent.KEYCODE_LEFT_BRACKET to 26,
        KeyEvent.KEYCODE_RIGHT_BRACKET to 27,
        KeyEvent.KEYCODE_BACKSLASH to 43,
        KeyEvent.KEYCODE_SEMICOLON to 39,
        KeyEvent.KEYCODE_APOSTROPHE to 40,
        KeyEvent.KEYCODE_GRAVE to 41,
        KeyEvent.KEYCODE_COMMA to 51,
        KeyEvent.KEYCODE_PERIOD to 52,
        KeyEvent.KEYCODE_SLASH to 53,
    )

    /** Returns the Linux code for an Android key code, or null if there isn't one. */
    fun fromKeyCode(keyCode: Int): Int? = when (keyCode) {
        in KeyEvent.KEYCODE_A..KeyEvent.KEYCODE_Z -> letters[keyCode - KeyEvent.KEYCODE_A]
        KeyEvent.KEYCODE_0 -> 11
        in KeyEvent.KEYCODE_1..KeyEvent.KEYCODE_9 -> 2 + keyCode - KeyEvent.KEYCODE_1
        in KeyEvent.KEYCODE_F1..KeyEvent.KEYCODE_F10 -> 59 + keyCode - KeyEvent.KEYCODE_F1
        else -> others[keyCode]
    }

    /** These belong to Android, and an app in a window shouldn't be able to take them. */
    fun isForAndroid(keyCode: Int): Boolean = when (keyCode) {
        KeyEvent.KEYCODE_BACK, KeyEvent.KEYCODE_HOME, KeyEvent.KEYCODE_POWER, KeyEvent.KEYCODE_APP_SWITCH,
        KeyEvent.KEYCODE_VOLUME_UP, KeyEvent.KEYCODE_VOLUME_DOWN, KeyEvent.KEYCODE_VOLUME_MUTE -> true
        else -> false
    }
}

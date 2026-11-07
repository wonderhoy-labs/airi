package com.wonderhoylabs.airi.agent

/** The wire protocol. `docs/protocol.md` describes it, and `proto.rs` is the agent's half. */
object Protocol {
    const val VERSION = 1
    const val PORT = 47821

    const val ROLE_CONTROL = 1
    const val ROLE_WINDOW = 2

    const val LIST_APPS = 1
    const val GET_ICON = 2
    const val LIST_WINDOWS = 3
    const val CLOSE_WINDOW = 4
    const val UPGRADE = 5

    const val OPEN = 16
    const val RESIZE = 17
    const val VISIBLE = 18
    const val FOCUS = 19
    const val FRAME_ACK = 20
    const val POINTER = 21
    const val TOUCH = 22
    const val KEY = 23
    const val TEXT = 24
    const val CLIPBOARD = 25
    const val CLOSE = 26

    const val INFO = 128
    const val APPS = 129
    const val ICON = 130
    const val WINDOWS = 131
    const val UPGRADED = 132

    const val ATTACHED = 144
    const val TITLE = 145
    const val FRAME = 146
    const val IME = 147
    const val CLIPBOARD_CHANGED = 148
    const val NEW_WINDOW = 149
    const val CLOSED = 150
    const val CURSOR = 151
    const val ERROR = 152

    const val POINTER_MOTION = 0
    const val POINTER_BUTTON_DOWN = 1
    const val POINTER_BUTTON_UP = 2
    const val POINTER_SCROLL = 3

    const val TOUCH_DOWN = 0
    const val TOUCH_MOVE = 1
    const val TOUCH_UP = 2
    const val TOUCH_CANCEL = 3
}

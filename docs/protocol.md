# Airi's protocol

This is how the Android app and the agent in Linux talk. Both ends ship together, and the app replaces
an agent of a different version, so this can change between releases. `PROTOCOL_VERSION` changes when it
does.

The agent listens on TCP port 47821 on Linux's loopback, and the Terminal app forwards that port to
Android's loopback. All numbers are little-endian, and strings are a `u32` byte length and then UTF-8.

## Handshake

Each side proves it has the 32-byte token without sending it. This matters in both directions, because
any Android app can connect to a forwarded port, and could listen on it while the agent isn't running.

1. The agent sends `"AIRI"`, a `u16` protocol version and a 32-byte nonce.
2. The app sends its own 32-byte nonce, and `HMAC-SHA256(token, "airi client" + agent nonce + app nonce)`.
3. The agent checks that, and sends `HMAC-SHA256(token, "airi server" + agent nonce + app nonce)`.
4. The agent sends `INFO`.

## Messages

After that, every message is a `u32` payload length, a `u8` type and the payload.

### From Android

| Type | Name | Payload |
| --- | --- | --- |
| 1 | `LIST_APPS` | |
| 2 | `GET_ICON` | string: app ID |
| 3 | `LIST_WINDOWS` | |
| 4 | `CLOSE_WINDOW` | `u32` window |
| 5 | `UPGRADE` | the new agent binary |
| 16 | `OPEN` | string: app ID, `u32` window or 0, `u32` width, `u32` height, `f32` scale |
| 17 | `RESIZE` | `u32` width, `u32` height, `f32` scale |
| 18 | `VISIBLE` | `u8` |
| 19 | `FOCUS` | `u8` |
| 20 | `FRAME_ACK` | |
| 21 | `POINTER` | `u8` kind, `f32` x, `f32` y, `u32` button, `f32` scroll x, `f32` scroll y |
| 22 | `TOUCH` | `u8` kind, `u32` ID, `f32` x, `f32` y |
| 23 | `KEY` | `u32` Linux key code, `u8` pressed |
| 24 | `TEXT` | string |
| 25 | `CLIPBOARD` | string |
| 26 | `CLOSE` | |

`OPEN` makes a connection into one Android window. It attaches to the given window, or else to a window of
that app with nothing showing it, or else launches the app. Sizes and positions are in pixels of the
picture, and the scale is pixels per Linux logical pixel.

Pointer kinds are motion (0), button down (1), button up (2) and scroll (3), and buttons are Linux codes
such as `BTN_LEFT`. Touch kinds are down (0), move (1), up (2) and cancel (3).

### From the agent

| Type | Name | Payload |
| --- | --- | --- |
| 128 | `INFO` | string: agent version, `u32` protocol version |
| 129 | `APPS` | JSON: `[{"id", "name", "comment"}]` |
| 130 | `ICON` | string: app ID, then a PNG, or nothing if it has no icon |
| 131 | `WINDOWS` | JSON: `[{"id", "title", "desktop_id", "attached"}]` |
| 132 | `UPGRADED` | `u8` success, and a string if it failed |
| 144 | `ATTACHED` | `u32` window, string: app ID, string: title |
| 145 | `TITLE` | string |
| 146 | `FRAME` | see below |
| 147 | `IME` | `u8`: whether a text field wants the keyboard |
| 148 | `CLIPBOARD` | string |
| 149 | `NEW_WINDOW` | `u32` window, string: app ID, string: title |
| 150 | `CLOSED` | |
| 151 | `CURSOR` | `u8` 0 hidden, 1 named and a string, or 2 and `i32` hotspot x and y, `u32` width and height, RGBA |
| 152 | `ERROR` | string |

## Frames

A frame is a `u32` sequence number, `u32` picture width, `u32` picture height and `u32` rectangle count.
Each rectangle is a `u32` x, y, width, height and compressed size, then that many bytes of raw LZ4 block
holding its RGBA pixels. A frame after a size change covers the whole picture.

The app sends `FRAME_ACK` for each frame it has drawn, and the agent keeps at most two frames in flight, so
a slow link costs frame rate and not latency. The picture is the size the app asked for. A Linux window
with a minimum size that won't fit gets a lower scale, down to 1. Only one that still doesn't fit makes the
picture larger, and then the app scales it down.

After `RESIZE`, the agent keeps the old picture until the window has drawn the new size, or 600 ms have
passed, so that nothing half-finished reaches the screen.

//! What the compositor does with messages from Android.

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

use serde::Serialize;
use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    input::{
        keyboard::{FilterResult, Keycode},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
        touch::{DownEvent, MotionEvent as TouchMotionEvent, UpEvent},
    },
    utils::{Logical, Point},
    wayland::text_input::TextInputSeat,
};
use tracing::{error, info, warn};

use crate::{
    apps, launch,
    net::{ConnId, NetEvent},
    proto::{self, Incoming, Payload, to_android},
    stage::{LAUNCH_TIMEOUT, PendingResize, RESIZE_PATIENCE, Stage, Waiting},
    state::{Event, State, next_serial},
    windows::window_title,
};

/// The Linux key code for each character a US keyboard can type, for apps that don't take text
/// directly. The bool is whether Shift is held.
fn key_for_char(c: char) -> Option<(u32, bool)> {
    const LETTERS: &[u32] = &[
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
    ];
    Some(match c {
        'a'..='z' => (LETTERS[c as usize - 'a' as usize], false),
        'A'..='Z' => (LETTERS[c as usize - 'A' as usize], true),
        '1'..='9' => (2 + (c as u32 - '1' as u32), false),
        '0' => (11, false),
        ' ' => (57, false),
        '\n' => (28, false),
        '\t' => (15, false),
        '-' => (12, false),
        '=' => (13, false),
        '[' => (26, false),
        ']' => (27, false),
        '\\' => (43, false),
        ';' => (39, false),
        '\'' => (40, false),
        '`' => (41, false),
        ',' => (51, false),
        '.' => (52, false),
        '/' => (53, false),
        '!' => (2, true),
        '@' => (3, true),
        '#' => (4, true),
        '$' => (5, true),
        '%' => (6, true),
        '^' => (7, true),
        '&' => (8, true),
        '*' => (9, true),
        '(' => (10, true),
        ')' => (11, true),
        '_' => (12, true),
        '+' => (13, true),
        '{' => (26, true),
        '}' => (27, true),
        '|' => (43, true),
        ':' => (39, true),
        '"' => (40, true),
        '~' => (41, true),
        '<' => (51, true),
        '>' => (52, true),
        '?' => (53, true),
        _ => return None,
    })
}

const KEY_LEFT_SHIFT: u32 = 42;

/// How long text input has to stay off before Android's keyboard closes.
const IME_HIDE_DELAY: Duration = Duration::from_millis(150);

#[derive(Serialize)]
struct WindowInfo {
    id: u32,
    title: String,
    desktop_id: String,
    attached: bool,
}

impl State {
    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Net(NetEvent::Connected { conn, role, out }) => {
                let info = Payload::default()
                    .string(env!("CARGO_PKG_VERSION"))
                    .u32(proto::PROTOCOL_VERSION as u32)
                    .u8(role);
                out.send(to_android::INFO, info.0);
                self.conns.insert(conn, out);
            }
            Event::Net(NetEvent::Message { conn, message }) => self.on_message(conn, message),
            Event::Net(NetEvent::Disconnected { conn }) => self.on_disconnect(conn),
            Event::Launched(launched) => self.on_app_exit(launched),
            Event::LinuxClipboard(text) => self.on_linux_clipboard(text),
            Event::Icon { conn, id, png } => {
                if let Some(out) = self.conns.get(&conn) {
                    let payload = Payload::default().string(&id).bytes(png.as_deref().unwrap_or_default());
                    out.send(to_android::ICON, payload.0);
                }
            }
        }
    }

    fn on_message(&mut self, conn: ConnId, message: Incoming) {
        match message {
            Incoming::ListApps => {
                self.apps = apps::scan();
                let json = serde_json::to_vec(&self.apps).unwrap_or_default();
                self.send(conn, to_android::APPS, json);
            }
            Incoming::GetIcon { id } => {
                let Some(app) = self.apps.iter().find(|app| app.id == id).cloned() else {
                    self.send(conn, to_android::ICON, Payload::default().string(&id).0);
                    return;
                };
                let events = self.events.clone();
                std::thread::spawn(move || {
                    let png = apps::icon_png(&app);
                    let _ = events.send(Event::Icon { conn, id, png });
                });
            }
            Incoming::ListWindows => {
                let windows: Vec<WindowInfo> = self
                    .windows
                    .iter()
                    .map(|t| WindowInfo {
                        id: t.id,
                        title: window_title(&t.window),
                        desktop_id: t.desktop_id.clone(),
                        attached: t.stage.is_some(),
                    })
                    .collect();
                let json = serde_json::to_vec(&windows).unwrap_or_default();
                self.send(conn, to_android::WINDOWS, json);
            }
            Incoming::CloseWindow { window } => self.close_window(window),
            Incoming::Upgrade { binary } => self.upgrade(conn, binary),
            Incoming::Open { desktop_id, window, width, height, scale } => {
                self.open_stage(conn, desktop_id, window, width, height, scale)
            }
            Incoming::Resize { width, height, scale } => self.resize_stage(conn, width, height, scale),
            Incoming::Visible(visible) => {
                if let Some(stage) = self.stages.get_mut(&conn) {
                    stage.visible = visible;
                    if visible {
                        stage.dirty = true;
                        stage.redraw_all = true;
                    }
                }
            }
            Incoming::Focus(focused) => {
                let Some(stage) = self.stages.get_mut(&conn) else { return };
                stage.focused = focused;
                if focused {
                    self.focus_stage(conn);
                    self.send_cursor();
                } else {
                    self.unfocus_stage(conn);
                }
            }
            Incoming::FrameAck => {
                if let Some(stage) = self.stages.get_mut(&conn) {
                    stage.frames_in_flight = stage.frames_in_flight.saturating_sub(1);
                }
            }
            Incoming::Pointer { kind, x, y, button, scroll_x, scroll_y } => {
                self.on_pointer(conn, kind, x, y, button, scroll_x, scroll_y)
            }
            Incoming::Touch { kind, id, x, y } => self.on_touch(conn, kind, id, x, y),
            Incoming::Key { code, pressed } => self.on_key(code, pressed),
            Incoming::Text(text) => self.on_text(&text),
            Incoming::Clipboard(text) => self.set_clipboard_from_android(text),
            Incoming::Close => {
                if let Some(id) = self.stages.get(&conn).and_then(|stage| stage.main) {
                    self.close_window(id);
                }
            }
        }
    }

    fn send(&self, conn: ConnId, kind: u8, payload: Vec<u8>) {
        if let Some(out) = self.conns.get(&conn) {
            out.send(kind, payload);
        }
    }

    fn close_window(&mut self, id: u32) {
        let Some(tracked) = self.windows.iter().find(|t| t.id == id) else { return };
        if let Some(toplevel) = tracked.window.toplevel() {
            toplevel.send_close();
        } else if let Some(surface) = tracked.window.x11_surface() {
            let _ = surface.close();
        }
    }

    /// Replaces this agent's binary with one sent by the app, then exits so that systemd restarts it.
    fn upgrade(&mut self, conn: ConnId, binary: Vec<u8>) {
        let result = (|| -> std::io::Result<()> {
            let exe = env::current_exe()?;
            let new = exe.with_extension("new");
            fs::write(&new, &binary)?;
            fs::set_permissions(&new, fs::Permissions::from_mode(0o755))?;
            fs::rename(&new, &exe)
        })();
        match result {
            Ok(()) => {
                info!("upgraded, restarting");
                self.send(conn, to_android::UPGRADED, Payload::default().u8(1).0);
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(300));
                    std::process::exit(0);
                });
            }
            Err(err) => {
                error!(%err, "couldn't upgrade");
                self.send(conn, to_android::UPGRADED, Payload::default().u8(0).string(&err.to_string()).0);
            }
        }
    }

    fn open_stage(&mut self, conn: ConnId, desktop_id: String, window: u32, width: u32, height: u32, scale: f32) {
        let Some(out) = self.conns.get(&conn).cloned() else { return };
        let view = sanitize_size(width, height);
        let scale = sanitize_scale(scale);
        let stage = Stage::new(conn, out.clone(), &self.dh, &mut self.renderer, view, scale, desktop_id.clone());
        let stage = match stage {
            Ok(stage) => stage,
            Err(err) => {
                error!(?err, "couldn't create a window");
                out.send(to_android::ERROR, Payload::default().string("Couldn't create the window.").0);
                return;
            }
        };
        self.stages.insert(conn, stage);

        if window != 0 {
            if self.windows.iter().any(|t| t.id == window) {
                self.attach_main(conn, window);
            } else {
                out.send(to_android::CLOSED, Vec::new());
            }
            return;
        }
        let existing = self
            .windows
            .iter()
            .find(|t| t.stage.is_none() && !desktop_id.is_empty() && t.desktop_id == desktop_id)
            .map(|t| t.id);
        if let Some(id) = existing {
            self.attach_main(conn, id);
            return;
        }
        if self.apps.iter().all(|app| app.id != desktop_id) {
            self.apps = apps::scan();
        }
        let Some(app) = self.apps.iter().find(|app| app.id == desktop_id).cloned() else {
            out.send(to_android::ERROR, Payload::default().string("This app isn't installed in Linux any more.").0);
            return;
        };
        let env_vars = launch::client_env(self);
        match launch::launch(&app, &env_vars, conn, self.events.clone()) {
            Ok(pid) => {
                if let Some(stage) = self.stages.get_mut(&conn) {
                    stage.waiting = Some(Waiting {
                        until: Instant::now() + LAUNCH_TIMEOUT,
                        launched_pid: Some(pid),
                        since_launch: true,
                    });
                }
            }
            Err(err) => {
                warn!(%err, app = app.id, "couldn't launch");
                let message = format!("Couldn't start {}: {err}", app.name);
                out.send(to_android::ERROR, Payload::default().string(&message).0);
            }
        }
    }

    fn resize_stage(&mut self, conn: ConnId, width: u32, height: u32, scale: f32) {
        let view = sanitize_size(width, height);
        let scale = sanitize_scale(scale);
        let renderer = &mut self.renderer;
        let Some(stage) = self.stages.get_mut(&conn) else { return };
        let target = stage.pending_resize.as_ref().map_or(stage.view, |pending| pending.view);
        if target == view && stage.scale == scale {
            return;
        }
        let window = stage.main.and_then(|id| self.windows.iter().find(|t| t.id == id));
        // With a window to wait for, keep showing what's there until it has drawn the new size. A
        // change of scale redraws everything anyway, and happens too rarely to be worth smoothing.
        if let (Some(window), true) = (window, stage.scale == scale) {
            // A window being dragged asks again every frame, so this keeps the first deadline. Waiting
            // for the app to catch up with the very latest size would freeze the picture for the drag.
            let until = stage.pending_resize.as_ref().map_or(Instant::now() + RESIZE_PATIENCE, |p| p.until);
            stage.pending_resize = Some(PendingResize { view, until });
            crate::windows::configure_window(&window.window, stage.logical_size_of(view), stage.focused);
            return;
        }
        stage.pending_resize = None;
        if let Err(err) = stage.resize(renderer, view, scale) {
            error!(?err, "couldn't resize a window");
            return;
        }
        stage.update_surface_scale();
        let size = stage.view_logical_size();
        if let Some(window) = window {
            crate::windows::configure_window(&window.window, size, stage.focused);
        }
    }

    fn on_disconnect(&mut self, conn: ConnId) {
        self.conns.remove(&conn);
        let Some(mut stage) = self.stages.remove(&conn) else { return };
        for tracked in self.windows.iter_mut().filter(|t| t.stage == Some(conn)) {
            tracked.stage = None;
            // Announce it again in case another Android window can take it.
            tracked.announced = false;
            stage.space.unmap_elem(&tracked.window);
        }
        self.dh.remove_global::<State>(stage.global.clone());
    }

    fn on_app_exit(&mut self, launched: launch::Launched) {
        let Some(stage) = self.stages.get_mut(&launched.conn) else { return };
        if launched.success || stage.main.is_some() {
            return;
        }
        if stage.waiting.as_ref().is_some_and(|waiting| waiting.since_launch) {
            let mut message = String::from("The app quit before opening a window.");
            if !launched.output_tail.is_empty() {
                message.push_str("\n\n");
                message.push_str(&launched.output_tail);
            }
            stage.out.send(to_android::ERROR, Payload::default().string(&message).0);
            stage.waiting = None;
        }
    }

    fn on_linux_clipboard(&mut self, text: String) {
        if self.android_clipboard.as_deref() == Some(&text) || self.linux_clipboard.as_deref() == Some(&text) {
            return;
        }
        self.linux_clipboard = Some(text.clone());
        self.android_clipboard = None;
        if let Some(stage) = self.stages.values().find(|stage| stage.focused) {
            stage.out.send(to_android::CLIPBOARD, Payload::default().string(&text).0);
        }
    }

    fn logical_point(stage: &Stage, x: f32, y: f32) -> Point<f64, Logical> {
        (x as f64 / stage.scale, y as f64 / stage.scale).into()
    }

    #[allow(clippy::too_many_arguments)]
    fn on_pointer(&mut self, conn: ConnId, kind: u8, x: f32, y: f32, button: u32, scroll_x: f32, scroll_y: f32) {
        let Some(stage) = self.stages.get(&conn) else { return };
        let location = Self::logical_point(stage, x, y);
        let under = stage.surface_under(location);
        let Some(pointer) = self.seat.get_pointer() else { return };
        let time = self.now_ms();
        pointer.motion(self, under, &MotionEvent { location, serial: next_serial(), time });
        match kind {
            proto::POINTER_BUTTON_DOWN | proto::POINTER_BUTTON_UP => {
                let state = if kind == proto::POINTER_BUTTON_DOWN {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                };
                pointer.button(self, &ButtonEvent { serial: next_serial(), time, button, state });
            }
            proto::POINTER_SCROLL => {
                // Android reports notches. Toolkits take both notches and the distance they stand for.
                let mut frame = AxisFrame::new(time).source(AxisSource::Wheel);
                if scroll_y != 0.0 {
                    frame = frame
                        .value(Axis::Vertical, scroll_y as f64 * 15.0)
                        .v120(Axis::Vertical, (scroll_y * 120.0) as i32);
                }
                if scroll_x != 0.0 {
                    frame = frame
                        .value(Axis::Horizontal, scroll_x as f64 * 15.0)
                        .v120(Axis::Horizontal, (scroll_x * 120.0) as i32);
                }
                pointer.axis(self, frame);
            }
            _ => {}
        }
        pointer.frame(self);
    }

    fn on_touch(&mut self, conn: ConnId, kind: u8, id: u32, x: f32, y: f32) {
        let Some(stage) = self.stages.get(&conn) else { return };
        let location = Self::logical_point(stage, x, y);
        let under = stage.surface_under(location);
        let Some(touch) = self.seat.get_touch() else { return };
        let time = self.now_ms();
        let slot = Some(id).into();
        match kind {
            proto::TOUCH_DOWN => {
                touch.down(self, under, &DownEvent { slot, location, serial: next_serial(), time });
            }
            proto::TOUCH_MOVE => {
                touch.motion(self, under, &TouchMotionEvent { slot, location, time });
            }
            proto::TOUCH_UP => {
                touch.up(self, &UpEvent { slot, serial: next_serial(), time });
            }
            proto::TOUCH_CANCEL => {
                touch.cancel(self);
                return;
            }
            _ => return,
        }
        touch.frame(self);
    }

    fn on_key(&mut self, code: u32, pressed: bool) {
        let Some(keyboard) = self.seat.get_keyboard() else { return };
        let state = if pressed { KeyState::Pressed } else { KeyState::Released };
        let time = self.now_ms();
        // Wayland key codes are the kernel's, offset by 8 for historical reasons.
        keyboard.input::<(), _>(self, Keycode::new(code + 8), state, next_serial(), time, |_, _, _| {
            FilterResult::Forward
        });
    }

    fn on_text(&mut self, text: &str) {
        let input = self.seat.text_input().clone();
        let mut delivered = false;
        input.with_active_text_input(|text_input, _| {
            text_input.commit_string(Some(text.to_string()));
            delivered = true;
        });
        if delivered {
            input.done(false);
            return;
        }
        // The app isn't using text input, so type it. Characters a US keyboard can't type are lost.
        for c in text.chars() {
            let Some((code, shift)) = key_for_char(c) else { continue };
            if shift {
                self.on_key(KEY_LEFT_SHIFT, true);
            }
            self.on_key(code, true);
            self.on_key(code, false);
            if shift {
                self.on_key(KEY_LEFT_SHIFT, false);
            }
        }
    }

    /// Runs after each batch of events: draws what's due and keeps Android up to date.
    pub fn tick(&mut self) {
        let time: Duration = self.clock.now().into();
        let renderer = &mut self.renderer;
        let now = Instant::now();
        for stage in self.stages.values_mut() {
            if stage.pending_resize.as_ref().is_some_and(|pending| now >= pending.until) {
                stage.finish_resize(renderer);
            }
            if stage.can_draw() {
                stage.draw(renderer, time);
            }
        }
        self.check_waiting();
        self.update_ime();
        self.cleanup_popups();

        // Apps that aren't on screen still need the odd frame callback, or some of them stop.
        if self.last_idle_frames.elapsed() >= Duration::from_secs(1) {
            self.last_idle_frames = Instant::now();
            let idle = &self.idle_output;
            for tracked in &self.windows {
                let hidden =
                    tracked.stage.and_then(|conn| self.stages.get(&conn)).is_none_or(|stage| !stage.visible);
                if hidden {
                    tracked.window.send_frame(idle, time, Some(Duration::ZERO), |_, _| Some(idle.clone()));
                }
            }
        }
    }

    /// Opens or closes Android's keyboard as text fields gain and lose focus. Toolkits can switch text
    /// input off and on again as they go, and closing for that would make the whole window jump.
    fn update_ime(&mut self) {
        let mut wanted = false;
        self.seat.text_input().with_active_text_input(|_, _| wanted = true);
        if wanted || !self.ime_wanted {
            self.ime_off_since = None;
        } else if self.ime_off_since.get_or_insert_with(Instant::now).elapsed() < IME_HIDE_DELAY {
            return;
        }
        if wanted == self.ime_wanted {
            return;
        }
        self.ime_wanted = wanted;
        self.ime_off_since = None;
        if let Some(stage) = self.stages.values().find(|stage| stage.focused) {
            stage.out.send(to_android::IME, Payload::default().u8(wanted as u8).0);
        }
    }

    /// How long the event loop can sleep before something needs doing.
    pub fn next_wake(&self) -> Duration {
        if self.stages.values().any(Stage::can_draw) {
            return Duration::ZERO;
        }
        let mut wake = Duration::from_millis(1000);
        if self.stages.values().any(|stage| stage.waiting.is_some()) {
            wake = wake.min(Duration::from_millis(100));
        }
        if self.ime_off_since.is_some() {
            wake = wake.min(Duration::from_millis(20));
        }
        let now = Instant::now();
        for pending in self.stages.values().filter_map(|stage| stage.pending_resize.as_ref()) {
            wake = wake.min(pending.until.saturating_duration_since(now));
        }
        wake
    }
}

fn sanitize_size(width: u32, height: u32) -> smithay::utils::Size<i32, smithay::utils::Physical> {
    (width.clamp(16, 16384) as i32, height.clamp(16, 16384) as i32).into()
}

fn sanitize_scale(scale: f32) -> f64 {
    if scale.is_finite() { (scale as f64).clamp(0.5, 8.0) } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_characters_map_to_keys() {
        assert_eq!(key_for_char('a'), Some((30, false)));
        assert_eq!(key_for_char('Z'), Some((44, true)));
        assert_eq!(key_for_char('1'), Some((2, false)));
        assert_eq!(key_for_char('0'), Some((11, false)));
        assert_eq!(key_for_char('!'), Some((2, true)));
        assert_eq!(key_for_char('é'), None);
    }

    #[test]
    fn bad_sizes_and_scales_are_bounded() {
        assert_eq!(sanitize_size(0, 100000), (16, 16384).into());
        assert_eq!(sanitize_scale(f32::NAN), 1.0);
        assert_eq!(sanitize_scale(100.0), 8.0);
    }
}

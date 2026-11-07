//! The compositor's state and the Wayland and X11 protocol handlers. What happens to a window once
//! it exists is in `windows.rs`, and how it reaches Android is in `stage.rs`.

use std::{
    borrow::Cow,
    collections::HashMap,
    os::fd::OwnedFd,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use smithay::{
    backend::renderer::{pixman::PixmanRenderer, utils::on_commit_buffer_handler},
    delegate_compositor, delegate_data_device, delegate_fractional_scale, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_single_pixel_buffer,
    delegate_text_input_manager, delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
    delegate_xwayland_shell,
    desktop::{
        PopupKind, PopupKeyboardGrab, PopupManager, PopupPointerGrab, PopupUngrabStrategy, Window,
        find_popup_root_surface, get_popup_toplevel_coords,
    },
    input::{
        Seat, SeatHandler, SeatState,
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState, XkbConfig},
        pointer::{CursorImageStatus, Focus},
    },
    output::Output,
    reexports::{
        calloop::{LoopHandle, channel::Sender},
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel,
        },
        wayland_server::{
            Client, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer::WlBuffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Clock, IsAlive, Logical, Monotonic, Rectangle, SERIAL_COUNTER, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent, is_sync_subsurface,
            with_states,
        },
        fractional_scale::{FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale},
        output::{OutputHandler, OutputManagerState},
        seat::WaylandFocus,
        selection::{
            SelectionHandler, SelectionSource, SelectionTarget,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                clear_data_device_selection, current_data_device_selection_userdata,
                request_data_device_client_selection, set_data_device_focus, set_data_device_selection,
            },
            primary_selection::{
                PrimarySelectionHandler, PrimarySelectionState, clear_primary_selection,
                current_primary_selection_userdata, request_primary_client_selection, set_primary_focus,
                set_primary_selection,
            },
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
            decoration::{XdgDecorationHandler, XdgDecorationState},
        },
        shm::{ShmHandler, ShmState},
        single_pixel_buffer::SinglePixelBufferState,
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        X11Surface, X11Wm, XWaylandClientData, XwmHandler,
        xwm::{Reorder, ResizeEdge as X11ResizeEdge, WmWindowProperty, XwmId},
    },
};
use tracing::warn;

use crate::{
    apps::App,
    launch::Launched,
    net::{ConnId, NetEvent, Outgoing},
    stage::Stage,
};

/// Everything that reaches the compositor from its other threads.
pub enum Event {
    Net(NetEvent),
    Launched(Launched),
    LinuxClipboard(String),
    Icon { conn: ConnId, id: String, png: Option<Vec<u8>> },
}

#[derive(Default)]
pub struct ClientState {
    pub compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client: ClientId) {}
    fn disconnected(&self, _client: ClientId, _reason: DisconnectReason) {}
}

/// Who owns the clipboard when it wasn't a Wayland client.
#[derive(Clone)]
pub enum ClipboardOwner {
    X11,
    Android(Arc<String>),
}

/// The keyboard goes to an X11 window through its own type, which sets X11's input focus as well.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyboardFocus {
    Surface(WlSurface),
    X11(X11Surface),
}

impl IsAlive for KeyboardFocus {
    fn alive(&self) -> bool {
        match self {
            Self::Surface(surface) => surface.alive(),
            Self::X11(surface) => surface.alive(),
        }
    }
}

impl From<PopupKind> for KeyboardFocus {
    fn from(popup: PopupKind) -> Self {
        Self::Surface(popup.wl_surface().clone())
    }
}

impl From<KeyboardFocus> for WlSurface {
    fn from(focus: KeyboardFocus) -> Self {
        match focus {
            KeyboardFocus::Surface(surface) => surface,
            KeyboardFocus::X11(surface) => surface.wl_surface().expect("a focused window has a surface"),
        }
    }
}

impl WaylandFocus for KeyboardFocus {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            Self::Surface(surface) => Some(Cow::Borrowed(surface)),
            Self::X11(surface) => surface.wl_surface().map(Cow::Owned),
        }
    }
}

impl KeyboardTarget<State> for KeyboardFocus {
    fn enter(&self, seat: &Seat<State>, data: &mut State, keys: Vec<KeysymHandle<'_>>, serial: Serial) {
        match self {
            Self::Surface(s) => KeyboardTarget::enter(s, seat, data, keys, serial),
            Self::X11(s) => KeyboardTarget::enter(s, seat, data, keys, serial),
        }
    }
    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        match self {
            Self::Surface(s) => KeyboardTarget::leave(s, seat, data, serial),
            Self::X11(s) => KeyboardTarget::leave(s, seat, data, serial),
        }
    }
    fn key(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        key: KeysymHandle<'_>,
        state: smithay::backend::input::KeyState,
        serial: Serial,
        time: u32,
    ) {
        match self {
            Self::Surface(s) => KeyboardTarget::key(s, seat, data, key, state, serial, time),
            Self::X11(s) => KeyboardTarget::key(s, seat, data, key, state, serial, time),
        }
    }
    fn modifiers(&self, seat: &Seat<State>, data: &mut State, modifiers: ModifiersState, serial: Serial) {
        match self {
            Self::Surface(s) => KeyboardTarget::modifiers(s, seat, data, modifiers, serial),
            Self::X11(s) => KeyboardTarget::modifiers(s, seat, data, modifiers, serial),
        }
    }
}

/// One Linux toplevel window, whether or not an Android window is showing it.
pub struct Tracked {
    pub id: u32,
    pub window: Window,
    pub desktop_id: String,
    pub stage: Option<ConnId>,
    pub announced: bool,
    pub last_title: String,
}

pub struct State {
    pub dh: DisplayHandle,
    pub handle: LoopHandle<'static, State>,
    pub events: Sender<Event>,
    pub clock: Clock<Monotonic>,

    pub compositor: CompositorState,
    pub shm: ShmState,
    pub xdg: XdgShellState,
    pub decoration: XdgDecorationState,
    pub seat_state: SeatState<State>,
    pub seat: Seat<State>,
    pub data_device: DataDeviceState,
    pub primary: PrimarySelectionState,
    pub _outputs: OutputManagerState,
    pub _viewporter: ViewporterState,
    pub _fractional: FractionalScaleManagerState,
    pub _single_pixel: SinglePixelBufferState,
    pub xwayland_shell: XWaylandShellState,
    pub popups: PopupManager,
    pub renderer: PixmanRenderer,

    pub xwm: Option<X11Wm>,
    pub x_display: Option<u32>,
    pub apps: Vec<App>,
    pub socket_name: String,

    pub conns: HashMap<ConnId, Outgoing>,
    pub stages: HashMap<ConnId, Stage>,
    pub windows: Vec<Tracked>,
    pub next_window_id: u32,
    pub idle_output: Output,
    pub last_idle_frames: Instant,

    pub ime_wanted: bool,
    pub android_clipboard: Option<String>,
    pub linux_clipboard: Option<String>,
    pub cursor: CursorImageStatus,
    pub started_at: Instant,
}

impl State {
    pub fn now_ms(&self) -> u32 {
        Duration::from(self.clock.now()).as_millis() as u32
    }

    /// Finds the toplevel a surface belongs to, walking up through subsurfaces and popups.
    pub fn root_surface(&self, surface: &WlSurface) -> WlSurface {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        if let Some(popup) = self.popups.find_popup(&root) {
            if let Ok(toplevel) = find_popup_root_surface(&popup) {
                return toplevel;
            }
        }
        root
    }

    /// Finds the Android window holding this surface, and marks it as needing a redraw.
    fn mark_dirty(&mut self, surface: &WlSurface) {
        let root = self.root_surface(surface);
        for stage in self.stages.values_mut() {
            if stage.contains_surface(&root) {
                stage.dirty = true;
                return;
            }
        }
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else { return };
        let Some(stage) = self.stages.values().find(|stage| stage.contains_surface(&root)) else { return };
        let Some(window) = stage.window_for_surface(&root) else { return };
        let Some(window_geometry) = stage.space.element_geometry(&window) else { return };
        let mut target = Rectangle::from_size(stage.logical_size());
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geometry.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    fn clipboard_text_mime(mime_types: &[String]) -> Option<String> {
        ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"]
            .iter()
            .find(|wanted| mime_types.iter().any(|mime| mime == *wanted))
            .map(|mime| mime.to_string())
    }

    /// Reads the end of a pipe on its own thread, because a client can take as long as it likes.
    fn read_clipboard_pipe(&self, read_end: OwnedFd) {
        let events = self.events.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut text = Vec::new();
            let mut file = std::fs::File::from(read_end);
            if file.by_ref().take(8 * 1024 * 1024).read_to_end(&mut text).is_ok() {
                if let Ok(text) = String::from_utf8(text) {
                    let _ = events.send(Event::LinuxClipboard(text));
                }
            }
        });
    }

    pub fn set_clipboard_from_android(&mut self, text: String) {
        if self.android_clipboard.as_deref() == Some(&text) || self.linux_clipboard.as_deref() == Some(&text) {
            return;
        }
        self.android_clipboard = Some(text.clone());
        let mime_types: Vec<String> =
            ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"].map(String::from).into();
        set_data_device_selection(
            &self.dh,
            &self.seat,
            mime_types.clone(),
            ClipboardOwner::Android(Arc::new(text)),
        );
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(SelectionTarget::Clipboard, Some(mime_types)) {
                warn!(?err, "couldn't offer the clipboard to X11");
            }
        }
    }

    pub fn start_xwayland(&mut self) {
        let result = smithay::xwayland::XWayland::spawn(
            &self.dh,
            None,
            std::iter::empty::<(String, String)>(),
            true,
            Stdio::null(),
            Stdio::null(),
            |_| (),
        );
        let (xwayland, client) = match result {
            Ok(started) => started,
            Err(err) => {
                warn!(%err, "couldn't start Xwayland, so X11 apps won't work");
                return;
            }
        };
        let result = self.handle.insert_source(xwayland, move |event, _, state| match event {
            smithay::xwayland::XWaylandEvent::Ready { x11_socket, display_number } => {
                match X11Wm::start_wm(state.handle.clone(), x11_socket, client.clone()) {
                    Ok(wm) => {
                        state.xwm = Some(wm);
                        state.x_display = Some(display_number);
                        crate::launch::write_env_file(state);
                    }
                    Err(err) => warn!(%err, "couldn't manage X11 windows"),
                }
            }
            smithay::xwayland::XWaylandEvent::Error => warn!("Xwayland failed to start"),
        });
        if let Err(err) = result {
            warn!(%err, "couldn't watch Xwayland");
        }
    }
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(state) = client.get_data::<XWaylandClientData>() {
            return &state.compositor_state;
        }
        &client.get_data::<ClientState>().expect("every client has state").compositor
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.popups.commit(surface);

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            self.toplevel_committed(&root);
        }

        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface) {
            if !popup.is_initial_configure_sent() {
                let _ = popup.send_configure();
            }
        }

        self.mark_dirty(surface);
        self.cursor_committed(surface);
    }
}
delegate_compositor!(State);

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}
delegate_shm!(State);

impl SeatHandler for State {
    type KeyboardFocus = KeyboardFocus;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&KeyboardFocus>) {
        let client = focused
            .and_then(WaylandFocus::wl_surface)
            .and_then(|surface| self.dh.get_client(surface.id()).ok());
        set_data_device_focus(&self.dh, seat, client.clone());
        set_primary_focus(&self.dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor = image;
        self.send_cursor();
    }
}
delegate_seat!(State);

impl SelectionHandler for State {
    type SelectionUserData = ClipboardOwner;

    fn new_selection(&mut self, target: SelectionTarget, source: Option<SelectionSource>, seat: Seat<Self>) {
        let mime_types = source.as_ref().map(|source| source.mime_types());
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(err) = xwm.new_selection(target, mime_types.clone()) {
                warn!(?err, "couldn't pass the selection on to X11");
            }
        }
        if target != SelectionTarget::Clipboard {
            return;
        }
        let Some(mime) = mime_types.as_deref().and_then(Self::clipboard_text_mime) else { return };
        let Ok((read_end, write_end)) = pipe() else { return };
        if let Err(err) = request_data_device_client_selection(&seat, mime, write_end) {
            warn!(?err, "couldn't read the clipboard");
            return;
        }
        self.read_clipboard_pipe(read_end);
    }

    fn send_selection(
        &mut self,
        target: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        owner: &ClipboardOwner,
    ) {
        match owner {
            ClipboardOwner::Android(text) => {
                let text = text.clone();
                std::thread::spawn(move || {
                    use std::io::Write;
                    let _ = std::fs::File::from(fd).write_all(text.as_bytes());
                });
            }
            ClipboardOwner::X11 => {
                if let Some(xwm) = self.xwm.as_mut() {
                    if let Err(err) = xwm.send_selection(target, mime_type, fd, self.handle.clone()) {
                        warn!(?err, "couldn't read the selection from X11");
                    }
                }
            }
        }
    }
}

pub fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}
impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}
delegate_data_device!(State);

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary
    }
}
delegate_primary_selection!(State);

impl OutputHandler for State {}
delegate_output!(State);

delegate_viewporter!(State);
delegate_single_pixel_buffer!(State);
delegate_text_input_manager!(State);

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let root = self.root_surface(&surface);
        let scale = self
            .stages
            .values()
            .find(|stage| stage.contains_surface(&root))
            .map(|stage| stage.scale)
            .or_else(|| self.stages.values().next().map(|stage| stage.scale));
        if let Some(scale) = scale {
            with_states(&surface, |states| {
                with_fractional_scale(states, |fractional| fractional.set_preferred_scale(scale));
            });
        }
    }
}
delegate_fractional_scale!(State);

impl XdgDecorationHandler for State {
    // Android draws any title bar, so ask apps not to draw their own. Not all of them agree.
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}
delegate_xdg_decoration!(State);

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // The client hasn't said what it is yet. That comes with its first commit.
        self.track_window(Window::new_wayland_window(surface));
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.window_gone(surface.wl_surface());
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.title_changed_for(surface.wl_surface());
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.app_id_changed_for(surface.wl_surface());
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            warn!(%err, "couldn't track a popup");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<State>::from_resource(&seat) else { return };
        let popup = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&popup) else { return };
        let Some(root_focus) = self.keyboard_focus_for_surface(&root) else { return };
        let Ok(mut grab) = self.popups.grab_popup(root_focus, popup, &seat, serial) else { return };

        if let Some(keyboard) = seat.get_keyboard() {
            if keyboard.is_grabbed()
                && !(keyboard.has_grab(serial) || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(pointer) = seat.get_pointer() {
            if pointer.is_grabbed()
                && !(pointer.has_grab(serial)
                    || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        // Android windows are always filled, so there's nothing to change, but it has to answer.
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Fullscreen);
        });
        if surface.is_initial_configure_sent() {
            surface.send_pending_configure();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
        });
        if surface.is_initial_configure_sent() {
            surface.send_pending_configure();
        }
    }
}
delegate_xdg_shell!(State);

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }

    fn surface_associated(&mut self, _xwm: XwmId, _surface: WlSurface, window: X11Surface) {
        self.x11_surface_linked(&window);
    }
}
delegate_xwayland_shell!(State);

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11 events only come from a running window manager")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}
    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        if let Err(err) = surface.set_mapped(true) {
            warn!(%err, "couldn't map an X11 window");
            return;
        }
        self.track_window(Window::new_x11_window(surface.clone()));
        self.toplevel_ready_x11(&surface);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.map_x11_override(surface);
    }

    fn unmapped_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.unmap_x11_override(&surface);
        self.x11_window_gone(&surface);
        if !surface.is_override_redirect() {
            let _ = surface.set_mapped(false);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, surface: X11Surface) {
        self.unmap_x11_override(&surface);
        self.x11_window_gone(&surface);
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        self.x11_configure_request(&surface, width, height);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        surface: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _above: Option<u32>,
    ) {
        self.x11_moved(&surface, geometry);
    }

    fn property_notify(&mut self, _xwm: XwmId, surface: X11Surface, property: WmWindowProperty) {
        if matches!(property, WmWindowProperty::Title) {
            if let Some(surface) = surface.wl_surface() {
                self.title_changed_for(&surface);
            }
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, _surface: X11Surface) {}
    fn unmaximize_request(&mut self, _xwm: XwmId, _surface: X11Surface) {}
    fn fullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        let _ = surface.set_fullscreen(true);
    }
    fn unfullscreen_request(&mut self, _xwm: XwmId, surface: X11Surface) {
        let _ = surface.set_fullscreen(false);
    }
    fn resize_request(&mut self, _xwm: XwmId, _surface: X11Surface, _button: u32, _edge: X11ResizeEdge) {}
    fn move_request(&mut self, _xwm: XwmId, _surface: X11Surface, _button: u32) {}

    fn allow_selection_access(&mut self, _xwm: XwmId, _selection: SelectionTarget) -> bool {
        true
    }

    fn send_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_type: String, fd: OwnedFd) {
        let result = match selection {
            SelectionTarget::Clipboard => request_data_device_client_selection(&self.seat, mime_type, fd)
                .map_err(|err| err.to_string()),
            SelectionTarget::Primary => {
                request_primary_client_selection(&self.seat, mime_type, fd).map_err(|err| err.to_string())
            }
        };
        if let Err(err) = result {
            warn!(%err, "couldn't send the selection to X11");
        }
    }

    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        match selection {
            SelectionTarget::Clipboard => {
                let text_mime = Self::clipboard_text_mime(&mime_types);
                set_data_device_selection(&self.dh, &self.seat, mime_types, ClipboardOwner::X11);
                // Read it now, because Android can't ask later if the window isn't around.
                if let (Some(mime), Some(xwm), Ok((read_end, write_end))) =
                    (text_mime, self.xwm.as_mut(), pipe())
                {
                    if xwm
                        .send_selection(SelectionTarget::Clipboard, mime, write_end, self.handle.clone())
                        .is_ok()
                    {
                        self.read_clipboard_pipe(read_end);
                    }
                }
            }
            SelectionTarget::Primary => {
                set_primary_selection(&self.dh, &self.seat, mime_types, ClipboardOwner::X11)
            }
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        match selection {
            SelectionTarget::Clipboard => {
                if current_data_device_selection_userdata(&self.seat).is_some() {
                    clear_data_device_selection(&self.dh, &self.seat);
                }
            }
            SelectionTarget::Primary => {
                if current_primary_selection_userdata(&self.seat).is_some() {
                    clear_primary_selection(&self.dh, &self.seat);
                }
            }
        }
    }
}

/// Creates the seat and every global the compositor offers.
pub struct Globals {
    pub compositor: CompositorState,
    pub shm: ShmState,
    pub xdg: XdgShellState,
    pub decoration: XdgDecorationState,
    pub seat_state: SeatState<State>,
    pub seat: Seat<State>,
    pub data_device: DataDeviceState,
    pub primary: PrimarySelectionState,
    pub outputs: OutputManagerState,
    pub viewporter: ViewporterState,
    pub fractional: FractionalScaleManagerState,
    pub single_pixel: SinglePixelBufferState,
    pub xwayland_shell: XWaylandShellState,
}

impl Globals {
    pub fn new(dh: &DisplayHandle) -> anyhow::Result<Self> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, "airi");
        seat.add_pointer();
        seat.add_touch();
        seat.add_keyboard(XkbConfig::default(), 400, 30)
            .map_err(|err| anyhow::anyhow!("couldn't set up the keyboard: {err:?}"))?;
        TextInputManagerState::new::<State>(dh);
        Ok(Self {
            compositor: CompositorState::new::<State>(dh),
            shm: ShmState::new::<State>(dh, vec![]),
            xdg: XdgShellState::new::<State>(dh),
            decoration: XdgDecorationState::new::<State>(dh),
            data_device: DataDeviceState::new::<State>(dh),
            primary: PrimarySelectionState::new::<State>(dh),
            outputs: OutputManagerState::new_with_xdg_output::<State>(dh),
            viewporter: ViewporterState::new::<State>(dh),
            fractional: FractionalScaleManagerState::new::<State>(dh),
            single_pixel: SinglePixelBufferState::new::<State>(dh),
            xwayland_shell: XWaylandShellState::new::<State>(dh),
            seat_state,
            seat,
        })
    }
}

pub fn next_serial() -> Serial {
    SERIAL_COUNTER.next_serial()
}

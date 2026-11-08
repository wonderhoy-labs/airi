//! What happens to Linux windows: which Android window each one goes to, and how it's laid out there.

use std::time::{Duration, Instant};

use smithay::{
	backend::renderer::utils::with_renderer_surface_state,
	desktop::{Window, WindowSurface},
	input::pointer::{CursorImageStatus, CursorImageSurfaceData},
	reexports::{
		wayland_protocols::xdg::shell::server::xdg_toplevel,
		wayland_server::{Resource, protocol::wl_surface::WlSurface},
	},
	utils::{Logical, Point, Rectangle, Size},
	wayland::{
		compositor::with_states,
		seat::WaylandFocus,
		shell::xdg::{SurfaceCachedState, XdgToplevelSurfaceData},
	},
	xwayland::{X11Surface, xwm::WmWindowType},
};

use crate::{
	apps,
	net::ConnId,
	proto::{Payload, to_android},
	stage::{LAUNCH_TIMEOUT, REPLACEMENT_GRACE, Stage, Waiting},
	state::{KeyboardFocus, State, Tracked, next_serial},
};

/// The size a window gets when no Android window is showing it yet.
const DEFAULT_SIZE: (i32, i32) = (800, 600);

/// How long a new Android window gets to open before its Linux window is offered again. Offering it
/// again sooner could open two, and the second would take the window from the first.
const ANNOUNCE_AGAIN_AFTER: Duration = Duration::from_secs(5);

pub fn window_title(window: &Window) -> String {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => with_states(toplevel.wl_surface(), |states| {
			states
				.data_map
				.get::<XdgToplevelSurfaceData>()
				.and_then(|data| data.lock().unwrap().title.clone())
				.unwrap_or_default()
		}),
		WindowSurface::X11(surface) => surface.title(),
	}
}

fn window_app_ids(window: &Window) -> Vec<String> {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => with_states(toplevel.wl_surface(), |states| {
			states
				.data_map
				.get::<XdgToplevelSurfaceData>()
				.and_then(|data| data.lock().unwrap().app_id.clone())
				.into_iter()
				.collect()
		}),
		WindowSurface::X11(surface) => vec![surface.class(), surface.instance()],
	}
}

/// Whether this process is the launched one or was started by it.
fn descends_from(mut pid: u32, ancestor: u32) -> bool {
	for _ in 0..32 {
		if pid == ancestor {
			return true;
		}
		if pid <= 1 {
			return false;
		}
		let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false };
		// The name is in brackets and can contain anything, so look after the last one.
		let Some(after_name) = stat.rsplit(')').next() else { return false };
		let Some(parent) = after_name.split_whitespace().nth(1).and_then(|p| p.parse().ok()) else {
			return false;
		};
		pid = parent;
	}
	false
}

impl State {
	pub fn track_window(&mut self, window: Window) -> u32 {
		let id = self.next_window_id;
		self.next_window_id += 1;
		self.windows.push(Tracked {
			id,
			window,
			desktop_id: String::new(),
			stage: None,
			announced: None,
			last_title: String::new(),
		});
		id
	}

	fn tracked_index_by_surface(&self, surface: &WlSurface) -> Option<usize> {
		self.windows.iter().position(|t| t.window.wl_surface().as_deref() == Some(surface))
	}

	pub fn keyboard_focus_for_surface(&self, root: &WlSurface) -> Option<KeyboardFocus> {
		if let Some(index) = self.tracked_index_by_surface(root) {
			return Some(self.keyboard_focus_for_window(&self.windows[index].window));
		}
		Some(KeyboardFocus::Surface(root.clone()))
	}

	fn keyboard_focus_for_window(&self, window: &Window) -> KeyboardFocus {
		match window.underlying_surface() {
			WindowSurface::Wayland(toplevel) => KeyboardFocus::Surface(toplevel.wl_surface().clone()),
			WindowSurface::X11(surface) => KeyboardFocus::X11(surface.clone()),
		}
	}

	/// Called for each commit of a toplevel's root surface.
	pub fn toplevel_committed(&mut self, root: &WlSurface) {
		let Some(index) = self.tracked_index_by_surface(root) else { return };
		let window = self.windows[index].window.clone();
		window.on_commit();
		let first_commit = match window.underlying_surface() {
			WindowSurface::Wayland(toplevel) => !toplevel.is_initial_configure_sent(),
			WindowSurface::X11(_) => false,
		};
		if first_commit {
			self.place_new_window(index);
			if let Some(toplevel) = window.toplevel() {
				toplevel.send_configure();
			}
		}
		if index >= self.windows.len() {
			return;
		}
		self.announce_when_drawn(index);
		let Some(conn) = self.windows[index].stage else { return };
		let id = self.windows[index].id;
		self.layout_after_commit(conn, id, &window);
	}

	/// Keeps a stage's picture big enough for its window, and its dialogs centred.
	fn layout_after_commit(&mut self, conn: ConnId, id: u32, window: &Window) {
		let renderer = &mut self.renderer;
		let Some(stage) = self.stages.get_mut(&conn) else { return };
		let size = window.geometry().size;
		if stage.main == Some(id) {
			let view = stage.pending_resize.as_ref().map_or(stage.view, |pending| pending.view);
			let wanted = stage.logical_size_of(view);
			let answered = has_reached(window, wanted);
			// Until the app has caught up, this commit is still the old size, and using it would make
			// the picture jump back for a moment.
			if stage.pending_resize.is_some() && !answered {
				return;
			}
			if answered && stage.learn_min_size(size, wanted) {
				let scale = stage.scale_for(view);
				if scale != stage.scale {
					stage.set_scale(scale);
					stage.hold_for(view);
					configure_window(window, stage.logical_size_of(view), stage.focused);
					return;
				}
			}
			stage.finish_resize(renderer);
			if size.w > 0 && size.h > 0 {
				stage.fit_to(renderer, size);
			}
		} else if stage.children.contains(&id) {
			let centre = centred(stage.logical_size(), size);
			if stage.space.element_location(window) != Some(centre) {
				stage.map(window.clone(), centre, false);
			}
		}
	}

	pub fn toplevel_ready_x11(&mut self, surface: &X11Surface) {
		let Some(index) = self.windows.iter().position(|t| t.window.x11_surface() == Some(surface)) else {
			return;
		};
		self.place_new_window(index);
	}

	/// An X11 window is mapped before Xwayland ties it to a Wayland surface. Until that happens its
	/// commits can't be matched to it and the keyboard has nowhere to go, so both are redone here.
	pub fn x11_surface_linked(&mut self, surface: &X11Surface) {
		let mut focused_stage = None;
		for stage in self.stages.values_mut() {
			let has_window = stage.space.elements().any(|window| window.x11_surface() == Some(surface));
			if has_window {
				stage.dirty = true;
				stage.redraw_all = true;
				if stage.focused {
					focused_stage = Some(stage.conn);
				}
			}
		}
		let Some(conn) = focused_stage else { return };
		if let Some(keyboard) = self.seat.get_keyboard() {
			// Setting the same focus again does nothing, and this one was never really delivered.
			keyboard.set_focus(self, None, next_serial());
		}
		self.focus_stage(conn);
	}

	/// Finds the Android window a new Linux window belongs in, now that it's said what it is.
	fn place_new_window(&mut self, index: usize) {
		let window = self.windows[index].window.clone();
		let id = self.windows[index].id;
		let desktop_id = self.resolve_desktop_id(&window);
		self.windows[index].desktop_id = desktop_id.clone();
		self.windows[index].last_title = window_title(&window);

		if let Some(conn) = self.parent_stage(&window) {
			self.attach_child(conn, id);
			return;
		}
		if let Some(conn) = self.find_waiting_stage(&window, &desktop_id) {
			self.attach_main(conn, id);
			return;
		}
		// Nothing asked for this window, so it needs one of its own.
		let size = self
			.stages
			.values()
			.find(|stage| stage.desktop_id == desktop_id)
			.or_else(|| self.stages.values().next())
			.map(Stage::view_logical_size)
			.unwrap_or_else(|| DEFAULT_SIZE.into());
		// Android hears about this once there's something to show. See `announce_when_drawn`.
		configure_window(&window, size, false);
	}

	/// Offers a window nothing is showing, now that it has drawn something. Some apps keep a window around
	/// that never appears, and offering those would open a blank Android window.
	fn announce_when_drawn(&mut self, index: usize) {
		let tracked = &self.windows[index];
		if tracked.stage.is_none() && tracked.announced.is_none() && has_drawn(&tracked.window) {
			self.announce_window(index);
		}
	}

	/// Tells an open Android window that another one is needed, since only a visible app can start one.
	/// One in the background gets refused, so this has to be able to happen again.
	fn announce_window(&mut self, index: usize) {
		let tracked = &mut self.windows[index];
		if tracked.announced.is_some_and(|time| time.elapsed() < ANNOUNCE_AGAIN_AFTER) {
			return;
		}
		let target = self
			.stages
			.values()
			.filter(|stage| stage.desktop_id == tracked.desktop_id)
			.max_by_key(|stage| stage.focused)
			.or_else(|| self.stages.values().max_by_key(|stage| stage.focused));
		if let Some(stage) = target {
			tracked.announced = Some(Instant::now());
			let payload = Payload::default().u32(tracked.id).string(&tracked.desktop_id).string(&tracked.last_title);
			stage.out.send(to_android::NEW_WINDOW, payload.0);
		}
	}

	/// Offers windows that nothing is showing to an Android window that has just come to the front.
	pub fn announce_unshown_windows(&mut self) {
		for index in 0..self.windows.len() {
			if self.windows[index].stage.is_none() && has_drawn(&self.windows[index].window) {
				self.announce_window(index);
			}
		}
	}

	pub fn resolve_desktop_id(&self, window: &Window) -> String {
		window_app_ids(window)
			.iter()
			.find_map(|id| apps::find_for_window(&self.apps, id))
			.map(|app| app.id.clone())
			.unwrap_or_default()
	}

	fn window_pid(&self, window: &Window) -> Option<u32> {
		match window.underlying_surface() {
			WindowSurface::Wayland(toplevel) => {
				let client = toplevel.wl_surface().client()?;
				client.get_credentials(&self.dh).ok().map(|creds| creds.pid as u32)
			}
			WindowSurface::X11(surface) => surface.pid(),
		}
	}

	fn find_waiting_stage(&self, window: &Window, desktop_id: &str) -> Option<ConnId> {
		let waiting = || self.stages.values().filter(|stage| stage.waiting.is_some() && stage.main.is_none());
		if !desktop_id.is_empty()
			&& let Some(stage) = waiting().find(|stage| stage.desktop_id == desktop_id)
		{
			return Some(stage.conn);
		}
		if let Some(pid) = self.window_pid(window) {
			let found = waiting().find(|stage| {
				stage.waiting.as_ref().and_then(|w| w.launched_pid).is_some_and(|launched| descends_from(pid, launched))
			});
			if let Some(stage) = found {
				return Some(stage.conn);
			}
		}
		// An app can identify its windows however it likes, or have another process open them. If
		// this window isn't anything we recognise and something is waiting, it's likely this.
		if desktop_id.is_empty() {
			return waiting().min_by_key(|stage| stage.conn).map(|stage| stage.conn);
		}
		None
	}

	/// Finds the stage of the window this one is a dialog of.
	fn parent_stage(&self, window: &Window) -> Option<ConnId> {
		match window.underlying_surface() {
			WindowSurface::Wayland(toplevel) => {
				let parent = toplevel.parent()?;
				let index = self.tracked_index_by_surface(&parent)?;
				self.windows[index].stage
			}
			WindowSurface::X11(surface) => {
				let is_dialog = matches!(
					surface.window_type(),
					Some(WmWindowType::Dialog | WmWindowType::Utility | WmWindowType::Splash)
				);
				if let Some(parent) = surface.is_transient_for()
					&& let Some(tracked) =
						self.windows.iter().find(|t| t.window.x11_surface().is_some_and(|s| s.window_id() == parent))
				{
					return tracked.stage;
				}
				if is_dialog {
					return self
						.stages
						.values()
						.find(|stage| stage.focused && stage.main.is_some())
						.map(|stage| stage.conn);
				}
				None
			}
		}
	}

	/// Makes a window the main one in a stage, filling the Android window.
	pub fn attach_main(&mut self, conn: ConnId, id: u32) {
		let Some(index) = self.windows.iter().position(|t| t.id == id) else { return };
		self.detach_from_stage(id);
		let window = self.windows[index].window.clone();
		let Some(stage) = self.stages.get_mut(&conn) else { return };
		stage.main = Some(id);
		stage.waiting = None;
		if stage.desktop_id.is_empty() {
			stage.desktop_id = self.windows[index].desktop_id.clone();
		}
		// Asking for the right size from the start saves the app laying everything out twice.
		stage.min_size = declared_min_size(&window);
		stage.set_scale(stage.scale_for(stage.view));
		configure_window(&window, stage.view_logical_size(), true);
		stage.map(window.clone(), (0, 0).into(), true);
		stage.update_surface_scale();
		stage.redraw_all = true;
		let focused = stage.focused;
		let tracked = &mut self.windows[index];
		tracked.stage = Some(conn);
		tracked.announced = Some(Instant::now());
		let payload = Payload::default().u32(id).string(&tracked.desktop_id).string(&tracked.last_title);
		self.stages[&conn].out.send(to_android::ATTACHED, payload.0);
		if focused {
			self.focus_stage(conn);
		}
	}

	/// Adds a dialog to a stage, on top of the main window.
	fn attach_child(&mut self, conn: ConnId, id: u32) {
		let Some(index) = self.windows.iter().position(|t| t.id == id) else { return };
		let window = self.windows[index].window.clone();
		let Some(stage) = self.stages.get_mut(&conn) else { return };
		stage.children.push(id);
		let limit = stage.view_logical_size();
		let wanted = window.geometry().size;
		let size = if wanted.w > 0 && wanted.h > 0 {
			Size::from((wanted.w.min(limit.w), wanted.h.min(limit.h)))
		} else {
			Size::default()
		};
		configure_dialog(&window, size, limit);
		stage.map(window.clone(), centred(stage.logical_size(), size), true);
		stage.update_surface_scale();
		let focused = stage.focused;
		let tracked = &mut self.windows[index];
		tracked.stage = Some(conn);
		tracked.announced = Some(Instant::now());
		if focused {
			self.focus_stage(conn);
		}
	}

	fn detach_from_stage(&mut self, id: u32) {
		let Some(tracked) = self.windows.iter_mut().find(|t| t.id == id) else { return };
		let Some(conn) = tracked.stage.take() else { return };
		let window = tracked.window.clone();
		if let Some(stage) = self.stages.get_mut(&conn) {
			stage.space.unmap_elem(&window);
			stage.children.retain(|child| *child != id);
			stage.dirty = true;
			if stage.main == Some(id) {
				stage.main = None;
				stage.out.send(to_android::CLOSED, Vec::new());
			}
		}
	}

	/// Gives the keyboard to whatever is on top in this stage.
	pub fn focus_stage(&mut self, conn: ConnId) {
		let Some(stage) = self.stages.get(&conn) else { return };
		let top = stage.children.last().copied().or(stage.main);
		let Some(top) = top else { return };
		let Some(tracked) = self.windows.iter().find(|t| t.id == top) else { return };
		let window = tracked.window.clone();
		let focus = self.keyboard_focus_for_window(&window);
		window.set_activated(true);
		if let Some(toplevel) = window.toplevel()
			&& toplevel.is_initial_configure_sent()
		{
			toplevel.send_pending_configure();
		}
		if let Some(surface) = window.x11_surface()
			&& let Some(xwm) = self.xwm.as_mut()
		{
			let _ = xwm.raise_window(surface);
		}
		if let Some(keyboard) = self.seat.get_keyboard() {
			keyboard.set_focus(self, Some(focus), next_serial());
		}
	}

	pub fn unfocus_stage(&mut self, conn: ConnId) {
		let Some(stage) = self.stages.get(&conn) else { return };
		let ids: Vec<u32> = stage.main.into_iter().chain(stage.children.iter().copied()).collect();
		let mut had_focus = false;
		let current = self.seat.get_keyboard().and_then(|keyboard| keyboard.current_focus());
		for tracked in self.windows.iter().filter(|t| ids.contains(&t.id)) {
			let window = tracked.window.clone();
			window.set_activated(false);
			if let Some(toplevel) = window.toplevel()
				&& toplevel.is_initial_configure_sent()
			{
				toplevel.send_pending_configure();
			}
			if let (Some(focus), Some(surface)) = (&current, window.wl_surface())
				&& focus.wl_surface().as_deref() == Some(&*surface)
			{
				had_focus = true;
			}
		}
		if had_focus && let Some(keyboard) = self.seat.get_keyboard() {
			keyboard.set_focus(self, None, next_serial());
		}
	}

	pub fn window_gone(&mut self, surface: &WlSurface) {
		let Some(index) = self.tracked_index_by_surface(surface) else { return };
		self.remove_window(index);
	}

	pub fn x11_window_gone(&mut self, surface: &X11Surface) {
		let Some(index) = self.windows.iter().position(|t| t.window.x11_surface() == Some(surface)) else {
			return;
		};
		self.remove_window(index);
	}

	fn remove_window(&mut self, index: usize) {
		let tracked = self.windows.remove(index);
		let Some(conn) = tracked.stage else { return };
		let Some(stage) = self.stages.get_mut(&conn) else { return };
		stage.space.unmap_elem(&tracked.window);
		stage.dirty = true;
		stage.children.retain(|child| *child != tracked.id);
		if stage.main != Some(tracked.id) {
			if stage.focused {
				self.focus_stage(conn);
			}
			return;
		}
		stage.main = None;
		if let Some(promoted) = stage.children.first().copied() {
			stage.children.remove(0);
			self.attach_main(conn, promoted);
		} else {
			stage.waiting =
				Some(Waiting { until: Instant::now() + REPLACEMENT_GRACE, launched_pid: None, since_launch: false });
		}
	}

	/// Ends the waits that have run out: Android windows whose app closed or never opened.
	pub fn check_waiting(&mut self) {
		let now = Instant::now();
		for stage in self.stages.values_mut() {
			let Some(waiting) = &stage.waiting else { continue };
			if now < waiting.until {
				continue;
			}
			if waiting.since_launch {
				let message = format!("The app didn't open a window within {} seconds.", LAUNCH_TIMEOUT.as_secs());
				stage.out.send(to_android::ERROR, Payload::default().string(&message).0);
			} else {
				stage.out.send(to_android::CLOSED, Vec::new());
			}
			stage.waiting = None;
		}
	}

	pub fn title_changed_for(&mut self, surface: &WlSurface) {
		let Some(index) = self.tracked_index_by_surface(surface) else { return };
		let title = window_title(&self.windows[index].window);
		if title == self.windows[index].last_title {
			return;
		}
		self.windows[index].last_title = title.clone();
		let id = self.windows[index].id;
		let Some(conn) = self.windows[index].stage else { return };
		if let Some(stage) = self.stages.get(&conn)
			&& stage.main == Some(id)
		{
			stage.out.send(to_android::TITLE, Payload::default().string(&title).0);
		}
	}

	pub fn app_id_changed_for(&mut self, surface: &WlSurface) {
		let Some(index) = self.tracked_index_by_surface(surface) else { return };
		let window = self.windows[index].window.clone();
		self.windows[index].desktop_id = self.resolve_desktop_id(&window);
	}

	pub fn map_x11_override(&mut self, surface: X11Surface) {
		let geometry = surface.geometry();
		let target = surface
			.is_transient_for()
			.and_then(|parent| {
				self.windows.iter().find(|t| t.window.x11_surface().is_some_and(|s| s.window_id() == parent))
			})
			.and_then(|tracked| tracked.stage)
			.or_else(|| {
				self.stages
					.values()
					.filter(|stage| stage.main.is_some_and(|main| self.is_x11_window(main)))
					.max_by_key(|stage| stage.focused)
					.map(|stage| stage.conn)
			});
		let Some(stage) = target.and_then(|conn| self.stages.get_mut(&conn)) else { return };
		let window = Window::new_x11_window(surface);
		stage.overrides.push(window.clone());
		stage.map(window, geometry.loc, false);
	}

	fn is_x11_window(&self, id: u32) -> bool {
		self.windows.iter().any(|t| t.id == id && t.window.is_x11())
	}

	pub fn unmap_x11_override(&mut self, surface: &X11Surface) {
		for stage in self.stages.values_mut() {
			let found = stage.overrides.iter().position(|w| w.x11_surface() == Some(surface));
			if let Some(index) = found {
				let window = stage.overrides.remove(index);
				stage.space.unmap_elem(&window);
				stage.dirty = true;
			}
		}
	}

	pub fn x11_moved(&mut self, surface: &X11Surface, geometry: Rectangle<i32, Logical>) {
		if !surface.is_override_redirect() {
			return;
		}
		for stage in self.stages.values_mut() {
			let window = stage.overrides.iter().find(|w| w.x11_surface() == Some(surface)).cloned();
			if let Some(window) = window {
				stage.map(window, geometry.loc, false);
			}
		}
	}

	/// X11 apps can ask to be any size. A main window has to keep filling its Android window.
	pub fn x11_configure_request(&mut self, surface: &X11Surface, width: Option<u32>, height: Option<u32>) {
		let tracked = self.windows.iter().find(|t| t.window.x11_surface() == Some(surface));
		let stage = tracked.and_then(|t| t.stage.and_then(|conn| self.stages.get(&conn)).map(|s| (t.id, s)));
		if let Some((id, stage)) = stage
			&& stage.main == Some(id)
		{
			let _ = surface.configure(Rectangle::from_size(stage.view_logical_size()));
			return;
		}
		let mut geometry = surface.geometry();
		if let Some(width) = width {
			geometry.size.w = width as i32;
		}
		if let Some(height) = height {
			geometry.size.h = height as i32;
		}
		let _ = surface.configure(geometry);
	}

	/// Sends the cursor's new look to the Android window with the keyboard.
	pub fn send_cursor(&mut self) {
		let Some(stage) = self.stages.values().max_by_key(|stage| stage.focused) else { return };
		let payload = match &self.cursor {
			CursorImageStatus::Hidden => Payload::default().u8(0),
			CursorImageStatus::Named(icon) => Payload::default().u8(1).string(icon.name()),
			CursorImageStatus::Surface(surface) => match cursor_image(surface) {
				Some(image) => image,
				None => return,
			},
		};
		stage.out.send(to_android::CURSOR, payload.0);
	}

	pub fn cursor_committed(&mut self, surface: &WlSurface) {
		if matches!(&self.cursor, CursorImageStatus::Surface(cursor) if cursor == surface) {
			self.send_cursor();
		}
	}
}

fn centred(area: Size<i32, Logical>, size: Size<i32, Logical>) -> Point<i32, Logical> {
	(((area.w - size.w) / 2).max(0), ((area.h - size.h) / 2).max(0)).into()
}

/// Tells a window to fill an area. Telling it it's maximized and tiled makes apps drop the shadows and
/// rounded corners that would otherwise show as a gap around the edge.
pub fn configure_window(window: &Window, size: Size<i32, Logical>, activated: bool) {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => {
			toplevel.with_pending_state(|state| {
				state.size = Some(size);
				state.states.set(xdg_toplevel::State::Maximized);
				state.states.set(xdg_toplevel::State::TiledLeft);
				state.states.set(xdg_toplevel::State::TiledRight);
				state.states.set(xdg_toplevel::State::TiledTop);
				state.states.set(xdg_toplevel::State::TiledBottom);
				if activated {
					state.states.set(xdg_toplevel::State::Activated);
				}
			});
			if toplevel.is_initial_configure_sent() {
				toplevel.send_pending_configure();
			}
		}
		WindowSurface::X11(surface) => {
			let _ = surface.set_maximized(true);
			let _ = surface.configure(Rectangle::from_size(size));
		}
	}
}

/// Whether a window has ever had something to show.
fn has_drawn(window: &Window) -> bool {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => {
			with_renderer_surface_state(toplevel.wl_surface(), |state| state.buffer().is_some()).unwrap_or(false)
		}
		WindowSurface::X11(surface) => surface.is_mapped(),
	}
}

/// The smallest size a window says it can be, which not every app does.
fn declared_min_size(window: &Window) -> Size<i32, Logical> {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => with_states(toplevel.wl_surface(), |states| {
			states.cached_state.get::<SurfaceCachedState>().current().min_size
		}),
		WindowSurface::X11(surface) => surface.min_size().unwrap_or_default(),
	}
}

/// Whether a window has drawn what it was last told to. An app that won't go that small counts, since
/// it has answered, and a size it refuses is dealt with by scaling the picture.
fn has_reached(window: &Window, size: Size<i32, Logical>) -> bool {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => toplevel.current_state().size == Some(size),
		WindowSurface::X11(surface) => surface.geometry().size == size,
	}
}

/// Tells a dialog how big it can be, leaving the choice to it unless it's too big to fit.
fn configure_dialog(window: &Window, size: Size<i32, Logical>, limit: Size<i32, Logical>) {
	match window.underlying_surface() {
		WindowSurface::Wayland(toplevel) => {
			toplevel.with_pending_state(|state| {
				state.bounds = Some(limit);
				state.size = (size.w > 0 && size.h > 0).then_some(size);
				state.states.set(xdg_toplevel::State::Activated);
			});
			if toplevel.is_initial_configure_sent() {
				toplevel.send_pending_configure();
			}
		}
		WindowSurface::X11(surface) => {
			let geometry = surface.geometry();
			let fitted = Size::from((geometry.size.w.min(limit.w), geometry.size.h.min(limit.h)));
			let centre = centred(limit, fitted);
			let _ = surface.configure(Rectangle::new(centre, fitted));
		}
	}
}

/// Copies a cursor made by an app out of its buffer, as RGBA.
fn cursor_image(surface: &WlSurface) -> Option<Payload> {
	use smithay::backend::renderer::utils::RendererSurfaceStateUserData;
	use smithay::reexports::wayland_server::protocol::wl_shm::Format;
	use smithay::wayland::shm::with_buffer_contents;
	with_states(surface, |states| {
		let hotspot = states.data_map.get::<CursorImageSurfaceData>()?.lock().ok()?.hotspot;
		let renderer_state = states.data_map.get::<RendererSurfaceStateUserData>()?.lock().ok()?;
		let scale = renderer_state.buffer_scale();
		let buffer = renderer_state.buffer()?;
		with_buffer_contents(buffer, |pointer, length, data| {
			if !matches!(data.format, Format::Argb8888 | Format::Xrgb8888) {
				return None;
			}
			let needed = data.offset as usize + (data.height * data.stride) as usize;
			if length < needed {
				return None;
			}
			// Safety: the bounds were just checked against the shared memory's length.
			let source = unsafe { std::slice::from_raw_parts(pointer, length) };
			let mut rgba = Vec::with_capacity((data.width * data.height * 4) as usize);
			for y in 0..data.height as usize {
				let start = data.offset as usize + y * data.stride as usize;
				for pixel in source[start..start + data.width as usize * 4].as_chunks::<4>().0 {
					rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
				}
			}
			Some(
				Payload::default()
					.u8(2)
					.i32(hotspot.x * scale)
					.i32(hotspot.y * scale)
					.u32(data.width as u32)
					.u32(data.height as u32)
					.bytes(&rgba),
			)
		})
		.ok()?
	})
}

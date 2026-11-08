//! Airi's Linux side. It's a Wayland compositor with no screen of its own: each Linux window gets a
//! private screen the size of an Android window, and goes to Android over a forwarded TCP port.

mod apps;
mod handler;
mod launch;
mod net;
mod proto;
mod stage;
mod state;
mod windows;

use std::{collections::HashMap, env, fs, path::PathBuf, sync::Arc, time::Instant};

use anyhow::{Context, bail};
use smithay::{
	backend::renderer::pixman::PixmanRenderer,
	desktop::PopupManager,
	input::pointer::CursorImageStatus,
	output::{Output, PhysicalProperties, Subpixel},
	reexports::{
		calloop::{
			EventLoop, Interest, Mode, PostAction,
			channel::{self, Event as ChannelEvent},
			generic::Generic,
		},
		wayland_server::Display,
	},
	utils::Clock,
	wayland::socket::ListeningSocketSource,
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::state::{ClientState, Event, Globals, State};

const SOCKET_NAME: &str = "airi-0";

fn config_dir() -> anyhow::Result<PathBuf> {
	if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
		return Ok(PathBuf::from(dir).join("airi"));
	}
	let home = env::var_os("HOME").context("HOME isn't set")?;
	Ok(PathBuf::from(home).join(".config/airi"))
}

fn read_token(path: Option<PathBuf>) -> anyhow::Result<Vec<u8>> {
	let path = match path {
		Some(path) => path,
		None => config_dir()?.join("token"),
	};
	let text = fs::read_to_string(&path).with_context(|| format!("couldn't read {}", path.display()))?;
	let text = text.trim();
	if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
		bail!("{} doesn't hold a 64-digit hex token", path.display());
	}
	(0..32).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).context("bad token")).collect()
}

fn serve(port: u16, token_path: Option<PathBuf>) -> anyhow::Result<()> {
	if env::var_os("XDG_RUNTIME_DIR").is_none() {
		bail!("XDG_RUNTIME_DIR isn't set");
	}
	let token = read_token(token_path)?;

	let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
	let display: Display<State> = Display::new()?;
	let mut dh = display.handle();

	let globals = Globals::new(&dh)?;
	let renderer = PixmanRenderer::new().map_err(|err| anyhow::anyhow!("couldn't start the renderer: {err:?}"))?;

	let (events, events_rx) = channel::channel::<Event>();
	let (net_events, net_rx) = channel::channel::<net::NetEvent>();

	let source = ListeningSocketSource::with_name(SOCKET_NAME)?;
	let socket_name = source.socket_name().to_string_lossy().into_owned();
	event_loop
		.handle()
		.insert_source(source, |stream, _, state: &mut State| {
			if let Err(err) = state.dh.insert_client(stream, Arc::new(ClientState::default())) {
				warn!(%err, "couldn't add a client");
			}
		})
		.map_err(|err| anyhow::anyhow!("couldn't watch the Wayland socket: {err}"))?;
	event_loop
		.handle()
		.insert_source(Generic::new(display, Interest::READ, Mode::Level), |_, display, state| {
			// Safety: the display isn't dropped while the event loop holds it.
			unsafe {
				display.get_mut().dispatch_clients(state)?;
			}
			Ok(PostAction::Continue)
		})
		.map_err(|err| anyhow::anyhow!("couldn't watch Wayland clients: {err}"))?;
	event_loop
		.handle()
		.insert_source(events_rx, |event, _, state: &mut State| {
			if let ChannelEvent::Msg(event) = event {
				state.on_event(event);
			}
		})
		.map_err(|err| anyhow::anyhow!("couldn't watch events: {err}"))?;
	event_loop
		.handle()
		.insert_source(net_rx, |event, _, state: &mut State| {
			if let ChannelEvent::Msg(event) = event {
				state.on_event(Event::Net(event));
			}
		})
		.map_err(|err| anyhow::anyhow!("couldn't watch the network: {err}"))?;

	let idle_output = Output::new(
		"airi-idle".into(),
		PhysicalProperties {
			size: (0, 0).into(),
			subpixel: Subpixel::Unknown,
			make: "Airi".into(),
			model: "Not on screen".into(),
		},
	);

	let mut state = State {
		dh: dh.clone(),
		handle: event_loop.handle(),
		events,
		clock: Clock::new(),
		compositor: globals.compositor,
		shm: globals.shm,
		xdg: globals.xdg,
		decoration: globals.decoration,
		seat_state: globals.seat_state,
		seat: globals.seat,
		data_device: globals.data_device,
		primary: globals.primary,
		_outputs: globals.outputs,
		_viewporter: globals.viewporter,
		_fractional: globals.fractional,
		_single_pixel: globals.single_pixel,
		xwayland_shell: globals.xwayland_shell,
		popups: PopupManager::default(),
		renderer,
		xwm: None,
		x_display: None,
		apps: apps::scan(),
		socket_name,
		conns: HashMap::new(),
		stages: HashMap::new(),
		windows: Vec::new(),
		next_window_id: 1,
		idle_output,
		last_idle_frames: Instant::now(),
		ime_wanted: false,
		ime_off_since: None,
		android_clipboard: None,
		linux_clipboard: None,
		cursor: CursorImageStatus::default_named(),
	};
	launch::write_env_file(&state);
	state.start_xwayland();
	net::listen(port, token, net_events)?;
	info!(socket = SOCKET_NAME, "ready");

	loop {
		let wake = state.next_wake();
		event_loop.dispatch(Some(wake), &mut state)?;
		state.tick();
		if let Err(err) = dh.flush_clients() {
			warn!(%err, "couldn't flush clients");
		}
	}
}

const USAGE: &str = "\
airi-agent: Airi's Linux side

    airi-agent serve [--port N] [--token-file PATH]   Run the compositor
    airi-agent run COMMAND [ARGS...]                  Run an app so that it shows up in Android
    airi-agent list                                   List the apps Airi can see
    airi-agent version                                Print the version
";

fn main() -> anyhow::Result<()> {
	tracing_subscriber::fmt()
		.with_env_filter(
			EnvFilter::try_from_env("AIRI_LOG").unwrap_or_else(|_| EnvFilter::new("airi_agent=info,smithay=warn")),
		)
		.without_time()
		.with_writer(std::io::stderr)
		.init();

	let args: Vec<String> = env::args().skip(1).collect();
	match args.first().map(String::as_str) {
		Some("serve") => {
			let mut port = proto::DEFAULT_PORT;
			let mut token_path = None;
			let mut rest = args[1..].iter();
			while let Some(arg) = rest.next() {
				match arg.as_str() {
					"--port" => port = rest.next().context("--port needs a number")?.parse()?,
					"--token-file" => {
						token_path = Some(PathBuf::from(rest.next().context("--token-file needs a path")?))
					}
					other => bail!("unknown option {other}\n\n{USAGE}"),
				}
			}
			serve(port, token_path)
		}
		Some("run") if args.len() > 1 => launch::exec_with_saved_env(&args[1..]),
		Some("list") => {
			for app in apps::scan() {
				println!("{}\t{}", app.id, app.name);
			}
			Ok(())
		}
		Some("version") => {
			println!("{}", env!("CARGO_PKG_VERSION"));
			Ok(())
		}
		_ => {
			print!("{USAGE}");
			Ok(())
		}
	}
}

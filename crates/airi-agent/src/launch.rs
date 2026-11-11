//! Starts Linux apps pointed at this compositor, and reports if they fail.

use std::{
	collections::VecDeque,
	env, fs,
	io::{BufRead, BufReader},
	os::unix::{fs::MetadataExt, process::CommandExt},
	path::PathBuf,
	process::{Command, Stdio},
	sync::OnceLock,
	thread,
};

use smithay::reexports::calloop::channel::Sender;
use tracing::{info, warn};

use crate::{
	apps::{self, App},
	net::ConnId,
	state::{Event, State},
};

/// A launched process ending, which matters if it happens before it opened a window.
pub struct Launched {
	pub conn: ConnId,
	pub success: bool,
	pub output_tail: String,
}

/// The environment that points apps here. Graphics settings follow the Terminal app's own scripts,
/// which pick them by whether Android gave the VM a GPU.
pub fn client_env(state: &State) -> Vec<(String, String)> {
	let mut vars: Vec<(String, String)> = vec![
		("WAYLAND_DISPLAY", state.socket_name.as_str()),
		("XDG_SESSION_TYPE", "wayland"),
		("XDG_CURRENT_DESKTOP", "Airi"),
		("GDK_BACKEND", "wayland,x11"),
		("QT_QPA_PLATFORM", "wayland;xcb"),
		("SDL_VIDEODRIVER", "wayland,x11"),
		("MOZ_ENABLE_WAYLAND", "1"),
		("ELECTRON_OZONE_PLATFORM_HINT", "auto"),
	]
	.into_iter()
	.map(|(k, v)| (k.to_string(), v.to_string()))
	.collect();
	if let Some(display) = state.x_display {
		vars.push(("DISPLAY".into(), format!(":{display}")));
	}
	if other_session(state)
		&& let Some(address) = own_bus()
	{
		vars.push(("DBUS_SESSION_BUS_ADDRESS".into(), address));
	}
	let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
	let exists = |path: &str| fs::metadata(path).is_ok();
	let gfxstream = "/usr/share/vulkan/icd.d/gfxstream_vk_icd.json";
	let lavapipe = "/usr/share/vulkan/icd.d/lvp_icd.json";
	if cmdline.split_whitespace().any(|word| word == "gfxstream_enabled") && exists(gfxstream) {
		for (k, v) in [
			("MESA_LOADER_DRIVER_OVERRIDE", "zink"),
			("VK_ICD_FILENAMES", gfxstream),
			("VK_DRIVER_FILES", gfxstream),
			("MESA_VK_WSI_DEBUG", "sw"),
			("XWAYLAND_NO_GLAMOR", "1"),
			("LIBGL_KOPPER_DRI2", "1"),
		] {
			vars.push((k.into(), v.into()));
		}
	} else if exists(lavapipe) {
		for (k, v) in [
			("MESA_LOADER_DRIVER_OVERRIDE", "zink"),
			("LIBGL_ALWAYS_SOFTWARE", "1"),
			("VK_ICD_FILENAMES", lavapipe),
			("VK_DRIVER_FILES", lavapipe),
		] {
			vars.push((k.into(), v.into()));
		}
	}
	vars
}

/// Whether this user has another graphical session running, such as the desktop a Minori machine shows over VNC. Apps
/// that keep one copy running, like Thunar, Xfce Terminal and Mousepad, find each other on the session bus, so on a
/// shared bus they'd hand their windows to the copies on that other desktop, and nothing would open here.
fn other_session(state: &State) -> bool {
	let uid = unsafe { libc::getuid() };
	let ours = |path: &std::path::Path| fs::metadata(path).map(|meta| meta.uid() == uid).unwrap_or(false);
	let x11 = fs::read_dir("/tmp/.X11-unix").into_iter().flatten().flatten().any(|entry| {
		let name = entry.file_name().to_string_lossy().into_owned();
		let display = name.strip_prefix('X').and_then(|n| n.parse::<u32>().ok());
		display.is_some() && display != state.x_display && ours(&entry.path())
	});
	let wayland =
		env::var_os("XDG_RUNTIME_DIR").and_then(|dir| fs::read_dir(dir).ok()).into_iter().flatten().flatten().any(
			|entry| {
				let name = entry.file_name().to_string_lossy().into_owned();
				name.starts_with("wayland-") && !name.ends_with(".lock") && name != state.socket_name
			},
		);
	x11 || wayland
}

/// A session bus for the apps Airi starts, apart from the user's, made the first time it's needed and kept for as long as
/// the agent runs. None if `dbus-daemon` isn't there, and then apps share the user's bus as before.
fn own_bus() -> Option<String> {
	static BUS: OnceLock<Option<String>> = OnceLock::new();
	BUS.get_or_init(|| {
		let mut command = Command::new("dbus-daemon");
		command
			.args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::null());
		// It goes when the agent does, so restarts don't leave buses behind.
		unsafe {
			command.pre_exec(|| {
				libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
				Ok(())
			});
		}
		let mut child = command.spawn().map_err(|err| warn!(%err, "couldn't start a session bus for apps")).ok()?;
		let mut address = String::new();
		BufReader::new(child.stdout.take()?).read_line(&mut address).ok()?;
		let address = address.trim().to_string();
		info!(%address, "apps get a session bus of their own, since another desktop is running");
		// Reap it if it ever ends.
		thread::spawn(move || child.wait());
		(!address.is_empty()).then_some(address)
	})
	.clone()
}

fn env_file() -> Option<PathBuf> {
	env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join("airi-env"))
}

/// Saves the environment so that `airi-agent run` can find it.
pub fn write_env_file(state: &State) {
	let Some(path) = env_file() else { return };
	let text: String = client_env(state).iter().map(|(k, v)| format!("{k}={v}\n")).collect();
	if let Err(err) = fs::write(&path, text) {
		warn!(%err, "couldn't save the environment for apps");
	}
}

/// Runs a command with this compositor's environment, for `airi-agent run`.
pub fn exec_with_saved_env(command: &[String]) -> anyhow::Result<()> {
	use anyhow::Context;
	let path = env_file().context("XDG_RUNTIME_DIR isn't set")?;
	let text = fs::read_to_string(&path).context("the Airi agent isn't running")?;
	let mut process = Command::new(&command[0]);
	process.args(&command[1..]);
	for line in text.lines() {
		if let Some((key, value)) = line.split_once('=') {
			process.env(key, value);
		}
	}
	Err(process.exec().into())
}

/// Starts an app for a waiting Android window, returning its process ID.
pub fn launch(app: &App, env_vars: &[(String, String)], conn: ConnId, events: Sender<Event>) -> std::io::Result<u32> {
	let argv = apps::exec_argv(app);
	let Some(program) = argv.first() else {
		return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "the app has no command"));
	};
	info!(app = app.id, ?argv, "launching");
	let mut command = Command::new(program);
	command
		.args(&argv[1..])
		.envs(env_vars.iter().map(|(k, v)| (k, v)))
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	if let Some(home) = env::var_os("HOME") {
		command.current_dir(home);
	}
	let mut child = command.spawn()?;
	let pid = child.id();
	let stdout = child.stdout.take();
	let stderr = child.stderr.take();
	thread::spawn(move || {
		// Keep the end of its output, in case it's about to explain why it failed.
		let (lines_tx, lines_rx) = std::sync::mpsc::channel::<String>();
		let forward = |stream: Option<Box<dyn std::io::Read + Send>>, tx: std::sync::mpsc::Sender<String>| {
			thread::spawn(move || {
				if let Some(stream) = stream {
					for line in BufReader::new(stream).lines().map_while(Result::ok) {
						if tx.send(line).is_err() {
							break;
						}
					}
				}
			})
		};
		let out = forward(stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), lines_tx.clone());
		let err = forward(stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), lines_tx);
		let mut tail: VecDeque<String> = VecDeque::with_capacity(8);
		for line in lines_rx {
			if tail.len() == 8 {
				tail.pop_front();
			}
			tail.push_back(line);
		}
		let _ = out.join();
		let _ = err.join();
		let success = child.wait().map(|status| status.success()).unwrap_or(false);
		let output_tail = tail.into_iter().collect::<Vec<_>>().join("\n");
		let _ = events.send(Event::Launched(Launched { conn, success, output_tail }));
	});
	Ok(pid)
}

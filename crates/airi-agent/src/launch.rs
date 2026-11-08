//! Starts Linux apps pointed at this compositor, and reports if they fail.

use std::{
	collections::VecDeque,
	env, fs,
	io::{BufRead, BufReader},
	path::PathBuf,
	process::{Command, Stdio},
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
	use std::os::unix::process::CommandExt;
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

//! Finds the graphical apps installed in Linux, from their `.desktop` files.

use std::{
	collections::{HashMap, HashSet},
	env, fs,
	path::{Path, PathBuf},
	sync::{Arc, OnceLock},
};

use serde::Serialize;

/// What Android needs to list an app. The id is the file name of its `.desktop` file.
#[derive(Clone, Debug, Serialize)]
pub struct App {
	pub id: String,
	pub name: String,
	pub comment: String,
	#[serde(skip)]
	pub exec: String,
	#[serde(skip)]
	pub icon: String,
	#[serde(skip)]
	pub wm_class: String,
}

/// Entries from the Terminal app's image that aren't apps anyone would open.
const NOT_APPS: &[&str] = &["url_forwarder.desktop", "org.freedesktop.Xwayland.desktop"];

/// Finds where `.desktop` files and icons can live, nearest first.
fn data_dirs() -> Vec<PathBuf> {
	let mut dirs = Vec::new();
	let home = env::var_os("HOME").map(PathBuf::from);
	match env::var_os("XDG_DATA_HOME") {
		Some(dir) => dirs.push(PathBuf::from(dir)),
		None => {
			if let Some(home) = &home {
				dirs.push(home.join(".local/share"));
			}
		}
	}
	if let Some(home) = &home {
		dirs.push(home.join(".local/share/flatpak/exports/share"));
	}
	dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
	match env::var("XDG_DATA_DIRS") {
		Ok(list) if !list.is_empty() => dirs.extend(list.split(':').map(PathBuf::from)),
		_ => {
			dirs.push(PathBuf::from("/usr/local/share"));
			dirs.push(PathBuf::from("/usr/share"));
		}
	}
	let mut seen = HashSet::new();
	dirs.retain(|dir| seen.insert(dir.clone()));
	dirs
}

fn on_path(program: &str) -> bool {
	if program.contains('/') {
		return Path::new(program).exists();
	}
	env::var_os("PATH").is_some_and(|path| env::split_paths(&path).any(|dir| dir.join(program).exists()))
}

/// Reads the `[Desktop Entry]` group of a desktop file.
fn parse_entry(text: &str) -> HashMap<&str, &str> {
	let mut fields = HashMap::new();
	let mut in_entry = false;
	for line in text.lines() {
		let line = line.trim();
		if line.starts_with('[') {
			in_entry = line == "[Desktop Entry]";
		} else if in_entry
			&& !line.starts_with('#')
			&& let Some((key, value)) = line.split_once('=')
		{
			fields.entry(key.trim()).or_insert(value.trim());
		}
	}
	fields
}

fn unescape(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	let mut chars = value.chars();
	while let Some(c) = chars.next() {
		if c != '\\' {
			out.push(c);
			continue;
		}
		match chars.next() {
			Some('s') => out.push(' '),
			Some('n') => out.push('\n'),
			Some('t') => out.push('\t'),
			Some('r') => out.push('\r'),
			Some('\\') => out.push('\\'),
			Some(other) => {
				out.push('\\');
				out.push(other);
			}
			None => {}
		}
	}
	out
}

fn parse_app(id: String, text: &str) -> Option<App> {
	let fields = parse_entry(text);
	let truthy = |key: &str| fields.get(key).is_some_and(|v| *v == "true");
	if fields.get("Type").copied() != Some("Application")
		|| truthy("NoDisplay")
		|| truthy("Hidden")
		|| truthy("Terminal")
		|| fields.contains_key("OnlyShowIn")
		|| NOT_APPS.contains(&id.as_str())
	{
		return None;
	}
	if let Some(program) = fields.get("TryExec")
		&& !on_path(&unescape(program))
	{
		return None;
	}
	let exec = unescape(fields.get("Exec")?);
	let name = unescape(fields.get("Name")?);
	Some(App {
		id,
		name,
		comment: unescape(fields.get("Comment").copied().unwrap_or("")),
		exec,
		icon: unescape(fields.get("Icon").copied().unwrap_or("")),
		wm_class: fields.get("StartupWMClass").copied().unwrap_or("").to_string(),
	})
}

/// Lists every app, sorted by name. A file nearer the user hides one of the same name further away.
pub fn scan() -> Vec<App> {
	let mut seen = HashSet::new();
	let mut apps = Vec::new();
	for dir in data_dirs() {
		let Ok(entries) = fs::read_dir(dir.join("applications")) else { continue };
		for entry in entries.flatten() {
			let id = entry.file_name().to_string_lossy().into_owned();
			if !id.ends_with(".desktop") || !seen.insert(id.clone()) {
				continue;
			}
			let Ok(text) = fs::read_to_string(entry.path()) else { continue };
			if let Some(app) = parse_app(id, &text) {
				apps.push(app);
			}
		}
	}
	apps.sort_by_key(|app| app.name.to_lowercase());
	apps
}

/// Splits an `Exec` line into a program and arguments, dropping the placeholders for files and URLs.
pub fn exec_argv(app: &App) -> Vec<String> {
	let mut words: Vec<String> = Vec::new();
	let mut current = String::new();
	let mut in_word = false;
	let mut quote = None;
	let mut chars = app.exec.chars();
	while let Some(c) = chars.next() {
		match (quote, c) {
			(Some(q), c) if c == q => quote = None,
			(Some('"'), '\\') => {
				if let Some(next) = chars.next() {
					current.push(next);
				}
			}
			(Some(_), c) => current.push(c),
			(None, '"' | '\'') => {
				quote = Some(c);
				in_word = true;
			}
			(None, '\\') => {
				if let Some(next) = chars.next() {
					current.push(next);
					in_word = true;
				}
			}
			(None, c) if c.is_whitespace() => {
				if in_word {
					words.push(std::mem::take(&mut current));
					in_word = false;
				}
			}
			(None, c) => {
				current.push(c);
				in_word = true;
			}
		}
	}
	if in_word {
		words.push(current);
	}
	let mut argv = Vec::with_capacity(words.len());
	for word in words {
		match word.as_str() {
			"%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" | "%k" => {}
			"%i" => {
				if !app.icon.is_empty() {
					argv.push("--icon".to_string());
					argv.push(app.icon.clone());
				}
			}
			"%c" => argv.push(app.name.clone()),
			_ => argv.push(word.replace("%%", "%")),
		}
	}
	argv
}

/// Works out which app a window belongs to, from its Wayland app id or X11 class.
pub fn find_for_window<'a>(apps: &'a [App], window_id: &str) -> Option<&'a App> {
	if window_id.is_empty() {
		return None;
	}
	let wanted = window_id.to_lowercase();
	let wanted = wanted.strip_suffix(".desktop").unwrap_or(&wanted);
	let by_id = |app: &&App| app.id.trim_end_matches(".desktop").to_lowercase() == wanted;
	let by_class = |app: &&App| !app.wm_class.is_empty() && app.wm_class.to_lowercase() == wanted;
	let by_program = |app: &&App| {
		exec_argv(app)
			.first()
			.and_then(|program| Path::new(program).file_name().map(|n| n.to_string_lossy().to_lowercase()))
			.is_some_and(|program| program == wanted)
	};
	// Android-style reverse domain names often end in the name a program goes by.
	let by_last_part = |app: &&App| {
		let id = app.id.trim_end_matches(".desktop").to_lowercase();
		id.rsplit('.').next() == Some(wanted) || wanted.rsplit('.').next() == Some(id.as_str())
	};
	apps.iter()
		.find(by_id)
		.or_else(|| apps.iter().find(by_class))
		.or_else(|| apps.iter().find(by_program))
		.or_else(|| apps.iter().find(by_last_part))
}

const SIZES: &[&str] =
	&["scalable", "512x512", "384x384", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64", "48x48"];

/// Looks an icon name up in the icon themes, largest first.
fn find_icon_file(name: &str) -> Option<PathBuf> {
	if name.is_empty() {
		return None;
	}
	if Path::new(name).is_absolute() {
		return Path::new(name).exists().then(|| PathBuf::from(name));
	}
	let dirs = data_dirs();
	for dir in &dirs {
		let hicolor = dir.join("icons/hicolor");
		if let Some(found) = find_in_theme(&hicolor, name) {
			return Some(found);
		}
	}
	for dir in &dirs {
		let Ok(themes) = fs::read_dir(dir.join("icons")) else { continue };
		let mut themes: Vec<PathBuf> = themes.flatten().map(|entry| entry.path()).collect();
		themes.sort();
		for theme in themes {
			if let Some(found) = find_in_theme(&theme, name) {
				return Some(found);
			}
		}
	}
	for dir in &dirs {
		for extension in ["svg", "png"] {
			let path = dir.join("pixmaps").join(format!("{name}.{extension}"));
			if path.exists() {
				return Some(path);
			}
		}
	}
	None
}

fn find_in_theme(theme: &Path, name: &str) -> Option<PathBuf> {
	for size in SIZES {
		for layout in [format!("{size}/apps"), format!("apps/{size}")] {
			for extension in ["svg", "png"] {
				let path = theme.join(&layout).join(format!("{name}.{extension}"));
				if path.exists() {
					return Some(path);
				}
			}
		}
	}
	None
}

const SVG_ICON_SIZE: u32 = 384;

fn render_svg(data: &[u8]) -> Option<Vec<u8>> {
	let mut options = resvg::usvg::Options::default();
	// Some icons are drawn with text, and that comes out blank without fonts to draw it in. Finding
	// the fonts is slow, so it's only done for those, and only once.
	if data.windows(5).any(|window| window == b"<text") {
		static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
		options.fontdb = FONTS
			.get_or_init(|| {
				let mut fonts = resvg::usvg::fontdb::Database::new();
				fonts.load_system_fonts();
				Arc::new(fonts)
			})
			.clone();
	}
	let tree = resvg::usvg::Tree::from_data(data, &options).ok()?;
	let size = tree.size();
	let scale = SVG_ICON_SIZE as f32 / size.width().max(size.height());
	let mut pixmap = resvg::tiny_skia::Pixmap::new(SVG_ICON_SIZE, SVG_ICON_SIZE)?;
	let dx = (SVG_ICON_SIZE as f32 - size.width() * scale) / 2.0;
	let dy = (SVG_ICON_SIZE as f32 - size.height() * scale) / 2.0;
	let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
	resvg::render(&tree, transform, &mut pixmap.as_mut());
	// A blank icon is worse than none, because none gets a stand-in and blank leaves a gap.
	if pixmap.pixels().iter().all(|pixel| pixel.alpha() == 0) {
		return None;
	}
	pixmap.encode_png().ok()
}

/// Returns an app's icon as a PNG, if it has one this can read.
pub fn icon_png(app: &App) -> Option<Vec<u8>> {
	let path = find_icon_file(&app.icon)?;
	let data = fs::read(&path).ok()?;
	match path.extension().and_then(|e| e.to_str()) {
		Some("png") => Some(data),
		Some("svg" | "svgz") => render_svg(&data),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn app(id: &str, exec: &str, wm_class: &str) -> App {
		App {
			id: id.into(),
			name: id.into(),
			comment: String::new(),
			exec: exec.into(),
			icon: String::new(),
			wm_class: wm_class.into(),
		}
	}

	#[test]
	fn hidden_and_terminal_entries_are_skipped() {
		let base = "[Desktop Entry]\nType=Application\nName=Thing\nExec=thing\n";
		assert!(parse_app("a.desktop".into(), base).is_some());
		for extra in ["NoDisplay=true\n", "Hidden=true\n", "Terminal=true\n", "OnlyShowIn=GNOME;\n"] {
			assert!(parse_app("a.desktop".into(), &format!("{base}{extra}")).is_none(), "{extra}");
		}
		assert!(parse_app("url_forwarder.desktop".into(), base).is_none());
	}

	#[test]
	fn only_the_main_group_counts() {
		let text =
			"[Desktop Entry]\nType=Application\nName=Real\nExec=real\n[Desktop Action new]\nName=Other\nExec=other\n";
		let app = parse_app("a.desktop".into(), text).unwrap();
		assert_eq!(app.name, "Real");
		assert_eq!(app.exec, "real");
	}

	#[test]
	fn exec_lines_are_split_and_placeholders_dropped() {
		let argv = exec_argv(&app("a", r#"env FOO="a b" thing --flag %U 100%%"#, ""));
		assert_eq!(argv, ["env", "FOO=a b", "thing", "--flag", "100%"]);
		let quoted = exec_argv(&app("a", r#""/opt/My App/run" 'one two' %f"#, ""));
		assert_eq!(quoted, ["/opt/My App/run", "one two"]);
	}

	#[test]
	fn windows_are_matched_to_apps() {
		let apps = vec![
			app("org.gnome.Calculator.desktop", "gnome-calculator", ""),
			app("debian-xterm.desktop", "xterm", ""),
			app("firefox-esr.desktop", "/usr/lib/firefox-esr/firefox-esr %u", "firefox-esr"),
			app("code.desktop", "/usr/share/code/code", "Code"),
			app("org.xfce.mousepad.desktop", "mousepad", ""),
		];
		let find = |id| find_for_window(&apps, id).map(|a| a.id.as_str());
		assert_eq!(find("org.gnome.Calculator"), Some("org.gnome.Calculator.desktop"));
		assert_eq!(find("XTerm"), Some("debian-xterm.desktop"));
		assert_eq!(find("firefox-esr"), Some("firefox-esr.desktop"));
		assert_eq!(find("Code"), Some("code.desktop"));
		assert_eq!(find("mousepad"), Some("org.xfce.mousepad.desktop"));
		assert_eq!(find(""), None);
		assert_eq!(find("something-else"), None);
	}
}

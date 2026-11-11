<p align="center">
  <img src="docs/logo.webp" alt="" width="180">
</p>

<h1 align="center">Airi</h1>

<p align="center">Linux apps on your Android launcher.</p>

<p align="center">
  <a href="https://github.com/wonderhoy-labs/airi/actions/workflows/ci.yml"><img src="https://github.com/wonderhoy-labs/airi/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License: Apache 2.0"></a>
  <a href="#license-and-credits"><img src="https://img.shields.io/badge/Momoi%20Airi-MORE%20MORE%20JUMP!-FFAACC" alt="Named for Momoi Airi of MORE MORE JUMP!"></a>
</p>

Android's Terminal app runs a real Debian in a virtual machine, and it can show graphical apps. The catch is that they
all live inside one Display window, as far as Android is concerned, in a little desktop of their own.

Airi takes them out of there. Each Linux app gets:
- its own icon on your launcher, next to your other apps
- its own window, with its own name and icon in Recents
- the size of whatever window Android gives it, including when you rotate or the keyboard opens
- touch and the on-screen keyboard

It doesn't need root, Shizuku or a computer. Setup takes one paste into the terminal.

![Airi's list of Linux apps, Calculator on the home screen, and Calculator open](docs/screenshot.webp)

> **This is early.** We've only tested it on one Pixel 10 Pro running Android 17, build `CP3A.260905.009`. Nothing in the code is tied to that
> model, so other phones with the Terminal app should work. If you try one, please
> [open an issue](https://github.com/wonderhoy-labs/airi/issues) and tell us how it went, even if it just worked.

## What you need

A phone with Android's Terminal app, which needs a 64-bit Arm phone that can run virtual machines. On a Pixel, turn on
**Linux development environment** in Developer options, then open the Terminal app and let it download Debian.

Linux needs its own apps, so install some in the terminal. For example:

```bash
sudo apt install gnome-calculator
```

## Install

Download the APK from [Releases](https://github.com/wonderhoy-labs/airi/releases) and open it on your phone, or install it
with `adb`:

```bash
adb install -r airi-v0.1.0.apk
```

You can also [build it yourself](#development).

## Set up

Android doesn't let apps run anything in Linux, so this is the one step you do by hand.
1. Open Airi and tap **Copy command and open Terminal**.
2. In the terminal, touch and hold, tap **Paste**, then press Enter.
3. When a notification asks about port 47821, tap **Allow always**. If you miss it, it's in the Terminal app's settings
   under **Security**.

Go back to Airi and your Linux apps are there. You only do this once, and the helper it installs starts by itself whenever
Linux does.

## Use it

**Launch** opens an app straight away.

**Add** puts it on your launcher as an app of its own. The first time, Android asks you to allow Airi to install apps, and then it asks
you to confirm each one. Play Protect hasn't seen these apps before, because they're made on your phone, so it may offer to scan each one.
**Remove** takes it off again.

Tapping one of these apps starts Linux if it isn't running. The Terminal app appears for a moment, and your app opens by itself about 30 to
45 seconds later.

| To do this           | Do this                                                                                                                   |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Leave an app running | Go home or swipe back, like any other app                                                                                 |
| Close an app         | Swipe it away in Recents, or use its own close button. An app with unsaved work may ask first, so open it again to answer |
| Show the keyboard    | It opens when a text field asks for it, or tap the round button on the right edge                                         |
| Paste into Linux     | Copy on Android, then go to the Linux app                                                                                 |

Some apps won't go as small as a phone, like Chromium, which needs 500 logical pixels of width. Airi zooms those out just enough to
fit, so they still fill the window. If you install something new in Linux, it shows up the next time you open Airi.

### From the terminal

To run something from the terminal and see it in Android, start it with `airi-agent`. It gets a window of its own when you next switch to
a Linux app, because Android doesn't let an app in the background open one.

```bash
~/.local/bin/airi-agent run mousepad
```

## Use it in your app

Any Android app can open Linux apps in Airi, and find the ones Airi has put on the launcher. The client library in
[`android/client/`](android/client/) does both. It's Java with no dependencies, and it runs on Android 8 and up, where
it just finds that Airi isn't there. Add it from Airi's releases in `settings.gradle.kts`:

```kotlin
dependencyResolutionManagement {
	repositories {
		google()
		mavenCentral()
		// Wonderhoy Labs libraries, straight from each project's GitHub releases.
		ivy {
			url = uri("https://github.com/wonderhoy-labs")
			patternLayout { artifact("[organisation]/releases/download/v[revision]/[organisation]-[module]-[revision].[ext]") }
			metadataSources { artifact() }
			content { includeGroup("airi") }
		}
	}
}
```

and in your module:

```kotlin
implementation("airi:client:0.3.0@aar")
```

Then:

```kotlin
if (Airi.isInstalled(context)) {
	// GNOME Calculator in a window of its own. Airi starts Linux first if it has to.
	startActivity(Airi.openIntent("org.gnome.Calculator.desktop"))
}

// The Linux apps on the launcher, with their names and icons.
for (app in Airi.launcherApps(context)) {
	Log.i("Linux", "${app.label} is ${app.desktopId}")
}
```

A Linux app goes by its desktop file ID, the name of its `.desktop` file, such as `org.gnome.Calculator.desktop`.
`ls /usr/share/applications` in the terminal lists most of them.

| Entry point                                     | What it does                                                                                                            |
| ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `Airi.isInstalled(context)`                     | Whether Airi is on the phone                                                                                            |
| `Airi.openIntent(desktopId)`                    | Opens a Linux app. With `startActivityForResult`, `RESULT_OK` means its window closed normally                          |
| `Airi.launcherApps(context)`                    | The Linux apps on the launcher, as `LinuxApp`s with a desktop file ID, name, package, `loadIcon()` and `launchIntent()` |
| `Airi.launcherApp(context, desktopId)`          | One of those, or null if that app isn't on the launcher                                                                 |
| `Airi.appUri(desktopId)`, `Airi.desktopId(uri)` | The `airi://app/<desktop file ID>` link, both ways                                                                      |

`openIntent` opens in your app's task, the way a launcher app of Airi's does. Add `Intent.FLAG_ACTIVITY_NEW_DOCUMENT`
to give it a task of its own, though then there's no result. For an app that's on the launcher, its `launchIntent()`
is better: it has one task, with its own name and icon in Recents.

The library is a thin layer over a contract, which also works without it. It won't change in a way that breaks an
app that uses it.

| Since | What                   | How                                                                                                                                                                                                 |
| ----- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.1.0 | Open a Linux app       | `ACTION_VIEW` on `airi://app/<desktop file ID>`, which Airi's `.viewer.ViewerActivity` takes. It sets `RESULT_OK` when the window closes normally                                                   |
| 0.1.0 | Find the launcher apps | Activities that answer `com.wonderhoylabs.airi.LINUX_APP`, each in a package of its own with the desktop file ID as text in its `desktop_id` asset. From Android 11, that needs a `<queries>` entry |
| 0.2.0 | The client library     | `airi-client-<version>.aar` on each release                                                                                                                                                         |
| 0.3.0 | List every Linux app   | For `adb` and other callers holding `android.permission.DUMP`: `content query --uri content://com.wonderhoylabs.airi.apps/apps` gives rows of `desktop_id`, `on_launcher` (1 or 0) and `name`, in that order, and `content read --uri content://com.wonderhoylabs.airi.apps/icon/<desktop file ID>` gives a PNG. There are no rows while Linux is off |

A page or tool that drives the phone over `adb`, such as [Asahi](https://github.com/wonderhoy-labs/asahi), can list
the Linux apps that way, then show one alone with
[Shinonome](https://github.com/wonderhoy-labs/shinonome)'s `--app airi://app/<desktop file ID>`.

## What has been tested

|                                          | Status                                                                                                        |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Wayland apps                             | Yes. Tried with GNOME Calculator, Mousepad, Foot and Chromium                                                 |
| X11 apps                                 | Yes, through Xwayland, and they look soft. Tried with XTerm                                                   |
| Touch                                    | Yes                                                                                                           |
| On-screen keyboard                       | Yes. Non-US characters need an app that takes text input directly                                             |
| Rotation                                 | Yes                                                                                                           |
| Launcher apps                            | Yes, with their own name and icon in Recents                                                                  |
| Starting Linux from an icon              | Yes, in 30 to 45 seconds                                                                                      |
| Going to the background                  | Yes. Android freezes Airi after a moment, and everything carries on when you come back                        |
| Dialogs and menus                        | Dialogs share their app's window. Only lightly tried                                                          |
| A physical keyboard                      | Written, but not tried on a real one                                                                          |
| A mouse                                  | Written, but not tried on a real one                                                                          |
| Clipboard                                | Written, but not tried yet                                                                                    |
| Other apps opening Linux apps            | The client library's tests pass on an emulator, against Airi and a launcher app. Not with Linux behind it yet |
| Listing apps over `adb`                  | 0.3.0 on a Pixel 10 Pro: ten rows and a 384x384 icon; Asahi's page listed them and opened Calculator alone    |
| Shown alone in a browser                 | GNOME Calculator through Shinonome's `--app airi://app/…` on a Pixel 10 Pro, filling 1920x1080                 |
| Sound                                    | Goes through the Terminal app and not Airi. We haven't checked it                                             |
| Pixel desktop mode and resizable windows | Should work, and we haven't tried it                                                                          |
| Video and games                          | Every frame is copied by the processor, so expect them to be slow. We haven't measured it                     |

## How it works

```
                    Android                                     Linux, in Android's VM
┌──────────────────────────────────────────┐      ┌───────────────────────────────────────────┐
│  Launcher icon (a tiny generated app)    │      │  airi-agent: a Wayland compositor         │
│        │                                 │      │  with no screen of its own                │
│        ▼                                 │      │        ▲                                  │
│  Airi's viewer  ──── one per window ─────┼──────┼──►  a private "screen" per Android window │
│  decompresses frames into a surface      │ TCP  │        ▲                                  │
│  sends touch, keys, text and size        │47821 │  Linux apps, directly or through Xwayland │
└──────────────────────────────────────────┘      └───────────────────────────────────────────┘
```

### The agent

Android's Terminal app has no way to give a Linux window its own Android window, so Airi brings its own compositor instead of
using the Terminal's. The agent is built with [Smithay](https://github.com/Smithay/smithay) and runs as a user service in Linux.

It gives every Android window a private screen of exactly that window's size, and the Linux app there fills it. Apps are told
they're maximized and tiled, so they drop their shadows and rounded corners. An app with a minimum size that doesn't fit gets told
the screen has a lower density, which gives it the room it needs while it still draws pixel for pixel. Dialogs stay on their parent's screen. A window
nobody asked for, like a second window from the same app, makes Android open a new one for it.

Drawing happens in software with Pixman. Only the parts of a window that changed go out, as LZ4-compressed rectangles of raw
pixels. There's no video encoder, so text stays sharp. Android acknowledges each frame, and with at most two in flight, a slow
moment costs frame rate instead of building up a delay.

The agent reads `.desktop` files to list apps, including Flatpak's, and renders their icons from SVG or PNG. It works out which app
a new window belongs to from its app ID, its X11 class or its process.

### The connection

The Terminal app already forwards ports from Linux to Android's loopback, so that's what Airi uses. The agent only listens on Linux's
own loopback, and the Terminal's forwarder reaches it there. We measured that route at about 280 MB/s.

[docs/protocol.md](docs/protocol.md) has the details.

### The launcher apps

Pinned shortcuts only reach the home screen, so **Add** makes a real app. Airi carries a template of about 11 KB, built with it, and
changes four things in a copy: the package, the name, the icon, and which Linux app it opens. Changing a compiled manifest means
rebuilding its string table, which is all `BinaryManifest.kt` does. Then Android's own signing library signs it, with a key that's
made on your phone and stays in Android's Keystore.

The app has no secrets and no native code. It starts Airi's viewer and stays underneath it, because Android names a task in Recents
after the activity at its root, and that's what makes it say Calculator and not Airi.

### Starting Linux

Only the Terminal app can start Linux, and it has to be opened to do that, so that's what Airi does. It comes back after a second and a half,
and the Terminal app carries on in the background.

That route has a trap in it. The Terminal app starts forwarding an allowed port early in boot, before anything in Linux is listening. If
something connects in that gap, which is what happens when Airi is waiting for Linux, the Terminal app waits ten seconds and then stops
forwarding the port, and it only looks again when a new listener appears. So for the first two minutes after boot, until someone gets through,
the agent listens afresh every 12 seconds.

### Keeping the picture steady

The keyboard is what makes this hard, because a window changes size every time it opens. Three things keep that from showing:
- The surface never changes size for the keyboard. It slides over, and the frames are drawn at one-to-one, so nothing gets stretched.
- After a resize, the agent keeps showing the old picture until the app has finished drawing the new one, for up to 600 ms.
- Android reports where the keyboard will end up as it starts to move, so the app redraws during the slide.

Key repeat comes from Android and not from the Linux apps, because an app busy redrawing misses a key going up and types the letter
over and over.

## Security and privacy

Worth reading, because the agent is a way into Linux.

**The token is what protects all of this.** It's 32 random bytes that Airi makes, and that travel to Linux in the setup command. With
it, you can open apps, send them input and see them, and replace the agent, which means running anything as the `droid` user. That
user has `sudo` without a password in the Terminal's Debian. Airi keeps the token in its private storage, and Linux keeps it in
`~/.config/airi/token`, readable only by `droid`.

Any app on your phone can connect to the forwarded port, but it can't do anything there without the token. Each end proves that it has
the token without sending it, so an app can't pretend to be the agent either.

The setup command is on your clipboard and on screen for a moment, marked as sensitive so that keyboards don't preview it. It starts with a
space, which keeps it out of the terminal's history. If you think it got out, tap **Set up Airi in Linux again**, after clearing Airi's storage so that it makes a new token.

**Traffic isn't encrypted.** It only crosses the phone's loopback, which other apps can't read without root.

**During setup, the agent sits in your Downloads folder** until the command deletes it. An app with access to all your files could
swap it in that time. If you have an app like that and don't trust it, read `install.sh` first and check the file.

**Airi's viewer is open to other apps,** because the launcher apps are signed with a key of their own, so Android can't limit it to them.
All another app can do with that is open a Linux app that's already installed.

**The app list is open to `adb`.** Anything holding `android.permission.DUMP`, which means the shell user that `adb`
runs as and the system, can read which Linux apps there are, their names and icons. Ordinary apps can't hold that
permission.

**Allowing Airi to install apps** only matters for **Add**. Android still asks you about each one, and you can turn it off afterwards. The
apps you've added keep working.

## Troubleshooting

**Airi says Linux isn't running.** Tap **Start Linux**, or just open an app from your launcher. If you've closed the Terminal app's
notification, Linux has stopped.

**Airi says Linux isn't running, but it is.** The Terminal app can show port 47821 as on while it isn't passing anything through.
In the Terminal app's settings, under **Security**, turn port 47821 off and on again.

**Airi can't find the helper you installed.** Check that port 47821 is on in the Terminal app's settings, under **Security**. Then check the
helper in the terminal:

```bash
systemctl --user status airi-agent
```

**Airi says the helper belongs to a different install.** That happens when you reinstall Airi, because it makes a new token. Tap **Set up
Airi in Linux again**.

**An app says it quit before opening a window.** Airi shows the end of what it printed. Try running it in the terminal for the rest.

**An entry opens nothing.** Some `.desktop` files aren't really apps, like Foot Server, which is a background service. Airi gives up on them
after 90 seconds.

**The keyboard doesn't come up.** Tap the round button on the right edge. Tapping a text field that's already focused doesn't open it.

**Android won't install an app you're adding after you reinstalled Airi.** The key that signs them goes when Airi does, and Android only updates
an app with the same key. Tap **Remove**, then **Add**. An app left over from before still opens, so you only need this to change it.

## Known issues

- **Linux froze for several minutes, twice, soon after we compiled Rust in it.** Android showed the Terminal app and its VM stuck in the
  kernel, with most of the VM's memory swapped out. It cleared up by itself the first time, and the Terminal app closed the second
  time. We don't know the exact cause, and it isn't specific to Airi, but heavy memory use in Linux seems to be what sets it off.
- X11 apps look soft, because they're drawn at normal size and scaled up. Wayland apps are sharp.
- Apps take 200 to 500 ms to redraw after the keyboard moves. The picture holds still in the meantime.
- Tapping a text field that already has focus doesn't open the keyboard, because Linux can't tell that apart from tapping anything else.
- Characters a US keyboard can't type get lost in apps that don't take text input directly. X11 apps are the usual case.
- Second windows of an app say Airi in Recents, because they don't have a launcher app under them.
- A window that opens while Airi is in the background only appears when you next switch to a Linux app. Android doesn't let apps in the
  background open windows.
- Not every entry in the list is a real app, and Airi can't always tell.
- Still untested: other phones, Android 15 and 16, tablets, a real mouse or keyboard, the clipboard, sound, desktop mode, and battery drain.

## Development

You need Rust, JDK 17 or newer, and the Android SDK with platform 37 and NDK 30. The app bundles the agent, so build that first:

Airi follows the [Wonderhoy Labs guideline](.claude/skills/wonderhoy-unification/SKILL.md), like every Wonderhoy Labs project.
The shared tools need Node 24 or newer and pnpm. Format before you commit, and check the result:

```bash
pnpm install && pnpm format && pnpm unify
```

```bash
pnpm install && pnpm agent
```

```bash
cd android && ./gradlew :app:assembleRelease
```

The APK ends up at `android/app/build/outputs/apk/release/app-release.apk`. It's signed with your debug key unless you set
`AIRI_KEYSTORE`, `AIRI_KEYSTORE_PASSWORD`, `AIRI_KEY_ALIAS` and `AIRI_KEY_PASSWORD`.

`pnpm agent` (`tools/build-agent.ts`) cross-compiles on a Mac or an x86 machine, against libraries it downloads from the same Debian release the
Terminal app runs, using the NDK's clang. Please don't compile in the phone's Linux. See [Known issues](#known-issues).

| Where                                    | What's in it                                                                        |
| ---------------------------------------- | ----------------------------------------------------------------------------------- |
| `crates/airi-agent/`                     | The agent: compositor, app list, launching, and the network side                    |
| `crates/airi-viewer/`                    | The native half of the viewer: decodes frames and draws them to a surface           |
| `android/app/`                           | The Airi app                                                                        |
| `android/stub/`                          | The template for the apps **Add** makes                                             |
| `android/client/`                        | The client library, for other apps                                                  |
| `android/app/src/main/assets/install.sh` | What the setup command runs                                                         |
| `docs/protocol.md`                       | How the two ends talk                                                               |
| `tools/`                                 | `build-agent.ts` and `third-party-licenses.ts`, and the shared Wonderhoy Labs tools |

To test against a real Linux from an emulator, which can't run the Terminal app, tunnel the port through `adb`. The emulator's app then
uses `127.0.0.1`, the same as on a phone. Debug builds read a token from `dev.properties` in the app's external files directory, as
`token=<64 hex digits>`.

```bash
adb -s <phone> forward tcp:47821 tcp:47821
```

```bash
adb -s <emulator> reverse tcp:47821 tcp:47821
```

Run the device tests with `am instrument`, because Gradle's connected task uninstalls the app:

```bash
adb shell am instrument -w com.wonderhoylabs.airi.test/androidx.test.runner.AndroidJUnitRunner
```

The client library's tests run on their own. Most of them need Airi installed, and a launcher app, or the template they're
made from, `android/stub/build/outputs/apk/release/stub-release.apk`:

```bash
cd android && ANDROID_SERIAL=<emulator> ./gradlew :client:connectedDebugAndroidTest
```

If you change the Rust dependencies, run `pnpm run licenses` and commit the result. CI checks that it's current with `pnpm verify`.

Pushing a tag like `v0.2.0`, matching `VERSION`, builds the agent, the APK and the client library and publishes a GitHub release through
[ci.yml](.github/workflows/ci.yml). It signs the APK with the repository's `AIRI_KEYSTORE_BASE64`, `AIRI_KEYSTORE_PASSWORD`,
`AIRI_KEY_ALIAS` and `AIRI_KEY_PASSWORD` secrets. Without them, each release gets a throwaway key and has to be installed
fresh instead of updating. `VERSION` and the version in `Cargo.toml` have to match, because the app replaces an agent of a different
version. `pnpm unify` checks that they do.

## License and credits

Airi is a [Wonderhoy Labs](https://github.com/wonderhoy-labs) project, released under the [Apache License 2.0](LICENSE). It isn't
affiliated with Google.

It stands on the work of others:
- [Smithay](https://github.com/Smithay/smithay), under the MIT License, which does the hard parts of being a compositor.
- [resvg](https://github.com/linebender/resvg), under the Apache License 2.0 or MIT License, which draws icons.
- [lz4_flex](https://github.com/PSeitz/lz4_flex), under the MIT License, which compresses frames.
- [apksig](https://android.googlesource.com/platform/tools/apksig/), under the Apache License 2.0, which signs the launcher apps.
- [Jetpack Compose and AndroidX](https://developer.android.com/jetpack), under the Apache License 2.0, for the app.
- [Material Color Utilities](https://github.com/material-foundation/material-color-utilities), under the Apache License 2.0, which
  makes the app's colours from Momoi Airi's image colour.

The app carries the notices for everything compiled into it, in
[third-party-licenses.txt](android/app/src/main/assets/third-party-licenses.txt).

Airi is named for Momoi Airi of MORE MORE JUMP!, from Project SEKAI COLORFUL STAGE! feat. Hatsune Miku. Wonderhoy Labs is a
fan project, not affiliated with SEGA, Colorful Palette or Crypton Future Media, who own Project SEKAI and its characters.
The Apache License doesn't cover the Airi name or logo.

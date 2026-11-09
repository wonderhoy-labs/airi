# AGENTS.md

Read the Wonderhoy Labs unification skill, [.claude/skills/wonderhoy-unification/SKILL.md](.claude/skills/wonderhoy-unification/SKILL.md),
before changing anything. It's the same in every Wonderhoy Labs repository. This file only adds what's particular
to Airi.

## What Airi is

Linux apps from Android's Terminal on the Android launcher, each in its own window. `airi-agent` is a Smithay
compositor inside the Terminal's Linux VM, reached on TCP 47821 through the Terminal's port forward. The app shows
each Linux window with `airi-viewer`, and **Add** builds a real launcher app (about 11 KB, from `android/stub`) for
each Linux app; **Launch** opens one without adding it. Named for Momoi Airi of MORE MORE JUMP!

## Layout

| Path                            | What                                                                         |
| ------------------------------- | ---------------------------------------------------------------------------- |
| `crates/airi-agent/`            | The compositor, app list, launching and network side, for 64-bit Arm Linux   |
| `crates/airi-viewer/`           | The viewer's native half, built into the app with cargo-ndk                  |
| `android/app/`                  | The Airi app                                                                 |
| `android/stub/`                 | The template for the launcher apps **Add** makes                             |
| `android/client/`               | The client library other apps use to open Linux apps, released as an AAR     |
| `tools/build-agent.ts`          | Builds the agent into `dist/airi-agent` for the app to bundle (`pnpm agent`) |
| `tools/third-party-licenses.ts` | Writes the app's license notices (`pnpm run licenses`, checked by `pnpm verify`) |

## Build, test and format

```bash
pnpm install && pnpm unify && pnpm verify
pnpm agent
cd android && ./gradlew spotlessApply :client:lintRelease :client:assembleRelease :app:testDebugUnitTest :app:lintRelease :app:assembleRelease
```

The agent only compiles for Linux. On a Mac, `pnpm agent` cross-compiles against Debian trixie's libraries with
the NDK's clang, and Clippy runs the same way:
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=target/debian-sysroot/aarch64-linux-gnu-clang PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_SYSROOT_DIR=target/debian-sysroot PKG_CONFIG_LIBDIR=target/debian-sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:target/debian-sysroot/usr/share/pkgconfig cargo clippy -p airi-agent --target aarch64-unknown-linux-gnu`.

## Things to know

- **Never compile in the phone's Linux VM.** Linux froze twice after that (the Terminal and crosvm stuck in the
  kernel, VM memory swapped out, about ten minutes each). Always cross-compile.
- **`VERSION` and the Cargo version must match.** The app replaces an agent of a different version, and doing so
  closes every open Linux app, so check with the maintainer before updating the agent on their phone. They test on
  it while work goes on, and their Terminal tab can change under you.
- **The emulator can't run Linux.** Test the app against the phone's agent: `adb -s <phone> forward tcp:47821
  tcp:47821`, `adb -s <emulator> reverse tcp:47821 tcp:47821`, and a `token=` line in the debug build's
  `dev.properties`. Start the emulator with `-crash-report-mode never`, or a crash consent dialog blocks it.
- Turning on Install unknown apps on the real phone is for the maintainer to do. Clean up test leftovers.
- **The launch contract is public.** Other apps use `airi://app/<desktop file ID>`, the `LINUX_APP` action and the
  `desktop_id` asset, through `android/client/` or directly, so keep them working and list what changes in the
  README's "Use it in your app". The stub repeats the link on purpose, so the launcher apps carry no library code.
- Never replace the Android signing key. CI signs with the `AIRI_KEYSTORE_BASE64`, `AIRI_KEYSTORE_PASSWORD`,
  `AIRI_KEY_ALIAS` and `AIRI_KEY_PASSWORD` secrets.

## Exceptions to the guideline

- The app and the agent are built for 64-bit Arm only (`arm64-v8a`, `aarch64-unknown-linux-gnu`), because Android's
  Linux terminal only exists there.
- Rust is tested on `ubuntu-24.04-arm` only, because Smithay doesn't build on macOS or Windows.

// Builds the agent for Android's Linux terminal, and puts it at dist/airi-agent for the app to bundle.
//
// On 64-bit Arm Linux this is a plain build. Anywhere else it cross-compiles, against libraries from the same
// Debian release the terminal runs, using the Android NDK's clang. Don't build in the terminal itself: Linux
// has frozen twice for us after compiling this there.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
	chmodSync,
	copyFileSync,
	existsSync,
	mkdirSync,
	mkdtempSync,
	readdirSync,
	readFileSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = "aarch64-unknown-linux-gnu";
const RELEASE = "trixie";
const MIRROR = "https://deb.debian.org/debian";
const PACKAGES = [
	"libc6",
	"libc6-dev",
	"libgcc-s1",
	"libgcc-14-dev",
	"libxkbcommon0",
	"libxkbcommon-dev",
	"libpixman-1-0",
	"libpixman-1-dev",
	"linux-libc-dev",
];

const run = (command: string, args: string[], options: { cwd?: string; env?: NodeJS.ProcessEnv } = {}) =>
	execFileSync(command, args, { cwd: options.cwd ?? REPO, env: options.env ?? process.env, stdio: "inherit" });

function cargoBuild(env: NodeJS.ProcessEnv = process.env) {
	run("cargo", ["build", "--release", "-p", "airi-agent", "--target", TARGET], { env });
	mkdirSync(join(REPO, "dist"), { recursive: true });
	copyFileSync(join(REPO, "target", TARGET, "release/airi-agent"), join(REPO, "dist/airi-agent"));
	console.info("Built dist/airi-agent");
}

/** Each wanted package's path on the mirror and its SHA-256, from a Debian package index. */
function findPackages(index: string, wanted: string[]): { name: string; file: string; sha256: string }[] {
	const found = new Map<string, Record<string, string>>();
	for (const block of index.split("\n\n")) {
		const fields: Record<string, string> = {};
		for (const line of block.split("\n")) {
			const at = line.indexOf(": ");
			if (at > 0 && line[0] !== " ") fields[line.slice(0, at)] = line.slice(at + 2);
		}
		const name = fields.Package;
		if (name && wanted.includes(name)) found.set(name, fields);
	}
	return wanted.map((name) => {
		const fields = found.get(name);
		if (!(fields?.Filename && fields.SHA256)) throw new Error(`${name} isn't in ${RELEASE}'s index`);
		return { name, file: fields.Filename, sha256: fields.SHA256 };
	});
}

async function download(url: string): Promise<Buffer> {
	const response = await fetch(url);
	if (!response.ok) throw new Error(`${url}: ${response.status} ${response.statusText}`);
	return Buffer.from(await response.arrayBuffer());
}

/** Unpacks the Debian libraries the agent links against into target/debian-sysroot, once. */
async function sysroot(): Promise<string> {
	const root = join(REPO, "target/debian-sysroot");
	if (existsSync(join(root, ".complete"))) return root;
	console.info(`Fetching ${RELEASE}'s libraries`);
	mkdirSync(join(REPO, "target"), { recursive: true });
	const work = mkdtempSync(join(REPO, "target/debian-"));
	try {
		const compressed = join(work, "Packages.xz");
		writeFileSync(compressed, await download(`${MIRROR}/dists/${RELEASE}/main/binary-arm64/Packages.xz`));
		run("xz", ["-d", compressed]);
		mkdirSync(root, { recursive: true });
		for (const { name, file, sha256 } of findPackages(readFileSync(join(work, "Packages"), "utf8"), PACKAGES)) {
			const deb = await download(`${MIRROR}/${file}`);
			const sum = createHash("sha256").update(deb).digest("hex");
			if (sum !== sha256) throw new Error(`${name}'s checksum is ${sum}, not ${sha256}`);
			const dir = join(work, name);
			mkdirSync(dir);
			writeFileSync(join(dir, `${name}.deb`), deb);
			run("ar", ["x", `${name}.deb`], { cwd: dir });
			const data = readdirSync(dir).find((entry) => entry.startsWith("data.tar."));
			if (!data) throw new Error(`${name} has no data archive`);
			run("tar", ["-xf", data, "-C", root], { cwd: dir });
		}
		writeFileSync(join(root, ".complete"), "");
	} finally {
		rmSync(work, { recursive: true, force: true });
	}
	return root;
}

/** The newest NDK's clang, which links for Linux as happily as for Android. */
function ndkClang(): string {
	const sdk = process.env.ANDROID_HOME ?? join(homedir(), "Library/Android/sdk");
	const ndks = existsSync(join(sdk, "ndk")) ? readdirSync(join(sdk, "ndk")) : [];
	const newest = ndks.sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).at(-1);
	const prebuilt = newest ? join(sdk, "ndk", newest, "toolchains/llvm/prebuilt") : "";
	const host = existsSync(prebuilt) ? readdirSync(prebuilt)[0] : undefined;
	const clang = host ? join(prebuilt, host, "bin/clang") : "";
	if (!existsSync(clang)) throw new Error(`Couldn't find the NDK's clang under ${sdk}/ndk`);
	return clang;
}

if (process.platform === "linux" && process.arch === "arm64") {
	cargoBuild();
} else {
	const clang = ndkClang();
	try {
		execFileSync("pkg-config", ["--version"], { stdio: "ignore" });
	} catch {
		throw new Error("This needs pkg-config");
	}
	const root = await sysroot();

	// Debian's libraries refer to /lib, which is a link that a package we don't need would have made.
	rmSync(join(root, "lib"), { force: true });
	symlinkSync("usr/lib", join(root, "lib"));

	// rustc works out how to call a linker from its name, so this one has to sound like clang.
	const linker = join(root, "aarch64-linux-gnu-clang");
	writeFileSync(
		linker,
		`#!/bin/sh\nexec "${clang}" --target=aarch64-linux-gnu --sysroot="${root}" -fuse-ld=lld "$@"\n`,
	);
	chmodSync(linker, 0o755);

	cargoBuild({
		...process.env,
		CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: linker,
		PKG_CONFIG_ALLOW_CROSS: "1",
		PKG_CONFIG_SYSROOT_DIR: root,
		PKG_CONFIG_LIBDIR: `${root}/usr/lib/aarch64-linux-gnu/pkgconfig:${root}/usr/share/pkgconfig`,
	});
}

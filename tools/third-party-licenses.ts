// Collects the license of everything compiled into the agent and the viewer, which the app ships.
// Run `pnpm run licenses` after changing Rust dependencies, and commit the result. `pnpm verify` checks it's current.
// Plain `pnpm licenses` runs pnpm's own command instead.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUTPUT = join(REPO, "android/app/src/main/assets/third-party-licenses.txt");
const TARGET = "aarch64-unknown-linux-gnu";
const LICENSE_FILE = /^(LICENSE|LICENCE|COPYING|UNLICENSE|license)/;

interface Package {
	id: string;
	name: string;
	version: string;
	license: string | null;
	manifest_path: string;
}

interface Metadata {
	packages: Package[];
	workspace_members: string[];
	resolve: { nodes: { id: string; deps: { pkg: string; dep_kinds: { kind: string | null }[] }[] }[] };
}

const compare = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

function shippedPackages(): Package[] {
	const metadata = JSON.parse(
		execFileSync("cargo", ["metadata", "--format-version", "1", "--filter-platform", TARGET], {
			cwd: REPO,
			encoding: "utf8",
			maxBuffer: 256 * 1024 * 1024,
		}),
	) as Metadata;
	const packages = new Map(metadata.packages.map((item) => [item.id, item]));
	const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
	const workspace = new Set(metadata.workspace_members);
	const seen = new Set<string>();
	const pending = [...workspace];
	for (let current = pending.pop(); current !== undefined; current = pending.pop()) {
		if (seen.has(current)) continue;
		seen.add(current);
		for (const dependency of nodes.get(current)?.deps ?? []) {
			// Build tools and test libraries don't end up in anything we ship.
			if (dependency.dep_kinds.some((kind) => kind.kind === null)) pending.push(dependency.pkg);
		}
	}
	// The version matters here: without it, two versions of one package would swap places between runs.
	return [...seen]
		.filter((id) => !workspace.has(id))
		.map((id) => packages.get(id))
		.filter((item): item is Package => item !== undefined)
		.sort((a, b) => compare(a.name, b.name) || compare(a.version, b.version));
}

function licenseTexts(item: Package): string[] {
	const root = dirname(item.manifest_path);
	return (
		readdirSync(root)
			.filter((name) => LICENSE_FILE.test(name) && statSync(join(root, name)).isFile())
			.map((name) => join(root, name))
			.sort(compare)
			// Some license files have Windows line endings. Python's read_text, which wrote the first notices, turned
			// those into plain newlines, and so does this.
			.map((path) => readFileSync(path, "utf8").replace(/\r\n?/g, "\n").trim())
	);
}

function notices(): { text: string; packages: number; groups: number } {
	// Most packages share a handful of licenses, so each text is only written once.
	const groups = new Map<string, string[]>();
	let count = 0;
	for (const item of shippedPackages()) {
		const name = `${item.name} ${item.version} (${item.license ?? "None"})`;
		const text = licenseTexts(item).join("\n\n") || `Licensed under ${item.license ?? "None"}.`;
		groups.set(text, [...(groups.get(text) ?? []), name]);
		count++;
	}
	const parts = ["Airi includes the following software.\n"];
	const sorted = [...groups].sort((a, b) => compare(a[1][0] ?? "", b[1][0] ?? ""));
	for (const [text, names] of sorted) {
		parts.push(`${"=".repeat(72)}\n${names.join("\n")}\n${"-".repeat(72)}\n${text}\n`);
	}
	return { text: parts.join("\n"), packages: count, groups: groups.size };
}

const { text, packages, groups } = notices();
if (process.argv.includes("--check")) {
	if (!existsSync(OUTPUT) || readFileSync(OUTPUT, "utf8") !== text) {
		console.error(
			"The third-party license notices are out of date. Run `pnpm run licenses` and commit the result.",
		);
		process.exit(1);
	}
	console.info(`The license notices are current: ${packages} packages, ${groups} license texts.`);
} else {
	writeFileSync(OUTPUT, text);
	console.info(`${packages} packages, ${groups} license texts`);
}

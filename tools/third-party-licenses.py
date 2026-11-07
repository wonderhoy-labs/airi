#!/usr/bin/env python3
"""Collects the license of everything compiled into the agent and the viewer, which the app ships."""
import json
import pathlib
import subprocess

REPO = pathlib.Path(__file__).resolve().parent.parent
OUTPUT = REPO / "android/app/src/main/assets/third-party-licenses.txt"
TARGET = "aarch64-unknown-linux-gnu"
PATTERNS = ("LICENSE*", "LICENCE*", "COPYING*", "UNLICENSE*", "license*")


def shipped_packages():
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET], cwd=REPO, text=True
        )
    )
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    workspace = set(metadata["workspace_members"])
    seen = set()
    pending = list(workspace)
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        for dependency in nodes[current]["deps"]:
            # Build tools and test libraries don't end up in anything we ship.
            if any(kind["kind"] is None for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    # The version matters here: without it, two versions of one package would swap places between runs.
    return sorted((packages[item] for item in seen - workspace), key=lambda package: (package["name"], package["version"]))


def license_texts(package):
    root = pathlib.Path(package["manifest_path"]).parent
    files = sorted({path for pattern in PATTERNS for path in root.glob(pattern) if path.is_file()})
    return [path.read_text(errors="replace").strip() for path in files]


def main():
    # Most packages share a handful of licenses, so each text is only written once.
    groups = {}
    for package in shipped_packages():
        name = f"{package['name']} {package['version']} ({package['license']})"
        texts = license_texts(package)
        groups.setdefault("\n\n".join(texts) or f"Licensed under {package['license']}.", []).append(name)
    parts = ["Airi includes the following software.\n"]
    for text, names in sorted(groups.items(), key=lambda group: group[1][0]):
        parts.append("=" * 72 + "\n" + "\n".join(names) + "\n" + "-" * 72 + "\n" + text + "\n")
    OUTPUT.write_text("\n".join(parts))
    print(f"{sum(len(names) for names in groups.values())} packages, {len(groups)} license texts")


main()

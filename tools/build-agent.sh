#!/usr/bin/env bash
# Builds the agent for Android's Linux terminal, and puts it at dist/airi-agent for the app to bundle.
#
# On 64-bit Arm Linux this is a plain build. Anywhere else it cross-compiles, against libraries from
# the same Debian release the terminal runs, using the Android NDK's clang. Don't build in the terminal
# itself: Linux has frozen twice for us after compiling this there.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
target=aarch64-unknown-linux-gnu
release=trixie
packages=(libc6 libc6-dev libgcc-s1 libgcc-14-dev libxkbcommon0 libxkbcommon-dev libpixman-1-0 libpixman-1-dev linux-libc-dev)
mirror=https://deb.debian.org/debian

cd "$repo"
mkdir -p dist

if [[ $(uname -s) == Linux && $(uname -m) == aarch64 ]]; then
    cargo build --release -p airi-agent --target "$target"
    cp "target/$target/release/airi-agent" dist/airi-agent
    exit
fi

sdk=${ANDROID_HOME:-$HOME/Library/Android/sdk}
ndk=$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)
clang=$(ls "$ndk"/toolchains/llvm/prebuilt/*/bin/clang 2>/dev/null | head -1)
[[ -x $clang ]] || { echo "Couldn't find the NDK's clang under $sdk/ndk" >&2; exit 1; }
command -v pkg-config > /dev/null || { echo "This needs pkg-config" >&2; exit 1; }

sysroot=$repo/target/debian-sysroot
if [[ ! -e $sysroot/.complete ]]; then
    echo "Fetching $release's libraries"
    work=$(mktemp -d "$repo/target/debian-XXXXXX")
    curl -fsSL -o "$work/Packages.xz" "$mirror/dists/$release/main/binary-arm64/Packages.xz"
    xz -d "$work/Packages.xz"
    mkdir -p "$sysroot"
    python3 -I "$repo/tools/find-debian-packages.py" "$work/Packages" "${packages[@]}" |
        while read -r name file sum; do
            curl -fsSL -o "$work/$name.deb" "$mirror/$file"
            echo "$sum  $work/$name.deb" | shasum -a 256 -c --quiet -
            mkdir "$work/$name"
            (cd "$work/$name" && ar x "../$name.deb" && tar -xf data.tar.* -C "$sysroot")
        done
    touch "$sysroot/.complete"
fi

# Debian's libraries refer to /lib, which is a link that a package we don't need would have made.
ln -sfn usr/lib "$sysroot/lib"

# rustc works out how to call a linker from its name, so this one has to sound like clang.
linker=$sysroot/aarch64-linux-gnu-clang
cat > "$linker" <<SCRIPT
#!/bin/sh
exec "$clang" --target=aarch64-linux-gnu --sysroot="$sysroot" -fuse-ld=lld "\$@"
SCRIPT
chmod +x "$linker"

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$linker
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR=$sysroot
export PKG_CONFIG_LIBDIR=$sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:$sysroot/usr/share/pkgconfig

cargo build --release -p airi-agent --target "$target"
cp "target/$target/release/airi-agent" dist/airi-agent

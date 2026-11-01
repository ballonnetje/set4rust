#!/usr/bin/env bash
# Builds the Linux or macOS packages into target/package, on that system, from anywhere:
#   Linux:  S4RS-Linux-x64-<version>.tar.gz, S4RS-Asset-Studio-Linux-x64-<version>.tar.gz
#   macOS:  S4RS-macOS-universal-<version>.tar.gz, S4RS-Asset-Studio-macOS-universal-<version>.tar.gz
#           (Apple silicon and Intel in one binary; needs both Rust targets:
#           rustup target add aarch64-apple-darwin x86_64-apple-darwin)
# Linux needs: cargo, a C compiler, pkg-config, libasound2-dev.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' crates/s4-game/Cargo.toml | head -n 1)"
[[ -n "$version" ]] || { echo "no version in crates/s4-game/Cargo.toml" >&2; exit 1; }

build="$root/target/package-build"
pkg="$root/target/package"
mkdir -p "$pkg"

# build [target]: the game, the launcher and the Asset Studio.
build() {
    local args=(--release --locked)
    [[ $# -gt 0 ]] && args+=(--target "$1")
    CARGO_TARGET_DIR="$build" cargo build "${args[@]}" -p s4-game -p s4-portable --features s4-game/package
    (cd studio && CARGO_TARGET_DIR="$build" cargo build "${args[@]}")
}

bins="$pkg/bin"
rm -rf "$bins"
mkdir -p "$bins"
case "$(uname -s)" in
    Linux)
        platform="Linux-x64"
        readme="README-linux.txt"
        build
        cp "$build/release/s4-game" "$build/release/s4-portable" "$build/release/s4-asset-studio" "$bins/"
        ;;
    Darwin)
        platform="macOS-universal"
        readme="README-macos.txt"
        export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
        build aarch64-apple-darwin
        build x86_64-apple-darwin
        for b in s4-game s4-portable s4-asset-studio; do
            lipo -create "$build/aarch64-apple-darwin/release/$b" "$build/x86_64-apple-darwin/release/$b" -output "$bins/$b"
        done
        ;;
    *)
        echo "unknown system $(uname -s): on Windows use tools/package/windows.ps1" >&2
        exit 1
        ;;
esac

# pack <folder in the archive> <archive> <file:name>...
pack() {
    local top="$1" archive="$2" stage="$pkg/stage"
    shift 2
    rm -rf "$stage"
    mkdir -p "$stage/$top"
    local entry
    for entry in "$@"; do
        cp "${entry%%:*}" "$stage/$top/${entry##*:}"
    done
    chmod 755 "$stage/$top"/*
    chmod 644 "$stage/$top"/*.txt
    tar -C "$stage" -czf "$pkg/$archive" "$top"
    rm -rf "$stage"
    echo "$pkg/$archive: $(du -h "$pkg/$archive" | cut -f 1)"
}

pack S4RS "S4RS-$platform-$version.tar.gz" \
    "$bins/s4-portable:S4RS" "$bins/s4-game:s4-game" "tools/package/$readme:S4RS-README.txt"
pack S4RS-Asset-Studio "S4RS-Asset-Studio-$platform-$version.tar.gz" \
    "$bins/s4-asset-studio:s4-asset-studio" "tools/package/README-asset-studio.txt:README.txt"
rm -rf "$bins"

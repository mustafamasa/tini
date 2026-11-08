#!/usr/bin/env bash
# Tını: builds whisper.cpp (transcription engine) for macOS / Apple Silicon with Metal.
#
#   bash engine/scripts/build_whisper_macos.sh   # -> app/src-tauri/binaries/whisper_cli-aarch64-apple-darwin
#
# Source: whisper.cpp v1.9.5 (MIT, pinned sha256). Static, Metal shaders embedded, runs on M1-M4.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORK="$REPO_ROOT/engine/.build/whisper"
URL="https://github.com/ggml-org/whisper.cpp/archive/refs/tags/v1.9.5.tar.gz"
SHA="ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720"
OUTPUT="$REPO_ROOT/app/src-tauri/binaries/whisper_cli-aarch64-apple-darwin"
JOBS="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || sysctl -n hw.ncpu)"

sha_matches() { [[ -f "$1" ]] && [[ "$(shasum -a 256 "$1" | cut -d' ' -f1)" == "$2" ]]; }
mkdir -p "$WORK"
ARCHIVE="$WORK/whisper-v1.9.5.tar.gz"
if ! sha_matches "$ARCHIVE" "$SHA"; then
    curl -L --fail --retry 3 --proto "=https" -o "$ARCHIVE.part" "$URL"
    mv "$ARCHIVE.part" "$ARCHIVE"
    sha_matches "$ARCHIVE" "$SHA" || { echo "sha256 mismatch: $ARCHIVE" >&2; exit 1; }
fi
SOURCE="$WORK/src"
[[ -f "$SOURCE/CMakeLists.txt" ]] || { mkdir -p "$SOURCE" && tar -xzf "$ARCHIVE" -C "$SOURCE" --strip-components 1; }

cmake -S "$SOURCE" -B "$WORK/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_ARCHITECTURES=arm64 \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 -DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON \
    -DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DWHISPER_BUILD_TESTS=OFF -DWHISPER_BUILD_SERVER=OFF -DWHISPER_SDL2=OFF
cmake --build "$WORK/build" --config Release --target whisper-cli -j "$JOBS"

mkdir -p "$(dirname "$OUTPUT")"
cp "$WORK/build/bin/whisper-cli" "$OUTPUT"
"$OUTPUT" --help >/dev/null 2>&1 || { echo "whisper-cli failed to run" >&2; exit 1; }
shasum -a 256 "$OUTPUT"
echo "WHISPER_OK: $OUTPUT"

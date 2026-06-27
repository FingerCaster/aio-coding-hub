#!/usr/bin/env bash
# repack-linux-appimage-wayland.sh
#
# Repackage an AIO Coding Hub AppImage to use system EGL/Mesa libraries
# instead of the bundled ones.
#
# Background
# ----------
# Tauri's AppImage bundles a snapshot of WebKitGTK together with its EGL/Mesa
# runtime dependencies.  On modern distributions (e.g. Arch Linux with a
# recent Mesa build) those bundled libraries conflict with the system stack,
# causing WebKitGTK to abort on startup:
#
#   "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting..."
#
# The runtime fix (WEBKIT_DISABLE_COMPOSITING_MODE=1, set automatically by the
# app when a Wayland session is detected) avoids the crash path, but disables
# GPU-accelerated compositing.  This script produces an alternative AppImage
# that removes the conflicting bundled EGL/Mesa/DRM libraries so WebKitGTK
# picks up the system versions at runtime – preserving GPU compositing on
# capable systems.
#
# Usage
# -----
#   ./scripts/repack-linux-appimage-wayland.sh <input.AppImage> [output.AppImage]
#
#   input.AppImage   Path to the original AppImage produced by the Tauri build.
#   output.AppImage  (optional) Destination path.  Defaults to
#                    <input-stem>-wayland.AppImage in the same directory.
#
# Requirements
# ------------
#   - appimagetool   Available from https://github.com/AppImage/AppImageKit/releases
#                    Must be on PATH or set via APPIMAGETOOL env var.
#   - fuse / fuse2   Required by AppImage extraction (or use --appimage-extract
#                    which works without FUSE).
#
# Ref: https://github.com/FingerCaster/aio-coding-hub/issues/93

set -euo pipefail

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------
INPUT_APPIMAGE="${1:-}"
if [[ -z "$INPUT_APPIMAGE" ]]; then
    echo "Usage: $0 <input.AppImage> [output.AppImage]" >&2
    exit 1
fi
if [[ ! -f "$INPUT_APPIMAGE" ]]; then
    echo "Error: input file not found: $INPUT_APPIMAGE" >&2
    exit 1
fi

INPUT_APPIMAGE="$(realpath "$INPUT_APPIMAGE")"
INPUT_DIR="$(dirname "$INPUT_APPIMAGE")"
INPUT_STEM="$(basename "$INPUT_APPIMAGE" .AppImage)"

OUTPUT_APPIMAGE="${2:-${INPUT_DIR}/${INPUT_STEM}-wayland.AppImage}"

# ---------------------------------------------------------------------------
# Locate appimagetool
# ---------------------------------------------------------------------------
APPIMAGETOOL="${APPIMAGETOOL:-appimagetool}"
if ! command -v "$APPIMAGETOOL" &>/dev/null; then
    echo "Error: appimagetool not found on PATH." >&2
    echo "  Download from https://github.com/AppImage/AppImageKit/releases" >&2
    echo "  or set the APPIMAGETOOL env var to its path." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Work directory
# ---------------------------------------------------------------------------
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

SQUASH_DIR="${WORK_DIR}/squashfs-root"

echo "[repack] Extracting: $INPUT_APPIMAGE"
# --appimage-extract works without FUSE and is universally available.
(cd "$WORK_DIR" && "$INPUT_APPIMAGE" --appimage-extract >/dev/null)

if [[ ! -d "$SQUASH_DIR" ]]; then
    echo "Error: extraction produced no squashfs-root directory." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Remove bundled EGL / Mesa / DRM libraries that conflict with system Mesa.
#
# These libraries are loaded by WebKitGTK's GPU process.  When the AppImage
# versions are older than the system Mesa, the EGL display creation fails.
# Removing them forces the dynamic linker to fall back to the system-provided
# versions at runtime.
#
# List is intentionally conservative: only remove what is known to conflict.
# ---------------------------------------------------------------------------
LIB_DIR="${SQUASH_DIR}/usr/lib"

LIBS_TO_REMOVE=(
    "libEGL.so"
    "libEGL.so.1"
    "libEGL.so.1.1.0"
    "libGLESv2.so"
    "libGLESv2.so.2"
    "libGLESv2.so.2.0.0"
    "libgbm.so"
    "libgbm.so.1"
    "libdrm.so"
    "libdrm.so.2"
    "libdrm.so.2.4.0"
    "libdrm_amdgpu.so.1"
    "libdrm_intel.so.1"
    "libdrm_nouveau.so.2"
    "libdrm_radeon.so.1"
)

removed=0
for lib in "${LIBS_TO_REMOVE[@]}"; do
    target="${LIB_DIR}/${lib}"
    if [[ -f "$target" || -L "$target" ]]; then
        echo "[repack] Removing bundled: $lib"
        rm -f "$target"
        (( removed++ )) || true
    fi
done

echo "[repack] Removed ${removed} bundled EGL/Mesa/DRM file(s)."

# ---------------------------------------------------------------------------
# Repack
# ---------------------------------------------------------------------------
echo "[repack] Repacking -> $OUTPUT_APPIMAGE"
ARCH=x86_64 "$APPIMAGETOOL" --no-appstream "$SQUASH_DIR" "$OUTPUT_APPIMAGE"

chmod +x "$OUTPUT_APPIMAGE"

echo "[repack] Done: $OUTPUT_APPIMAGE"

#!/usr/bin/env bash
# Fetch the MuJoCo release the `mujoco-rs` bindings are generated against and
# lay it out the way the bindings' build script and the dynamic loader expect.
#
#   tools/setup-mujoco.sh [<dir>]      # default: <workspace>/.mujoco
#
# Prints the two environment variables to export before building or running
# anything that links MuJoCo. The version is pinned: the bindings mirror the C
# structs of exactly this release, and a different `libmujoco` (the one a pip
# wheel ships, for instance) has another layout.
set -euo pipefail

MUJOCO_VERSION="3.12.0"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dir="${1:-$here/.mujoco}"
mkdir -p "$dir"

case "$(uname -s)-$(uname -m)" in
  Darwin-*)
    dmg="mujoco-${MUJOCO_VERSION}-macos-universal2.dmg"
    lib="$dir/libmujoco.${MUJOCO_VERSION}.dylib"
    if [ ! -e "$lib" ]; then
      curl -sSL -o "$dir/$dmg" \
        "https://github.com/google-deepmind/mujoco/releases/download/${MUJOCO_VERSION}/${dmg}"
      mount="$dir/mnt"
      mkdir -p "$mount"
      hdiutil attach -nobrowse -quiet "$dir/$dmg" -mountpoint "$mount"
      rm -rf "$dir/mujoco.framework"
      cp -R "$mount/mujoco.framework" "$dir/"
      hdiutil detach -quiet "$mount"
      rmdir "$mount"
      rm "$dir/$dmg"
      # The build links `-lmujoco`; the loader resolves the versioned name.
      ln -sf "mujoco.framework/Versions/Current/libmujoco.${MUJOCO_VERSION}.dylib" "$dir/libmujoco.dylib"
      ln -sf "mujoco.framework/Versions/Current/libmujoco.${MUJOCO_VERSION}.dylib" "$lib"
    fi
    echo "export MUJOCO_DYNAMIC_LINK_DIR=$dir"
    echo "export DYLD_LIBRARY_PATH=$dir\${DYLD_LIBRARY_PATH:+:\$DYLD_LIBRARY_PATH}"
    ;;
  Linux-x86_64|Linux-aarch64)
    arch="$(uname -m)"
    tar="mujoco-${MUJOCO_VERSION}-linux-${arch}.tar.gz"
    if [ ! -e "$dir/lib/libmujoco.so" ]; then
      curl -sSL -o "$dir/$tar" \
        "https://github.com/google-deepmind/mujoco/releases/download/${MUJOCO_VERSION}/${tar}"
      tar -xzf "$dir/$tar" -C "$dir" --strip-components=1
      rm "$dir/$tar"
    fi
    echo "export MUJOCO_DYNAMIC_LINK_DIR=$dir/lib"
    echo "export LD_LIBRARY_PATH=$dir/lib\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
    ;;
  *)
    echo "unsupported platform: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac

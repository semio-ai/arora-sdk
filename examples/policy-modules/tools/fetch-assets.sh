#!/usr/bin/env bash
# Fetch the Microduck model and policies at the revisions this example was
# built against, into assets/microduck/ (ignored by git: 23 MB of meshes and
# 8 MB of networks are not worth carrying in history, and both are published
# under Apache-2.0 by Pollen Robotics at the sources below).
#
#   tools/fetch-assets.sh
#
# Needs git and curl. The module embeds the policies at build time, so run
# this before `cargo build`.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
assets="$here/assets/microduck"
mkdir -p "$assets"

# microduck_rl, branch develop: the MJCF and meshes.
RL_REPO="https://github.com/pollen-robotics/microduck_rl"
RL_REV="cb70b79"
# The Hugging Face policies repository, tag v5.
POLICIES_REPO="https://huggingface.co/pollen-robotics/microduck-policies"
POLICIES_REV="1b56c39"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ ! -e "$assets/model/scene.xml" ]; then
  git clone -q --filter=blob:none --no-checkout "$RL_REPO" "$tmp/rl"
  git -C "$tmp/rl" checkout -q "$RL_REV" -- src/mjlab_microduck/robot/microduck LICENSE
  rm -rf "$assets/model"
  mkdir -p "$assets/model"
  cp -R "$tmp/rl/src/mjlab_microduck/robot/microduck/." "$assets/model/"
  cp "$tmp/rl/LICENSE" "$assets/model/LICENSE"
fi

if [ ! -e "$assets/policies/manifest.json" ]; then
  mkdir -p "$assets/policies"
  for file in manifest.json README.md alpha_walking.onnx alpha_stand.onnx alpha_sitstand.onnx \
              alpha_ground_pick.onnx roulade.onnx ball_kick_left.onnx ball_kick_right.onnx \
              velstand.onnx roller.onnx roller_crouch.onnx; do
    curl -sSL -o "$assets/policies/$file" "$POLICIES_REPO/resolve/$POLICIES_REV/$file"
  done
fi

echo "assets in $assets"

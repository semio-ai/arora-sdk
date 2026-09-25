#!/usr/bin/env python3
"""Render a device recording as a video, or replay it in MuJoCo's viewer.

    uv run --python 3.12 --with mujoco --with numpy --with imageio --with imageio-ffmpeg \
        python tools/replay.py recording.csv --video walk.mp4 [--model assets/microduck/model/scene.xml]
    mjpython tools/replay.py recording.csv --view      # interactive, macOS needs mjpython

The recording is the CSV `arora-hal-mujoco` writes (`--record` on the device):
one row per control period with `time`, every joint's `.position` and `.ctrl`,
and the base's position and orientation. Replaying sets the model's state from
those columns and renders it — no physics runs, so what you see is exactly what
the recorded run did.
"""

import argparse
import csv
import sys
from pathlib import Path

import mujoco
import mujoco.viewer
import numpy as np


def load(path: Path):
    with open(path, newline="") as f:
        rows = list(csv.DictReader(f))
    if not rows:
        sys.exit(f"{path}: empty recording")
    joints = [c[: -len(".position")] for c in rows[0] if c.endswith(".position")]
    return rows, joints


def state(model, row, joints):
    """qpos for one row: the free joint from base.*, then each named joint."""
    qpos = np.array(model.qpos0, dtype=float)
    base = [f"base.{k}" for k in ("x", "y", "z", "qw", "qx", "qy", "qz")]
    if all(k in row for k in base):
        qpos[0:7] = [float(row[k]) for k in base]
    for name in joints:
        joint = model.joint(name)
        qpos[joint.qposadr[0]] = float(row[f"{name}.position"])
    return qpos


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("recording", type=Path)
    parser.add_argument("--model", type=Path, default=Path(__file__).resolve().parent.parent / "assets/microduck/model/scene.xml")
    parser.add_argument("--video", type=Path, help="write an MP4 here")
    parser.add_argument("--view", action="store_true", help="replay in the interactive viewer")
    parser.add_argument("--fps", type=int, default=25)
    parser.add_argument("--width", type=int, default=960)
    parser.add_argument("--height", type=int, default=540)
    args = parser.parse_args()

    rows, joints = load(args.recording)
    model = mujoco.MjModel.from_xml_path(str(args.model))
    # The offscreen buffer must fit the frames; the scene's default is 640×480.
    model.vis.global_.offwidth = max(model.vis.global_.offwidth, args.width)
    model.vis.global_.offheight = max(model.vis.global_.offheight, args.height)
    data = mujoco.MjData(model)
    times = np.array([float(r["time"]) for r in rows])
    period = float(np.median(np.diff(times))) if len(times) > 1 else 0.02

    if args.video:
        import imageio

        renderer = mujoco.Renderer(model, height=args.height, width=args.width)
        camera = mujoco.MjvCamera()
        camera.type = mujoco.mjtCamera.mjCAMERA_FREE
        camera.distance = 0.9
        camera.elevation = -15
        camera.azimuth = 135
        every = max(1, int(round(1.0 / (args.fps * period))))
        with imageio.get_writer(str(args.video), fps=args.fps, codec="libx264", quality=8) as writer:
            for row in rows[::every]:
                data.qpos[:] = state(model, row, joints)
                mujoco.mj_forward(model, data)
                camera.lookat[:] = data.body("trunk_base").xpos
                renderer.update_scene(data, camera=camera)
                writer.append_data(renderer.render())
        print(f"wrote {args.video} ({len(rows[::every])} frames at {args.fps} fps, {times[-1] - times[0]:.1f} s)")

    if args.view:
        import time

        with mujoco.viewer.launch_passive(model, data) as viewer:
            start = time.perf_counter()
            for row in rows:
                data.qpos[:] = state(model, row, joints)
                mujoco.mj_forward(model, data)
                viewer.sync()
                due = start + float(row["time"]) - times[0]
                delay = due - time.perf_counter()
                if delay > 0:
                    time.sleep(delay)
                if not viewer.is_running():
                    break


if __name__ == "__main__":
    main()

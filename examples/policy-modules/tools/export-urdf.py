#!/usr/bin/env python3
"""Export an MJCF robot as a URDF with its visual meshes, for Semio Studio.

    uv run --python 3.12 --with mujoco python tools/export-urdf.py \
        [--model assets/microduck/model/scene.xml] [--out assets/microduck/urdf] [--name microduck]

Studio builds a robot from a URDF and its meshes (STL or DAE, matched by file
name): select the `.urdf` and every `.stl` of the output directory in its
"Upload URDF" dialog. The exported robot is what Studio draws, not what
MuJoCo simulates, so only the kinematics and the looks are carried over:

- one link per MJCF body under the floating base, named after the body;
- one `revolute` joint per hinge, under the MJCF joint's name, with the
  hinge's axis and range; a hinge must sit at its body's origin, as
  onshape-to-robot places them;
- the visual geoms (those that collide with nothing) as `<visual>` meshes,
  coloured by their material; collision geoms, inertias and the floor are
  left out.

The root link is the base body with its free joint dropped: Studio moves the
whole robot through its own root translation and rotation, which the device
publishes from the base pose.
"""

import argparse
import math
import shutil
import sys
from pathlib import Path
from xml.sax.saxutils import quoteattr

import mujoco

HERE = Path(__file__).resolve().parent.parent


def rpy(quat):
    """URDF roll, pitch, yaw (fixed axes X, Y, Z) of a MuJoCo `[w, x, y, z]`."""
    w, x, y, z = (float(v) for v in quat)
    norm = math.sqrt(w * w + x * x + y * y + z * z)
    w, x, y, z = w / norm, x / norm, y / norm, z / norm
    # The rotation matrix R = Rz(yaw) Ry(pitch) Rx(roll), row by row.
    r00, r01, r02 = 1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)
    r10, r11, r12 = 2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)
    r20, r21, r22 = 2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)
    pitch = math.asin(max(-1.0, min(1.0, -r20)))
    # At ±90° of pitch roll and yaw turn about the same axis; the MJCF's
    # quaternions are rounded to six digits, which leaves cos(pitch) around
    # 1e-6 there and roll and yaw each arbitrary. Only their combination is
    # defined: put it all in roll.
    if math.cos(pitch) > 1e-4:
        roll = math.atan2(r21, r22)
        yaw = math.atan2(r10, r00)
    else:
        pitch = math.copysign(math.pi / 2, -r20)
        yaw = 0.0
        roll = math.atan2(-r12, r11)
    return roll, pitch, yaw


def numbers(values):
    return " ".join(f"{float(v):.6g}" for v in values)


def origin(pos, quat):
    return f'<origin xyz="{numbers(pos)}" rpy="{numbers(rpy(quat))}"/>'


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--model", type=Path, default=HERE / "assets/microduck/model/scene.xml")
    parser.add_argument("--out", type=Path, default=HERE / "assets/microduck/urdf")
    parser.add_argument(
        "--name",
        default="microduck",
        help="the robot name; Studio files the model under it, capitalised, as its model family",
    )
    args = parser.parse_args()

    spec = mujoco.MjSpec.from_file(str(args.model))
    meshdir = args.model.parent / (spec.meshdir or "")
    # A mesh declared by its file alone is named after the file's stem.
    meshes = {mesh.name or Path(mesh.file).stem: mesh for mesh in spec.meshes}
    materials = {material.name: material for material in spec.materials}

    roots = [
        body
        for body in spec.worldbody.bodies
        if any(joint.type == mujoco.mjtJoint.mjJNT_FREE for joint in body.joints)
    ]
    if len(roots) != 1:
        sys.exit(f"expected one floating body under the world, found {len(roots)}")

    args.out.mkdir(parents=True, exist_ok=True)
    links, joints = [], []
    used_meshes = set()

    def visuals(body):
        out = []
        for geom in body.geoms:
            if geom.contype != 0 or geom.conaffinity != 0:
                continue  # a collision geom
            if geom.type != mujoco.mjtGeom.mjGEOM_MESH:
                continue
            mesh = meshes[geom.meshname]
            file = Path(mesh.file)
            if file.suffix.lower() != ".stl":
                sys.exit(f"mesh {mesh.name}: {file.name} is not an STL")
            used_meshes.add(file.name)
            material = materials.get(geom.material)
            rgba = material.rgba if material is not None else geom.rgba
            scale = [float(v) for v in mesh.scale]
            # Studio reads a scale as one value or three; three always parse.
            scale_attr = "" if scale == [1.0, 1.0, 1.0] else f' scale="{numbers(scale)}"'
            out.append(
                "    <visual>"
                f"{origin(geom.pos, geom.quat)}"
                f"<geometry><mesh filename={quoteattr(file.name)}{scale_attr}/></geometry>"
                f"<material name={quoteattr(geom.material or mesh.name)}>"
                f'<color rgba="{numbers(rgba)}"/></material>'
                "</visual>"
            )
        return out

    def walk(body, parent):
        name = body.name
        links.append(f"  <link name={quoteattr(name)}>\n" + "\n".join(visuals(body)) + "\n  </link>")
        if parent is not None:
            hinges = [j for j in body.joints if j.type == mujoco.mjtJoint.mjJNT_HINGE]
            if len(hinges) != len(body.joints):
                sys.exit(f"body {name} has a joint that is not a hinge")
            if len(hinges) > 1:
                sys.exit(f"body {name} has {len(hinges)} hinges; URDF allows one joint per link")
            if hinges:
                hinge = hinges[0]
                if any(abs(float(v)) > 1e-9 for v in hinge.pos):
                    sys.exit(f"joint {hinge.name} is not at its body's origin")
                lower, upper = (float(v) for v in hinge.range)
                joints.append(
                    f'  <joint name={quoteattr(hinge.name)} type="revolute">'
                    f"<parent link={quoteattr(parent)}/><child link={quoteattr(name)}/>"
                    f'{origin(body.pos, body.quat)}<axis xyz="{numbers(hinge.axis)}"/>'
                    f'<limit lower="{lower:.6g}" upper="{upper:.6g}" effort="1" velocity="10"/>'
                    "</joint>"
                )
            else:
                joints.append(
                    f'  <joint name={quoteattr(name + "_fixed")} type="fixed">'
                    f"<parent link={quoteattr(parent)}/><child link={quoteattr(name)}/>"
                    f"{origin(body.pos, body.quat)}</joint>"
                )
        for child in body.bodies:
            walk(child, name)

    walk(roots[0], None)

    urdf = args.out / f"{args.name}.urdf"
    urdf.write_text(
        '<?xml version="1.0"?>\n'
        f"<!-- Exported from {args.model.name} by tools/export-urdf.py, for Semio Studio. -->\n"
        f"<robot name={quoteattr(args.name)}>\n" + "\n".join(links + joints) + "\n</robot>\n"
    )
    for file_name in sorted(used_meshes):
        shutil.copyfile(meshdir / file_name, args.out / file_name)
    revolute = sum(1 for joint in joints if 'type="revolute"' in joint)
    print(f"{urdf}: {len(links)} links, {revolute} joints, {len(used_meshes)} meshes")
    error = check(args.model, urdf, roots[0].name, args.name)
    print(f"checked against the MJCF: every visual mesh within {error * 1e6:.1f} µm")
    if error > 1e-5:
        sys.exit("the URDF does not reproduce the MJCF's visuals")


def check(model_path, urdf_path, root, name):
    """Pose the MJCF (in its first keyframe) and the URDF alike and return the
    largest distance between a visual mesh vertex of one and of the other, in
    the root body's frame — what Studio draws against what MuJoCo simulates."""
    import numpy as np

    m1 = mujoco.MjModel.from_xml_path(str(model_path))
    d1 = mujoco.MjData(m1)
    if m1.nkey:
        mujoco.mj_resetDataKeyframe(m1, d1, 0)
    mujoco.mj_forward(m1, d1)

    # MuJoCo reads a URDF with its visuals dropped, the root link merged into
    # the world, and no inertia to simulate: keep the visuals and give every
    # link a token inertia, since only kinematics are compared.
    robot = f"<robot name={quoteattr(name)}>"
    text = urdf_path.read_text().replace(
        robot,
        robot + '<mujoco><compiler discardvisual="false" '
        f"meshdir={quoteattr(str(urdf_path.parent))}/></mujoco>",
        1,
    )
    spec = mujoco.MjSpec.from_string(text)
    for body in spec.bodies[1:]:
        body.mass = 0.01
        body.inertia = [1e-5] * 3
        body.explicitinertial = True
    m2 = spec.compile()
    d2 = mujoco.MjData(m2)
    for j in range(m2.njnt):
        d2.qpos[m2.jnt_qposadr[j]] = d1.joint(m2.joint(j).name).qpos[0]
    mujoco.mj_forward(m2, d2)

    rot = d1.body(root).xmat.reshape(3, 3)
    pos = d1.body(root).xpos

    def vertices(m, d, g):
        k = m.geom_dataid[g]
        v = m.mesh_vert[m.mesh_vertadr[k] : m.mesh_vertadr[k] + m.mesh_vertnum[k]]
        return v @ d.geom_xmat[g].reshape(3, 3).T + d.geom_xpos[g]

    mesh = mujoco.mjtGeom.mjGEOM_MESH
    visual = [
        g
        for g in range(m1.ngeom)
        if m1.geom_type[g] == mesh and m1.geom_contype[g] == 0 and m1.geom_conaffinity[g] == 0
    ]
    drawn = [g for g in range(m2.ngeom) if m2.geom_type[g] == mesh]
    if len(visual) != len(drawn):
        sys.exit(f"{len(visual)} visual meshes in the MJCF, {len(drawn)} in the URDF")
    worst = 0.0
    for a, b in zip(visual, drawn):
        # Each model re-centres a mesh on its own inertia, so vertices are
        # compared as sets, in the root's frame.
        w1 = np.sort((vertices(m1, d1, a) - pos) @ rot, axis=0)
        w2 = np.sort(vertices(m2, d2, b), axis=0)
        worst = max(worst, float(np.abs(w1 - w2).max()))
    return worst


if __name__ == "__main__":
    main()

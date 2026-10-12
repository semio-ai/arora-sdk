# Proposal: target keys as an entity, `k/target.c`

Status: draft, for review.
Date: 2026-10-12.

A device's inputs are mostly targets: the value a client asks one component of
an entity to reach — a joint's position, its velocity, its torque. This
document proposes one key form for every target, `k/target.c`, states the forms
it replaces, and sets the rule by which devices and clients use both until
every client writes the new one.

In short:

- **A target is an entity under the entity it targets.** The target for the
  component `c` of the entity at path `k` is `k/target.c`:
  `hip_yaw/target.position`, `hip_yaw/target.velocity`,
  `hip_yaw/target.torque`. The measured value stays `k.c`
  (`hip_yaw.position`).
- **One form for every value type and component.** No `target_` prefix spelled
  per component, and no name of its own for the value a client sends:
  `studio_value` is retired as a name.
- **Devices accept both forms** and list both as open inputs; a client writes
  the form the device lists, preferring `k/target.c`.
- **A change that sets both forms of one target applies `k/target.c`.**

Keys are paths, and an `.attribute` is a naming convention of the consumers that
adopt it ([keys are paths](design_decisions.md#keys-are-paths-attributes-are-a-convention)).
This proposal is a change to that convention as the HALs and their clients use
it; stores, bridges and key meta are unaffected.

---

## 1. What exists

- **Measured values** are `k.c`, the entity's path with the component as its
  attribute: `hip_yaw.position`; from a ROS 2 `JointState`, also
  `hip_yaw.velocity` and `hip_yaw.effort`.
- **Targets** are `k.target_c`, the component prefixed with `target_`:

  | Reader | Target keys | Measured key it drives |
  |---|---|---|
  | `FakeHal` (arora-hal) | `<joint>.target_position` | `<joint>.position`, mirrored |
  | `RestfulHal` (arora-hal-restful) | `<joint>.target_position`, `<joint>.target_velocity` | `<joint>.position`, `<joint>.velocity`, mirrored |
  | `Ros2Hal` (arora-hal-ros2), joint commands | root-namespace `<joint>.target_position` | `<joint>.position`, from the robot |
  | `Ros2Hal`, `JointTrajectory` | `common.target_trajectory` | — |

- **Semio Studio** writes, for each animatable a device pairs with,
  `<id>.studio_value` with the raw value of any type; for a number also
  `<id>.target_position`, and `<id>.target_velocity` while an animation plays.
  No device reads `studio_value`. A string or boolean target has no key a
  device reads; under the current form it would be `k.target_value`.
- **A device's inputs are its key meta.** A remote may write only a key whose
  meta is `editable`, stated on the key or on a prefix that covers it, and
  `ListKeys` lists the keys a device holds or describes, with their meta. No HAL
  describes its keys yet; that is the key-description trait of
  [HAL components](proposal-hal-components.md) 3.5
  ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)).

## 2. The convention

The target for the component `c` of the entity at path `k` is the key
`k/target.c`: an entity named `target` under `k`, its attribute the component
it targets.

| Value | Measured | Target | Target, current form |
|---|---|---|---|
| A joint's position | `hip_yaw.position` | `hip_yaw/target.position` | `hip_yaw.target_position` |
| A joint's velocity | `hip_yaw.velocity` | `hip_yaw/target.velocity` | `hip_yaw.target_velocity` |
| A joint's torque | `hip_yaw.torque` | `hip_yaw/target.torque` | `hip_yaw.target_torque` |
| A string or a boolean | `k.value` | `k/target.value` (see 6) | `k.target_value` |
| A namespaced joint | `robot/hip_yaw.position` | `robot/hip_yaw/target.position` | `robot/hip_yaw.target_position` |

Attributes after the component carry over to the measured key:
`hip_yaw/target.position.x` targets `hip_yaw.position.x`. A key whose last
segment is `target` with no attribute, or a `target.c` at the root (no `k`), is
not a target.

### Why

- **One form for every value type and component.** A target is `target.` plus
  the component's own name, so a component a device adds — torque, stiffness,
  a colour — has its target without a new name. The current form spells a
  `target_` variant of each component, and the value a client sends for any
  other type needed a name of its own (`studio_value`); neither is needed.
- **A target and its measurement name the same entity and component.**
  `hip_yaw/target.position` and `hip_yaw.position` differ only by `/target`, so
  a reader derives one from the other without a table of component names.
- **The targets of an entity are a subtree.** A prefix covers keys on `/`
  boundaries, so the prefix `hip_yaw` covers `hip_yaw/target.position` and
  `hip_yaw/target.velocity` and not `hip_yaw.position`: a device opens an
  entity's targets with one prefix meta, and its measured values stay closed.
- **The name says what the key is, not who writes it.** A target is a target
  whether Studio, a behavior or another device sets it; `studio_value` named
  the writer.

## 3. Both forms during the transition

Studio and the devices ship separately, so for a while a device may meet a
client that writes either form, and a client a device that reads only the
current one. The rule:

1. **A device accepts both forms.** A HAL reads a target from `k/target.c` and
   from `k.target_c` alike. `arora_hal::set_targets` reads both from a change;
   the HALs in arora-sdk use it (§4).
2. **A device lists both forms as open inputs.** Where it declares an input
   by key, it states `editable` meta on `k/target.c` and on `k.target_c`; a HAL
   that describes its keys (HAL components 3.5) describes both for each target
   it reads.
3. **A client resolves each input's key from what the device lists**
   (`ListKeys`): `k/target.c` when listed, `k.target_c` otherwise. A key the
   device does not list by name is written in the current form, which every
   device reads.
4. **A client writes one form per target.** A change that sets both forms of
   one target applies `k/target.c` and ignores `k.target_c`: the keys of a
   `StateChange` carry no order, so "last write wins" cannot apply within one
   change, and the form decides. Across changes, the later write wins, as for
   any key.
5. **Nothing new emits `studio_value`.** A client sends a string or boolean
   target as `k/target.value` (see 6).

The current form is retired by a major of each HAL crate once Studio writes
`k/target.c` and no device in use depends on `k.target_c`.

## 4. What changes per HAL

| HAL | Reads, current form | Reads, added | Both in one change |
|---|---|---|---|
| `FakeHal` (arora-hal 4.2) | `<joint>.target_position` | `<joint>/target.position` | `<joint>/target.position`'s value is held and sensed back |
| `RestfulHal` (arora-hal-restful 4.1) | `<joint>.target_position`, `<joint>.target_velocity` | `<joint>/target.position`, `<joint>/target.velocity` | the `<joint>/target.…` value is requested and mirrored |
| `Ros2Hal` (arora-hal-ros2 5.2) | root-namespace `<joint>.target_position` | root-namespace `<joint>/target.position` | the `<joint>/target.position` value is commanded |

- `Ros2Hal`'s joint commands carry positions only, so no velocity or effort
  target is read in either form. `common.target_trajectory` is unchanged
  until 6 settles it.
- A HAL outside arora-sdk's main line that reads `k.target_c` keys — the NAOqi
  HAL ([arora-sdk#258](https://github.com/semio-ai/arora-sdk/pull/258)), the
  MuJoCo HAL of the policy-modules example
  ([arora-sdk#243](https://github.com/semio-ai/arora-sdk/pull/243)) — reads
  its targets through `set_targets` to accept both.
- No HAL declares inputs today; the device opens them through its store's key
  meta. Rule 2 applies to a device that does, and to HALs once they describe
  their keys.

On the Studio side, the device commander resolves each animatable's target key
by rule 3 and stops writing `<id>.studio_value`.

## 5. Alternatives

| Alternative | Why not |
|---|---|
| Keep `k.target_c` | A name per component, and a second name (`studio_value`) for the value of any other type. |
| `k.c.target` | Attributes after the component are a path into the value (`Key::select`); `target` would read as a field of the position. |
| `target/k.c`, all targets under one root | An entity's keys no longer share its path, and a namespaced device has two candidate spellings (`target/robot/hip_yaw.position`, `robot/target/hip_yaw.position`). |
| Accept only the new form | Studio and devices ship separately; a device updated before Studio would lose every input. |

## 6. Open questions

1. **Strings and booleans: `k/target.value` or `k/target`?** `k/target.value`
   keeps "a target has components" for every type. `k/target` is shorter, but
   makes `target` a value for some entities and a holder of components for
   others, and leaves the measured counterpart (`k`? `k.value`?) to be named.
2. **Trajectories.** A `JointTrajectory` spans joints, so it is no one joint's
   component. Candidates: `common/target.trajectory` (the convention with
   `common` as the entity), `target.trajectory` under the device's root, or
   keeping `common.target_trajectory` as a key outside the convention.
3. **Torque or effort.** ROS 2's `JointState` reports `effort`, the HAL
   mirrors it as `<joint>.effort`, and a torque target would be
   `<joint>/target.torque`. One name per quantity keeps a target and its
   measurement aligned.
4. **Transports.** A ROS 2 topic name has no `.`, so neither form travels as a
   topic path as it is; an exposure profile names each one it carries. Whether
   the `/target` segment changes how profiles group a joint's keys is open.

# Proposal: target keys as an entity, `k/target.c`

Status: draft, for review.
Date: 2026-10-22.

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
- **A change that sets both forms of one target applies `k/target.c`, and
  the device keeps the two forms in agreement:** it writes the value that
  wins into both, since a store notifies a write only when it changes a key's
  value (§3).
- **The axis of a structured value comes after the component:**
  `eye/target.position.x` targets `eye.position.x`, the field `x` of the
  value `eye.position` (§5). Studio's per-axis keys, axis first today
  (`eye.x.target_position`, `eye.x.position`), move to it in stages that are
  each safe to ship (§6).
- **A whole value covers its fields.** `k/target.c` covers
  `k/target.c.<field>`, as `k.c` covers `k.c.<field>`: a later write of the
  whole drops earlier writes of its fields, and within one change the whole
  applies first (§5).
- **A device has the keys it describes.** The component names the quantity
  (`position`, `color`, `intensity`), and no component implies another: a
  position brings no velocity, a measured value no target, and a value set
  directly, with no dynamics behind it, takes `k.c` itself (§2).

Keys are paths, and an `.attribute` is a naming convention of the consumers that
adopt it ([keys are paths](design_decisions.md#keys-are-paths-attributes-are-a-convention)).
This proposal is a change to that convention as the HALs and their clients use
it; stores, bridges and key meta are unaffected.

It sets how the address of a target is spelled. Semio Studio's rule of one
address per value under a device (decision D3 of its Studio-on-Arora plan,
semio_studio `docs/studio-on-arora/05-decisions.md`) stands: `k/target.c` is
how a target's address is spelled under it, in place of the
`k.target_position` D3 names.

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
- **Studio's structured values are numbers, one per axis.** A vector, euler or
  colour animatable is stored as one number animatable per axis, with the id
  `<id>.<axis>` and a group naming the axis (§6.1). Its keys put the axis
  first: `<id>.<axis>.target_position` for the input, `<id>.<axis>.position`
  for the feedback. In arora's key grammar the component is the first
  attribute (`Key::get_component`), so to a HAL the component of
  `eye.x.target_position` is `x`, and `set_targets` reads it as no target.
- **A device's inputs are its key meta.** A remote may write only a key whose
  meta is `editable`, stated on the key or on a prefix that covers it, and
  `ListKeys` lists the keys a device holds or describes, with their meta. A HAL
  describes its keys (`Hal::describe_keys`, arora-hal 4.3;
  [HAL components](proposal-hal-components.md) 3.5), and the runtime states the
  description to the store: `FakeHal`, `RestfulHal` and `Ros2Hal` describe
  each target they read, in both forms, as an open input. A HAL that describes
  no key leaves its host to open its inputs through key meta.

## 2. The convention

The target for the component `c` of the entity at path `k` is the key
`k/target.c`: an entity named `target` under `k`, its attribute the component
it targets.

| Value | Measured | Target | Target, current form |
|---|---|---|---|
| A joint's position | `hip_yaw.position` | `hip_yaw/target.position` | `hip_yaw.target_position` |
| A joint's velocity | `hip_yaw.velocity` | `hip_yaw/target.velocity` | `hip_yaw.target_velocity` |
| A joint's torque | `hip_yaw.torque` | `hip_yaw/target.torque` | `hip_yaw.target_torque` |
| A light's colour | `light.color` | `light/target.color` | `light.target_color` |
| A string or a boolean | `k.value` | `k/target.value` (see 8) | `k.target_value` |
| A namespaced joint | `robot/hip_yaw.position` | `robot/hip_yaw/target.position` | `robot/hip_yaw.target_position` |
| One axis of a structured value | `eye.position.x` | `eye/target.position.x` | `eye.target_position.x` |

Attributes after the component carry over to the measured key:
`eye/target.position.x` targets `eye.position.x`. They select a field of the
component's value, so the axis of a vector, an euler or a colour comes after
the component, never before it (§5). A key whose last
segment is `target` with no attribute, or a `target.c` at the root (no `k`), is
not a target. Under a namespace, a last segment `target.c` is always this form:
`x/target.target_position` targets `x.target_position`, so a joint named
`target` keeps the current form only at the root.

The component names the quantity, whatever the value's type: a joint's
`position`, a light's `color` (`light/target.color`; one channel,
`light.color.r`), an LED's `intensity` (`led/target.intensity`). A number is
not a position for being a number.

### Which keys a device has

A device has the keys it describes, and this convention spells them. A HAL
describes each component it reports as `k.c`, closed, and each component it
takes as a target as `k/target.c`, open, with `k.target_c` beside it during
the transition (`arora_hal::target_keys` gives both). Nothing exists
undescribed, and no component implies another:

- **A position brings no velocity or acceleration.** A joint `k1` described
  with a position, measured and targeted, has `k1.position` and
  `k1/target.position` (and `k1.target_position` during the transition), and
  no other key. That is what `FakeHal` describes for each joint.
- **A measured value brings no target, and a target no measured value.** An
  actuator driven feedforward, such as a light, describes its target and no
  measured value: a light described with a colour target has
  `light/target.color`, and no `light.color` unless it reports one.
- **A value set directly has no target.** A target is the value a component
  is commanded to reach, which a robot's joint does through its own dynamics.
  A device with nothing between a write and its value, such as a Vizij face
  that renders each value as it is written, describes `k.c` itself as open
  and no target: a client writes `k.c`.

The convention says how a described component's keys are spelled; it is not a
list of components every actuator has. A client drives each device by the keys
that device describes (§3, rule 3).

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
   the HALs in arora-sdk use it (§4,
   [arora-sdk#334](https://github.com/semio-ai/arora-sdk/pull/334)).
2. **A device lists both forms as open inputs.** Where it declares an input
   by key, it states `editable` meta on `k/target.c` and on `k.target_c`; a HAL
   that describes its keys (HAL components 3.5) describes both for each target
   it reads.
3. **A client resolves each input's key from what the device lists**
   (`ListKeys`): the first of `k/target.c`, `k.target_c` and `k.c` it lists
   open. A device that lists none of an input's forms is written in the
   current form, which every HAL in arora-sdk reads. That fallback lasts until
   every device a client drives describes its keys; a client then drives only
   what a device lists, and states the rest as not driven.
4. **A client writes one form per target.** A change that sets both forms of
   one target applies `k/target.c` and ignores `k.target_c`, whatever value
   `k/target.c` holds: the keys of a `StateChange` carry no order, so "last
   write wins" cannot apply within one change, and the form decides. Arora's
   runtime hands its HAL each step's store changes merged key by key into one
   change (`flush`, `crates/arora/src/runtime.rs`), so writes to the two forms
   within one step also reach the HAL as one change, and the form decides
   there too. That loses no order between remotes: a writer writes one form,
   and the writes of two remotes within one step carry no order between them.
   It does override one order the runtime sets: it applies a step's bridge
   events before it ticks the behavior (`Arora::step`), so on one key the
   behavior's write wins, but where a remote and the behavior write different
   forms of one target within one step, `k/target.c`'s value applies,
   whichever of them wrote it. Across steps, the
   later write wins, whichever form each was written in; a HAL that merges a
   backlog of changes into one keeps that (`arora_hal::fold_newest_wins`), and
   rule 5 keeps the store from dropping the later write. A whole target
   covers its fields: a later write or unset of the whole drops earlier
   writes of its fields, and within one change the whole applies before its
   fields (§5). The two forms are distinct keys of the store: a store or a
   bridge never merges them.
5. **A device keeps the two forms of a target in agreement.** It writes the
   value that wins by rule 4 into both keys, and an unset of either form, with
   neither set, into both. A client sees both keys move together, whichever
   form it wrote, and the write to the form it did not use is the device's
   own. Every HAL arora's runtime feeds reads its targets from the store's
   notifications, the step's changes the runtime hands it (`write_hal`), so
   for those HALs the runtime applies the rule. It does not yet
   ([ARORA-187](https://linear.app/semio-ai/issue/ARORA-187)).
   - **Why it is required:** a store notifies a write only when it changes
     the key's value (`DataStore::write`): a key set to the value it already
     holds is left out of what observers see. Were each form to hold only
     what was last written to it, a write that returns one form to the value
     it holds would not be notified, though the other form has moved since:
     `hip_yaw.target_position` set to 1, `hip_yaw/target.position` to 2, then
     `hip_yaw.target_position` to 1, in three steps, would leave the HAL at 2
     where rule 4 gives 1. With the value that wins in both keys, the next
     write to either form is a change, and is notified.
   - **A consumer that reads the store's values needs it too.** A node graph,
     like Vizij's, reads each input's current value every tick. It cannot tell
     which of two keys was written last, or that both arrived in one change;
     the store's `write` sees each change whole, the unit `set_targets` reads,
     so the rule is applied there and the graph reads one form
     ([vizij-rs#199](https://github.com/vizij-ai/vizij-rs/pull/199)).
   - **Why not a third key:** the alternative is an internal key holding the
     resolved target, which every bridge would relay and every client would
     see as one more key with no writer of its own.
6. **Nothing new emits `studio_value`.** A client sends a string or boolean
   target as `k/target.value` (see 8).

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
  until 8 settles it.
- A HAL outside arora-sdk's main line that reads `k.target_c` keys — the NAOqi
  HAL ([arora-sdk#258](https://github.com/semio-ai/arora-sdk/pull/258)), the
  MuJoCo HAL of the policy-modules example
  ([arora-sdk#243](https://github.com/semio-ai/arora-sdk/pull/243)) — reads
  its targets through `set_targets` to accept both.
- The three HALs describe each target they read in both forms
  (`Hal::describe_keys`, arora-hal 4.3), as rule 2 states. A device whose HAL
  describes no key opens its inputs through its store's key meta, and rule 2
  applies to that meta.

On the Studio side, the device commander resolves each animatable's target key
by rule 3 and stops writing `<id>.studio_value`; §6 covers its per-axis keys.

## 5. Components of a structured value: the axis after the component

A component's value may be structured: a vector, an euler, a colour, a
quaternion. An axis is a field of that value, and its key names the field
after the component:

| | The whole value | One axis |
|---|---|---|
| Measured | `eye.position` | `eye.position.x` |
| Target | `eye/target.position` | `eye/target.position.x` |
| Target, current form | `eye.target_position` | `eye.target_position.x` |

`set_targets` (arora-hal 4.2) reads every target of this table, each for the
measured key beside it. A key with the axis before the component —
`eye.x.target_position`, `eye.x/target.position` — is no target.

A device lists what it takes: the whole value, its axes, or both.
`eye/target.position` and `eye/target.position.x` are distinct keys, related
by the rule below.

### A whole value and its fields

For any key, not only a target: `k.c` covers `k.c.<field>`, and `k/target.c`
covers `k/target.c.<field>`. The rule holds for keys in general because a
per-field write may land on a rig input or a key-table entry as well as on a
target (§6). It orders writes; it is no prefix (prefixes match on `/`
boundaries only, below), and a store or a bridge carries each key as it is.

- **A later write or unset of the whole drops earlier writes of its fields.**
  `eye/target.position.x` set to 5, then `eye/target.position` set to
  (0, 0, 0) in a later change, leaves x at 0.
- **Within one change, the whole applies first, then its fields.** A change
  that sets `eye/target.position` to (0, 0, 0) and `eye/target.position.x` to
  5 leaves x at 5. The keys of a change carry no order, so the rule decides,
  as the form does in §3 rule 4.
- **A device that lists both the whole and its fields keeps them in
  agreement**, as §3 rule 5 does for the two forms of a target, so a consumer
  that reads the store sees `eye/target.position` and `eye/target.position.x`
  agree.
- **Merging a field write into the whole is optional.** A device MAY accept a
  write of one field to an input that takes the whole value, and merge it
  into the value it holds; it need not. Most HALs and bridges cannot: a ROS 2
  command or a REST request carries the whole value, and Arora devices
  exchange whole values at the pace their steps set. A client writes what the
  device lists (§3 rule 3): the whole value, where that is all it lists.
- **Whether a per-field write is meaningful depends on the value.** The fields
  of a vector, an euler or a colour are independent; those of a quaternion or
  a pose are not, and one of them set alone makes no valid value. Saying
  which needs a declaration, on the key's meta or the value's type, which
  this proposal does not settle (§8).

In arora-hal, `set_targets` and `fold_newest_wins` read a whole target and a
field's as unrelated
([ARORA-186](https://linear.app/semio-ai/issue/ARORA-186) applies the rule):

- `set_targets` returns one target per measured key, in no particular order
  (`HashMap::into_values`), so a whole and one of its fields come out as two
  targets in either order. Under the rule it returns the whole first.
- `fold_newest_wins` drops an earlier target only when the later change sets
  or unsets one with the same measured key, so an earlier
  `eye/target.position.x` survives a later `eye/target.position`. Under the
  rule a later set or unset of the whole, in either form, also drops the
  earlier sets of its fields' targets, in either form.

### Why the axis comes after

- **The component names the quantity; the axis selects a field of its value.**
  `eye.position` is the position, whole: a vector. `eye.position.x` is a field
  of it. This is the order of arora-types' selector paths: `Key::select`
  descends one attribute per level, so `eye.position.x` reads the field `x`
  out of the value `eye.position` holds. `select` matches a `KeyValue` field
  by its string key, a `Structure` field by its id (a UUID) and an array
  element by its index, so the name `x` selects directly only from a
  `KeyValue`. From a `Structure`, the reader first maps the name to the
  field's id through the value's structure type, which names each field id;
  from an array, to the element's index. In `eye.x.position`, `x` would be a
  value holding a `position`, and there is no such value.
- **Axis first has no spelling in the `k/target.c` form.** `eye.x/target.position`
  puts a `.` before the last `/`, and attributes belong to the last segment
  only ([keys are paths](design_decisions.md#keys-are-paths-attributes-are-a-convention));
  `eye/target.x.position` makes `x` the targeted component. Keeping the axis
  first would mean keeping `k.target_c` for structured values for good, or a
  second grammar for them.
- **arora's grammar already reads it this way.** The component is the first
  attribute (`Key::get_component`), and `set_targets` keeps the attributes
  after it when it derives the measured key, so `eye/target.position.x`
  targets `eye.position.x` with no rule of its own, and `FakeHal` senses it
  back as a position. To the same grammar the component of
  `eye.x.target_position` is `x`: no HAL in arora-sdk reads it as a target.
- **One target per quantity.** The target of `eye`'s position is
  `eye/target.position`, and its axes are fields of it, so a device that
  takes the vector whole and one that takes it per axis name the same target,
  and a client moves from one to the other by the select path alone.
- **Paths and prefixes, as they are today.** A store, a bridge and a
  subscription give `.` no meaning: prefix meta and prefix subscriptions match
  on `/` boundaries (`prefix_covers`). So `eye.position` covers the key
  `eye.position` alone, not `eye.position.x`, and `eye.x` does not cover
  `eye.x.position` either: neither order makes a quantity's axes a prefix
  today. What does is the target form: the prefix `eye` covers
  `eye/target.position.x` with every other target of `eye`. Should prefixes
  ever match on `.` boundaries too, axis after makes `eye.position` cover the
  whole quantity and `eye/target.position` its whole target; axis first makes
  `eye.x` cover one axis across unrelated quantities (`eye.x.position`,
  `eye.x.velocity`), a grouping no consumer reads.
- **The formats keys meet name fields the same way.** A ROS 2 message field is
  its path from the message root, quantity before axis: `pose.position.x`,
  `twist.linear.x` (`ros2 topic echo --field pose.position.x`), so an exposure
  profile maps a key's attributes onto a field path as they are. Vizij's
  `TypedPath` is `namespace/…/node.field.subfield`: a node's `translation`,
  then `.x`. Studio's live-data layout states a component's attribute the same
  way (`left_arm/shoulder.position.x`).

### Objections

| Objection | Answer |
|---|---|
| Axis first keeps an axis's target and its measurement side by side (`eye.x.target_position`, `eye.x.position`). | Under `k/target.c` a target is an entity of its own, `eye/target.…`, in either order, so neither order keeps the two side by side. A target and its measurement are paired by a rule — remove `/target` — not by sorting. What axis after keeps together is the quantity: `eye.position.x`, `.y` and `.z` sort and group as one, which is what a plot, a key tree and a reader of the whole vector want. |
| Studio's data and its saved layouts use axis first. | Studio persists animatable ids (`<id>.<axis>`) and outputs, not device keys. They stay, and the device key is derived from them (§6.1). The device keys Studio persists verbatim are the rows of saved live-data layouts, handled by stage (d) of §6.3. Recordings are not persisted. |
| Studio's per-axis animatables are first-class: each axis has its own id, name, range, track and row. | They stay first-class in Studio; only the device key the World spells for them changes. A device key names the device's value, and on a device an axis is a field of a quantity. Studio already records which value an axis belongs to (`group.id`, `group.type`, `group.axis`), so the key carries what Studio knows, in the device's terms. A device that takes each axis on its own lists `eye/target.position.x` and is written per axis; nothing makes it take the whole value. |

### What it costs

- **Studio's per-axis keys change spelling, feedback included:**
  `<id>.<axis>.target_position` becomes `<id>/target.position.<axis>`, and
  `<id>.<axis>.position` becomes `<id>.position.<axis>`.
- **A device with per-axis keys carries both spellings for a while:** it lists
  both as inputs and publishes its feedback under both (§6.3 a).
- **Saved live-data rows that name a per-axis key** are rewritten (§6.3 d).

Production Studio sends no per-axis target today (§6.1), so the change of
input spelling breaks nothing in production.

## 6. Studio's per-axis keys

Paths below are semio_studio's. *Main* is Studio's production line.
*Staging* carries the Studio-on-Arora World (`packages/studio-arora`), which
resolves each animatable's input and feedback keys per bound device
(semio_studio PR 542).

A target key is a device's input: the output of the pipeline that drives the
device, not what a track addresses. A device's behavior writes the animation
module's values to the keys of a key table (the module's `output_keys`,
written by the stepping behavior's `Scatter` node,
[arora-sdk#399](https://github.com/semio-ai/arora-sdk/pull/399)), and
`k/target.c` is one possible entry of that table. A track may drive a rig
input or another controllable, from which a rig derives the joint values; its
key is then no device target. The derivation below of an axis's device key
from its animatable id is the binding while a track writes a device input
directly.

### 6.1 How Studio spells an axis

- **Ids.** A vector (`x`, `y`, `z`), euler (`r`, `p`, `y`), RGB (`r`, `g`,
  `b`) or HSL (`h`, `s`, `l`) animatable is split into number animatables,
  one per axis, with the id `<id>.<axis>`, the output `<output>.<axis>` and
  `group: { id, type, axis, order }` (`AnimatableGroupInfo`,
  `packages/utils/src/animated-values.ts`). The split happens on load
  (`packages/scene/src/functions/gltf-loading/map-features.ts`,
  `packages/vizij/src/functions/gltf-loading/map-features.ts`) and in the
  animation migration (`packages/animation/src/functions/migrate-animation-data.ts`,
  `normalize-track-data.ts`). Studio has no quaternion animatable.
- **Device keys, on staging.** The World's key for an animatable is its
  output or its id (`packages/studio-arora/src/animatable.rs`), so an axis's
  key is `<id>.<axis>`, and it stays the key on the animation plane and in
  the key table. The World spells an axis's device keys from its group's key
  `k` (that key without the trailing `.<axis>`, the axis read from
  `group.axis`) and the axis. Its inputs are `k/target.c.<axis>`,
  `k.<axis>.target_c` and the bare `k.<axis>` (`Declared::input_in`),
  resolved per device (§6.2). Its feedback is read at `k.c.<axis>` where the
  device lists it, else at `k.<axis>.c` (`resolve_feedback`). An entity's
  writes address `k.<axis>.target_c` (`DECLARED_FORM`), which an emulated
  device declares as its input, and an emulated device reports `k.<axis>.c`.
  The component `c` is `position` for every number.
- **Device keys, on main.** The device commander
  (`apps/studio/src/store/subscriptions/device-subscriptions.ts`) reads
  feedback as any `<name>.position`, so it reads `<id>.<axis>.position`. It
  takes the animatable id of a values-store key as the text between its first
  and second `.`, so an axis id `<id>.<axis>` matches no animatable and no
  per-axis target is sent.
- **The `vizij` device** takes `<id>.<axis>.target_position` and, with
  [vizij-rs#199](https://github.com/vizij-ai/vizij-rs/pull/199),
  `<id>/target.position.<axis>`, kept in agreement (§3 rule 5). It publishes
  feedback as `<id>.<axis>.position`.

### 6.2 What changes, by surface

- **The World's per-device resolution.** On staging the World tries an
  axis's inputs as `k/target.c.<axis>`, Studio's `k.<axis>.target_c`, then
  the bare `k.<axis>`, under the rules PR 542 sets for a scalar (the first
  form listed open; not driven when a target form is listed closed;
  `k.<axis>.target_c`, unchecked, when none is listed), and `undriven` names
  the keys tried. A scalar's sequence is `k/target.c`, `k.target_c`, then the
  bare `k`. Both lack `k.c`, the form a device with no dynamics takes (§2),
  which §3 rule 3 tries third. It joins both sequences before the bare key:
  `k/target.c`, `k.target_c`, `k.c`, then `k` for a scalar;
  `k/target.c.<axis>`, `k.<axis>.target_c`, `k.c.<axis>`, then `k.<axis>`
  for an axis. A device that lists only `eye.position.x` open is then written
  there, not in `eye.x.target_position`, which it does not read. Like a bare
  key, `k.c` or `k.c.<axis>` listed closed is the device's feedback, an
  output, and states nothing about its targets.
- **The component.** The World names the component of every number
  `position` (`Animatable::declared`), so a colour's channel reads
  `light.position.r`. It names a grouped animatable's component from its
  group's type instead, `light.color.r` for a colour (§2;
  [STUDIO-394](https://linear.app/semio-ai/issue/STUDIO-394)). That renames
  keys devices read and publish, so it ships with stage (c), and the
  `position` spellings stay candidates of their own until stage (e): the
  World tries each sequence with the group's component, then with `position`,
  then the bare key. For the channel `r` of `light`, the inputs are
  `light/target.color.r`, `light.r.target_color`, `light.color.r`, then
  `light/target.position.r` and `light.r.target_position`, which the `vizij`
  device takes (§6.1), then `light.position.r`, then `light.r`. The feedback
  is `light.color.r`, `light.r.color`, then `light.position.r`,
  `light.r.position`. A device that lists none is written and read in
  `position`, axis first (`light.r.target_position`, `light.r.position`), as
  on staging.
- **The scene binding** (`packages/utils/src/scene-binding.ts`). It spells no
  key: it reads each device's input and feedback keys from the World
  (`inputKeys`, `feedbackKeys`), and `AnimatableKeys.feedback` holds one key
  per device. The component the World names reaches the scene's feedback
  with no mapping of its own.
- **Device pairing.** Pairing lists the device's keys (`ListKeys`) and the
  World resolves each animatable from them, axes included; a device that
  lists only one spelling of an axis is written in that one.
- **The live-data table** (`apps/studio/src/components/widgets/live-data-table.tsx`).
  It builds its tree from `/` and `.` segments, so axis after groups `x`, `y`,
  `z` under their quantity instead of each axis holding its own quantities. A
  row the device does not stream directly is resolved by navigating from the
  key's *first* `.` into the entity's value; a row `eye.position.x` over a
  streamed whole `eye.position` needs navigation from the longest streamed
  prefix instead.
- **Saved layouts.** A live-data layout (`liveDataViewConfigs`, per user and
  device, `apps/studio/src/hooks/use-live-data-layout.ts`) stores rows as raw
  device keys and collapsed groups as key prefixes, so a row or group naming
  a per-axis key in the old spelling shows nothing once the device stops
  publishing it.
- **Recordings** (`packages/animation/src/functions/recorder.ts`). A recording
  names `{ device, key }` pairs and is kept in memory only. It takes its keys
  from the World's per-device keys, so it follows the resolution; nothing
  recorded is persisted to migrate.
- **Timeline and animation tracks.** A track addresses an animatable id, one
  scalar track per axis (`<id>.<axis>`), never a device key. An animation
  delivered to an emulated device is keyed `animation/<instance>/<k>`, which
  for an axis is `animation/<instance>/<id>.<axis>`: a bare key whose field
  follows the value, the order of §5, so it stays. Each track's route goes to
  the input key the device resolved, so routes follow the resolution with no
  change of their own.

### 6.3 The plan

Each stage is safe to ship on its own: none removes a spelling something still
writes or reads.

| Stage | What ships | Gated by | If skipped |
|---|---|---|---|
| **(a) Devices take both spellings.** | A device with per-axis keys — the `vizij` device first — lists `k/target.c.<axis>` and `k.<axis>.target_c` as open inputs, keeps them in agreement (§3 rule 5), and publishes its feedback under both `k.c.<axis>` and `k.<axis>.c`. For a grouped value whose component (c) renames, `color` for a colour, it does so under both component names: it takes `light/target.color.r` beside `light/target.position.r` and `light.r.target_position`, and publishes `light.color.r` beside its `position` feedback. | Nothing: it adds keys. vizij-rs#199 ships the inputs under `position`; the feedback under `k.c.<axis>` and the new component's keys are to add. | A device that moves to axis after alone is not driven or read by a Studio before (b), and one that moves to the new component alone is not driven or read by a Studio before (c). |
| **(b) Studio resolves axes per device.** | The World resolves an axis's input and feedback per device as in §6.1 and §6.2, with its own spelling and feedback unchanged (`k.<axis>.target_c`, `k.<axis>.c`). | semio_studio PR 542, on staging; the direct-write form `k.c.<axis>` is to add. | Studio cannot drive a device that lists only axis after, and setting `DECLARED_FORM` to `k/target.c` would declare `<id>.<axis>/target.position`, which no device reads as a target. |
| **(c) Studio switches its spelling.** | `DECLARED_FORM` becomes `k/target.c` for every animatable, and a grouped animatable's component becomes its group's, `color` for a colour (§6.2): emulated devices declare `k/target.c.<axis>` and report `k.c.<axis>` in that component. The World keeps the `position` spellings as candidates of their own, and writes and reads a device that lists none of an axis's spellings in `position`, axis first. The live-data table resolves from the longest streamed prefix. | This proposal accepted; (a) shipped on every device Studio drives per axis, the new component's feedback included; (b). | Studio keeps writing a spelling and a component meant to retire, and (e) cannot start. |
| **(d) Saved data.** | Projects and animations: nothing, their ids do not change. Live-data layouts: a row or collapsed group whose key the device no longer lists is rewritten to the other spelling when the device lists that one, and the layout saved; or a one-time migration of `liveDataViewConfigs`. Recordings: nothing, none is persisted. | (c). | Once (e) ships, saved per-axis rows go blank, silently. |
| **(e) The axis-first spelling and the `position` of a grouped value retire.** | Devices stop listing `k.<axis>.target_c` and publishing `k.<axis>.c`, and stop taking and publishing a grouped value under `position` (a major of each); the World drops those spellings from its preference. | Studio in production at (c); (d) shipped; no device in use relying on axis first or on `position` for a grouped value. As for `k.target_c` (§3). | Retired early, a Studio before (b) or (c) loses the inputs and the feedback of every axis. |

The World reads feedback per device, as it resolves inputs
(`resolve_feedback`): a device is read in the spelling and the component it
lists, and a device that lists none in the spelling devices publish today,
`k.<axis>.position`. Publishing both spellings from (a) keeps a device's
per-axis feedback where a Studio before (b) and the saved live-data rows read
it, and the device that carries the cost — the `vizij` device, the device
outside Studio with per-axis targets — already keeps two forms of each
target.

## 7. Alternatives

| Alternative | Why not |
|---|---|
| Keep `k.target_c` | A name per component, and a second name (`studio_value`) for the value of any other type. |
| `k.c.target` | Attributes after the component are a path into the value (`Key::select`); `target` would read as a field of the position. |
| `target/k.c`, all targets under one root | An entity's keys no longer share its path, and a namespaced device has two candidate spellings (`target/robot/hip_yaw.position`, `robot/target/hip_yaw.position`). |
| Accept only the new form | Studio and devices ship separately; a device updated before Studio would lose every input. |
| The axis before the component, `k.<axis>.c` | No spelling in the `k/target.c` form, and the axis becomes the component in arora's grammar (§5). |
| A third, internal key for the resolved target, instead of keeping the two forms in agreement | Every bridge relays it, and every client sees one more key with no writer of its own (§3 rule 5). |

## 8. Open questions

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

### Related, not settled here

These bear on the keys but are outside this proposal.

- **The authoring experience does not change.** Users create tracks per axis
  from the sidebar controls, and splines keep their per-axis and per-group
  display. Which key a track reaches on a device, a target or a rig input
  (§6), stays below the animation library.
- **Track paths and sub-channels.** The editor organizes tracks by path
  (`namespaceId/instanceId/trackId`), each with sub-channels (the spline, its
  handles, its violations). Aligning those paths with this key syntax would be
  an internal change.
- **Which values' fields can be written independently** (§5): a declaration,
  on key meta or the value's type.
- **Tracing violations back to tracks and groups**
  ([ARORA-61](https://linear.app/semio-ai/issue/ARORA-61)). §5's relation lets
  a field's violation roll up to its quantity. Past that, an error passes back
  per field through a separable node (composing a vector from its axes) and
  only per group through a coupled one (IK), so each node declares which it
  is.
- **Group-level constraints.** Key meta carries one range per key; a norm, a
  reach limit or self-collision across joints has no place to be declared,
  in Studio or on a device.
- **Several writers of one value.** The later-write rules here order the
  writes a device's HAL-level keys receive, a single writer's delayed changes
  included. How a value written by several different sources is resolved
  belongs to the node-graph work.

Field independence, violation tracing and group-level constraints belong to
the key-schema and attribution note of Studio's target architecture ([STUDIO-81](https://linear.app/semio-ai/issue/STUDIO-81)).

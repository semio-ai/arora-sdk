# Proposal: a device's HAL components and their models

Status: agreed, 2026-10-09.

A device can be more than one piece of hardware: a robot with a face on its
screen, a base and an arm from two vendors. This document states how one HAL
is composed of several components, and how a device tells a client which 3D
models make it up and where they sit. It fixes the shape of the HAL module's
model functions before the first one ships
([arora-sdk#294](https://github.com/semio-ai/arora-sdk/pull/294)), so the
single-component case does not set an API that a composed HAL
([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)) would break.

In short:

- A HAL is made of **named components**, given when the HAL is built and fixed
  for its life. A device still holds one HAL.
- **A device's models are fixed for the life of the device.** Changing one
  means restarting Arora.
- **One HAL module** lists the components' models as an associative array, and
  serves the bytes of the ones it may serve, per component or all at once.
- **A published model travels as a reference** that a client resolves from
  Studio with its own rights; a private model never crosses the bridge. A Vizij
  face's GLB is servable.
- **A mount** places a component's model on a `screen` element of another
  component's model, which is how Studio already puts a face on a robot.
- **A HAL describes the keys it reads and the keys it writes** through a trait
  it shares with the data store.

---

## 1. What exists

- **One HAL per device.** `AroraBuilder::with_hal` takes one `Box<dyn Hal>`;
  bridges are a vector. Every store change is written to the HAL, which acts on
  the keys it handles ([architecture](architecture.md)).
- **One description.** `HalDescription` names one device: model family,
  hardware and software version. Registration with Studio sends it, and Studio
  matches a device to a model on its family alone
  ([models and devices](studio-v2-models-and-devices.md), decision 6).
- **One model.** `HalAssets::model_glb` returns one GLB.
- **Studio composes models.** A robot model declares `screen` elements (RobotData
  type `screen`, semio_studio `packages/scene/src/types/screen.ts`). A project's
  `slotConfig` pairs a screen with the root of a face, and the scene draws that
  face on the screen (`packages/scene/src/renderables/screen.tsx`).
- **Studio's device document references one asset**:
  `publishedAssetReference { assetType: model | face, scopedName, versionRange }`
  (semio_studio `packages/core/src/access/documents.ts`).
- **A device resolves its model from Studio** at start, into a
  `ResolvedRef { version, releaseId, artifactKey, contentHash }`, and never
  serves a private model over the bridge: the router has no transport-level
  authentication ([models and devices](studio-v2-models-and-devices.md), 2.2).
- **The data store describes its keys**: `DataStore::meta`, `all_meta`,
  `set_meta` and `set_prefix_meta` hold each key's `KeyMeta` — its type, range,
  unit, rest value, and whether a remote may write it.

## 2. The cases the design holds

1. **A stand-alone face**: one component, one model (`vizij --studio`).
2. **A robot**: one component, its model a published release (`arora-hal-ros2`).
3. **A robot with a face on its screen**: two components; the face's model is
   drawn on one of the robot model's screens.
4. **A machine driven by two drivers**, such as a base and an arm from different
   vendors: two components, two models, the arm's model attached to the base's.
5. **A component without a model**: a sensor array, a speaker.

## 3. Design

### 3.1 A HAL is made of named components

- A composed HAL is built from its components, each under a name unique within
  it: `robot`, `face`, `arm`. The components are given at build and do not
  change afterwards; the device holds the composed HAL as its one HAL.
- A HAL that is not composed is one component. Its name is `device`.
- **Each component owns the keys it describes** (3.5). Two components describing
  one key is a build error, not a merge rule. A write goes to the component that
  owns its key; the components' update streams merge into the HAL's one stream.
- **Keys carry no component prefix.** RobotData keys are element UUIDs, so two
  models never share one; a face's keys already carry its face id
  (`rig/<faceId>/…`). A prefix would change every key Studio and ROS clients
  address today.

### 3.2 The device's description

- The device's description — family, hardware and software version, what
  Studio matches on — is provided explicitly, as it is today.
- Each component can carry its own description.
- The composed HAL either states the device's description or inherits it from
  one of its components, by name.

### 3.3 One HAL module: the models, then their bytes

The runtime registers one HAL module (`hal_module::ID`). A device's models do
not change while it runs: a different model means restarting Arora. So the
module answers from what the HAL was built with, and clients ask once.

- **`models()`** lists every component that has a model, as an associative
  array from the component's name to its model. Arora values have no typed map,
  so it travels as a list of records keyed by `component`:

  | Field | Type | Meaning |
  |---|---|---|
  | `component` | `string` | The component's name. |
  | `description` | `Option<{ family, hardware_version, software_version }>` | The component's own description, when it has one. |
  | `reference` | `Option<{ scoped_name, version, content_hash }>` | The published release the component uses, as resolved. Absent for a model with no release, such as a face loaded from a file. |
  | `content_hash` | `Option<string>` | SHA-256 of the GLB, when the device holds the bytes. A client caches by it. |
  | `servable` | `bool` | Whether the device returns the bytes. |
  | `mount` | `Option<{ component, element }>` | The component, and the `screen` element of its model, that this model is drawn on. Absent for a model that is not mounted. |

  A component without a model is absent from the list.
- **`model_glb(component: string) -> Option<bytes>`**: one component's model.
  `None` when the component has no model or its model is not servable.
- **`model_glbs()`**: every servable model, as a list of `{ component, glb }`.
  It saves a client one call per component when it draws the whole device.

On Studio's client these are `retrieveModels`, `retrieveModelGlb` and
`retrieveModelGlbs`.

### 3.4 References travel; private bytes do not

- A component whose model is a published release states its `reference`. A
  client fetches the release from Studio with its own identity, and Studio's
  rules decide what it may read. The device does not need to send those bytes.
- The device serves a model's bytes only when the component marks it servable:
  - a public release;
  - a Vizij face's GLB, servable by default, because a client needs it to
    reproduce the device in 3D;
  - other local content the operator lets the device serve.
- A model the device holds under a private grant
  ([models and devices](studio-v2-models-and-devices.md), decision 10) is never
  servable.

The rule "a device never serves a private model" then holds by construction,
not by a comment on `HalAssets`.

### 3.5 A HAL describes its keys

A HAL states the keys it reads (actuator targets, the inputs a write reaches)
and the keys it writes (sensors, measured state), each with its `KeyMeta`. The
data store already describes its keys the same way, so the two share one trait:
the key-description half of `DataStore` (`meta`, `all_meta`) moves into a trait
that both `DataStore` and `Hal` implement.

- The runtime routes a write to the component that describes its key, and
  refuses at build a key two components describe.
- The runtime copies a HAL's key descriptions into the store at build, so a
  device's inputs are open to remotes because its HAL says so, with no second
  declaration.

### 3.6 Mounts and Studio's slots

- A mount names a parent component and a `screen` element of the parent's
  model. Studio maps it onto `slotConfig`: the parent's screen holds the
  component's model.
- The composed HAL states the mounts, because it knows which screen the face is
  on. A project can still choose otherwise in its own `slotConfig`.
- With the face drawn from the image its view publishes
  ([VIZ-159](https://linear.app/semio-ai/issue/VIZ-159)), the mount is what
  tells the scene which screen shows it.

### 3.7 What Studio needs

Studio's device document references one published asset. A device of several
components needs a list of references, one per component, with their mounts.
That is a Studio change, in Stage 2 of
[models and devices](studio-v2-models-and-devices.md).

## 4. Alternatives

| Alternative | Why not |
|---|---|
| One model per device | Fails case 3 as soon as a robot gains a face. |
| The device merges its components' models into one GLB | The device takes on asset composition; a private robot mesh would be merged into served bytes; Studio could no longer tell the face from the robot to bind each. |
| One HAL module per component | A call naming no module becomes ambiguous; `DescribeMethods` lists one module per function id, so a client cannot discover them. |
| The models as store keys that notify a change | Models do not change while a device runs, so there is nothing to notify. |
| Several HALs held by the device rather than one composed HAL | The device would route and merge them itself; composing them in a HAL keeps the device's loop unchanged. |
| A component prefix on keys | See 3.1: it moves every key clients address. |

## 5. What this changes in arora-sdk#294

1. The HAL module serves `models()`, `model_glb(component)` and `model_glbs()`,
   under function ids `arora-hal` defines.
2. Studio's client gains `retrieveModels`, `retrieveModelGlb` and
   `retrieveModelGlbs`, which call the HAL module by its id
   ([studio-bridge#104](https://github.com/semio-ai/studio-bridge/pull/104)).
   `retrieveGlb` and `GET_MODEL_GLB_FUNCTION_ID` stay as they are: no Arora
   device ever answered them, and removing them would be a studio-bridge-msgs
   major.
3. Resolving a call that names no module is not needed for models; it stays in
   #294 only if it is wanted on its own merits.
4. `HalAssets` states each component's model — reference, content hash,
   servable, mount — and returns bytes only for a servable one. A HAL that is
   not composed reports one component, `device`.

The composed HAL itself, and the shared key-description trait (3.5), are
[ARORA-103](https://linear.app/semio-ai/issue/ARORA-103).

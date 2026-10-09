# Proposal: a device's HAL components and their models

Status: proposal, for review.
Date: 2026-10-09.

A device can be more than one piece of hardware: a robot with a face on its
screen, a base and an arm from two vendors. This proposal states how a device
holds several HALs, and how it tells a client which 3D models make it up and
where they sit. It sets the shape of the HAL module's model functions before
the first one ships ([arora-sdk#294](https://github.com/semio-ai/arora-sdk/pull/294)),
so that the single-HAL case does not fix an API that the multi-HAL case
([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)) would have to break.

In short:

- A device is a set of **named HAL components**. Each owns its keys and
  describes itself.
- There is **one HAL module**. Its functions take the component's name.
- **Each component's model is state**, published in the store under
  `arora/hal/<component>/model`: the published release it uses, its content
  hash, whether the device serves its bytes, and where it is mounted. A client
  subscribes to it like any key.
- **Bytes travel only when the component allows it.** A published model is
  named by reference and fetched from Studio with the client's own rights; a
  private model never crosses the bridge.
- **A mount** places a component's model on a `screen` element of another
  component's model, which is how Studio already puts a face on a robot.

---

## 1. What exists

- **One HAL per device.** `AroraBuilder::with_hal` takes one `Box<dyn Hal>`;
  bridges are a vector. Every store change is written to the HAL, which acts on
  the keys it handles ([architecture](architecture.md)).
- **One description.** `HalDescription` names one device: model family,
  hardware and software version. Registration with Studio sends it, and Studio
  matches a device to a model on its family alone
  ([models and devices](studio-v2-models-and-devices.md), decision 6).
- **One model.** `HalAssets::model_glb` returns one GLB. arora-sdk#294 serves it
  as `model_glb() -> Option<bytes>`, the HAL module's one function, under the
  function id Studio's client calls (`retrieveGlb`).
- **Studio composes models.** A robot model declares `screen` elements (RobotData
  type `screen`, semio_studio `packages/scene/src/types/screen.ts`). A project's
  `slotConfig` pairs a screen with the root of a face, and the scene draws that
  face on the screen (`packages/scene/src/renderables/screen.tsx`).
- **Studio's device document references one asset**:
  `publishedAssetReference { assetType: model | face, scopedName, versionRange }`
  (semio_studio `packages/core/src/access/documents.ts`).
- **A device resolves its model from Studio** at start, into a
  `ResolvedRef { version, releaseId, artifactKey, contentHash }`, and **never
  serves a private model over the bridge**: the router has no transport-level
  authentication ([models and devices](studio-v2-models-and-devices.md), 2.2).

## 2. The cases a design must hold

1. **A stand-alone face**: one component, one model (`vizij --studio`).
2. **A robot**: one component, its model a published release (`arora-hal-ros2`).
3. **A robot with a face on its screen**: two components; the face's model is
   drawn on one of the robot model's screens.
4. **A machine driven by two drivers**, such as a base and an arm from different
   vendors: two components, two models, the arm's model attached to the base's.
5. **A component without a model**: a sensor array, a speaker.

In each case a model can change while the device runs: Vizij reloads a face, an
explicit assignment ([models and devices](studio-v2-models-and-devices.md), 2.5)
swaps a release.

## 3. Proposal

### 3.1 A device is a set of named HAL components

- The host adds each HAL under a name it chooses, unique on the device:
  `robot`, `face`, `arm`. A device with one HAL names it too.
- Each component describes itself (`HalDescription`). The device's own identity,
  the family Studio matches on, is the host's to state; by default it is the
  description of the component no other component is mounted on.
- **Each component owns the keys it declares.** Two components declaring one key
  is a build error, not a merge rule ([ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)).
  A write goes to the component that owns its key; the components' update streams
  merge into the step's one inbound, as the bridges' do.
- **Keys carry no component prefix.** RobotData keys are element UUIDs, so two
  models never share one; a face's keys already carry its face id
  (`rig/<faceId>/…`). A prefix would change every key Studio and ROS clients
  address today.

### 3.2 One HAL module, addressed by component

The runtime keeps one HAL module (`hal_module::ID`). Its functions take the
component's name; nothing in its shape assumes a device has one HAL.

**A component's model is state.** The runtime publishes it under
`arora/hal/<component>/model`, a record:

| Field | Type | Meaning |
|---|---|---|
| `reference` | `Option<{ scoped_name, version, content_hash }>` | The published release the component uses, as resolved. Absent for a model with no release, such as a face loaded from a file. |
| `content_hash` | `Option<string>` | SHA-256 of the GLB, when the device holds the bytes. A client caches by it. |
| `servable` | `bool` | Whether `model_glb` returns the bytes. |
| `mount` | `Option<{ component, element }>` | The component, and the `screen` element of its model, that this model is drawn on. Absent for a model that is not mounted. |

- A component without a model has no such key.
- Remotes read it and cannot write it.
- A model that changes is a store change. A client that subscribed learns of it
  the way it learns of any value, with no polling and no extra message.

**The bytes go through a function**:
`model_glb(component: string) -> Option<bytes>`. It returns `None` when the
component has no model or its model is not servable. Models are megabytes,
fetched once per content hash, so they are a call, not a key.

### 3.3 References travel; private bytes do not

- A component whose model is a published release states its `reference`. A
  client fetches the release from Studio with its own identity, and Studio's
  rules decide what it may read. The device does not need to send those bytes.
- The device serves a model's bytes only when the component marks it
  `servable`:
  - a public release;
  - local content that the operator lets the device serve, such as a face its
    user authored and loaded from a file.
- A model the device holds under a private grant
  ([models and devices](studio-v2-models-and-devices.md), decision 10) is never
  servable.

The rule "a device never serves a private model" then holds by construction,
not by a comment on `HalAssets`.

### 3.4 Mounts and Studio's slots

- A mount names a parent component and a `screen` element of the parent's
  model. Studio maps it onto `slotConfig`: the parent's screen holds the
  component's model.
- The host that composes the device states the mounts, because it knows which
  screen the face is on. A project can still choose otherwise in its own
  `slotConfig`.
- With the face drawn from the image its view publishes
  ([VIZ-159](https://linear.app/semio-ai/issue/VIZ-159)), the mount is what
  tells the scene which screen shows it.

## 4. Alternatives

| Alternative | Why not |
|---|---|
| One model per device, the first HAL's | Fails case 3 as soon as a robot gains a face. |
| The device merges its components' models into one GLB | The device takes on asset composition; a private robot mesh would be merged into served bytes; Studio could no longer tell the face from the robot to bind each. |
| One HAL module per component | A call naming no module becomes ambiguous; `DescribeMethods` lists one module per function id, so a client cannot discover them. |
| A `models()` function returning the list | It works, but a client has to poll it to learn that a model changed. A key notifies. |
| A component prefix on keys | See 3.1: it moves every key clients address. |

## 5. What this changes in arora-sdk#294

1. **`model_glb` takes a component.** Its function id becomes one that
   `arora-hal` defines. Studio's client gains `retrieveModel(deviceId, component)`,
   which calls it on the HAL module by id, plus a read of
   `arora/hal/*/model` ([studio-bridge#104](https://github.com/semio-ai/studio-bridge/pull/104)).
   `retrieveGlb` and `GET_MODEL_GLB_FUNCTION_ID` stay as they are: no Arora
   device ever answered them, and removing them would be a studio-bridge-msgs
   major.
2. **Resolving a call that names no module** is then not needed for models. It
   stays in #294 only if it is wanted on its own merits.
3. **The one HAL gets a name.** `with_hal(hal)` keeps working and names its
   component `device`; [ARORA-103](https://linear.app/semio-ai/issue/ARORA-103)
   adds the named, repeatable form.
4. **`HalAssets` states the model, not only its bytes**: its reference, its
   content hash and whether it is servable. `model_glb` returns bytes only for a
   servable model. Mounts are the host's, set where it adds the component.
5. **The runtime publishes `arora/hal/<component>/model`** at build, and again
   when the component's model changes.

## 6. Open questions

1. **How a HAL signals that its model changed**: a notice on its update stream,
   or a watch the runtime polls each step.
2. **The device's family when two components have no mount**, as in case 4
   without an attachment: the host states it, or registration fails.
3. **Whether a face loaded from a file is servable by default** in Vizij, or only
   when the operator says so.
4. **Studio's device document** references one published asset. A device of
   several components needs a list; that is a Studio change (Stage 2).
5. **How a component declares the keys it owns** so writes can be routed and
   overlaps refused at build: from `read_all` at build, or from the key metadata
   it writes.

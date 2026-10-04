# Robot models and device communication between arora-sdk and Studio v2, in three stages

**Tracking:** Linear ARORA-127.

**Sources:**
- Studio production `main` at `64f8291b0`.
- arora-sdk `origin/main` at `1e3ec7a`.
- studio-bridge `origin/main` at `af2cd13`.

## Summary

The work has three stages:

1. **Stage 1, quick fix**. arora-sdk builds, passes CI and runs against the live Studio v2 again. Arora development can continue against production. Studio does not change. This stage must be done first and fast.
2. **Stage 2, device-server rework**. Devices use forward-looking mechanisms: enrollment, a device credential, model resolution from Studio at run time, and private models. This stage needs Studio changes.
3. **Stage 3, workflow refinements**. Studio screens and flows improve. These changes do not touch the rules.

This document gives the full design of Stage 1. It gives an outline of Stages 2 and 3, with their decisions and open questions. Thus the team can agree on the later stages before work on them starts.

## Terms

| Term | Meaning in this document |
|---|---|
| **Device** | A physical device that runs arora. |
| **Device document** | The Firestore document `devices/{id}`. It records what the device is: its owner, grants, name, family and model reference. The "Data Model" page of the Arora Architecture board calls it the Device Identity. |
| **Device account** | The anonymous Firebase Authentication user of the device. The device authenticates as this user when it reads or writes Firestore and Storage. Its uid is the id of the device document. The rules let the device write `devices/{id}` only when its uid is equal to `id`. |
| **Device directory** | The local directory that keeps the data of the device between runs. It holds the refresh token of the device account and the modules. |
| **Engine Config** | The robot config: the HAL and the settings that a runtime uses. Example: the `arora-hal-ros2` JSON config. |
| **Engine instance** | One run of arora on a device. |
| **Session** | A live network of engine instances. A session is specific to the Active Scene of one project. A device is in a maximum of one session at a time. |
| **Binding** | A Resolved Entity Binding: the association between an engine instance and a project entity. Studio calls this "pairing" today. A binding exists only while its session exists. |
| **Scoped name** | The address of a published asset, for example `@quori/quori`. |
| **Range** | A semver range, for example `^1.0.0`. |
| **Resolve** | To select the release that a range refers to. |
| **`ResolvedRef`** | The result of resolve: `{version, releaseId, artifactKey, contentHash}`. Studio stores the same shape in `project.resolved`. |
| **Private model** | A published model that has no public audience. Its `publicRole` is null. |
| **Rung** | An access level in Studio: viewer, editor, admin or owner. |

## The current state

### What is broken

- **arora-sdk CI fails on each new run**. `crates/arora-hal-ros2/build.rs` downloads six models from legacy Storage paths at build time. The Studio v2 Storage rules have no rule for `model/…`, so Storage refuses each download with 403. The build script then panics. The last green run on `main` was on 2 Oct 2026, before the v2 cutover. The CI run of semio-ai/arora-sdk#268 on 3 Oct 2026 fails at this step.
- **Local builds fail** unless the developer sets `ARORA_HAL_ROS2_SKIP_MODELS=1`.
- **An overrides file cannot set a local model**. `apply_overrides` does not copy `model_glb_path` or `joint_ids`. Thus `arora-ros2 quori overrides.json` cannot use a local GLB.
- **The legacy model files go away**. Studio plans to delete them near 9 Oct 2026. Even before that, clients cannot read them.

### What works with Studio v2 unchanged

- **Registration**. The published bridge client writes the legacy device shape. The rules accept it, and `normalizeLegacyDevice` changes it to the v2 shape. Studio lists devices by their grants. Thus a converted device appears in the device list of its owner.
- **Re-registration**. The bridge client sends its registration once per process start, and the write creates a missing document. Thus a device whose document Studio dropped registers again at its next start.
- **Public model resolution**. A device can resolve a public model with four reads, and no read needs a token:
  1. Read `scopedNames/{@scope:slug}`. The result gives the `publishedAssetId`.
  2. Read `publishedAssets/{id}`. The result gives `assetType`, `deprecated` and `publicRole`.
  3. List `publishedAssets/{id}/releases`. Each release gives `version`, `deprecated`, `family` and `artifacts.primary`. The `primary` artifact gives the Storage path and the `contentHash`.
  4. Download the bytes from that Storage path. The `contentHash` is the SHA-256 hash of the bytes.

  Studio tests prove this chain. The "external consumer read surface" block in the semio_studio file `apps/studio/rules-tests/rules.test.ts` covers the reads. The release artifact tests in `storage-rules.test.ts` cover the bytes. Each public case has a private pair that the rules refuse. The full rules suite passed on the emulator against production `main`: 785 of 785 tests.
- **Configuration**. The bridge client already contains the Firebase project, the Storage bucket and the emulator hosts, in `FirebaseOptions` and `FirebaseEmulatorOptions`.

### What the current device model cannot do

The device account came from the legacy bridge, as a shim. No v2 design chose it. It has these limits:

1. **The rules cannot tell a device from a person**. The device account is an ordinary anonymous user. Thus it can do all that an anonymous visitor can do, for example create projects.
2. **Device accounts look the same as guest browser sessions**. No data marks which anonymous accounts belong to devices.
3. **The identity is a refresh token on disk**. If someone deletes the device directory, the device becomes a new device. If someone copies the directory, two devices share one identity.
4. **The owner is self-declared**. The device writes its own document and names any uid as owner. That person does not accept it. A device cannot register under an organization.
5. **No product path revokes a device**. A deleted device document leaves the device account valid.
6. **A device has only its own access**. On a published asset, the device account gets access only from the public audience or from a grant to its own uid. No rule gives a device the access of its owner.

## Decisions

Each decision has its reason.

1. **The work has three stages, and Stage 1 comes first**. Stage 1 clears the blocker for arora development and for arora-sdk CI. It also gives time to agree on Stages 2 and 3 before their work starts.
2. **Models resolve at run time, never at build time**. Models change after the build. A build also has no owner, so it can get only public assets. Licensed meshes must not be in build output.
3. **A person or an organization owns a device, as with a project. A device is never an organization member**. Membership would give the device all members-only assets and organization projects. It would also give the device the right to create resources in the organization.
4. **A device can get exactly the assets that a project with the same owner can use** (STUDIO-312).
   - A device that an organization owns gets public assets, assets that the organization owns, and assets granted to the organization.
   - A device that a person owns gets public assets and assets granted to that person. The access that a person gets from an organization membership does not apply to their personal device.
   - A device only receives content. Thus a device has no rung. The entitlement of its owner sets its scope.
5. **Stage 1 keeps the legacy registration of the bridge client. Stage 2 replaces the device account and self-registration with enrollment**. The device account is a shim with the limits above. Effort goes to its replacement, not to the shim.
6. **The compatibility check compares only `family`**. Studio uses the same rule to offer a device for a binding.
7. **The Engine Config names the model of the device**. The model stays with the joint map of the HAL, which depends on it.
8. **A device encrypts private models in its device directory**.
9. **The resolver is a new crate. It is the first part of the unified registry**. It does not use the current `ReadableRegistry`. The registry merge replaces `RecordType` and identity by UUID or folder path. The merged registry uses the Studio v2 shape and terms.
10. **An operator can grant a private model to one device, but only when one account owns both**. This applies until Stage 2 lets a device use the entitlement of its owner.

## Stage 1: quick fix

**Goal:** arora-sdk CI passes. A developer builds arora and runs it against the live Studio v2. Studio does not change.

### Changes in arora-sdk

1. **Delete the download code from `build.rs`**. The build needs no network. Delete the build dependencies that only the download uses.
2. **Keep a default model path, but do not fill it at build time**. The built-in configs keep `crates/arora-hal-ros2/models/<name>.glb` as their default path. The developer puts the GLB there.
3. **`apply_overrides` copies `model_glb_path` and `joint_ids`**. Then an overrides file can set a local model for a built-in robot.
4. **Give a clear error when the model file is missing**. The message names the two ways to supply a model: the default path, or `model_glb_path` in a config or overrides file.
5. **Tests use a synthetic fixture GLB**. The fixture is in the repository. It has `RobotData` joints and no mesh. The tests that read a downloaded model, for example the NAO joint-state test, use it.
6. **`ARORA_HAL_ROS2_SKIP_MODELS` has no effect after this change**. Delete it from the documentation.

### Actions outside the code

- **A Studio operator gives developers the GLB files from the cutover backup**. NAO and Pepper use licensed meshes, so Semio decides who receives them. Semio does not publish Ur3, Ur5 or G1 again. These stay as local copies.
- **The anonymous-account cleanup keeps device accounts**. Before the cleanup runs, add to its keep list each anonymous account whose uid has a device document. After a developer's first run, the device account of that developer is one of them.
- **Optional, not a blocker:** a Studio operator publishes Quori with a public audience:
  1. Create the `@quori` organization.
  2. Upload the Quori GLB into a project that `@quori` owns. Set `family` to `quori`, which is the exact `model_family` of the Engine Config.
  3. Publish `@quori/quori` at version 1.0.0 with a public audience.

### How a developer runs against the live Studio

1. Put the GLB at `crates/arora-hal-ros2/models/<name>.glb`, or set `model_glb_path` in an overrides file or a full config.
2. Start `arora-ros2` with Studio on. Give the uid of the Studio account of the developer as owner, and the family of the robot, for example `quori`.
3. The bridge client registers the device in the legacy shape. `normalizeLegacyDevice` changes it to the v2 shape. The device appears in the device list of the developer.
4. In Studio, open a project that the developer owns. The project contains the same GLB as a model, with the same family. The developer can upload it as a project asset. After the Quori publication, the developer can add `@quori/quori` as a dependency.
5. Make the binding in Studio. Values flow. The joint ids match, because Studio and the device use the same GLB.

If the device does not appear in step 3, the trigger did not convert it. The semio_studio operator script `apps/studio/scripts/normalize-legacy-devices.mjs` converts such devices.

### Verification

- arora-sdk CI passes on the Stage 1 change.
- The implementer runs steps 1 to 3 against the Firebase emulators with a local Studio. This work runs nothing against production or staging.
- The first run of a developer against production is the live check.

### Not in Stage 1

Model resolution from Studio, the device-directory cache, encryption, `publishedAssetReference` in registration, a change to registration, and all Studio changes.

## Stage 2: device-server rework (outline)

**Goal:** devices communicate with Studio through forward-looking mechanisms. Each part below gives what it must do. The Studio half of each part is an open question, not a design.

### 2.1 Enrollment and a device credential

- A person approves the device and selects its owner, a person or an organization. Studio checks the standing of that person for that owner. A usual pattern is "enter this code on the website", the OAuth device authorization grant.
- The server creates the device document. The device does not write its own document.
- The device gets a credential that marks it as a device. The rules can then limit a device to device paths and to content reads. Studio can revoke the credential.
- This part replaces the device account and self-registration.
- **arora-sdk side:** show the enrollment code in the terminal UI and in the headless log. Keep the credential in the device directory. Give the credential to the bridge and to the resolver through one token-source interface.
- **Coordinate with the bridge client**. semio-ai/arora-sdk#268 moves arora to `arora-studio-bridge-client` 9. The credential change also goes into the bridge client.

### 2.2 Model resolution at run time

This part does not wait for 2.1. Public models work with Studio unchanged. The resolver first uses no token, then the token-source interface.

- **The resolver crate** changes a reference `{scopedName, range}` into verified bytes:
  - It resolves with the rules of the Studio function `resolveDependency`. A version is `MAJOR.MINOR.PATCH` and 1.0.0 or higher. Deprecated releases and releases without a `primary` content hash are not candidates. The newest candidate in the range is the result.
  - It downloads the artifact and verifies the SHA-256 hash against `contentHash`.
  - It has two backends: Studio v2, and a local file.
  - Its errors name their cause. Reads go to the server, not to a local Firestore cache. Thus a failed read never looks like a missing asset.
  - It does not depend on the asset type. A model is its first use.
- **The device directory** keeps the result:

  ```text
  <device dir>/assets/
  ├── blobs/<contentHash>   public bytes as downloaded, private bytes encrypted
  ├── key                   key for private blobs, readable by the OS user of the device only
  └── resolved.json         scopedName → ResolvedRef, the same shape as project.resolved
  ```

  - The device resolves again at each start when it can connect. If it cannot connect, it uses the `ResolvedRef` in `resolved.json`.
  - `resolved.json` is a map, not one model slot.
  - The device encrypts private bytes with the scheme of its refresh token (XSalsa20-Poly1305, a key file, permissions for the OS user only). This protects a copy of the cache. It does not protect against a person who controls the OS account.
  - The device decrypts private bytes into memory and gives them to the HAL as bytes.
- **The Engine Config** gets `model: {scopedName, versionRange}`. `model_glb_path` stays as a local override.
- **Family check:** the device refuses a release when its `family` is different from `model_family`. It uses a release without `family` and logs a warning. Registration takes its family from the Engine Config.
- **No private model through the bridge:** a device never serves a private model through `HalAssets::model_glb`. The bridge router has no transport-level authentication. No Studio code calls `retrieveGlb` in the bridge client.
- **The joint map of the HAL** goes behind one interface. A future binding can replace it.
- **Registration** writes `publishedAssetReference`. Both device shapes accept this field today.

### 2.3 A device uses the entitlement of its owner

Decision 4 needs this part. It depends on 2.1, because the rules must know that a caller is a device and who owns it. A rules change must stay within the Storage limit of two document reads. The STUDIO-128 signed-URL callable can check the full entitlement. Until this part exists, private models need the operator workaround of decision 10.

### 2.4 Devices that an organization owns

Enrollment under an organization needs the device list to find such devices. The list rule for devices matches only a direct grant to the caller (`uid() in grants`). A device that an organization owns has no owner grant to a person. Thus the owners and admins of the organization do not see it. This needs a rules change.

### 2.5 The first binding message

Studio and the bridge send the joint table of the bound entity when a binding starts. The device then does these steps:

1. It verifies that the table contains each joint name of the device.
2. If a name is missing, it refuses the binding and gives the reason.
3. It puts the table in the HAL until the binding ends.

Then the ids from Studio always match the device, for models in a project, published models and private models of other owners. Also, the device and the project cannot use different releases.

### Dependencies of Stage 2

- STUDIO-312: a grant to a different organization needs a durable organization principal. A customer organization needs it to use a licensed model that Semio owns.
- STUDIO-128: the signed-URL callable, if 2.3 uses it.

## Stage 3: workflow refinements (outline)

These changes do not touch the rules.

- **Transfer a device between a person and an organization**. The rules already allow an owner to do this. They check for a verified account and the creation minimum of the organization. No Studio screen offers it yet.
- **Device settings**. Set and change `publishedAssetReference` in Studio. Create a project from a device (STUDIO-303, STUDIO-299).
- **Binding feedback**. Studio offers compatible entities. It explains why a binding fails: a different family, or a missing joint. It tells "not claimed" from "a different owner".
- **The catalogue**. Publish NAO and Pepper as private models. Their owner is an open decision.

## Options kept open

- A device can later have dependencies. For example, its hardware can be a model asset, and its software can be a HAL module from the registry. Three parts of Stage 2 leave space for this:
  - The resolution map.
  - The resolver that accepts all asset types.
  - The hardware and software strings, which no code compares.
- With dependencies, three statements would apply:
  - The dependencies of a device say what the device is.
  - A binding says what the device must run.
  - Compatibility means that the model of the binding satisfies the hardware dependency of the device. Today this means equal `family` values.
- Matching by explicit asset dependency (STUDIO-325) can replace the family check. The check is in one place.

## Sessions and bindings

This section gives the intended model from the "Data Model" board. No stage builds all of it.

- The Device Identity, which is the device document, relates only to the physical device. It has no relation to a project, a session or a model asset.
- A project entity is "Model + Behavior → Engine". Studio assembles the entity into a behavior package. Studio sends the package with its binding to the engine instance.
- Thus the session pushes project content to the engine instance. A Studio client that has the project open does the push. A device never needs read access to a project. The person who makes the binding decides.
- A device can move between sessions during one run. No durable record connects a device to a project. On the board, the only persisted network configuration is the Session Preset.
- The model of the device describes the hardware. It is also the model that the engine instance uses when it has no binding. When a binding exists, the binding controls the model.
- The joint table of Stage 2.5 is the first content of a binding. A later stage adds the behavior package, for example for animation playback on the device (ARORA-70).

## Operational risks

- **A cleanup of anonymous accounts can delete device accounts**. No data marks them. If a cleanup deletes a device account, the device registers again as a new device. Stage 1 gives the mitigation: keep each account whose uid has a device document.
- **App Check enforcement on Firestore or Storage stops devices**. Today it would stop registration. After Stage 2.2, it would also stop model resolution. Stage 2.1 must give the device credential a path through App Check.

## Not in scope

- Matching by explicit asset dependency (STUDIO-325).
- The behavior package and animation playback on the device (ARORA-70).
- Device deletion.
- The contents of a type hash. This is the open decision on the registry board.

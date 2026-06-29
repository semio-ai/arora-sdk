# Removing arora's dependency on `semio-record` via `arora-types`

Deep study with a working, tested prototype. Goal: drop the private `semio-record`
crate from `arora-engine` by generalizing the "record" notion (versioning +
freezing) behind a trait set in `arora-types`, without copying semio-record.

Repos analyzed (read-only):
- semio-record: `/Users/victor.paleologue/Code/Semio/semio-record/crates/semio-record`
- arora-types:  `/Users/victor.paleologue/Code/Semio/arora-types`
- arora-engine: `/Users/victor.paleologue/Code/Semio/arora-engine`

Prototype: `<scratch>/record-study/arora-record-proto` (self-contained; std + serde + uuid + semver).

---

## 1. Usage inventory

`semio-record` is declared as a git dependency (`branch = "main"`) in **9**
arora-engine manifests:
`arora-registry`, `arora-module-cpp`, `arora-cli`, `arora-module-core`,
`arora-behavior-tree-types`, `arora-behavior-tree` (deps + dev-deps),
`arora-module-rust`, `arora-module-cli`, and `modules/polly`.

Exact items used, grouped by crate and purpose (file:line evidence):

### arora-registry — the hub
- `crates/arora-registry/src/lib.rs:13-18` — `record::RecordDefn`, `ty::FrozenTy`,
  `ty::PrimitiveKind`, and the six `*Defn` record types:
  `module::v0::Module`, `enumeration::v0::Enumeration`, `structure::v0::Structure`,
  `folder::v0::Folder`, `organization::v0::Organization`, `user::v0::User`.
- `lib.rs:141-154` — uses `<XDefn as RecordDefn>::{Unfrozen, Public, Frozen}`
  associated-type projections to define `Enumeration`, `Structure`, `Module`,
  `User`, `Organization`, `Folder` and their `*Public`/`*Frozen` aliases.
  **This is the single deepest coupling: it uses the full `RecordDefn` trait
  machinery, not just data types.**
- `lib.rs:158,176-182,336-350` — `PrimitiveKind`, `FrozenTy::{Primitive,FrozenScalar,FrozenArray}`.
- `src/remote.rs:10`, `src/remote_cached.rs:10`, `src/local/mod.rs:8` —
  `record::{Freezer, FrozenReference, UnfrozenReference}`; each registry
  **implements `Freezer`** (`remote.rs:266`, `remote_cached.rs:191`,
  `local/mod.rs:108`). `local/mod.rs:110-135`: `freeze()` picks the newest
  version matching the requirement → builds `FrozenReference` with
  `record::Version` (`local/mod.rs:133`).
- `src/local/editable.rs:8` — `enumeration::v0::unfrozen::Enumeration`,
  `record::Freeze`; `editable.rs:186` — `module::v0::frozen::ExportKind::Function`.
- `src/local/mod.rs:152-174` (tests) — `enumeration::v0::frozen::EnumerationVariant`,
  `module::v0::frozen::{Export, ExportKind, Function}`, `record::FrozenReference`,
  `ty::{FrozenScalar, FrozenTy, Primitive, PrimitiveKind}`.

### arora-module-core
- `src/resolve.rs:17-22` — `record::{Freeze, Freezer, UnfrozenReference, VersionReq}`,
  `acl::Acl`, `module::v0::unfrozen::{Export, Function, Parameter}`,
  `ty::{UnfrozenArray, UnfrozenScalar, UnfrozenTy}`. Body (`resolve.rs:172,195,221,244`)
  constructs `module::v0::unfrozen::ExportKind::Function`.
- `src/header.rs:18-22` — `module::v0::frozen::ExportKind`, `record::Freezer`,
  `ty::{FrozenTy, PrimitiveKind}`.
- `src/lib.rs:10` — `module::v0::frozen::Export`, `record::Freezer`;
  `lib.rs:22,38` — generic bound `R: ReadableRegistry + Freezer`.

### arora-module-rust
- `src/lib.rs:21-26` — `ty::PrimitiveKind`, `module::v0::frozen::{ExportKind, Parameter}`,
  `record::FrozenReference`, `ty::{FrozenScalar, FrozenTy, Primitive}`.

### arora-module-cpp
- `src/ty.rs:2` — `ty::{FrozenTy, PrimitiveKind}`.
- `src/declare.rs:9-12` — `structure::v0::frozen::StructureField`, `ty::{FrozenTy, PrimitiveKind}`.
- `src/main.rs:18` — `module::v0::frozen::{ExportKind, Parameter}`.

### arora-behavior-tree-types
- `src/lib.rs:3` — `folder::v0::public::Public` (declares the BT folder).
- `src/status.rs:1-5` — `acl::Acl`, `enumeration::v0::unfrozen::{Enumeration, EnumerationVariant}`,
  `ty::{Primitive, PrimitiveKind, UnfrozenTy}` (declares the `Status` enum).
- `src/tick_id.rs:1-5` — `acl::Acl`, `structure::v0::unfrozen::{Structure, StructureField}`,
  `ty::{Primitive, PrimitiveKind, UnfrozenTy}` (declares the `TickId` struct).

### arora-behavior-tree
- `src/behavior_tree.rs:20` — `module::v0::frozen::Function`.
- `src/schema_groot.rs:5-6` — `module::v0::frozen::Parameter`, `ty::FrozenTy`.
- `src/tests.rs:19` — `module::v0::frozen::ExportKind`, `record::Freezer`.

### arora-cli / arora-module-cli / modules/polly
- `arora-cli/src/main.rs:19-20` — `module::v0::frozen::ExportKind`, `record::Freezer`.
- `arora-module-cli/src/main.rs:13`, `src/generate.rs:7` — `record::Freezer`.
- `modules/polly/tests/polly_tests.rs:16` — `module::v0::frozen::ExportKind`, `record::Freezer`.

### Verdict on the hypothesis ("arora only uses the module parts")
**Refuted.** arora uses four record kinds (`module`, `structure`, `enumeration`,
`folder`) plus the `record` infrastructure (`RecordDefn`, `Freeze`, `Freezer`,
`Version`, `VersionReq`, `Frozen/UnfrozenReference`), `ty` (`FrozenTy`,
`UnfrozenTy`, `Primitive`, `PrimitiveKind`, `Frozen/UnfrozenScalar/Array`), and
`acl::Acl`. `organization`/`user` appear only as `RecordDefn` aliases in the
registry (`lib.rs:144-149`) — defined but barely exercised. The behavior-tree
crates additionally need the *unfrozen builder* shapes of `structure` and
`enumeration`. So the surface is "module + structure + enumeration + folder +
ty + the record/freeze/version core + acl", not "module only".

### Adjacent constraint
`semio-client` (also private) is imported alongside semio-record across the
registry and module-core (`Selector`, `RecordType`; e.g. `arora-registry/src/lib.rs:12`,
`arora-module-core/src/resolve.rs:16`). Removing semio-record does not by itself
remove the private-repo dependency of `arora-registry`; that crate is the
Semio-store boundary and is the right place to keep the private adapter. See §5.

---

## 2. Redundancy analysis

semio-record and arora-types model **the same domain twice**, with different
vocabulary:

| Concept | semio-record | arora-types | Redundant? |
|---|---|---|---|
| Resolved vs. unresolved type graph | `frozen` vs. `unfrozen` modules | `low` (ids resolved) vs. `high` (string ids) | **Partially** — both encode "two forms", but `low/high` is a *parse/resolve* split, `frozen/unfrozen` is a *version-pinning* split. Not the same axis. |
| Type reference | `ty::FrozenTy`/`UnfrozenTy` = `Primitive` \| `Scalar(ref)` \| `Array(ref)` (ty.rs:279,424) | `module::low::TypeRef` = `Scalar{id}` \| `Array{id}` \| `Map{key,value}` (module/low.rs:17) | **Yes, overlapping.** arora-types adds `Map`; semio-record splits `Primitive` out of the ref enum. |
| Primitive kinds | `ty::PrimitiveKind` (25 variants incl. array-prims) (ty.rs:19) | `ty::*_ID` well-known UUIDs + `PRIMITIVE_TYPES` map (ty/mod.rs:11-127) | **Yes.** Two encodings of the same primitive set; arora keys by UUID, semio by enum. |
| Structure | `structure::v0::{unfrozen,frozen}::{Structure, StructureField}` (uses `IndexMap`, `Acl`, `parent`) | `ty::{low,high}::{Structure, StructureField}` (plain `HashMap`, no acl/parent) (ty/low.rs:16) | **Yes**, modulo acl/parent/ordering. |
| Enumeration | `enumeration::v0::{unfrozen,frozen}::{Enumeration, EnumerationVariant}` | `ty::{low,high}::{Enumeration, EnumerationValue}` (ty/low.rs:38) | **Yes**, modulo acl/parent. |
| Module/header | `module::v0::{unfrozen,frozen}::{Module, Export, ExportKind, Function, Parameter}` | `module::{low,high}::{ModuleDefinition, Header, ExportSymbol, ExportFunction, Parameter, ...}` (module/low.rs) | **Yes**, overlapping; arora-types adds `imports`, `Executor`, `executable`, `executable_mime`, `author/license/description`; semio adds `executable: Option<Uuid>` blob ref + `dependencies: Vec<Reference>`. |
| Version | `record::Version(semver::Version)`, `VersionReq(Option<semver::VersionReq>)` (record.rs:27,43) | `SemanticVersion{major,minor,patch}` + `From<SemanticVersion> for semver::Version` (lib.rs:50) | **Yes.** Two version newtypes; semio's also carries a *requirement* type, arora's does not. |
| Folder | `folder::v0::public::Public{name,parent}` (folder/v0/public.rs:10) | *(none)* | **Additional** (semio-only). Trivial 2-field struct. |
| ACL | `acl::Acl` | *(none)* | **Additional** (semio-only); permissions. |

### Genuinely additional in semio-record (the parts that actually do work)
1. **The version-pinning axis itself.** `FrozenReference{id, version}` vs.
   `UnfrozenReference{id, version_req}` (record.rs:77,85). arora-types has **no**
   notion of a version *requirement* nor a pinned dependency reference — its
   `TypeRef` carries a bare id/string. This is the single concept arora-types is
   missing and the one worth importing.
2. **The `Freeze`/`Freezer` mechanism** (record.rs:113-125): walk a value,
   resolve each `UnfrozenReference` → `FrozenReference` via a store. This is the
   "freezing" verb.
3. **`RecordDefn`** (record.rs:156): a per-record-kind registry of associated
   types (`Action`/`Unfrozen`/`Frozen`/`Public`/`Private`) + `const TYPE`,
   `const SCHEMA_VERSION`. arora consumes this as type-level plumbing
   (`<XDefn as RecordDefn>::Unfrozen`).
4. **Store/persistence machinery** that arora does *not* use: `Action`/`Apply`
   (CRDT-ish edit actions), `Migrate`, `BlobDependencies`, `acl`, `patch`,
   `serial`, `schema_version`, `migrate`, plus a large catalog of record kinds
   (animation, scene, platform, workspace, organization, user...). **This is the
   bulk of semio-record and is Semio-store-specific.**

### Key type definitions (quoted)
semio-record `record.rs:113-125`:
```rust
#[async_trait] pub trait Freeze<F: Freezer> {
  type Frozen;
  async fn freeze(&self, freezer: &F) -> Result<Self::Frozen, F::Error>;
}
#[async_trait] pub trait Freezer: Send + Sync {
  type Error: std::error::Error;
  async fn freeze(&self, reference: &UnfrozenReference) -> Result<FrozenReference, Self::Error>;
}
```
semio-record `record.rs:77-88`:
```rust
pub struct FrozenReference   { pub id: Uuid, pub version: Version }
pub struct UnfrozenReference { pub id: Uuid, pub version_req: VersionReq }
```
arora-types `module/low.rs:17-21` (the redundant ref):
```rust
pub enum TypeRef { Scalar{id: Uuid}, Array{id: Uuid}, Map{key_id: Uuid, value_id: Uuid} }
```

**Conclusion:** ~80% of what arora touches in semio-record is *data redundant*
with arora-types (types, structures, enums, modules, primitives, version). The
~20% that is genuinely additional and worth generalizing is: (a) version
*requirement* + pinned reference, and (b) the `Freeze`/`Freezer` verb. The
store-specific 80%-of-the-crate-by-size (actions, migrate, acl, blob, the
record catalog) is **not** used by arora and must not be copied.

---

## 3. Design

We want a small, neutral abstraction in `arora-types` that expresses
"versioning + freezing" for arbitrary types — not hard-wired to module records.

### Design options

**(a) Fold a neutral record model into `arora-types` directly.**
Re-create `Module`/`Structure`/`Enumeration` "unfrozen/frozen" data types inside
arora-types and have arora-engine use those instead of semio-record's.
- Pros: arora-engine drops semio-record outright; one source of truth.
- Cons: This is exactly the "copy semio-record into arora-types" the brief warns
  against. It also duplicates the data shapes arora-types *already* has
  (`module::low/high`, `ty::low/high`) under a third naming axis. High churn,
  redundant.

**(b) A `Freeze`/`Versioned` trait set + blanket impls in `arora-types` (RECOMMENDED).**
Add only the *missing concepts* — `Version`, `VersionReq`, `FrozenReference`,
`UnfrozenReference`, the `Versioned` trait, and the `Freeze`/`Resolver` traits
with blanket impls for containers — and implement those traits on arora-types'
**existing** `low`/`high` types (extended so a `TypeRef` can carry a version
requirement). No new data model; arora-types' own types gain the freeze/version
behavior. The store-backed resolver stays in arora-registry (private), now
implementing arora-types' `Resolver` rather than semio-record's `Freezer`.
- Pros: zero data redundancy (reuses `module`/`ty` modules); the generalization
  is genuinely type-agnostic (works for Structure, Enumeration, Header, BT types);
  small surface; keeps the private store concern out of arora-types.
- Cons: requires extending arora-types `TypeRef`/refs to carry version reqs
  (a wire-format change — see §5 for compat); requires writing `Freeze` impls on
  arora-types types (mechanical, mostly via blanket impls).

**(c) A separate `arora-record` crate.**
Put the trait set (and maybe neutral data) in a new public crate that both
arora-types and arora-engine depend on.
- Pros: clean layering if the record concept grows; lets non-arora consumers
  reuse it; keeps arora-types lean.
- Cons: another crate/repo to publish and version during an already-ongoing repo
  split; for a ~150-line trait set it is over-structured. Can be promoted to this
  later if needed.

### Recommendation: **(b)**, with the trait set placed in a new
`arora_types::record` module. If arora-types itself must stay tiny, the *same*
module can later be lifted into an `arora-record` crate (option c) with no API
change — the traits are identical. Start in-tree.

### The trait set (as prototyped, abbreviated)

```rust
// arora_types::record::reference
pub struct Version(pub semver::Version);
pub struct VersionReq(pub Option<semver::VersionReq>); // None = "any"
pub struct UnfrozenReference { pub id: Uuid, pub version_req: VersionReq }
pub struct FrozenReference   { pub id: Uuid, pub version: Version }

// arora_types::record::versioned — version tagging + compatibility, any type
pub enum Compat { Identical, BackwardCompatible, Incompatible }
pub trait Versioned {
  fn id(&self) -> Uuid;
  fn version(&self) -> Version;
  fn frozen_reference(&self) -> FrozenReference { /* default */ }
  fn unfrozen_reference(&self, req: VersionReq) -> UnfrozenReference { /* default */ }
  fn compatibility(from: &Version, to: &Version) -> Compat { /* semver default */ }
}

// arora_types::record::freeze — the freezing verb, store-agnostic
pub trait Resolver {                 // == semio-record `Freezer`, but neutral
  type Error: std::error::Error;
  fn resolve(&self, r: &UnfrozenReference) -> Result<FrozenReference, Self::Error>;
}
pub trait Freeze<R: Resolver> {      // == semio-record `Freeze<F>`
  type Frozen;
  fn freeze(&self, resolver: &R) -> Result<Self::Frozen, R::Error>;
}

// Blanket impls give structure-preserving freezing for free:
impl<R:Resolver, T:Freeze<R>> Freeze<R> for Vec<T> { type Frozen = Vec<T::Frozen>; ... }
impl<R:Resolver, T:Freeze<R>> Freeze<R> for Option<T> { ... }
impl<R:Resolver, K:Clone+Eq+Hash, V:Freeze<R>> Freeze<R> for HashMap<K,V> { ... }
impl<R:Resolver> Freeze<R> for UnfrozenReference { type Frozen = FrozenReference;
  fn freeze(&self, r:&R)->Result<_,_>{ r.resolve(self) } } // base case
```

How an arora type implements it (prototype `examples.rs`, mirroring real shapes):

```rust
impl Versioned for Structure { fn id(&self)->Uuid{self.id} fn version(&self)->Version{self.version.clone()} }

impl<R: Resolver> Freeze<R> for Structure {
  type Frozen = FrozenStructure;
  fn freeze(&self, r: &R) -> Result<Self::Frozen, R::Error> {
    Ok(FrozenStructure {
      id: self.id, version: self.version.clone(), name: self.name.clone(),
      fields: self.fields.freeze(r)?,     // blanket HashMap impl
    })
  }
}
// ...and identically for ModuleHeader (Vec<Parameter>, Vec<UnfrozenReference>),
// Enumeration, behavior-tree TickId/Status — same machinery, no special-casing.
```

### How this replaces semio-record's role for arora
- `record::{Freeze, Freezer}` → `arora_types::record::{Freeze, Resolver}`. The
  registries (`Local/Remote/RemoteCached`) swap `impl Freezer` →
  `impl Resolver` (the body is unchanged: pick newest matching version,
  `local/mod.rs:117`).
- `record::{Version, VersionReq, Frozen/UnfrozenReference}` →
  `arora_types::record::*`. (arora-types already has `SemanticVersion`; the new
  `Version`/`VersionReq` add the *requirement* it lacks. `SemanticVersion` can be
  kept and given `From`/`Into<Version>` for wire compat.)
- `ty::{FrozenTy, UnfrozenTy, Primitive, PrimitiveKind}` and the
  `module/structure/enumeration/folder` data types → arora-types' existing
  `ty::{low,high}` and `module::{low,high}` (extended), with `Freeze` impls.
- `RecordDefn` projections in `arora-registry/src/lib.rs:141-154` → drop the
  trait; define the aliases concretely against arora-types types (e.g.
  `pub type Structure = arora_types::ty::high::Structure;`).

### What stays Semio-private
- The **store-backed resolver** (the actual `Resolver` impl that talks to the
  registry/network) lives in `arora-registry` and may keep using `semio-client`'s
  `Selector`/`RecordType`. arora-types defines the trait; arora-registry provides
  the implementation. Nothing private leaks into arora-types.
- semio-record's `Action`/`Apply`/`Migrate`/`acl`/`blob`/`patch`/`serial` and the
  full record catalog stay in semio-record for the Semio store. arora simply
  stops importing them. `acl::Acl` usages in arora (`status.rs`, `tick_id.rs`,
  `resolve.rs`) are only `Acl::default()` for builder construction → replaced by
  arora-types builders that don't carry an ACL field (arora-types Structure/Enum
  already have no acl).

### Trade-offs summary
Option (b) buys the smallest, non-redundant change and a genuinely generic
abstraction, at the cost of one wire-format extension (version reqs on refs) and
writing mechanical `Freeze` impls. (a) is rejected as redundant; (c) is (b)'s
natural future home once the concept needs an independent release cadence.

---

## 4. Prototype + tests

Crate: `<scratch>/record-study/arora-record-proto` — self-contained
(std + serde + serde_json + uuid + semver), depends on **no** private repo.
Layout: `reference.rs`, `versioned.rs`, `freeze.rs`, `examples.rs`,
`tests/record.rs`.

Demonstrates, against shapes mirrored from the real code:
- `Versioned` + `Freeze`/`Resolver` traits with blanket impls (Vec/Option/HashMap/ref).
- An `InMemoryRegistry` implementing `Resolver` exactly like
  `LocalRegistry::freeze` (newest version matching the requirement).
- The traits applied to **four** distinct types: `UnfrozenTy`, `Structure`,
  `ModuleHeader`, plus the leaf `UnfrozenReference` — proving non-hard-wiring.
- Freeze → unfreeze → re-freeze round-trip stability.
- Version tagging + semver compatibility classification.
- serde wire round-trip of the frozen form.

### `cargo test` result (actual)

```
running 8 tests
test resolver_picks_newest_matching ... ok
test resolver_errors_surface ... ok
test version_compat_rules ... ok
test versioned_reference_tagging ... ok
test freeze_module_header_record ... ok
test frozen_form_serde_roundtrips ... ok
test freeze_unfreeze_roundtrip_pins_exact_version ... ok
test freeze_structure_record ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; finished in 0.00s
```

**8 passed, 0 failed** — green on first compile/run. No fundamental obstacle hit.

### Notable design findings from the prototype
- **Freezing is lossy on the requirement.** Once pinned, the original range is
  gone; `unfreeze()` reconstructs an *exact* (`=x.y.z`) requirement, which is the
  natural inverse and makes the round trip *idempotent* (re-freezing yields the
  same pin). This matches semio-record, which has no unfreeze at all — arora may
  not need one, but it falls out cleanly when wanted.
- **Sync vs. async.** semio-record's `Freeze`/`Freezer` are `#[async_trait]`
  because the store resolver is async. The prototype is sync to stay
  dependency-free; for the real migration, `Resolver::resolve` should be
  `async` (keep `#[async_trait]`) since `arora-registry`'s resolvers are async.
  The trait *shape* is identical; only `async` + the blanket-impl bodies change
  (`.await`). This is the one place the real impl differs from the prototype and
  is a known, bounded delta — not an obstacle.
- **Blanket impls eliminate most hand-written code.** Only leaf enums
  (`UnfrozenTy`) and record structs need explicit `Freeze`; every `Vec`/`Option`/
  `HashMap`/`IndexMap` field is automatic. (Add an `IndexMap` blanket impl for the
  real types, which use `IndexMap` to keep YAML field order stable.)

---

## 5. Migration plan

Sequenced to land additive, compat-preserving changes first, then flip consumers,
then drop the dep. Designed to interleave with the ongoing repo split (arora-types
already lives in its own repo; semio-record in another).

### Phase 0 — Land the trait set in arora-types (additive, no consumer change)
1. Add `arora_types::record` module: `Version`, `VersionReq`,
   `Frozen/UnfrozenReference`, `Versioned`, `Compat`, `Resolver` (async),
   `Freeze`, blanket impls (Vec/Option/HashMap/**IndexMap**/reference). This is the
   prototype, made async. Ship as a minor arora-types release.
2. Provide `From<SemanticVersion> for Version` and back, so existing arora-types
   `SemanticVersion` fields keep working and the new `Version` interops.
   *No wire change yet.* arora-engine still on semio-record.

### Phase 1 — Extend arora-types' ref types to carry version requirements
3. arora-types `module::{low,high}::TypeRef` and module `dependencies` currently
   carry bare ids and no version requirement. Add an optional
   `version_req: VersionReq` (serde `#[serde(default)]`, omitted == "any") to the
   reference-bearing variants, and a frozen counterpart (`version: Version`).
   `#[serde(default)]` keeps **old documents deserializing unchanged** → wire/serde
   compatible. Implement `Freeze` for `ty::{low/high}` and `module::{low/high}`
   types and for `ty::low::{Structure, Enumeration}` using the blanket impls.
4. Implement `Versioned` for `module::low::Header`, `ty::low::{Structure,
   Enumeration}` (they already carry `id` + `SemanticVersion`/derivable version).

### Phase 2 — Migrate the leaf consumers (no store coupling) first
These only construct/translate data; migrate them to arora-types types:
5. **arora-behavior-tree-types** (`status.rs`, `tick_id.rs`, `lib.rs`): replace
   `semio_record::enumeration/structure/folder/ty` builders with arora-types
   `ty::{low,high}::{Structure, Enumeration}` and a small arora-types `Folder`
   (port the 2-field `folder::v0::public::Public` into arora-types as the only
   new data type — it has no arora-types equivalent). Drop `acl::Acl` (arora-types
   structures have no ACL).
6. **arora-module-rust**, **arora-module-cpp**: swap `ty::{FrozenTy, PrimitiveKind,
   Primitive, FrozenScalar}`, `module::v0::frozen::{ExportKind, Parameter}`,
   `structure::v0::frozen::StructureField` for arora-types frozen equivalents
   (`ty::low::*`, `module::low::*`). Codegen output (Rust/C++) is unaffected since
   it reads the same fields.
7. **arora-behavior-tree**: swap `module::v0::frozen::{Function, Parameter}`,
   `ty::FrozenTy` for arora-types `module::low`/`ty::low`.

### Phase 3 — Migrate the registry (the store boundary)
8. **arora-registry** is the deepest coupling (`RecordDefn` projections +
   `impl Freezer`). Steps:
   - Replace `lib.rs:141-154` `<XDefn as RecordDefn>::{Unfrozen,Frozen,Public}`
     aliases with concrete arora-types types
     (`type Structure = arora_types::ty::high::Structure;` etc.). Drop
     `RecordDefn`.
   - Change `impl Freezer for {Local,Remote,RemoteCached}Registry`
     (`local/mod.rs:108`, `remote.rs:266`, `remote_cached.rs:191`) to
     `impl arora_types::record::Resolver`. Bodies are unchanged (newest matching
     version). `RegistryError` already implements `std::error::Error`, satisfying
     `Resolver::Error`.
   - `get_primitive` (`lib.rs:336`) maps arora-types `*_ID` UUIDs to a primitive;
     keep it, retargeting from `PrimitiveKind` to arora-types' primitive repr.
   - `editable.rs:186` `ExportKind::Function` match → arora-types export symbol.
   - **`semio-client` stays** here (it is the network/store client, orthogonal to
     semio-record). The registry remains the one private-store seam; that is by
     design (§3 "what stays private").
9. **arora-module-core** (`resolve.rs`, `header.rs`, `lib.rs`): replace the
   `R: ReadableRegistry + Freezer` bound with `R: ReadableRegistry + Resolver`;
   swap `module::v0::unfrozen::{Export, Function, Parameter}` / `ty::Unfrozen*`
   construction for arora-types `module::high` / `ty::high`. Drop `acl::Acl`.

### Phase 4 — Migrate the CLIs and drop the dependency
10. **arora-cli**, **arora-module-cli**, **modules/polly**: swap
    `module::v0::frozen::ExportKind` + `record::Freezer` for arora-types
    equivalents + `Resolver`.
11. Remove `semio-record = { git = ... }` from all **9** manifests.
    `cargo build --workspace` + full test suite must be green.

### Wire / serde compatibility
- All new fields land behind `#[serde(default)]` (version reqs omitted == any) so
  existing YAML/JSON records deserialize unchanged.
- The frozen form's serde tags (`tag="type"/"kind"`, `rename_all="camelCase"`,
  `content="value"`) must be reproduced **exactly** as in semio-record
  (`ty.rs:273-278`, `frozen.rs:72`) for any data that crosses the wire to the
  Semio store. Pin these with golden serde round-trip tests (prototype test
  `frozen_form_serde_roundtrips` is the template) before flipping the registry.
- `SemanticVersion` (`{major,minor,patch}`) and `Version(semver)` serialize
  differently. Keep `SemanticVersion` as the on-disk module-header version
  (unchanged) and use `Version` only inside references; bridge with `From`.

### Freezer/registry interplay
The only behavioral contract to preserve is "freeze = resolve each unfrozen ref
to the newest version satisfying its requirement" (`local/mod.rs:117`). Because
the new `Resolver` trait has the same signature and the registry bodies are
copied verbatim, this is mechanical. Add a regression test that freezes a
multi-version fixture and asserts the picked versions (prototype
`resolver_picks_newest_matching`).

### Where semio-record drops entirely vs. stays behind an adapter
- **Drops entirely** for arora: all of `ty`, `module`, `structure`, `enumeration`,
  `folder`, `record`, `acl` usages are replaceable by arora-types + the new trait
  set. After Phase 4 no arora crate imports `semio_record`.
- **Stays in semio-record** (not arora's concern): the Semio store/server still
  uses semio-record's full record catalog, actions, migrate, acl, blobs. If a
  Semio store component must consume arora modules through the old types, add a
  thin `From`/adapter *in semio-record (or a Semio-side glue crate)* mapping
  arora-types frozen modules ↔ semio-record frozen modules — kept on the private
  side, never in arora-types.

### Risks
- **Wire drift**: easy to mismatch a serde tag/rename vs. semio-record and break
  store interop. Mitigation: golden-format tests captured from semio-record output
  before migration; assert byte-equality.
- **`RecordDefn` removal** touches the registry's public type aliases used across
  arora-engine; a large but mechanical rename. Mitigation: do it as one atomic PR
  with the aliases kept name-identical (`Structure`, `ModuleFrozen`, ...).
- **async blanket impls**: `Freeze` over containers with an async `Resolver`
  needs `async-trait` + sequential `for` loops (not `.map().collect()`); slightly
  more code than the sync prototype. Bounded, known.
- **`IndexMap` ordering**: arora-types structures use `HashMap`; semio-record uses
  `IndexMap` to keep YAML field order stable (structure/v0/unfrozen.rs comment).
  If stable YAML diffs matter for arora, switch arora-types Structure/Enumeration
  to `IndexMap` (and add the `IndexMap` blanket `Freeze` impl) during Phase 1.

### Sequencing vs. the repo split
arora-types and semio-record are already separate repos; arora-engine pulls both
by git. The plan is split-friendly: Phase 0/1 are arora-types-only releases;
Phases 2-4 are arora-engine-only and bump the arora-types pin. semio-record needs
**no** change to be dropped (arora just stops depending on it). If an
`arora-record` crate (option c) is later desired, lift `arora_types::record` into
it with no API change and re-export from arora-types for one deprecation cycle.

---

## Appendix: file:line index of the deepest couplings
- `RecordDefn` projections: `arora-registry/src/lib.rs:141-154`.
- `Freezer` impls (the resolve-to-newest semantics): `arora-registry/src/local/mod.rs:108-136`,
  `remote.rs:266`, `remote_cached.rs:191`.
- Generic `Freezer` bound on the analyze pipeline: `arora-module-core/src/lib.rs:22,38`.
- Unfrozen builders that must move to arora-types: `arora-behavior-tree-types/src/status.rs:1`,
  `tick_id.rs:1`, `arora-module-core/src/resolve.rs:17-22`.
- semio-record core to generalize: `semio-record/crates/semio-record/src/record.rs:77-125`,
  `ty.rs:271-524`.

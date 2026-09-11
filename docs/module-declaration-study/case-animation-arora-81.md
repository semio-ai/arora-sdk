# Case: the animation module (ARORA-81)

[ARORA-81](https://linear.app/semio-ai/issue/ARORA-81/generate-a-modules-own-id-function-ids-and-parameter-ids-as-public)
asks for the ids of `vizij-animation-module` to be generated rather than
transcribed. Under the declaration macros they are not generated *from* the
YAML: the Rust declaration is where they are pinned, and the YAML — when one
is still wanted — is written from `header()`.

Prototype: [`prototype/cases/animation`](prototype/cases/animation/src/lib.rs)
declares six of the thirteen exports (the ones with a struct parameter, an
array return, and two-argument functions) with stand-in implementations, and
[`prototype/cases/host`](prototype/cases/host/tests/animation.rs) dispatches
them.

## Today

`crates/interop/vizij-animation-module`: `module.yaml` + `types/*.yaml`
records; `build.rs` generates `src/arora_generated/` (shims, codec, structs
with `Into`/`TryFrom<Value>`, `*_RAW_ID`) and `records/`; `src/lib.rs`
implements the functions with `Option<T>` parameters and, by hand, "mirror
`module.yaml`":

```rust
pub mod ids {
    pub const MODULE: Uuid = Uuid::from_u128(0x76697a69_6a00_0000_0d00_000000000000);
    pub const ADD_INSTANCE: Uuid = Uuid::from_u128(0x76697a69_6a00_0000_0f00_000000000003);
    // … eleven more; no parameter ids
}
```

`crates/vizij/src/animation.rs`, by hand, positional, undescribed:

```rust
ModuleBuilder::new(ids::MODULE)
    .function(ids::ADD_INSTANCE, |call| {
        u32_result(vizij_animation_module::add_instance(arg_u32(&call, 0), arg_u32(&call, 1)))
    })
```

Plus `tests/host_ramp.rs` (every id again, as strings, parameters included)
and vizij-web's `animationModule.ts` (every id again, in TypeScript).

## With the declaration macros

The boundary types carry their own schema and conversions:

```rust
#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000100")]
pub struct AnimationClip {
  #[arora(id = "76697a69-6a00-0000-0100-000000000001")] pub name: String,
  #[arora(id = "76697a69-6a00-0000-0100-000000000002")] pub duration: u32,
  #[arora(id = "76697a69-6a00-0000-0100-000000000003")] pub tracks: Vec<AnimTrack>,
}
```

The module declares itself:

```rust
#[arora::module(id = "76697a69-6a00-0000-0d00-000000000000", name = "vizij-animation", version = "0.2.0")]
pub mod animation {
    #[export(id = "76697a69-6a00-0000-0f00-000000000001")]
    pub fn load_animation(#[param(id = "76697a69-6a00-0000-0f01-000000000001")] clip: AnimationClip) -> u32 { … }

    #[export(id = "76697a69-6a00-0000-0f00-000000000003")]
    pub fn add_instance(
        #[param(id = "76697a69-6a00-0000-0f03-000000000001")] player: u32,
        #[param(id = "76697a69-6a00-0000-0f03-000000000002")] anim: u32,
    ) -> u32 { … }

    #[export(id = "76697a69-6a00-0000-0f00-000000000004")]
    pub fn step(#[param(id = "76697a69-6a00-0000-0f04-000000000001")] dt_ns: u64) -> Vec<TrackOutput> { … }
    // …
}
```

And the device registers it:

```rust
// crates/vizij/src/animation.rs, whole
pub fn host_module() -> HostModule { arora::host_module!(vizij_animation_module::animation) }
```

What that removes: `ids`, the positional `arg_*` helpers and their closures,
the string ids in `host_ramp.rs`, `types/*.yaml`, `records/`, and — once
[M4](README.md#mechanisms) lands — `build.rs` and `src/arora_generated/`.
What it adds: every function is described, so `DescribeMethods` lists the
thirteen; and the TypeScript tables become something to generate from
`header()` rather than to transcribe.

## Established

| Claim | Evidence |
|---|---|
| The six declared exports equal the generator's, export for export (struct parameter as `Scalar { id }`, array return as `Array { id }`) | `animation/tests/header.rs::the_declared_exports_reproduce_the_generated_ones`, `…::a_struct_parameter_and_an_array_return_declare_by_their_type_ids` |
| `#[derive(AroraValue)]` produces the exact `Value` the ARORA-55 generated `Into<Value>` produces | `animation/tests/conversions.rs::a_struct_converts_to_the_generated_wire_shape` |
| Nested arrays of structures round-trip; a structure of the wrong id is refused | `animation/tests/conversions.rs::nested_arrays_of_structures_round_trip`, `…::a_structure_of_the_wrong_id_is_refused` |
| **Reversed arguments give the same result** (VIZ-129's property) | `host/tests/animation.rs::arguments_are_matched_by_id_whatever_their_order` |
| A struct argument crosses the engine boundary; an array of structures comes back and decodes | `host/tests/animation.rs::a_structure_argument_crosses_the_boundary_and_an_array_comes_back` |
| Every declared function is discoverable | `host/tests/animation.rs::every_declared_function_is_discoverable` |
| The module crate builds for `wasm32-wasip1` without the engine | `cargo build -p case-animation --target wasm32-wasip1` |
| `AnimationClip` is pinned at its record's version (`1.1.0`) in the described signature | `host/tests/animation.rs::a_user_type_is_pinned_at_its_declared_version` |

## What ARORA-81 becomes

Its title no longer describes the change. Under this study, the ticket is
"declare the animation module in Rust": the ids stop being generated or
transcribed because the declaration is their only home. The SDK half — the
macros, the derive extension, `ModuleBuilder::from_declaration` — is the work
this study scopes; the vizij half is [VIZ-87](https://linear.app/semio-ai/issue/VIZ-87/export-generated-ids-for-the-animation-module)
and [VIZ-129](https://linear.app/semio-ai/issue/VIZ-129/the-animation-host-module-reads-call-arguments-by-position-not-by),
both closed by the same conversion.

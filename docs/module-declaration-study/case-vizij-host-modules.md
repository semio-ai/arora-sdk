# Case: vizij's host-only modules

`vizij-arora-tts`, `tts_piper`, `viseme` and `gaze` register `HostModule`s
that were never a `module.yaml`: ids as consts (two of them name-hashed),
frozen `Function`s built by hand — three times for the same `say` — and no
header, so nothing to export to a store or another language.

Prototype: [`prototype/cases/vizij-skills`](prototype/cases/vizij-skills/src/lib.rs)
declares `say(text, voice, &mut viseme) -> Status` and `look_at(policy,
target: Vec<f32>, frame) -> Status`.

## Today

`vizij-arora-behavior/src/speech.rs`:

```rust
pub fn say_signature() -> Function {
    let mut parameters = HashMap::new();
    let mut parameter_ordering = Vec::new();
    for (id, name, kind, mutable) in [
        (SAY_TEXT_PARAM_ID, "text", PrimitiveKind::String, false),
        (SAY_VOICE_PARAM_ID, "voice", PrimitiveKind::String, false),
        (SAY_VISEME_PARAM_ID, "viseme", PrimitiveKind::String, true),
    ] { … }
    Function { parameters, parameter_ordering, return_ty: FrozenTy::FrozenScalar(… STATUS_ENUMERATION_ID @ 1.0.0 …) }
}
```

`gaze.rs`: `module_id()` = `gen_uuid_from_str("gaze-module")`, parameter ids
hashed from their names.

## With the declaration macros

```rust
#[arora::module(id = "31ca2243-5719-4862-aa57-c30d27cab62e", name = "tts-piper", version = "1.0.0")]
pub mod tts {
  #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
  pub fn say(
    #[param(id = "881dc182-…")] text: String,
    #[param(id = "f56ca142-…")] voice: String,
    #[param(id = "a1fbf58b-…")] viseme: &mut String,
  ) -> Status { … }
}
```

`say_signature()` is `tts::host_functions()[0].2`; the three copies collapse
into the one declaration. Gaze's ids become pinned. And a host-only module now
has an exportable interface — `record(parent)`, the store's form, which names
no executor because a module linked into the host has none.

## Established

| Claim | Evidence |
|---|---|
| The declared `say` signature is the hand-built one: three strings, `viseme` mutable, `Status@1.0.0` back | `vizij-skills/tests/signatures.rs::say_declares_text_voice_and_a_mutable_viseme_returning_status` |
| `Vec<f32>` declares as `ArrayF32` / `TypeRef::Array { F32 }` | `vizij-skills/tests/signatures.rs::look_at_declares_an_f32_array_target` |
| A host-only module exports its interface as a store record that reads back | `vizij-skills/tests/signatures.rs::a_host_only_module_exports_its_interface_as_a_record` |
| The viseme comes back through the mutable parameter | `host/tests/skills.rs::say_reports_its_viseme_through_the_mutable_parameter` |

## Executor

None: a host-only module is linked, not loaded — [Q10](open-questions.md#q10).

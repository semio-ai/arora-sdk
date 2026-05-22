# Cargo migration journal

Chronological log of learnings and decisions. Newest at the bottom.

---

## 2026-05-22 — Kickoff & initial survey

**Repo shape:** Cargo workspace already exists at root with 13 crates +
`modules/nao`. The other modules (`polly`, `test-cpp`, `test-cpp-2`,
`test-rust-wasm`, `behavior-tree-nodes`, `transcribe`) are not in the workspace
— they are independent Cargo projects or pure C++ targets driven from CMake.

**Top-level orchestration today:** `CMakeLists.txt` builds Rust by spawning
`cargo build` with target flags. `PreLoad.cmake` downloads WASI SDK 25 from
GitHub releases via `FetchContent`. The toolchain file is set globally before
CMake's project() resolves, which is why it's in PreLoad.

**Key observation:** `modules/nao/build.rs` already runs cmake *from* cargo —
it's the inverse pattern of the rest of the repo. So the cargo-first idea is
already partially proven for the trickiest module. We're generalizing nao's
pattern to the rest.

**Cross-crate ordering pain:** Several modules' CMake steps shell out to
`arora-module-cli` (the host tool) to generate C++ from `module.yaml`. Cargo
doesn't naturally express "build crate X before running build.rs of crate Y"
across the workspace. Options:
  - The consumer's `build.rs` runs `cargo build -p arora-module-cli` itself
    (annoying, double cargo invocation, but works).
  - An `xtask` orchestrates the full graph.
  - Cargo's `[[bin]]` artifact deps (unstable feature `artifact-dependencies`)
    could express this cleanly but require nightly.
  - Decision deferred to Stage 2 — likely go with the build.rs approach for
    portability, accepting the recursive-cargo cost.

**WASM modules are executables, not cdylibs:** `modules/test-cpp` builds an
*executable* with `-Wl,--no-entry -Wl,--export=...`. That's not a thing cargo
can do natively for C++. The WASM C++ modules need direct invocation of
wasi-sdk's clang. We can't use the `cmake` or `cc` crates' default linking
behavior — we need to manually wire the link command. Note for Stage 3.

**libqi cost:** `modules/nao` `FetchContent`s libqi from
`github.com/semio-ai/libqi.git` (pinned commit), which also pulls OpenSSL and
Boost transitively. This is the iteration killer. User wants a minimal C++
header-only stub that lets nao compile + link but crashes if anyone calls into
it. That's Stage 1.

**Decision — plan/journal location:** `docs/cargo-migration/` per user. Visible
in git, picked up by other agents naturally.

**Decision — stub aggressiveness:** Minimal headers, no implementations.
Functions either are declared but not defined (linker errors on call) or are
declared with `__attribute__((noreturn))` and abort. Lean toward the latter for
predictability — `__builtin_trap()` in inline definitions, so the symbol exists
but calling it crashes loudly.

**Next step:** Survey what symbols `modules/nao/src/nao.cpp` actually uses from
libqi to size the stub.

---

## 2026-05-22 — Decisions: WASI target & host-tool pattern

**WASI target choice.** Surveyed rustc platform-support and wasi-sdk releases:

- `wasm32-wasip1`: Tier 2, full std, default of wasi-sdk ≥ 31.
- `wasm32-wasip2`: Tier 2, but built around the WASI 0.2 component model — more
  intrusive change, and not what wasi-sdk emits by default.
- `wasm32-wasip3`: Tier 3, not for production.
- `wasm32-unknown-unknown`: still works for pure-Rust Wasm with no system
  interface; what we use today for `arora-buffers` / `arora-util`.

Decision: migrate Rust+C++ WASM builds to **`wasm32-wasip1`**, bump WASI SDK to
**v33** (latest, April 2026). Aligning the Rust and C++ sides on the same
preview eliminates the current implicit mismatch
(`wasm32-unknown-unknown` Rust + WASI-SDK clang for C++). Component model is a
future migration.

**Host-tool pattern.** Researched the field. The clean answer is
`-Z bindeps` artifact-dependencies with `target = "host"`, but as of May 2026
this is still nightly-only (cargo issue #9096, open since 2021). The pragmatic
stable pattern, used by wasm-bindgen / bevy / similar:

1. Consumer `build.rs` checks `$ARORA_MODULE_CLI`. If set, use it.
2. Otherwise, invoke `env::var_os("CARGO")` with `build -p arora-module-cli
   --release --target-dir <shared host dir>`.
3. Scrub these env vars before the inner cargo invocation, or it will inherit
   the cross-compilation and try to build the host tool for wasm32:
   - `CARGO_BUILD_TARGET`
   - `CARGO_TARGET_DIR`
   - `CARGO_ENCODED_RUSTFLAGS`
   - `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`
4. Point the shared host-tool target dir at `target/host-tools/` so all
   consumers reuse one build (cargo holds file locks per target-dir, so a
   shared dir is correct; collisions just serialize).

Also: emit `rerun-if-changed` for the CLI source dir and the `module.yaml`, and
`rerun-if-env-changed=ARORA_MODULE_CLI`. Use `env::var_os("CARGO")` — never
`"cargo"` from `$PATH`, or rust-toolchain pinning breaks.

This will be factored into a small helper crate `crates/arora-host-tools` (or
similar) so every consumer doesn't repeat the env-scrubbing dance. Name TBD.

**Branch created:** `cargo-first-build`, off `arora-types-crates-io`.

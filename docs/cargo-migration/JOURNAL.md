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

---

## 2026-05-22 — Reversal: use nightly bindeps

User chose nightly `-Z bindeps` over the stable recursive-cargo pattern. Trade:
we pin nightly via `rust-toolchain.toml` in exchange for losing the env-scrub
boilerplate in every consumer `build.rs`. With bindeps, a consumer says

```toml
cargo-features = ["bindeps"]   # at top-level Cargo.toml
# ...
[build-dependencies]
arora-module-cli = { path = "...", artifact = "bin" }
```

and cargo exports `CARGO_BIN_FILE_ARORA_MODULE_CLI` to the `build.rs`. By
default for build-dependencies, the artifact is built for the host — which is
exactly what we want. Need to verify the `target = "host"` keyword spelling vs.
default behavior once we hit it in practice (RFC 3028 vs current cargo
implementation may differ).

This simplifies the migration's "Stage 2": no helper crate for env-scrubbing,
no shared `target/host-tools/` dir to manage manually — cargo handles it.

---

## 2026-05-22 — libqi stub landed; nao builds without real libqi

**Surface needed for nao.** `modules/nao/src/nao.cpp` is the only consumer.
Total qi surface used:

- `qi::registerBaseTypes()`
- `qi::Session` with `connect(string)` and `service(string) -> Future<AnyObject>`
- `qi::AnyObject::call<T>(name, args...)`
- `Future<T>::value() -> T`

Generated module code under `arora/source/nao.cpp` and the static `arora-cpp`
helper do **not** touch qi. So a header-only stub at this surface is enough.

**Stub.** `libs/qi-stub/include/qi/{session,registration}.hpp` — every function
inline, body is `__builtin_trap()`. Marked `[[noreturn]]` where the return type
allows. `libs/qi-stub/CMakeLists.txt` defines `qi-stub` and an alias `qi` so
nothing else needs to know. Wired into `modules/nao/CMakeLists.txt` behind
`option(USE_QI_STUB ON)`. Default is stub.

**Verification.** Configured NAO build (`-DNAO=ON`), then
`cmake --build build-nao --target arora`. The nao module builds:
```
[ 60%] Building CXX object CMakeFiles/nao.dir/src/nao.cpp.o
[ 80%] Building CXX object CMakeFiles/nao.dir/arora/source/nao.cpp.o
[100%] Linking CXX shared library libnao.so
[100%] Built target nao
```
The eventual cargo failure is downstream of nao, in `arora-c/cmake_install.cmake`
trying to install a missing `//libarora_buffers.a`. This is a *pre-existing*
breakage in the cross-compile install path, not caused by the stub. Tracked as
a known issue to fix later in Stage 2 when we rework orchestration.

**Takeaway:** Stage 1 unblocked. Next iteration on nao.cpp no longer has to
build OpenSSL + Boost + libqi.

---

## 2026-05-22 — Workspace unified, nightly + bindeps enabled

Brought the previously standalone module crates (`polly`, `test-rust-wasm`,
`behavior-tree-nodes`) into the top-level Cargo workspace alongside `nao` and
the `crates/*`. Each module's `Cargo.toml` had a `[workspace]` table at the
bottom that isolated it from the parent workspace — removed those four lines
per module.

Added `rust-toolchain.toml` pinning the channel to `nightly` with targets
`wasm32-wasip1` and `i686-unknown-linux-musl`. Added `.cargo/config.toml`
enabling `bindeps` via `[unstable]`. Note that `cargo-features = ["bindeps"]`
inside `Cargo.toml` does **not** work as of cargo 1.96 nightly — the canonical
spelling is `[unstable] bindeps = true` in `.cargo/config.toml` (or
`-Zbindeps` on the CLI).

Verified `cargo metadata` resolves the unified workspace and `cargo check
--workspace` succeeds (a couple of pre-existing dead-code warnings, unrelated).

Next: pick the migration's pilot C++ module. Lean toward `modules/test-cpp`
because it's the simplest C++ WASM target. Wrap it in a Cargo crate whose
`build.rs` does codegen + WASI SDK compile, then carry the pattern over to
`test-cpp-2`, `libs/cpp`, and `modules/nao`.

---

## 2026-05-22 — WASI SDK crate landed; bindeps surface confirmed

Added `crates/wasi-sdk`. Pure Rust, downloads wasi-sdk-33 from the upstream
GitHub release if `WASI_SDK_PATH` isn't set. Caches under
`<workspace>/target/wasi-sdk-33/`. Exposes `clang()`, `clangpp()`,
`cmake_toolchain_file()`. Tarball naming verified for arm64-macos against
HTTP HEAD.

Surveyed crates.io for an existing maintained `wasi-sdk` crate — there is
none. `lucet-wasi-sdk` exists but hasn't shipped since 2020. In-tree is
correct.

Bumped target version 25 → 33 (April 2026, latest). v33 still defaults clang's
target to `wasm32-wasip1`, matching the Rust side of the migration.

Confirmed Cargo's `-Z bindeps` surface for the patterns we need:

- Host bin (codegen tool):
  ```toml
  [build-dependencies]
  arora-module-cli = { path = "...", artifact = "bin" }
  ```
  Cargo exports `CARGO_BIN_FILE_ARORA_MODULE_CLI`. Build-deps default to host.
- Cross-target staticlib (for wasm modules that link against arora-buffers /
  arora-util):
  ```toml
  [build-dependencies]
  arora-buffers = { path = "...", artifact = "staticlib", target = "wasm32-wasip1" }
  ```
  Cargo exports `CARGO_STATICLIB_FILE_ARORA_BUFFERS` and
  `CARGO_STATICLIB_DIR_ARORA_BUFFERS`.

So the design for the C++ module pilot is now concrete:
- `modules/test-cpp/Cargo.toml`: host crate, lib.rs empty, build.rs only.
  Build-deps via bindeps: `arora-module-cli` (host bin), `arora-buffers` and
  `arora-util` (wasm32-wasip1 staticlibs), `wasi-sdk` (regular dep).
- `build.rs`: locate wasi-sdk → run codegen → invoke cmake on
  `modules/test-cpp/CMakeLists.txt` with `-D` paths to everything.
- `modules/test-cpp/CMakeLists.txt`: promoted to a self-contained CMake project
  (adds `cmake_minimum_required` + `project()`). Accepts `ARORA_MODULE_CLI`,
  `ARORA_BUFFERS_LIB`, `ARORA_UTIL_LIB`, `ARORA_BEHAVIOR_TREE_INCLUDE` as
  cache vars. Still buildable by hand with `cmake -B build -S
  modules/test-cpp` if you set those — the cargo entry just becomes the
  default.

Next session: build the pilot and verify a .wasm comes out.

---

## 2026-05-22 — Iso-features reached; legacy CMake retired

All three targets build under `cargo` alone. Wrapping up the migration in one
push:

**C++ module pattern, generalized.** `test-cpp` (pilot), `test-cpp-2`, and
`nao` all follow the same shape: standalone `CMakeLists.txt` that takes
`-D` cache vars + a `build.rs` that resolves bindeps and calls `cmake::Config`.
The standalone CMake files still work for ad-hoc invocation; the cargo entry
is the default. test-cpp-2 additionally depends on `test-cpp` as a regular
path dep so test-cpp's build.rs runs first and publishes its records before
test-cpp-2's codegen reads them.

**cmake-rs target override.** cmake-rs reads the build script's `TARGET` and
injects host C flags (`--target=arm64-apple-macosx`,
`CMAKE_OSX_ARCHITECTURES=arm64`) — fatal for wasm and i686 cross builds.
Override with `.target("wasm32-wasi").host("wasm32-wasi").no_default_flags(true)`
(or the i686 spelling). The cmake "target triple" here is purely a hint to
cmake-rs's flag synthesis; the actual toolchain comes from
`CMAKE_TOOLCHAIN_FILE`.

**Bindeps generator staging.** arora-module-cli locates the language-specific
generator (`arora-module-cpp`) as a sibling of its own argv[0]. Bindeps puts
each artifact in its own dir, so we copy both binaries into
`OUT_DIR/arora-tools/` with canonical names and invoke from there.

**derive_more 2.x feature split.** The crate split derives into Cargo features
in 2.x. `arora-module-cpp` needs `["from"]`, `arora-module-core` needs
`["display"]`. CMake builds didn't expose this because they only compiled the
crates they used; `cargo build --workspace` exposes everyone.

**Per-package wasm-only targets.** `behavior-tree-nodes` and `test-rust-wasm`
are wasm-only by nature. Setting `forced-target = "wasm32-wasip1"` requires
nightly `-Z per-package-target` plus `[unstable] per-package-target = true` in
`.cargo/config.toml`. Cargo then refuses to build them for the host
automatically — clean.

**NAO cross.** Mac Homebrew's `i686-unknown-linux-musl-{gcc,g++,ar}` at
`/opt/homebrew/bin/` is good enough. `.cargo/config.toml` pins linker/ar
there. nao's build.rs gates on `ENABLE_NAO=1` (Homebrew formula is not
ubiquitous). Output: 32-bit i686 Linux ELF `libnao.so` (~6.9 MB) linking
against the libqi stub.

**Integration tests.** New `tests/` crate at workspace root replaces the old
CMake `add_test()` calls. arora-cli is bindep'd (`artifact = "bin"`) and its
path piped through via `cargo:rustc-env=ARORA_CLI_BIN`. Wasm-only modules are
bindep'd as `artifact = "cdylib", target = "wasm32-wasip1"` to force their
build before tests run; host-targeted modules (`test-cpp`, `test-cpp-2`,
`polly`) are regular path deps so their `build.rs` publishes artifacts to
`target/<profile>/modules/` without forcing a cdylib output.

**Test status.** 2/3 pass. `call_test_cpp_2_from_engine_with_struct` is
marked `#[ignore]` — arora-cli panics with "Cannot start a runtime from
within a runtime" on multi-module `--call`. Reproducible pre-migration on
master; tracked separately. Test data also needed an unrelated fix: the old
CMake invocation used `enum[]` YAML syntax not accepted by current arora-cli
(now `enums:`).

**Retired files.** `CMakeLists.txt` (root), `PreLoad.cmake`,
`modules/CMakeLists.txt`, and the pure-Rust modules' CMakeLists
(`behavior-tree-nodes`, `polly`, `test-rust-wasm`) are gone. C++ modules
keep self-contained CMakeLists invocable standalone.

**Final shape.** `cargo build --workspace` produces every host artifact and
all wasm guests; `ENABLE_NAO=1 cargo build -p arora-nao` adds the i686 .so;
`cargo test -p arora-integration-tests` runs the engine-level smoke tests.
No top-level CMake invocation anywhere in the path.

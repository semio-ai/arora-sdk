# Vizij Engine Backend Experiment

This branch set is a local integration experiment across three repositories:

- `engine`
- `vizij-rs`
- `vizij-web`

The branches are meant to be checked out together under one parent directory so
the Arora engine experiment can build against the matching Vizij Rust runtime
experiment and the web apps can prepare Arora browser assets from the engine
checkout.

## Recommended Checkout Layout

Use these sibling directory names:

```text
<workspace>/
  engine-vizij-backend-experiment/
  vizij-rs-vizij-engine-backend-experiment/
  vizij-web-vizij-engine-backend-experiment/
```

Each checkout should be on the branch:

```text
chris/vizij-engine-backend-experiment
```

This layout is not tied to Chris's machine. The parent directory can be any
path. What matters is that `engine-vizij-backend-experiment` and
`vizij-rs-vizij-engine-backend-experiment` are siblings.

## Why The Sibling Name Matters

The `engine` branch includes local Cargo path dependencies into the sibling
`vizij-rs` checkout:

```text
../../../vizij-rs-vizij-engine-backend-experiment
```

Those relative paths appear in the Vizij Arora module manifests:

- `modules/vizij-animation/Cargo.toml`
- `modules/vizij-node-graph/Cargo.toml`
- `modules/vizij-orchestrator/Cargo.toml`
- `modules/vizij-orchestrator-composed/Cargo.toml`

Keeping the recommended sibling layout lets Cargo resolve the local Vizij Rust
crates without editing manifests.

## If Your Checkout Names Are Different

Either rename your sibling checkout to:

```text
vizij-rs-vizij-engine-backend-experiment
```

or update the `path = ".../vizij-rs-vizij-engine-backend-experiment/..."`
entries in the four module `Cargo.toml` files listed above.

For example, if your `vizij-rs` checkout is named `vizij-rs`, change:

```toml
path = "../../../vizij-rs-vizij-engine-backend-experiment/crates/api/vizij-api-core"
```

to:

```toml
path = "../../../vizij-rs/crates/api/vizij-api-core"
```

Do not replace these with a machine-specific absolute path unless you are doing
a temporary private experiment.

## Web Asset Preparation

The `vizij-web` branch can build and copy Arora browser assets from the engine
checkout. From the `vizij-web-vizij-engine-backend-experiment` checkout, the
default asset script looks for a sibling checkout named
`engine-vizij-backend-experiment`:

```bash
pnpm --filter vizij-authoring prepare:arora-web
pnpm --filter demo-vizij-player prepare:arora-web
```

If your engine checkout has a different path, pass it explicitly:

```bash
ARORA_ENGINE_PATH=/path/to/engine-vizij-backend-experiment pnpm --filter vizij-authoring prepare:arora-web
ARORA_ENGINE_PATH=/path/to/engine-vizij-backend-experiment pnpm --filter demo-vizij-player prepare:arora-web
```

The same path can also be passed with:

```bash
node scripts/prepare-arora-web-assets.mjs --engine-root /path/to/engine-vizij-backend-experiment
```

## Smoke Test

From `engine-vizij-backend-experiment`, this verifies the branch set in the
recommended sibling layout:

```bash
cargo build \
  -p test-rust-wasm \
  -p vizij-animation \
  -p vizij-node-graph \
  -p vizij-orchestrator \
  -p vizij-orchestrator-composed \
  --target wasm32-wasip1 \
  --release

GECKODRIVER=$(which geckodriver) wasm-pack test --headless --firefox --release crates/arora-web
```

Expected result: the `arora-web` browser test runs four tests, including
`load_and_call_composed_vizij_orchestrator_wasm`, and all pass.

# polly

Speaks text aloud through AWS Polly, Amazon's text-to-speech service: the
module `polly` synthesizes the text and plays it on the host's audio output.
It is native code — it needs threads, a network stack and an audio library —
built as a shared library for the `native` executor, or linked into a host
and registered with `HostModule::of::<polly::polly::Module>()`.

Its interface is declared in Rust with
[`arora-module`](../../crates/arora-module/readme.md)'s macros
([`src/lib.rs`](src/lib.rs)); the header a host loads the library with is
`polly::polly::header(executor)`, the executor named `native`.

| Function | Signature |
|---|---|
| `say` | `(text: String) -> Status` |
| `hello_world` | `() -> Status` — says "Hello, world!" |

`say` is a long-running function: the first call starts the synthesis and
playback on a Tokio runtime and returns `Running`, each later call with the
same text polls it, and the call after it ends returns `Success` or `Failure`
([`docs/async-functions.md`](../../docs/async-functions.md)). Utterances are
keyed by their text.

The AWS credentials come from the environment, typically
`~/.aws/credentials`, and the region from it or `eu-west-3`. The tests in
[`tests/`](tests/polly_tests.rs) speak through a real account and speaker,
so they are `#[ignore]`d.

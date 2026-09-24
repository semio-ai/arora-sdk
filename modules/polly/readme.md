# AWS Polly module

This module contacts the AWS Text-to-Speech (TTS) service called "Polly".
It uses the AWS credentials from your environment,
typically from the file `~/.aws/credentials`.

It is meant to be built for the host: a `cdylib` the `native` executor loads.

The module declares its Arora interface in Rust, with
[`arora-module`](../../crates/arora-module/readme.md)'s macros — `say(text)`
and `hello_world()`, both returning a behavior `Status`. The header the engine
loads it with is written from that declaration.

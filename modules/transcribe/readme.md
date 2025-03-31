# AWS Polly module

This module contacts the AWS Text-to-Speech (TTS) service called "Polly".
It uses the AWS credentials from your environment,
typically from the file `~/.aws/credentials`.

It is meant to be built for the host.
It is triggered via the engine repository's `CMakeLists.txt`
with the proper CMake variables,
but it calls `cargo` build using your current environment.

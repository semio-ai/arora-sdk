# Changelog

All notable changes to `arora-util`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.1] - 2026-10-29

### Changed

- The build script generates the C header `arora/util.h` under its `OUT_DIR`,
  and no longer writes `target/include/arora/util.h` outside the package. The
  crate declares `links = "arora_util"`, which hands the header's directory to
  the build script of a crate with a regular dependency on it, as
  `DEP_ARORA_UTIL_INCLUDE`.

## [0.1.0] - 2022-01-22

- Release.

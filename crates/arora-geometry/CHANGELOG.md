# Changelog

All notable changes to `arora-geometry`. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow
[Semantic Versioning](https://semver.org/).

## [1.0.0] - 2026-10-23

First release. The geometry types over `arora_types::value::Value`, with
the ids, constructors and accessors `arora-shape` declared for them:
`vec2`, `vec3`, `vec4`, `quat` and `transform`, the plain `Transform`, and
`kind`. Blending (`blend::blend_values`, `blend::step_blend`) blends any
two values of the same form: numbers lerp, quaternions slerp, and any other
structures blend field by field, each field keeping its type, so an integer
field steps. Values without a blend, and values of different forms, step.

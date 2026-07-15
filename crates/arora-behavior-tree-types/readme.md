# Behavior Tree Types

This library provides the declaration of
the enumeration [`behavior_tree::Status`](src/status.rs)
and the structure [`behavior_tree::TickId`](src/tick_id.rs),
in the form of a [Semio Record](https://github.com/semio-ai/semio-record).
It is useful for [local code generation](https://github.com/semio-ai/arora-sdk),
or to feed a [local registry](https://github.com/semio-ai/arora-sdk).
These are basic types required to implement [behavior trees](../arora-behavior-tree/readme.md).

See [`arora-behavior-tree-types-yaml`](../arora-behavior-tree-types-yaml/readme.md)
for the serialized versions of these types.

# arora-behavior-tree-types

The two types a [behavior tree](../arora-behavior-tree/readme.md) and the
modules it calls share: `Status`, what a node's tick gives, and `TickId`, the
handle through which a control node a module provides ticks a child. Both are
records of the `arora` scope, `arora.Status` and `arora.TickId`.

## The types

- **`Status`**: `Success`, `Failure` or `Running`. It is defined in
  [`arora-behavior`](../arora-behavior/src/status.rs), which every behavior
  interpreter speaks, and re-exported here. A module function returning it
  is a tree leaf whose status the tree reads.
- **`TickId`** ([`src/tick_id.rs`](src/tick_id.rs)): `{ callable_id: u64 }`,
  one child of a control node. A module function whose first parameter is
  `children`, an array of `arora.TickId`, holds children in a tree: it
  receives one `TickId` per child, and ticks a child by calling its callable
  through indirect dispatch (`arora_dispatch_indirect` in a guest), which
  answers the child's `Status`.

Both derive [`AroraType`](../arora-types-derive/readme.md) under their
records' ids, so `Value::from` and `TryFrom<Value>` give and read the records'
value form.

## Using them

- **A module declared in Rust** names them as its functions' types:
  `fn seq(children: Vec<TickId>) -> Status`.
- **A `module.yaml`** names `arora.Status` and `arora.TickId`; the code
  generator resolves them from their record files, which
  [`arora-behavior-tree-types-yaml`](../arora-behavior-tree-types-yaml/readme.md)
  writes. The Rust it generates takes an array argument as an `Option`, as in
  the [`test-behavior-tree-nodes`](../../modules/test-behavior-tree-nodes/readme.md)
  module: `fn seq(children: Option<Vec<TickId>>) -> Status`.
- **A registry** takes their records from `declare_status_enumeration(parent)`
  and `declare_tick_id_structure(parent)`, under the ids and versions
  `STATUS_ENUMERATION_ID`, `STATUS_ENUMERATION_VERSION`,
  `TICK_ID_STRUCTURE_ID` and `TICK_ID_STRUCTURE_VERSION`, with `parent` the
  `arora` scope's id (`arora_types::record::scope::ARORA`).

`declare_behavior_tree_folder` and `BEHAVIOR_TREE_FOLDER_ID` are deprecated:
the types live in the `arora` scope, not in a `behavior_tree` folder.

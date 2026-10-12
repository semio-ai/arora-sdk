# blackboard

Behavior-tree leaves over the store's strings, for trees that switch on a request written
from outside (an editor's live data, a script, a terminal key, another device):

| Leaf | Signature | Does |
|---|---|---|
| `Equals` | `equals(value, expected) -> Status` | succeeds when `value` is `expected`, fails otherwise: a condition |
| `Assign` | `assign(value, target: &mut String) -> Status` | writes `value` into `target` and succeeds |

A `Sequence` headed by `Equals` runs its branch only while a key holds a value, and an
`Assign` at its end hands the request back:

```xml
<Sequence>
  <Equals value="{command.behavior}" expected="ready" />
  <!-- the branch -->
  <Assign value="idle" target="{command.behavior}" />
</Sequence>
```

The crate builds as a wasm guest (`cdylib`, for `with_declared_module`) and as a library
whose module a device registers in-process (`HostModule::of::<blackboard::blackboard::Module>()`).
A tree's out-parameter binds to a key that exists: seed `target` before the tree runs.

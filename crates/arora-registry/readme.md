# Registry

A package manager for Arora.
The registry is meant to be a server provided by the project `semio-db`.
A `RemoteRegistry` is a local handle to access the remote server,
using `semio-client` and `semio-record`.
It is read-only, *i.e.* it implements only the trait `ReadableRegistry`.

A `LocalRegistry` behaves similarly, but locally.
It is editable, *i.e.* it implements the trait `EditableRegistry`.
It supports the addition of `Structure`, `Enumeration` and `Module`
on the fly and provides a local index to look them up fast.

A `RemoteCachedRegistry` couples a `RemoteRegistry`
with a `LocalRegistry` used for caching every record queried remotely.

## Record versions

Programs using a registry should refer to records using version requirements,
if not explicit versions. Access to the latest non-tagged
(and therefore *unfrozen*) versions would result in potentially incompatible dependencies.

Use `EditableRegistry::tag_<record>` functions to produce tagged versions,
with dependencies frozen according to the existing tagged records.

Use `ReadableRegistry::get_<record>_tagged` functions to retrieve tagged versions.

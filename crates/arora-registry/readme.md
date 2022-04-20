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

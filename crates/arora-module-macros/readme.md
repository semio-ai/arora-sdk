# arora-module-macros

The proc-macros behind [`arora-module`](../arora-module/readme.md):
`#[module]`, `#[export]` / `#[param]`, `declare_module!` and
`module_from_header!`.

Depend on `arora-module`, not on this crate: the code these macros generate
reaches `arora-types` and `arora-buffers` through that facade.

# arora-id

How Arora spells identifiers. An identifier is a UUID; its canonical spelling
everywhere is the hex form. In Rust source, where ids are pinned by hand, the
same 128 bits may be spelled as thirteen emoji — base 1024 over a fixed
alphabet — so a declaration or a diff reads at a glance:

```text
e1b4bda7-1c7b-4322-b9a0-552201b8a011  ⇄  🎯🚤🧪💲🌼🏪🔘😊🉑🍉⚪🔕♍
```

`parse` accepts either spelling; `encode`/`decode` convert. The alphabet is
part of the encoding and never changes (see the crate docs for its
derivation).

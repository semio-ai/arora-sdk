//! Every error a declaration reports at compile time, as the compiler shows
//! it: one case per file under `compile-fail/`, beside the output it expects.
//! `TRYBUILD=overwrite cargo test -p arora-module --test compile_fail`
//! rewrites the expected output after a deliberate change.

#[test]
fn declaration_errors() {
    trybuild::TestCases::new().compile_fail("tests/compile-fail/*.rs");
}

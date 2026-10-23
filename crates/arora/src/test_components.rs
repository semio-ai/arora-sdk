//! Guest components for the device's tests, written in the text format: each
//! serves the `arora:module` world with a fixed answer per function, or loops
//! forever in one.

use uuid::Uuid;

/// What a function of a [`component`] does when called.
pub(crate) enum Answer {
    /// Returns these bytes, whatever the argument.
    Bytes(Vec<u8>),
    /// Loops forever: the call only ends when the host interrupts it.
    Spin,
}

/// A component serving `functions`, each answering as its [`Answer`] says.
pub(crate) fn component(functions: &[(Uuid, Answer)]) -> Vec<u8> {
    // Memory layout: the function ids from 1024, the `functions` result at
    // 512, the `dispatch` result at 768, the answers from 4096, and the
    // allocator's heap, which every call resets, from 32768.
    let mut data = String::new();
    let mut branches = String::new();
    let mut answer_at = 4096usize;
    for (index, (id, answer)) in functions.iter().enumerate() {
        let (hi, lo) = id.as_u64_pair();
        let id_at = 1024 + 16 * index;
        data += &format!(
            "(data (i32.const {id_at}) \"{}{}\")\n",
            escaped(&hi.to_le_bytes()),
            escaped(&lo.to_le_bytes())
        );
        let body = match answer {
            Answer::Bytes(bytes) => {
                data += &format!("(data (i32.const {answer_at}) \"{}\")\n", escaped(bytes));
                let body = format!(
                    "(i32.store8 (i32.const 768) (i32.const 0))
                     (i32.store (i32.const 772) (i32.const {answer_at}))
                     (i32.store (i32.const 776) (i32.const {len}))
                     (return (i32.const 768))",
                    len = bytes.len(),
                );
                answer_at += bytes.len().next_multiple_of(8);
                body
            }
            Answer::Spin => "(loop $forever (br $forever))".to_string(),
        };
        branches += &format!(
            "(if (i32.and (i64.eq (local.get 0) (i64.const {hi})) (i64.eq (local.get 1) (i64.const {lo})))
                (then {body}))\n",
            hi = hi as i64,
            lo = lo as i64,
        );
    }
    format!(
        r#"(component
            (type $types (instance
                (type (record (field "hi" u64) (field "lo" u64)))
                (export "id" (type (eq 0)))))
            (import "arora:module/types@0.2.0" (instance $types (type $types)))
            (alias export $types "id" (type $id))
            (core module $guest
                (memory (export "memory") 1)
                (global $heap (mut i32) (i32.const 32768))
                (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
                    (local $at i32)
                    (local.set $at (global.get $heap))
                    (global.set $heap (i32.add (global.get $heap) (local.get 3)))
                    (local.get $at))
                {data}
                (func (export "functions") (result i32)
                    (i32.store (i32.const 512) (i32.const 1024))
                    (i32.store (i32.const 516) (i32.const {count}))
                    (i32.const 512))
                (func (export "dispatch") (param i64 i64 i32 i32) (result i32)
                    (global.set $heap (i32.const 32768))
                    {branches}
                    ;; An unknown function: an empty answer.
                    (i32.store8 (i32.const 768) (i32.const 0))
                    (i32.store (i32.const 772) (i32.const 0))
                    (i32.store (i32.const 776) (i32.const 0))
                    (i32.const 768)))
            (core instance $main (instantiate $guest))
            (alias core export $main "memory" (core memory $memory))
            (alias core export $main "cabi_realloc" (core func $realloc))
            (type $ids (list $id))
            (type $functions (func (result $ids)))
            (func $functions (type $functions)
                (canon lift (core func $main "functions") (memory $memory)))
            (export "functions" (func $functions))
            (type $bytes (list u8))
            (type $result (result $bytes (error string)))
            (type $dispatch (func (param "method" $id) (param "arg" $bytes) (result $result)))
            (func $dispatch (type $dispatch)
                (canon lift (core func $main "dispatch") (memory $memory) (realloc $realloc)))
            (export "dispatch" (func $dispatch)))"#,
        count = functions.len(),
    )
    .into_bytes()
}

fn escaped(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

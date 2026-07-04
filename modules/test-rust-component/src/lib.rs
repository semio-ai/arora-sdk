//! Component-model test guest.
//!
//! Implements the `arora:module/module` world from `crates/arora-engine/wit/arora-module.wit`:
//! a single `dispatch` export routing by function id. Used by the
//! integration tests to exercise the `ComponentExecutor`.

wit_bindgen::generate!({
    world: "module",
    path: "../../crates/arora-engine/wit",
    additional_derives: [PartialEq],
});

/// `echo` function id: returns its argument buffer unchanged.
/// dd6a4f61-9f88-4376-9d50-1e92e95a73aa
const ECHO_FN: Id = Id {
    hi: 0xdd6a_4f61_9f88_4376,
    lo: 0x9d50_1e92_e95a_73aa,
};

/// `call-indirect` function id: interprets the first 8 bytes of the
/// argument as a little-endian callable id, invokes it through
/// `host.dispatch-indirect`, and returns the host's buffer.
/// 1bd1cf09-e94c-4c14-b653-32bd96157c41
const CALL_INDIRECT_FN: Id = Id {
    hi: 0x1bd1_cf09_e94c_4c14,
    lo: 0xb653_32bd_9615_7c41,
};

struct Component;

impl Guest for Component {
    fn dispatch(method: Id, arg: Vec<u8>) -> Result<Vec<u8>, String> {
        if method == ECHO_FN {
            return Ok(arg);
        }
        if method == CALL_INDIRECT_FN {
            let bytes: [u8; 8] = arg
                .get(..8)
                .and_then(|s| s.try_into().ok())
                .ok_or_else(|| "call-indirect needs an 8-byte callable id".to_string())?;
            return arora::module::host::dispatch_indirect(u64::from_le_bytes(bytes));
        }
        Err(format!(
            "unknown function {:016x}{:016x}",
            method.hi, method.lo
        ))
    }
}

export!(Component);

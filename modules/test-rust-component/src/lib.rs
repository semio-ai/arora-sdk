//! Component-model test guest.
//!
//! Implements the `arora:module/module` world from `crates/arora-engine/wit/arora-module.wit`
//! directly, with wit-bindgen: `functions` and a `dispatch` export routing by
//! function id. The integration tests drive it through the native
//! `ComponentExecutor`, and the engine's browser tests through
//! `BrowserComponentExecutor`, with the same expectations.

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

/// `wasi` function id: calls each WASI 0.2 interface an Arora host provides
/// and returns what it observed, as text, a `name=value` line each. A host
/// that lays out an argument or a result wrongly shows here as a wrong value,
/// a trap or an import it does not provide.
/// 0455052e-bb89-41ec-a963-2bbe82ea583b
const WASI_FN: Id = Id {
    hi: 0x0455_052e_bb89_41ec,
    lo: 0xa963_2bbe_82ea_583b,
};

/// `trap` function id: panics, which traps the guest.
/// 42681638-6fe3-4004-92fb-bb048dc3bbab
const TRAP_FN: Id = Id {
    hi: 0x4268_1638_6fe3_4004,
    lo: 0x92fb_bb04_8dc3_bbab,
};

/// `error-buffer` function id: returns, as its result, an error buffer whose
/// message is its argument — an error written as a core module writes one.
/// 2041d791-4440-4dc9-b54d-1004e314cf99
const ERROR_BUFFER_FN: Id = Id {
    hi: 0x2041_d791_4440_4dc9,
    lo: 0xb54d_1004_e314_cf99,
};

/// `call` function id: its argument is a module id, a function id (16 bytes
/// each) and the argument buffer to call that function with, through
/// `host.dispatch`; returns what the call returns.
/// 473a50b2-f275-4767-989c-69aa2c2def5b
const CALL_FN: Id = Id {
    hi: 0x473a_50b2_f275_4767,
    lo: 0x989c_69aa_2c2d_ef5b,
};

/// `spin` function id: loops forever, so a call to it only ends when the
/// host interrupts it.
/// 5d0c4a3e-7b21-4f6e-9a8d-2c41b6e0f3a1
const SPIN_FN: Id = Id {
    hi: 0x5d0c_4a3e_7b21_4f6e,
    lo: 0x9a8d_2c41_b6e0_f3a1,
};

/// `grow` function id: tries to reserve 64 MiB, growing its memory as it
/// must, and returns `[1]` if it could, `[0]` if the allocator could not grow
/// the memory.
/// 9e7b1c52-0d3a-4b8f-8e61-f4a2c9d7b053
const GROW_FN: Id = Id {
    hi: 0x9e7b_1c52_0d3a_4b8f,
    lo: 0x8e61_f4a2_c9d7_b053,
};

/// The module whose `spin` this guest's `functions` calls, when one is loaded
/// under this id, so that a test makes instantiating the guest call a guest
/// that loops. With none loaded the call fails at once, and the failure is
/// ignored.
/// 6f3a9c2e-1b84-4d07-a5e2-8c9d0b7f4e16
const LOAD_HOOK_MODULE: Id = Id {
    hi: 0x6f3a_9c2e_1b84_4d07,
    lo: 0xa5e2_8c9d_0b7f_4e16,
};

struct Component;

impl Guest for Component {
    fn functions() -> Vec<Id> {
        let _ = arora::module::host::dispatch(LOAD_HOOK_MODULE, SPIN_FN, &[]);
        vec![
            ECHO_FN,
            CALL_INDIRECT_FN,
            WASI_FN,
            TRAP_FN,
            ERROR_BUFFER_FN,
            CALL_FN,
            SPIN_FN,
            GROW_FN,
        ]
    }

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
        if method == TRAP_FN {
            panic!("trap requested");
        }
        if method == SPIN_FN {
            loop {
                std::hint::black_box(());
            }
        }
        if method == GROW_FN {
            let mut reserved = Vec::<u8>::new();
            return Ok(vec![reserved.try_reserve_exact(64 << 20).is_ok() as u8]);
        }
        if method == ERROR_BUFFER_FN {
            let mut writer = arora_buffers::BufferWriter::new();
            writer.add_error(&String::from_utf8_lossy(&arg));
            return Ok(writer.finalize().into());
        }
        if method == CALL_FN {
            let id = |bytes: &[u8]| Id {
                hi: u64::from_be_bytes(bytes[..8].try_into().unwrap()),
                lo: u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
            };
            if arg.len() < 32 {
                return Err("call needs a module id and a function id".to_string());
            }
            return arora::module::host::dispatch(id(&arg[..16]), id(&arg[16..32]), &arg[32..]);
        }
        #[cfg(target_os = "wasi")]
        if method == WASI_FN {
            return Ok(wasi::probe().into_bytes());
        }
        Err(format!(
            "unknown function {:016x}{:016x}",
            method.hi, method.lo
        ))
    }
}

export!(Component);

#[cfg(target_os = "wasi")]
mod wasi {
    use wasip2::{cli, clocks, filesystem, io, random};

    /// What the WASI interfaces return. Everything here holds on any host
    /// that runs a guest without arguments or an environment.
    pub fn probe() -> String {
        let mut report = Vec::new();
        let mut line = |name: &str, value: String| report.push(format!("{name}={value}"));

        let before = clocks::monotonic_clock::now();
        clocks::monotonic_clock::subscribe_duration(1_000).block();
        let after = clocks::monotonic_clock::now();
        line("monotonic-advances", (after >= before).to_string());
        line(
            "monotonic-resolution-positive",
            (clocks::monotonic_clock::resolution() > 0).to_string(),
        );
        let wall = clocks::wall_clock::now();
        // 2020-01-01T00:00:00Z.
        line(
            "wall-after-2020",
            (wall.seconds > 1_577_836_800).to_string(),
        );
        line(
            "wall-nanoseconds-below-a-second",
            (wall.nanoseconds < 1_000_000_000).to_string(),
        );
        let resolution = clocks::wall_clock::resolution();
        line(
            "wall-resolution-positive",
            (resolution.seconds > 0 || resolution.nanoseconds > 0).to_string(),
        );

        line(
            "random-bytes",
            random::random::get_random_bytes(40).len().to_string(),
        );
        random::random::get_random_u64();
        line(
            "insecure-random-bytes",
            random::insecure::get_insecure_random_bytes(3)
                .len()
                .to_string(),
        );
        random::insecure::get_insecure_random_u64();
        random::insecure_seed::insecure_seed();

        line(
            "environment",
            cli::environment::get_environment().len().to_string(),
        );
        line(
            "arguments",
            cli::environment::get_arguments().len().to_string(),
        );
        line(
            "initial-cwd",
            format!("{:?}", cli::environment::initial_cwd()),
        );
        line(
            "preopened-directories",
            filesystem::preopens::get_directories().len().to_string(),
        );

        let stdout = cli::stdout::get_stdout();
        line(
            "stdout-writable",
            stdout.check_write().is_ok_and(|n| n > 0).to_string(),
        );
        line("stdout-flushes", stdout.flush().is_ok().to_string());
        line(
            "stdout-writes-zeroes",
            stdout.write_zeroes(0).is_ok().to_string(),
        );
        let wrote = stdout
            .blocking_write_and_flush(b"test-rust-component: stdout\n")
            .and_then(|()| {
                cli::stderr::get_stderr().blocking_write_and_flush(b"test-rust-component: stderr\n")
            });
        line("standard-streams-written", wrote.is_ok().to_string());
        cli::terminal_stdin::get_terminal_stdin();
        cli::terminal_stdout::get_terminal_stdout();
        cli::terminal_stderr::get_terminal_stderr();

        // Standard output is always ready to write.
        let ready = io::poll::poll(&[&stdout.subscribe(), &stdout.subscribe()]);
        line("poll-ready", format!("{ready:?}"));
        line("stdout-ready", stdout.subscribe().ready().to_string());
        // Waiting for an instant long past, the clock's zero, returns.
        clocks::monotonic_clock::subscribe_instant(0).block();
        line("past-instant-waited", true.to_string());

        report.join("\n")
    }
}

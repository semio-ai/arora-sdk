//! What an Arora guest built as a component is made of, and what it may import.
//!
//! A component built for `wasm32-wasip2` — by Rust, or by wasi-sdk through
//! `wasm-component-ld` — holds one core module with the guest's code, and the
//! modules `wit-component` adds around it:
//!
//! - when the guest imports a function whose canonical-ABI form writes into
//!   the guest's memory, a **shim** of indirect calls through a table it
//!   exports as `$imports`, and the **fixups** that fill the table once that
//!   memory exists, which export nothing;
//! - when the guest exports `_initialize` (a WASI reactor, as a C or C++
//!   guest is), an **initializer**: a module exporting nothing whose start
//!   function calls it when the component is instantiated.
//!
//! [`guest_module`] finds the guest's module in such a component and refuses
//! any other layout. The browser's component executor instantiates that module
//! alone, so a component it cannot run is one this refuses; the guests'
//! parity test holds every guest the repository builds to it.

/// Whether `bytes` is a component rather than a core module: the layer field
/// of the preamble, after the magic and the version, is 1.
pub fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == *b"\0asm" && bytes[6..8] == [1, 0]
}

/// The guest's core module in a component, and whether the component calls
/// the guest's `_initialize` when it is instantiated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestModule<'a> {
    pub bytes: &'a [u8],
    pub initialize: bool,
}

/// The guest's core module in `component`: the one module that is none of the
/// modules `wit-component` adds. Fails, saying why, for a component laid out
/// otherwise — more than one guest module, a shim without fixups, more than
/// one initializer, a nested component, a component-level start function — or
/// for bytes that are not a component.
pub fn guest_module(component: &[u8]) -> Result<GuestModule<'_>, String> {
    let refuse = |why: &str| {
        format!(
            "a component an Arora host runs holds one guest core module, as wasm32-wasip2 \
             toolchains emit it: {why}"
        )
    };
    if !is_component(component) {
        return Err(refuse("this is not a component"));
    }
    let mut guests = Vec::new();
    let (mut shims, mut fixups, mut initializers) = (0, 0, 0);
    for section in Sections::new(&component[8..]) {
        let (id, contents) = section.map_err(refuse)?;
        match id {
            CORE_MODULE_SECTION => match core_role(contents).map_err(refuse)? {
                CoreRole::Guest => guests.push(contents),
                CoreRole::Shim => shims += 1,
                CoreRole::Fixups => fixups += 1,
                CoreRole::Initializer => initializers += 1,
            },
            COMPONENT_SECTION => return Err(refuse("it nests a component")),
            START_SECTION => return Err(refuse("it has a start function")),
            _ => {}
        }
    }
    if guests.len() != 1 || shims > 1 || fixups > 1 || shims != fixups || initializers > 1 {
        return Err(refuse(&format!(
            "it holds {} guest module(s), {shims} shim(s), {fixups} fixups module(s) and \
             {initializers} initializer(s)",
            guests.len()
        )));
    }
    Ok(GuestModule {
        bytes: guests[0],
        initialize: initializers == 1,
    })
}

/// The interfaces every Arora host provides a component, as
/// `namespace:package/interface`: the `arora:module` world's, and WASI 0.2's
/// `cli`, `io`, `clocks` and `random` — no filesystem, no sockets. A
/// component's import names one with a version, which [`provided`] checks.
/// The native component executor links each of them (`add_wasi_to_linker`
/// and the world's bindings), and the browser's implements their functions: a
/// change here is a change to both.
pub const HOST_INTERFACES: &[&str] = &[
    "arora:module/types",
    "arora:module/host",
    "wasi:cli/environment",
    "wasi:cli/exit",
    "wasi:cli/stdin",
    "wasi:cli/stdout",
    "wasi:cli/stderr",
    "wasi:cli/terminal-input",
    "wasi:cli/terminal-output",
    "wasi:cli/terminal-stdin",
    "wasi:cli/terminal-stdout",
    "wasi:cli/terminal-stderr",
    "wasi:clocks/monotonic-clock",
    "wasi:clocks/wall-clock",
    "wasi:io/error",
    "wasi:io/poll",
    "wasi:io/streams",
    "wasi:random/random",
    "wasi:random/insecure",
    "wasi:random/insecure-seed",
];

/// Whether every Arora host provides the interface `name`
/// (`namespace:package/interface@version`): one of [`HOST_INTERFACES`], at a
/// version compatible with the one the hosts implement — WASI 0.2, and the
/// `arora:module` world's 0.2.
pub fn provided(name: &str) -> bool {
    let Some((interface, version)) = name.split_once('@') else {
        return false;
    };
    // WASI's version and the world's are both 0.2.
    version.starts_with("0.2.") && HOST_INTERFACES.contains(&interface)
}

const CORE_MODULE_SECTION: u8 = 1;
const COMPONENT_SECTION: u8 = 4;
const START_SECTION: u8 = 9;
const CORE_EXPORT_SECTION: u8 = 7;
const CORE_START_SECTION: u8 = 8;

/// What a core module embedded in a component is there for.
enum CoreRole {
    Guest,
    Shim,
    Fixups,
    Initializer,
}

/// A module exporting `$imports` is the shim; any other module exporting
/// something is the guest. Of the modules exporting nothing, one with a start
/// function is the initializer — whichever export of the guest it calls, the
/// guest's `_initialize` is the one `wit-component` calls at instantiation —
/// and one without is the fixups.
fn core_role(module: &[u8]) -> Result<CoreRole, &'static str> {
    if module.len() < 8 || module[..4] != *b"\0asm" {
        return Err("a core module section does not hold a core module");
    }
    let (mut exports, mut starts) = (Vec::new(), false);
    for section in Sections::new(&module[8..]) {
        let (id, contents) = section?;
        match id {
            CORE_EXPORT_SECTION => exports = export_names(contents)?,
            CORE_START_SECTION => starts = true,
            _ => {}
        }
    }
    Ok(if exports.iter().any(|name| name == "$imports") {
        CoreRole::Shim
    } else if !exports.is_empty() {
        CoreRole::Guest
    } else if starts {
        CoreRole::Initializer
    } else {
        CoreRole::Fixups
    })
}

fn export_names(mut section: &[u8]) -> Result<Vec<String>, &'static str> {
    let count = leb_u32(&mut section)?;
    let mut names = Vec::new();
    for _ in 0..count {
        let len = leb_u32(&mut section)? as usize;
        let name = section
            .get(..len)
            .ok_or("an export name runs past its section")?;
        names.push(String::from_utf8_lossy(name).into_owned());
        section = &section[len..];
        // The export's kind, then its index.
        section = section.get(1..).ok_or("an export runs past its section")?;
        leb_u32(&mut section)?;
    }
    Ok(names)
}

/// The sections of a module or component, after its preamble: each an id and
/// its contents.
struct Sections<'a> {
    rest: &'a [u8],
}

impl<'a> Sections<'a> {
    fn new(rest: &'a [u8]) -> Self {
        Self { rest }
    }
}

impl<'a> Iterator for Sections<'a> {
    type Item = Result<(u8, &'a [u8]), &'static str>;

    fn next(&mut self) -> Option<Self::Item> {
        let (&id, mut rest) = self.rest.split_first()?;
        let section = leb_u32(&mut rest).and_then(|len| {
            let len = len as usize;
            let contents = rest.get(..len).ok_or("a section runs past the end")?;
            Ok((contents, &rest[len..]))
        });
        Some(match section {
            Ok((contents, rest)) => {
                self.rest = rest;
                Ok((id, contents))
            }
            Err(e) => {
                self.rest = &[];
                Err(e)
            }
        })
    }
}

fn leb_u32(bytes: &mut &[u8]) -> Result<u32, &'static str> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let (&byte, rest) = bytes.split_first().ok_or("a number runs past the end")?;
        *bytes = rest;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("a number is longer than 32 bits")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const GUEST: &str = r#"(core module (memory (export "memory") 1))"#;
    const SHIM: &str = r#"(core module (table (export "$imports") 1 1 funcref))"#;
    const FIXUPS: &str = r#"(core module (import "" "$imports" (table 1 1 funcref)))"#;
    const INITIALIZER: &str = r#"(core module (import "" "" (func $f)) (start $f))"#;

    fn layout(modules: &[&str]) -> Result<bool, String> {
        let component = wat::parse_str(format!("(component {})", modules.join(" "))).unwrap();
        guest_module(&component).map(|guest| guest.initialize)
    }

    #[test]
    fn a_guest_alone_or_with_wit_component_s_modules_is_run() {
        assert_eq!(layout(&[GUEST]), Ok(false));
        assert_eq!(layout(&[GUEST, SHIM, FIXUPS]), Ok(false));
        assert_eq!(layout(&[GUEST, SHIM, FIXUPS, INITIALIZER]), Ok(true));
        assert_eq!(layout(&[GUEST, INITIALIZER]), Ok(true));
    }

    #[test]
    fn a_component_laid_out_otherwise_is_refused() {
        for modules in [
            &[GUEST, GUEST][..],
            &[SHIM, FIXUPS],
            &[GUEST, SHIM],
            &[GUEST, FIXUPS],
            &[GUEST, SHIM, FIXUPS, SHIM, FIXUPS],
            &[GUEST, INITIALIZER, INITIALIZER],
        ] {
            assert!(layout(modules).is_err(), "{modules:?}");
        }
        let nested = wat::parse_str(format!("(component {GUEST} (component))")).unwrap();
        assert!(guest_module(&nested)
            .unwrap_err()
            .contains("nests a component"));
        let core = wat::parse_str("(module)").unwrap();
        assert!(guest_module(&core).unwrap_err().contains("not a component"));
    }

    #[test]
    fn only_the_hosts_interfaces_at_their_versions_are_provided() {
        assert!(provided("wasi:io/streams@0.2.0"));
        assert!(provided("wasi:random/insecure-seed@0.2.9"));
        assert!(provided("arora:module/host@0.2.0"));
        assert!(!provided("arora:module/host@0.1.0"));
        assert!(!provided("wasi:io/streams@0.3.0"));
        assert!(!provided("wasi:filesystem/types@0.2.0"));
        assert!(!provided("wasi:sockets/tcp@0.2.0"));
        assert!(!provided("id"));
    }
}

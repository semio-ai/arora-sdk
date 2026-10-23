//! What an Arora guest built as a component is made of, and what it may import.
//!
//! A component built for `wasm32-wasip2` — by Rust, or by wasi-sdk through
//! `wasm-component-ld` — holds one core module with the guest's code, and the
//! modules `wit-component` adds around it:
//!
//! - when the guest imports a function whose canonical-ABI form writes into
//!   the guest's memory, a **shim** of indirect calls through a table it
//!   exports as `$imports`, importing nothing, and the **fixups** that fill
//!   the table once that memory exists, which export nothing and import only
//!   functions and that table;
//! - when the guest exports `_initialize` (a WASI reactor, as a C or C++
//!   guest is), an **initializer**: a module exporting nothing whose start
//!   function is its one import, which the component instantiates once,
//!   giving it the guest's `_initialize`, so it runs when the component is
//!   instantiated.
//!
//! Around those modules, the component implements the `arora:module` world:
//! it imports interfaces every Arora host provides ([`HOST_INTERFACES`]), and
//! exports `functions` and `dispatch` with the world's types, each lifting the
//! guest module's export of the same name with the guest's memory, its
//! `cabi_realloc`, UTF-8 strings and its `cabi_post_` function.
//!
//! [`guest_module`] finds the guest's module in such a component and refuses
//! any other. The browser's component executor instantiates that module alone,
//! runs its `_initialize` when the component has an initializer, and calls its
//! exports in their canonical-ABI form; the component's own layer is to do no
//! more and no other than that, as far as this reads it. The guests' parity
//! test holds every guest the repository builds to it.
//!
//! It reads the component's imports, exports and their types, the wiring of
//! its exports, and what its other modules are given; it does not validate the
//! rest of its binary as wasmtime does, so a component malformed elsewhere,
//! which no toolchain emits, runs in the browser as this reads it and is
//! refused natively. Nor does it check how the guest module's own imports are
//! wired: the browser implements each by the interface function its name
//! gives, of those a host provides, whatever the component gives it.

use std::cell::Cell;

use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport,
    ComponentExternalKind, ComponentFuncType, ComponentImport, ComponentOuterAliasKind,
    ComponentType, ComponentTypeRef, ComponentValType, ExternalKind, Instance,
    InstanceTypeDeclaration, Parser, Payload, PrimitiveValType, TypeBounds, TypeRef,
};

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
/// one initializer, a nested component, a component-level start function —
/// for one that does not implement the world as the module's description
/// says, and for bytes that are not a component.
pub fn guest_module(component: &[u8]) -> Result<GuestModule<'_>, String> {
    let refuse = |why: String| {
        format!(
            "a component an Arora host runs implements the arora:module world around one guest \
             core module, as wasm32-wasip2 toolchains emit it: {why}"
        )
    };
    if !is_component(component) {
        return Err(refuse("this is not a component".to_string()));
    }
    Outer::read(component)
        .and_then(|outer| outer.guest())
        .map_err(refuse)
}

/// The interfaces every Arora host provides a component, as
/// `namespace:package/interface`: the `arora:module` world's, and WASI 0.2's
/// `cli`, `io`, `clocks`, `random` and `filesystem` — a filesystem with no
/// directory preopened, so a guest sees no file — and no sockets. A
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
    "wasi:filesystem/types",
    "wasi:filesystem/preopens",
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

/// What a core module embedded in a component is there for.
#[derive(Clone, Copy, PartialEq)]
enum CoreRole {
    Guest,
    Shim,
    Fixups,
    Initializer,
}

/// A core module the component defines.
struct CoreModule<'a> {
    bytes: &'a [u8],
    role: CoreRole,
    exports: Vec<&'a str>,
    /// Its imports: each its module, name and kind.
    imports: Vec<(&'a str, &'a str, ExternalKind)>,
    /// The function its start section runs.
    start: Option<u32>,
}

/// A module exporting `$imports` is the shim; any other module exporting
/// something is the guest. Of the modules exporting nothing, one with a start
/// function is the initializer, which the component is to instantiate with
/// the guest's `_initialize` as that function ([`Outer::check_initializer`]),
/// and one without is the fixups.
fn core_module(bytes: &[u8]) -> Result<CoreModule<'_>, String> {
    let (mut exports, mut imports, mut start) = (Vec::new(), Vec::new(), None);
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|e| format!("a core module does not read: {e}"))? {
            Payload::ExportSection(section) => {
                for export in section {
                    exports.push(export.map_err(|e| e.to_string())?.name);
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import = import.map_err(|e| e.to_string())?;
                    let kind = match import.ty {
                        TypeRef::Func(_) => ExternalKind::Func,
                        TypeRef::Table(_) => ExternalKind::Table,
                        TypeRef::Memory(_) => ExternalKind::Memory,
                        TypeRef::Global(_) => ExternalKind::Global,
                        _ => ExternalKind::Tag,
                    };
                    imports.push((import.module, import.name, kind));
                }
            }
            Payload::StartSection { func, .. } => start = Some(func),
            _ => {}
        }
    }
    let starts = start.is_some();
    let role = if exports.contains(&"$imports") {
        CoreRole::Shim
    } else if !exports.is_empty() {
        CoreRole::Guest
    } else if starts {
        CoreRole::Initializer
    } else {
        CoreRole::Fixups
    };
    Ok(CoreModule {
        bytes,
        role,
        imports,
        start,
        exports,
    })
}

/// A core instance of the component.
enum CoreInstance<'a> {
    /// A module's instance, and its arguments: each a name and the core
    /// instance given under it.
    Instantiate {
        module: u32,
        args: Vec<(&'a str, u32)>,
    },
    /// Core items gathered under names: each its name, kind and index.
    FromExports(Vec<(&'a str, ExternalKind, u32)>),
}

/// Where a core function, memory or table of the component comes from.
#[derive(Clone, Copy, PartialEq)]
enum CoreItem<'a> {
    /// The export of a core instance under a name.
    Export(u32, &'a str),
    /// A lowered or built-in function.
    Other,
}

/// What a function of the component is.
#[derive(Clone)]
enum Func {
    /// A core function lifted with a type and options.
    Lift {
        core_func: u32,
        ty: u32,
        options: Box<[CanonicalOption]>,
    },
    /// An import's function.
    Other,
}

/// A type of the component, or of an instance type, as far as the world's
/// functions name types.
enum Ty<'a> {
    Defined(ComponentDefinedType<'a>),
    Func(ComponentFuncType<'a>),
    Instance(Box<[InstanceTypeDeclaration<'a>]>),
    /// The type at an earlier index of the same scope: one exported, or
    /// imported with an equality bound.
    Same(u32),
    /// A type of the enclosing scope.
    Outer(u32),
    /// The type an instance exports under a name.
    InstanceExport(u32, &'a str),
    Other,
}

impl<'a> From<ComponentType<'a>> for Ty<'a> {
    fn from(ty: ComponentType<'a>) -> Self {
        match ty {
            ComponentType::Defined(defined) => Ty::Defined(defined),
            ComponentType::Func(func) => Ty::Func(func),
            ComponentType::Instance(declarations) => Ty::Instance(declarations),
            ComponentType::Component(_) | ComponentType::Resource { .. } => Ty::Other,
        }
    }
}

/// A value type, resolved: the forms the world's functions are made of.
#[derive(Debug, PartialEq)]
enum Val {
    Primitive(PrimitiveValType),
    Record(Vec<(String, Val)>),
    List(Box<Val>),
    Result(Option<Box<Val>>, Option<Box<Val>>),
}

/// A function type, resolved.
#[derive(Debug, PartialEq)]
struct Signature {
    params: Vec<Val>,
    result: Option<Val>,
}

/// The types of the world's exports: `functions: func() -> list<id>` and
/// `dispatch: func(method: id, arg: list<u8>) -> result<list<u8>, string>`,
/// an `id` being `record { hi: u64, lo: u64 }`.
fn world_signature(export: &str) -> Signature {
    use PrimitiveValType::{String, U64, U8};
    let id = || {
        Val::Record(vec![
            ("hi".into(), Val::Primitive(U64)),
            ("lo".into(), Val::Primitive(U64)),
        ])
    };
    let bytes = || Val::List(Box::new(Val::Primitive(U8)));
    if export == "functions" {
        Signature {
            params: vec![],
            result: Some(Val::List(Box::new(id()))),
        }
    } else {
        Signature {
            params: vec![id(), bytes()],
            result: Some(Val::Result(
                Some(Box::new(bytes())),
                Some(Box::new(Val::Primitive(String))),
            )),
        }
    }
}

/// How deep a type may refer through aliases and instance types; the world's
/// go three deep.
const TYPE_DEPTH: u32 = 16;

/// How many types resolving the exports' types may visit in all; the world's
/// visit a dozen. Types may share, so depth alone does not bound the work.
const TYPE_BUDGET: u32 = 10_000;

/// A component's own layer: its index spaces, as far as the checks read them.
#[derive(Default)]
struct Outer<'a> {
    /// The core modules, each one the component defines: it aliases none.
    modules: Vec<CoreModule<'a>>,
    core_instances: Vec<CoreInstance<'a>>,
    core_funcs: Vec<CoreItem<'a>>,
    core_memories: Vec<CoreItem<'a>>,
    core_tables: Vec<CoreItem<'a>>,
    funcs: Vec<Func>,
    types: Vec<Ty<'a>>,
    /// The instances: an imported one's type.
    instances: Vec<Option<u32>>,
    /// The functions exported, by name.
    exports: Vec<(&'a str, u32)>,
    /// How many more types resolving the exports' types may visit.
    type_budget: Cell<u32>,
}

impl<'a> Outer<'a> {
    fn read(component: &'a [u8]) -> Result<Self, String> {
        let mut outer = Outer {
            type_budget: Cell::new(TYPE_BUDGET),
            ..Outer::default()
        };
        let read_error = |e: wasmparser::BinaryReaderError| format!("it does not read: {e}");
        // The core modules' own payloads come between their section and their
        // end; none of them is the component's.
        let mut in_module = false;
        for payload in Parser::new(0).parse_all(component) {
            let payload = payload.map_err(read_error)?;
            if in_module {
                in_module = !matches!(payload, Payload::End(_));
                continue;
            }
            match payload {
                Payload::ModuleSection {
                    unchecked_range, ..
                } => {
                    let (start, end) = (unchecked_range.start, unchecked_range.end);
                    let module = core_module(&component[start as usize..end as usize])?;
                    outer.modules.push(module);
                    in_module = true;
                }
                Payload::ComponentSection { .. } => return Err("it nests a component".into()),
                Payload::ComponentStartSection { .. } => {
                    return Err("it has a start function".into())
                }
                Payload::ComponentInstanceSection(_) => {
                    return Err("it defines a component instance".into())
                }
                Payload::InstanceSection(section) => {
                    for instance in section {
                        outer
                            .core_instances
                            .push(match instance.map_err(read_error)? {
                                Instance::Instantiate { module_index, args } => {
                                    CoreInstance::Instantiate {
                                        module: module_index,
                                        args: args.iter().map(|a| (a.name, a.index)).collect(),
                                    }
                                }
                                Instance::FromExports(exports) => CoreInstance::FromExports(
                                    exports.iter().map(|e| (e.name, e.kind, e.index)).collect(),
                                ),
                            });
                    }
                }
                Payload::ComponentAliasSection(section) => {
                    for alias in section {
                        outer.alias(alias.map_err(read_error)?)?;
                    }
                }
                Payload::ComponentCanonicalSection(section) => {
                    for function in section {
                        match function.map_err(read_error)? {
                            CanonicalFunction::Lift {
                                core_func_index,
                                type_index,
                                options,
                            } => outer.funcs.push(Func::Lift {
                                core_func: core_func_index,
                                ty: type_index,
                                options,
                            }),
                            // Every other canonical function is a core one.
                            _ => outer.core_funcs.push(CoreItem::Other),
                        }
                    }
                }
                Payload::ComponentTypeSection(section) => {
                    for ty in section {
                        outer.types.push(ty.map_err(read_error)?.into());
                    }
                }
                Payload::ComponentImportSection(section) => {
                    for import in section {
                        outer.import(import.map_err(read_error)?)?;
                    }
                }
                Payload::ComponentExportSection(section) => {
                    for export in section {
                        outer.export(export.map_err(read_error)?)?;
                    }
                }
                _ => {}
            }
        }
        Ok(outer)
    }

    /// A module aliased, rather than defined, is refused: it would instantiate
    /// without a role, as the guest's twin for one.
    fn alias(&mut self, alias: ComponentAlias<'a>) -> Result<(), String> {
        match alias {
            ComponentAlias::CoreInstanceExport {
                kind,
                instance_index,
                name,
            } => match kind {
                ExternalKind::Func => self.core_funcs.push(CoreItem::Export(instance_index, name)),
                ExternalKind::Memory => self
                    .core_memories
                    .push(CoreItem::Export(instance_index, name)),
                ExternalKind::Table => self
                    .core_tables
                    .push(CoreItem::Export(instance_index, name)),
                _ => {}
            },
            ComponentAlias::InstanceExport {
                kind,
                instance_index,
                name,
            } => match kind {
                ComponentExternalKind::Func => self.funcs.push(Func::Other),
                ComponentExternalKind::Type => {
                    self.types.push(Ty::InstanceExport(instance_index, name))
                }
                ComponentExternalKind::Instance => self.instances.push(None),
                ComponentExternalKind::Module => return Err("it aliases a core module".into()),
                ComponentExternalKind::Component | ComponentExternalKind::Value => {}
            },
            ComponentAlias::Outer { kind, .. } => match kind {
                ComponentOuterAliasKind::Type => self.types.push(Ty::Other),
                ComponentOuterAliasKind::CoreModule => {
                    return Err("it aliases a core module".into())
                }
                ComponentOuterAliasKind::CoreType | ComponentOuterAliasKind::Component => {}
            },
        }
        Ok(())
    }

    /// An interface every host provides, or a type equal to one the
    /// component has: an import of anything else is refused.
    fn import(&mut self, import: ComponentImport<'a>) -> Result<(), String> {
        let name = import.name.full_name();
        match import.ty {
            ComponentTypeRef::Instance(ty)
                if import.name.implements.is_none() && provided(&name) =>
            {
                self.instances.push(Some(ty));
                Ok(())
            }
            ComponentTypeRef::Type(TypeBounds::Eq(ty)) => {
                self.types.push(Ty::Same(ty));
                Ok(())
            }
            _ => Err(format!(
                "it imports {name}, which is not an interface every Arora host provides"
            )),
        }
    }

    fn export(&mut self, export: ComponentExport<'a>) -> Result<(), String> {
        let name = export.name.name;
        if export.kind != ComponentExternalKind::Func {
            return Err(format!("it exports {name}, which is not a function"));
        }
        let func = self
            .funcs
            .get(export.index as usize)
            .cloned()
            .ok_or_else(|| format!("its export {name} names no function"))?;
        // An export is a new index of its kind.
        self.funcs.push(func);
        self.exports.push((name, export.index));
        Ok(())
    }

    /// The guest module, once the layout and the world's exports check.
    fn guest(&self) -> Result<GuestModule<'a>, String> {
        let defined = || self.modules.iter().enumerate();
        let count = |role| defined().filter(|(_, m)| m.role == role).count();
        let (guests, shims, fixups, initializers) = (
            count(CoreRole::Guest),
            count(CoreRole::Shim),
            count(CoreRole::Fixups),
            count(CoreRole::Initializer),
        );
        if guests != 1 || shims > 1 || fixups > 1 || shims != fixups || initializers > 1 {
            return Err(format!(
                "it holds {guests} guest module(s), {shims} shim(s), {fixups} fixups module(s) \
                 and {initializers} initializer(s)"
            ));
        }
        let (index, guest) = defined()
            .find(|(_, m)| m.role == CoreRole::Guest)
            .expect("one guest module");

        let instances = self.instantiations(index);
        let [(instance, _)] = instances[..] else {
            return Err(format!(
                "it instantiates its guest module {} times",
                instances.len()
            ));
        };

        let mut names: Vec<&str> = self.exports.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        if names != ["dispatch", "functions"] {
            return Err(format!(
                "it exports {names:?}, where the world exports functions and dispatch"
            ));
        }
        for &(name, func) in &self.exports {
            self.check_export(name, func, instance, guest)?;
        }
        let shim = defined().find(|(_, m)| m.role == CoreRole::Shim);
        let fixups = defined().find(|(_, m)| m.role == CoreRole::Fixups);
        if let (Some(shim), Some(fixups)) = (shim, fixups) {
            self.check_shim(shim, fixups)?;
        }
        let initializer = defined().find(|(_, m)| m.role == CoreRole::Initializer);
        if let Some((index, initializer)) = initializer {
            self.check_initializer(index, initializer, instance, guest)?;
        }
        Ok(GuestModule {
            bytes: guest.bytes,
            initialize: initializer.is_some(),
        })
    }

    /// The instances of the module at `index`, each with its arguments.
    fn instantiations(&self, index: usize) -> Vec<(u32, &[(&'a str, u32)])> {
        (0..self.core_instances.len() as u32)
            .filter_map(|i| match &self.core_instances[i as usize] {
                CoreInstance::Instantiate { module, args } if *module as usize == index => {
                    Some((i, &args[..]))
                }
                _ => None,
            })
            .collect()
    }

    /// What the instance made with `args` is given for the import
    /// `module`/`name` of `kind`: the export of a module instance passed whole,
    /// or the item an instance of exports gathers under that name.
    fn given(
        &self,
        args: &[(&'a str, u32)],
        module: &str,
        name: &'a str,
        kind: ExternalKind,
    ) -> Option<CoreItem<'a>> {
        let &(_, instance) = args.iter().find(|(arg, _)| *arg == module)?;
        match self.core_instances.get(instance as usize)? {
            CoreInstance::Instantiate { .. } => Some(CoreItem::Export(instance, name)),
            CoreInstance::FromExports(exports) => {
                let &(_, found, index) = exports.iter().find(|(export, _, _)| *export == name)?;
                let space = match kind {
                    ExternalKind::Func => &self.core_funcs,
                    ExternalKind::Table => &self.core_tables,
                    ExternalKind::Memory => &self.core_memories,
                    _ => return None,
                };
                (found == kind).then(|| space.get(index as usize).copied())?
            }
        }
    }

    /// The shim and the fixups touch nothing of the guest's: the shim imports
    /// nothing and starts nothing; the fixups, instantiated once, import only
    /// functions and the shim instance's `$imports` table, which they fill.
    /// So neither changes the guest's memory or tables, which the browser,
    /// running neither, would not see changed.
    fn check_shim(
        &self,
        (shim_index, shim): (usize, &CoreModule<'a>),
        (fixups_index, fixups): (usize, &CoreModule<'a>),
    ) -> Result<(), String> {
        let refuse = || {
            "its shim and fixups modules do more than wit-component's, which fill the shim's \
             table"
                .to_string()
        };
        if !shim.imports.is_empty() || shim.start.is_some() {
            return Err(refuse());
        }
        let [(shim_instance, _)] = self.instantiations(shim_index)[..] else {
            return Err(refuse());
        };
        let [(_, args)] = self.instantiations(fixups_index)[..] else {
            return Err(refuse());
        };
        let table = Some(CoreItem::Export(shim_instance, "$imports"));
        for &(module, name, kind) in &fixups.imports {
            let wired = match kind {
                ExternalKind::Func => true,
                ExternalKind::Table => self.given(args, module, name, kind) == table,
                _ => false,
            };
            if !wired {
                return Err(refuse());
            }
        }
        Ok(())
    }

    /// The initializer runs the guest instance's `_initialize` once, as the
    /// browser does: instantiated once, its one import — the function its
    /// start runs — given the guest instance's `_initialize`, which the guest
    /// exports.
    fn check_initializer(
        &self,
        index: usize,
        initializer: &CoreModule<'a>,
        instance: u32,
        guest: &CoreModule<'a>,
    ) -> Result<(), String> {
        let refuse = || {
            "its initializer does not run its guest module's _initialize once, as \
             wit-component's does"
                .to_string()
        };
        let [(module, name, ExternalKind::Func)] = initializer.imports[..] else {
            return Err(refuse());
        };
        if initializer.start != Some(0) || !guest.exports.contains(&"_initialize") {
            return Err(refuse());
        }
        let [(_, args)] = self.instantiations(index)[..] else {
            return Err(refuse());
        };
        if self.given(args, module, name, ExternalKind::Func)
            != Some(CoreItem::Export(instance, "_initialize"))
        {
            return Err(refuse());
        }
        Ok(())
    }

    /// The export `name` lifts the guest instance's export of that name as
    /// the browser calls it, with the world's type.
    fn check_export(
        &self,
        name: &str,
        func: u32,
        instance: u32,
        guest: &CoreModule<'a>,
    ) -> Result<(), String> {
        let Some(Func::Lift {
            core_func,
            ty,
            options,
        }) = self.funcs.get(func as usize)
        else {
            return Err(format!("its {name} does not lift a core function"));
        };
        let guest_s = |item: Option<&CoreItem<'a>>, export: &str| {
            item == Some(&CoreItem::Export(instance, export))
        };
        if !guest_s(self.core_funcs.get(*core_func as usize), name) {
            return Err(format!(
                "its {name} lifts a function other than its guest module's {name}"
            ));
        }
        let post_name = format!("cabi_post_{name}");
        let (mut memory, mut realloc, mut post_return) = (false, false, None);
        for option in options.iter() {
            match *option {
                CanonicalOption::UTF8 => {}
                CanonicalOption::Memory(m)
                    if guest_s(self.core_memories.get(m as usize), "memory") =>
                {
                    memory = true
                }
                CanonicalOption::Realloc(f)
                    if guest_s(self.core_funcs.get(f as usize), "cabi_realloc") =>
                {
                    realloc = true
                }
                CanonicalOption::PostReturn(f) => post_return = Some(f),
                other => {
                    return Err(format!(
                        "its {name} is lifted with {other:?}, where the browser lifts it with \
                         its guest module's memory and cabi_realloc and UTF-8 strings"
                    ))
                }
            }
        }
        // `dispatch` takes a list, which the host allocates with
        // `cabi_realloc`; both return lists in the guest's memory.
        if !memory || (name == "dispatch" && !realloc) {
            return Err(format!(
                "its {name} is lifted without its guest module's memory or cabi_realloc"
            ));
        }
        // The browser calls the guest's `cabi_post_` function when it exports
        // one: the component is to, and with no other.
        let post_returns = match post_return {
            Some(f) => guest_s(self.core_funcs.get(f as usize), &post_name),
            None => !guest.exports.contains(&post_name.as_str()),
        };
        if !post_returns {
            return Err(format!(
                "its {name} is lifted with a post-return function other than its guest module's \
                 {post_name}"
            ));
        }
        let found = self.signature(*ty)?;
        if found != world_signature(name) {
            return Err(format!(
                "its {name} is {found:?}, where the world's is {:?}",
                world_signature(name)
            ));
        }
        Ok(())
    }

    /// The function type at `index` of the component's types.
    fn signature(&self, mut index: u32) -> Result<Signature, String> {
        let scope = Scope {
            types: &self.types,
            parent: None,
        };
        for _ in 0..TYPE_DEPTH {
            match self.types.get(index as usize) {
                Some(Ty::Func(func)) if !func.async_ => {
                    let params = func
                        .params
                        .iter()
                        .map(|(_, ty)| self.val(&scope, *ty, 0))
                        .collect::<Result<_, _>>()?;
                    let result = func.result.map(|ty| self.val(&scope, ty, 0)).transpose()?;
                    return Ok(Signature { params, result });
                }
                Some(Ty::Same(earlier)) if *earlier < index => index = *earlier,
                _ => break,
            }
        }
        Err("an export's type is not a function the world's are".into())
    }

    fn val(&self, scope: &Scope<'_, 'a>, ty: ComponentValType, depth: u32) -> Result<Val, String> {
        match ty {
            ComponentValType::Primitive(primitive) => Ok(Val::Primitive(primitive)),
            ComponentValType::Type(index) => self.ty(scope, index, depth),
        }
    }

    /// The type at `index` of `scope`, resolved.
    fn ty(&self, scope: &Scope<'_, 'a>, index: u32, depth: u32) -> Result<Val, String> {
        let unsupported = || format!("its exports name a type the world's do not ({index})");
        let budget = self.type_budget.get();
        if depth > TYPE_DEPTH || budget == 0 {
            return Err(unsupported());
        }
        self.type_budget.set(budget - 1);
        let depth = depth + 1;
        match scope.types.get(index as usize).ok_or_else(unsupported)? {
            Ty::Defined(defined) => match defined {
                ComponentDefinedType::Primitive(primitive) => Ok(Val::Primitive(*primitive)),
                ComponentDefinedType::Record(fields) => fields
                    .iter()
                    .map(|(name, ty)| Ok((name.to_string(), self.val(scope, *ty, depth)?)))
                    .collect::<Result<_, String>>()
                    .map(Val::Record),
                ComponentDefinedType::List(ty) => {
                    Ok(Val::List(Box::new(self.val(scope, *ty, depth)?)))
                }
                ComponentDefinedType::Result { ok, err } => {
                    let case = |ty: &Option<ComponentValType>| {
                        ty.map(|ty| self.val(scope, ty, depth).map(Box::new))
                            .transpose()
                    };
                    Ok(Val::Result(case(ok)?, case(err)?))
                }
                _ => Err(unsupported()),
            },
            Ty::Same(earlier) if *earlier < index => self.ty(scope, *earlier, depth),
            Ty::Outer(outer) => self.ty(scope.parent.ok_or_else(unsupported)?, *outer, depth),
            Ty::InstanceExport(instance, name) if scope.parent.is_none() => {
                let declarations = match self.instances.get(*instance as usize) {
                    Some(Some(ty)) => match self.types.get(*ty as usize) {
                        Some(Ty::Instance(declarations)) => declarations,
                        _ => return Err(unsupported()),
                    },
                    _ => return Err(unsupported()),
                };
                let (types, exports) = instance_types(declarations);
                let &(_, exported) = exports
                    .iter()
                    .find(|(export, _)| export == name)
                    .ok_or_else(unsupported)?;
                let inner = Scope {
                    types: &types,
                    parent: Some(scope),
                };
                self.ty(&inner, exported, depth)
            }
            _ => Err(unsupported()),
        }
    }
}

/// The types of a scope: the component's, or an instance type's within it.
struct Scope<'s, 'a> {
    types: &'s [Ty<'a>],
    parent: Option<&'s Scope<'s, 'a>>,
}

/// The types an instance type's declarations define, and the indices of the
/// types it exports, by name.
fn instance_types<'a>(
    declarations: &[InstanceTypeDeclaration<'a>],
) -> (Vec<Ty<'a>>, Vec<(&'a str, u32)>) {
    let (mut types, mut exports) = (Vec::new(), Vec::new());
    for declaration in declarations {
        match declaration {
            InstanceTypeDeclaration::Type(ty) => types.push(ty.clone().into()),
            InstanceTypeDeclaration::Alias(ComponentAlias::Outer {
                kind: ComponentOuterAliasKind::Type,
                count,
                index,
            }) => types.push(if *count == 1 {
                Ty::Outer(*index)
            } else {
                Ty::Other
            }),
            InstanceTypeDeclaration::Alias(ComponentAlias::InstanceExport {
                kind: ComponentExternalKind::Type,
                ..
            }) => types.push(Ty::Other),
            InstanceTypeDeclaration::Export {
                name,
                ty: ComponentTypeRef::Type(bounds),
            } => {
                exports.push((name.name, types.len() as u32));
                types.push(match bounds {
                    TypeBounds::Eq(ty) => Ty::Same(*ty),
                    TypeBounds::SubResource => Ty::Other,
                });
            }
            _ => {}
        }
    }
    (types, exports)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// A guest core module exporting what the world's lifts call.
    const GUEST: &str = r#"(core module $guest
        (memory (export "memory") 1)
        (func (export "_initialize"))
        (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 8)
        (func (export "functions") (result i32) i32.const 0)
        (func (export "cabi_post_functions") (param i32))
        (func (export "dispatch") (param i64 i64 i32 i32) (result i32) i32.const 0)
        (func (export "cabi_post_dispatch") (param i32)))"#;
    const SHIM: &str = r#"(core module $shim (table (export "$imports") 1 1 funcref))"#;
    const FIXUPS: &str = r#"(core module $fixups (import "" "$imports" (table 1 1 funcref)))"#;
    const ANOTHER_SHIM: &str = r#"(core module (table (export "$imports") 1 1 funcref))"#;
    const ANOTHER_FIXUPS: &str = r#"(core module (import "" "$imports" (table 1 1 funcref)))"#;
    /// The shim and the fixups instantiated as wit-component instantiates
    /// them: the fixups given the shim instance's table.
    const FILL: &str = r#"
        (core instance $shim-instance (instantiate $shim))
        (alias core export $shim-instance "$imports" (core table $imports))
        (core instance $fixups-args (export "$imports" (table $imports)))
        (core instance (instantiate $fixups (with "" (instance $fixups-args))))"#;
    const INITIALIZER: &str = r#"(core module $initializer (import "" "" (func $f)) (start $f))"#;
    const ANOTHER_INITIALIZER: &str = r#"(core module (import "" "" (func $f)) (start $f))"#;
    /// The initializer instantiated as wit-component instantiates it: given
    /// the guest instance's `_initialize`.
    const INITIALIZE: &str = r#"
        (alias core export $main "_initialize" (core func $initialize))
        (core instance $initialize-args (export "" (func $initialize)))
        (core instance (instantiate $initializer (with "" (instance $initialize-args))))"#;

    /// The world's `id` type, as wit-component imports it: from the `types`
    /// interface, then as the world's own type.
    const ID: &str = r#"
        (type $types (instance
            (type (record (field "hi" u64) (field "lo" u64)))
            (export "id" (type (eq 0)))))
        (import "arora:module/types@0.2.0" (instance $types (type $types)))
        (alias export $types "id" (type $types-id))
        (import "id" (type $id (eq $types-id)))"#;

    /// The world's exports, lifted from the guest instance as wit-component
    /// lifts them.
    const EXPORTS: &str = r#"
        (core instance $main (instantiate $guest))
        (alias core export $main "memory" (core memory $memory))
        (alias core export $main "cabi_realloc" (core func $realloc))
        (type $ids (list $id))
        (type $functions (func (result $ids)))
        (func $functions (type $functions)
            (canon lift (core func $main "functions") (memory $memory)
                (post-return (core func $main "cabi_post_functions"))))
        (export "functions" (func $functions))
        (type $bytes (list u8))
        (type $result (result $bytes (error string)))
        (type $dispatch (func (param "method" $id) (param "arg" $bytes) (result $result)))
        (func $dispatch (type $dispatch)
            (canon lift (core func $main "dispatch") (memory $memory) (realloc $realloc)
                string-encoding=utf8 (post-return (core func $main "cabi_post_dispatch"))))
        (export "dispatch" (func $dispatch))"#;

    fn component(parts: &[&str]) -> Vec<u8> {
        wat::parse_str(format!("(component {})", parts.join(" "))).unwrap()
    }

    fn layout(modules: &[&str]) -> Result<bool, String> {
        let mut parts = vec![ID];
        parts.extend_from_slice(modules);
        parts.push(EXPORTS);
        if modules.contains(&INITIALIZER) {
            parts.push(INITIALIZE);
        }
        if modules.contains(&SHIM) && modules.contains(&FIXUPS) {
            parts.push(FILL);
        }
        guest_module(&component(&parts)).map(|guest| guest.initialize)
    }

    /// The world's component, with `from` in its text replaced by `to`.
    fn changed(from: &str, to: &str) -> Result<GuestModule<'static>, String> {
        let text = format!("{ID} {GUEST} {EXPORTS}");
        assert!(text.contains(from), "{from}");
        let bytes = component(&[&text.replacen(from, to, 1)]).leak();
        guest_module(bytes)
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
        let other_guest = r#"(core module (func (export "a")))"#;
        for modules in [
            &[GUEST, other_guest][..],
            &[GUEST, SHIM],
            &[GUEST, FIXUPS],
            &[GUEST, SHIM, FIXUPS, ANOTHER_SHIM, ANOTHER_FIXUPS],
            &[GUEST, INITIALIZER, ANOTHER_INITIALIZER],
        ] {
            assert!(layout(modules).is_err(), "{modules:?}");
        }
        let nested = component(&[GUEST, "(component)"]);
        assert!(guest_module(&nested)
            .unwrap_err()
            .contains("nests a component"));
        let core = wat::parse_str("(module)").unwrap();
        assert!(guest_module(&core).unwrap_err().contains("not a component"));
    }

    #[test]
    fn an_initializer_not_running_the_guest_s_initialize_once_is_refused() {
        let initialize = |parts: &[&str]| guest_module(&component(parts)).map(|g| g.initialize);
        assert_eq!(
            initialize(&[ID, GUEST, INITIALIZER, EXPORTS, INITIALIZE]),
            Ok(true)
        );
        let twice = format!(
            r#"{INITIALIZE} (core instance (instantiate $initializer (with "" (instance $initialize-args))))"#
        );
        let other_export = INITIALIZE.replace(r#""_initialize""#, r#""functions""#);
        let other_argument = INITIALIZE.replace(r#"(with "" "#, r#"(with "other" "#);
        let idle = r#"(core module $initializer (import "" "" (func)) (func $idle) (start $idle))"#;
        let not_a_reactor = GUEST.replace(r#"(func (export "_initialize"))"#, "");
        for parts in [
            // Defined, never instantiated, so its start function never runs.
            &[ID, GUEST, INITIALIZER, EXPORTS][..],
            &[ID, GUEST, INITIALIZER, EXPORTS, &twice],
            &[ID, GUEST, INITIALIZER, EXPORTS, &other_export],
            &[ID, GUEST, INITIALIZER, EXPORTS, &other_argument],
            // A start function other than the function it is given.
            &[ID, GUEST, idle, EXPORTS, INITIALIZE],
            &[ID, &not_a_reactor, INITIALIZER, EXPORTS, INITIALIZE],
        ] {
            let error = initialize(parts).unwrap_err();
            assert!(error.contains("initializer"), "{parts:?}: {error}");
        }
    }

    #[test]
    fn a_shim_or_fixups_touching_more_than_the_shim_s_table_is_refused() {
        let layout = |shim: &str, fixups: &str, fill: &str| {
            guest_module(&component(&[ID, GUEST, shim, fixups, EXPORTS, fill])).map(|_| ())
        };
        assert_eq!(layout(SHIM, FIXUPS, FILL), Ok(()));
        // The fixups given the guest's own table to write into.
        let guest_table = FILL.replace(
            r#"(alias core export $shim-instance "$imports" (core table $imports))"#,
            r#"(alias core export $main "table" (core table $imports))"#,
        );
        // Fixups that could write into the guest's memory.
        let with_memory = FIXUPS.replace(")))", r#")) (import "" "memory" (memory 1)))"#);
        let shim_importing = SHIM.replace("(table", r#"(import "" "f" (func)) (table"#);
        let shim_starting = SHIM.replace("funcref)", "funcref) (func $s) (start $s)");
        let never_filled = FILL.replace(
            r#"(core instance (instantiate $fixups (with "" (instance $fixups-args))))"#,
            "",
        );
        // Filled once as wit-component fills it, then once more with the
        // guest's table.
        let filled_twice = format!(
            r#"{FILL}
            (alias core export $main "table" (core table $guest-table))
            (core instance $guest-args (export "$imports" (table $guest-table)))
            (core instance (instantiate $fixups (with "" (instance $guest-args))))"#
        );
        for (shim, fixups, fill) in [
            (SHIM, FIXUPS, guest_table.as_str()),
            (SHIM, with_memory.as_str(), FILL),
            (shim_importing.as_str(), FIXUPS, FILL),
            (shim_starting.as_str(), FIXUPS, FILL),
            (SHIM, FIXUPS, never_filled.as_str()),
            (SHIM, FIXUPS, filled_twice.as_str()),
        ] {
            let error = layout(shim, fixups, fill).unwrap_err();
            assert!(
                error.contains("shim and fixups"),
                "{fixups} {fill}: {error}"
            );
        }
    }

    #[test]
    fn a_component_aliasing_a_module_or_defining_a_component_instance_is_refused() {
        let alias = changed(
            EXPORTS,
            &format!("{EXPORTS} (alias outer 0 0 (core module))"),
        );
        assert!(alias.unwrap_err().contains("aliases a core module"));
        // A module an imported interface's instance exports.
        let from_an_instance = changed(
            EXPORTS,
            &format!(
                r#"{EXPORTS}
                (import "wasi:io/poll@0.2.9" (instance $poll (export "m" (core module))))
                (alias export $poll "m" (core module))"#
            ),
        );
        assert!(from_an_instance
            .unwrap_err()
            .contains("aliases a core module"));
        let instance = changed(EXPORTS, &format!("{EXPORTS} (instance)"));
        assert!(instance.unwrap_err().contains("component instance"));
    }

    #[test]
    fn types_the_world_s_are_not_are_refused_without_resolving_them_whole() {
        // An export lifted with another memory than the guest's.
        let other_memory = changed(
            r#"(canon lift (core func $main "functions") (memory $memory)"#,
            r#"(canon lift (core func $main "functions") (memory 1)"#,
        );
        assert!(other_memory.unwrap_err().contains("memory"));

        // `dispatch`'s `method` of the type `types` define, as `$method`.
        let method_of = |types: &str| {
            let text = format!("{ID} {GUEST} {EXPORTS}")
                .replace(
                    "(type $bytes (list u8))",
                    &format!("(type $bytes (list u8)) {types}"),
                )
                .replace(r#"(param "method" $id)"#, r#"(param "method" $method)"#);
            guest_module(component(&[&text]).leak()).map(|_| ())
        };
        assert_eq!(
            method_of(r#"(type $method (record (field "hi" u64) (field "lo" u64)))"#),
            Ok(())
        );
        // A record holding itself.
        assert!(method_of(r#"(type $method (record (field "a" $method)))"#).is_err());
        // Records sharing their fields' types, 4 wide and 13 deep: 4^13
        // visits whole, refused within the budget.
        let mut dag = String::from(r#"(type $r0 (record (field "a" u64)))"#);
        for depth in 1..=13 {
            let below = depth - 1;
            let field = |name| format!(r#"(field "{name}" $r{below})"#);
            dag += &format!(
                " (type $r{depth} (record {} {} {} {}))",
                field("a"),
                field("b"),
                field("c"),
                field("d")
            );
        }
        dag += r#" (type $method (record (field "hi" $r13) (field "lo" $r13)))"#;
        let started = std::time::Instant::now();
        assert!(method_of(&dag).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn a_component_importing_what_a_host_does_not_provide_is_refused() {
        for import in [
            r#"(import "wasi:sockets/tcp@0.2.0" (instance))"#,
            r#"(import "arora:module/host@0.1.0" (instance))"#,
            r#"(import "f" (func))"#,
            r#"(import "r" (type (sub resource)))"#,
        ] {
            let error = changed(ID, &format!("{ID} {import}")).unwrap_err();
            assert!(
                error.contains("which is not an interface"),
                "{import}: {error}"
            );
        }
        assert!(changed(
            ID,
            &format!(r#"{ID} (import "wasi:io/poll@0.2.9" (instance))"#)
        )
        .is_ok());
    }

    #[test]
    fn a_component_without_the_world_s_exports_is_refused() {
        let without_functions = changed(r#"(export "functions" (func $functions))"#, "");
        assert!(without_functions.unwrap_err().contains(r#"["dispatch"]"#));
        let another = changed(
            r#"(export "dispatch" (func $dispatch))"#,
            r#"(export "dispatch" (func $dispatch)) (export "more" (func $dispatch))"#,
        );
        assert!(another.unwrap_err().contains("\"more\""));
        let instance = changed(
            r#"(export "dispatch" (func $dispatch))"#,
            r#"(export "dispatch" (func $dispatch)) (export "i" (instance $types))"#,
        );
        assert!(instance.unwrap_err().contains("not a function"));
    }

    #[test]
    fn an_export_lifted_otherwise_than_the_browser_calls_it_is_refused() {
        for (from, to, why) in [
            // Another core function, under the export's name.
            (
                r#"(canon lift (core func $main "dispatch")"#,
                r#"(canon lift (core func $main "functions")"#,
                "lifts a function other than",
            ),
            // Strings the browser does not read.
            ("string-encoding=utf8", "string-encoding=utf16", "UTF16"),
            // Another allocator than the guest's.
            (
                "(realloc $realloc)",
                r#"(realloc (core func $main "functions"))"#,
                "lifted with Realloc",
            ),
            // No allocator for the argument the host passes `dispatch`.
            (
                "(realloc $realloc)",
                "",
                "without its guest module's memory or cabi_realloc",
            ),
            // No memory for the lists it returns.
            (
                r#"(canon lift (core func $main "functions") (memory $memory)"#,
                r#"(canon lift (core func $main "functions")"#,
                "without its guest module's memory or cabi_realloc",
            ),
            // No post-return, where the guest exports one, which the browser calls.
            (
                r#"(post-return (core func $main "cabi_post_dispatch"))"#,
                "",
                "post-return",
            ),
            // Another post-return than the guest's.
            (
                r#"(post-return (core func $main "cabi_post_dispatch"))"#,
                r#"(post-return (core func $main "cabi_post_functions"))"#,
                "post-return",
            ),
            // The guest instantiated twice.
            (
                "(core instance $main (instantiate $guest))",
                "(core instance $main (instantiate $guest)) (core instance (instantiate $guest))",
                "2 times",
            ),
        ] {
            let error = changed(from, to).unwrap_err();
            assert!(error.contains(why), "{to}: {error}");
        }
    }

    #[test]
    fn an_export_of_another_type_than_the_world_s_is_refused() {
        for (from, to) in [
            (
                "(type $result (result $bytes (error string)))",
                "(type $result (list u8))",
            ),
            (r#"(field "lo" u64)"#, r#"(field "low" u64)"#),
            (r#"(param "arg" $bytes)"#, r#"(param "arg" string)"#),
            ("(type $ids (list $id))", "(type $ids (list u64))"),
        ] {
            let error = changed(from, to).unwrap_err();
            assert!(error.contains("where the world's is"), "{to}: {error}");
        }
        // Parameter names are not part of the type a host checks.
        assert!(changed(r#"(param "method" $id)"#, r#"(param "m" $id)"#).is_ok());
    }

    #[test]
    fn only_the_hosts_interfaces_at_their_versions_are_provided() {
        assert!(provided("wasi:io/streams@0.2.0"));
        assert!(provided("wasi:random/insecure-seed@0.2.9"));
        assert!(provided("arora:module/host@0.2.0"));
        assert!(!provided("arora:module/host@0.1.0"));
        assert!(!provided("wasi:io/streams@0.3.0"));
        assert!(provided("wasi:filesystem/preopens@0.2.0"));
        assert!(!provided("wasi:filesystem/preopens@0.3.0"));
        assert!(!provided("wasi:sockets/tcp@0.2.0"));
        assert!(!provided("id"));
    }
}

//! A contract is implemented by several modules, each under its own module
//! id and with its own state; its declaration gives them one set of ids, one
//! record and one signature per function.

use arora_engine::engine::{EngineBuilder, PinnedEngine};
use arora_engine::module::HostModule;
use arora_types::call::{Call, CallBridge, CallError};
use arora_types::record::module::frozen::ExportKind;
use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::record::FrozenReference;
use arora_types::value::{StructureField, Value};
use arora_types::{AroraType, Uuid};

#[derive(Debug, Clone, PartialEq, AroraType)]
#[arora(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000001")]
pub struct Receipt {
    #[arora(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000011")]
    total: u32,
    #[arora(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000012")]
    note: String,
}

/// A running tally.
#[arora_module::contract(name = "tally")]
pub trait Tally {
    /// Adds `amount` to the tally, which it also writes to `total`.
    #[export(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000100")]
    fn add(
        &mut self,
        #[param(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000101")] amount: u32,
        #[param(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000102")] note: Option<String>,
        #[param(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000103")] total: &mut u32,
    ) -> Receipt;

    /// Empties the tally.
    #[export(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000200")]
    fn reset(&mut self);
}

/// A contract named after its trait.
#[arora_module::contract]
pub trait PlayViseme {
    #[export(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000300")]
    fn play_viseme(&mut self, #[param(id = "7c0a1b2c-3d4e-4f50-8a6b-000000000301")] shape: String);
}

/// Counts what it is given.
struct Counting {
    sum: u32,
}

impl Tally for Counting {
    fn add(&mut self, amount: u32, note: Option<String>, total: &mut u32) -> Receipt {
        self.sum += amount;
        *total = self.sum;
        Receipt {
            total: self.sum,
            note: note.unwrap_or_else(|| "none".to_string()),
        }
    }

    fn reset(&mut self) {
        self.sum = 0;
    }
}

/// Counts twice what it is given.
struct Doubling {
    sum: u32,
}

impl Tally for Doubling {
    fn add(&mut self, amount: u32, _note: Option<String>, total: &mut u32) -> Receipt {
        self.sum += 2 * amount;
        *total = self.sum;
        Receipt {
            total: self.sum,
            note: "doubled".to_string(),
        }
    }

    fn reset(&mut self) {
        self.sum = 0;
    }
}

const COUNTING: Uuid = Uuid::from_u128(0x7c0a1b2c_3d4e_4f50_8a6b_00000000c001);
const DOUBLING: Uuid = Uuid::from_u128(0x7c0a1b2c_3d4e_4f50_8a6b_00000000c002);

fn engine() -> PinnedEngine {
    let mut engine = EngineBuilder::new().build();
    for module in [
        HostModule::from_exports(COUNTING, tally::exports(Counting { sum: 0 })),
        HostModule::from_exports(DOUBLING, tally::exports(Doubling { sum: 0 })),
    ] {
        engine.register_module(module.id(), Box::new(module));
    }
    engine
}

fn field(id: Uuid, value: Value) -> StructureField {
    StructureField {
        id,
        value: Box::new(value),
    }
}

fn add(
    engine: &mut PinnedEngine,
    module: Uuid,
    args: Vec<StructureField>,
) -> Result<(Receipt, Value), CallError> {
    let result = engine.arora_call(Call {
        module_id: Some(module),
        id: tally::ids::add::FUNCTION,
        args,
    })?;
    let receipt = Receipt::try_from(result.ret).expect("a receipt");
    let [total] = result.mutated.as_slice() else {
        panic!("one mutated argument, got {:?}", result.mutated);
    };
    assert_eq!(total.id, tally::ids::add::TOTAL);
    Ok((receipt, (*total.value).clone()))
}

fn add_args(amount: u32) -> Vec<StructureField> {
    vec![
        field(tally::ids::add::AMOUNT, Value::U32(amount)),
        field(tally::ids::add::TOTAL, Value::U32(0)),
    ]
}

#[test]
fn each_implementation_serves_the_contract_under_its_own_module_id_with_its_own_state() {
    let mut engine = engine();
    add(&mut engine, COUNTING, add_args(3)).expect("add");
    let (receipt, total) = add(&mut engine, COUNTING, add_args(4)).expect("add");
    assert_eq!(receipt.total, 7);
    assert_eq!(total, Value::U32(7));

    let (receipt, total) = add(&mut engine, DOUBLING, add_args(4)).expect("add");
    assert_eq!(
        receipt.total, 8,
        "the other implementation's state is its own"
    );
    assert_eq!(total, Value::U32(8));
}

#[test]
fn the_functions_of_one_implementation_share_its_state() {
    let mut engine = engine();
    add(&mut engine, COUNTING, add_args(5)).expect("add");
    engine
        .arora_call(Call {
            module_id: Some(COUNTING),
            id: tally::ids::reset::FUNCTION,
            args: vec![],
        })
        .expect("reset");
    let (receipt, _) = add(&mut engine, COUNTING, add_args(2)).expect("add");
    assert_eq!(receipt.total, 2, "reset emptied the tally add reads");
}

#[test]
fn an_absent_optional_argument_is_none_and_a_present_one_arrives() {
    let mut engine = engine();
    let (receipt, _) = add(&mut engine, COUNTING, add_args(1)).expect("add");
    assert_eq!(receipt.note, "none");

    let mut args = add_args(1);
    args.push(field(
        tally::ids::add::NOTE,
        Value::Option(Some(Box::new(Value::String("lunch".into())))),
    ));
    let (receipt, _) = add(&mut engine, COUNTING, args).expect("add");
    assert_eq!(receipt.note, "lunch");
}

#[test]
fn an_absent_mutable_argument_fails_the_call_by_name() {
    let mut engine = engine();
    let error = add(
        &mut engine,
        COUNTING,
        vec![field(tally::ids::add::AMOUNT, Value::U32(1))],
    )
    .expect_err("the call lacks `total`");
    assert!(
        error
            .to_string()
            .contains("missing parameter `total` of `add`"),
        "{error}"
    );
}

#[test]
fn the_record_describes_the_contract_s_functions_in_declaration_order() {
    let parent = Uuid::from_u128(0xf01de5);
    let record = tally::record(parent);
    assert_eq!(record.parent, parent);
    assert_eq!(record.name, "tally");
    assert_eq!(tally::NAME, "tally");
    assert_eq!(record.executable, None);
    assert_eq!(
        record.dependencies,
        vec![FrozenReference {
            id: Receipt::arora_type_id(),
            version: Receipt::arora_type_version(),
        }]
    );

    let add = &record.exports[&tally::ids::add::FUNCTION];
    assert_eq!(add.name, "add");
    let ExportKind::Function(signature) = &add.kind;
    assert_eq!(
        signature.parameter_ordering,
        vec![
            tally::ids::add::AMOUNT,
            tally::ids::add::NOTE,
            tally::ids::add::TOTAL
        ]
    );
    let note = &signature.parameters[&tally::ids::add::NOTE];
    assert_eq!(
        note.ty,
        FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::String)),
        })
    );
    assert!(!note.mutable);
    assert!(signature.parameters[&tally::ids::add::TOTAL].mutable);

    let reset = &record.exports[&tally::ids::reset::FUNCTION];
    assert_eq!(reset.name, "reset");
}

#[test]
fn the_host_module_describes_each_function_for_introspection() {
    let module = HostModule::from_exports(COUNTING, tally::exports(Counting { sum: 0 }));
    assert_eq!(module.id(), COUNTING);
    let mut names: Vec<(&str, Uuid)> = module
        .descriptions()
        .iter()
        .map(|d| (d.name.as_str(), d.id))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            ("add", tally::ids::add::FUNCTION),
            ("reset", tally::ids::reset::FUNCTION)
        ]
    );
}

#[test]
fn a_contract_without_a_name_is_named_after_its_trait() {
    assert_eq!(play_viseme::NAME, "play_viseme");
    assert_eq!(
        play_viseme::ids::play_viseme::FUNCTION,
        Uuid::from_u128(0x7c0a1b2c_3d4e_4f50_8a6b_000000000300)
    );
}

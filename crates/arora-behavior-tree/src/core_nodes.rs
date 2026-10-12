//! The core string nodes, native to every behavior tree: `SetString`,
//! `UnsetString`, `IsStringSet`, `WaitStringSet` and `RegexMatch`.
//!
//! Like the control nodes, they are dispatched natively — no module needs to
//! be loaded, and they are not in the function index. Like any module
//! function, they are **value-based**: a node never names a store key. It
//! receives the values its ports are bound to as `Option<String>` (`None`
//! when the bound variable holds nothing, e.g. a store key that is not set)
//! and writes through its out-parameters (`&mut Option<String>`: `Some`
//! writes the bound variable, creating the key if it did not exist, `None`
//! unsets it). Which keys those are is the tree's business — its `{var}`
//! bindings.
//!
//! The functions are those of the `test-behavior-tree-nodes` module that first
//! implemented them, with the same ids, parameter ids, names and semantics.

use arora_types::value::{ConversionError, Value};
use regex::Regex;
use uuid::Uuid;

use crate::arora_generated::behavior_tree::status::Status;
use crate::error::BehaviorTreeError;
use crate::nodes::{
    IS_STR_SET_FUNCTION_ID, IS_STR_SET_VALUE_PARAM_ID, REGEX_MATCH_FIRST_MATCH_PARAM_ID,
    REGEX_MATCH_FUNCTION_ID, REGEX_MATCH_MATCHER_PARAM_ID, REGEX_MATCH_VALUE_PARAM_ID,
    SET_STR_FUNCTION_ID, SET_STR_VALUE_PARAM_ID, SET_STR_VARIABLE_PARAM_ID, UNSET_STR_FUNCTION_ID,
    UNSET_STR_VARIABLE_PARAM_ID, WAIT_STR_SET_FUNCTION_ID, WAIT_STR_SET_VALUE_PARAM_ID,
};
use crate::variable::VariableCell;

// The node functions
//==============================================================
/// `SetString(variable, value)`: `variable` takes `value`; `Success`. An
/// unset `value` unsets `variable`.
pub fn set_str(variable: &mut Option<String>, value: Option<String>) -> Status {
    *variable = value;
    Status::Success
}

/// `UnsetString(variable)`: `variable` is unset; `Success`.
pub fn unset_str(variable: &mut Option<String>) -> Status {
    *variable = None;
    Status::Success
}

/// `IsStringSet(value)`: `Success` if `value` is a non-empty string,
/// `Failure` if it is unset or empty.
pub fn is_str_set(value: Option<String>) -> Status {
    match value {
        Some(value) if !value.is_empty() => Status::Success,
        _ => Status::Failure,
    }
}

/// `WaitStringSet(value)`: `Success` if `value` is a non-empty string,
/// `Running` while it is unset or empty.
pub fn wait_str_set(value: Option<String>) -> Status {
    match value {
        Some(value) if !value.is_empty() => Status::Success,
        _ => Status::Running,
    }
}

/// `RegexMatch(value, matcher, first_match)`: `Success` if the regular
/// expression `matcher` matches somewhere in `value`, writing the matched
/// text (the whole match, not a capture group) to `first_match`. `Failure`,
/// leaving `first_match` as it was, if `value` or `matcher` is unset,
/// `matcher` is not a valid regular expression, or nothing matches.
pub fn regex_match(
    value: Option<String>,
    matcher: Option<String>,
    first_match: &mut Option<String>,
) -> Status {
    let (Some(value), Some(matcher)) = (value, matcher) else {
        return Status::Failure;
    };
    let Ok(re) = Regex::new(&matcher) else {
        return Status::Failure;
    };
    match re.find(&value) {
        Some(found) => {
            *first_match = Some(found.as_str().to_string());
            Status::Success
        }
        None => Status::Failure,
    }
}

// Their signatures
//==============================================================
/// A core node's parameter: its Groot attribute name and its id.
pub struct CoreParameter {
    pub name: &'static str,
    pub id: Uuid,
}

const fn param(name: &'static str, id: Uuid) -> CoreParameter {
    CoreParameter { name, id }
}

const SET_STR_PARAMETERS: &[CoreParameter] = &[
    param("variable", SET_STR_VARIABLE_PARAM_ID),
    param("value", SET_STR_VALUE_PARAM_ID),
];
const UNSET_STR_PARAMETERS: &[CoreParameter] = &[param("variable", UNSET_STR_VARIABLE_PARAM_ID)];
const IS_STR_SET_PARAMETERS: &[CoreParameter] = &[param("value", IS_STR_SET_VALUE_PARAM_ID)];
const WAIT_STR_SET_PARAMETERS: &[CoreParameter] = &[param("value", WAIT_STR_SET_VALUE_PARAM_ID)];
const REGEX_MATCH_PARAMETERS: &[CoreParameter] = &[
    param("value", REGEX_MATCH_VALUE_PARAM_ID),
    param("matcher", REGEX_MATCH_MATCHER_PARAM_ID),
    param("first_match", REGEX_MATCH_FIRST_MATCH_PARAM_ID),
];

/// The parameters of the core node `function`, or `None` if `function` is not
/// a core node.
pub fn core_node_parameters(function: Uuid) -> Option<&'static [CoreParameter]> {
    Some(match function {
        SET_STR_FUNCTION_ID => SET_STR_PARAMETERS,
        UNSET_STR_FUNCTION_ID => UNSET_STR_PARAMETERS,
        IS_STR_SET_FUNCTION_ID => IS_STR_SET_PARAMETERS,
        WAIT_STR_SET_FUNCTION_ID => WAIT_STR_SET_PARAMETERS,
        REGEX_MATCH_FUNCTION_ID => REGEX_MATCH_PARAMETERS,
        _ => return None,
    })
}

// Ticking them
//==============================================================
/// Tick the core node `function` over its bound parameter cells (`cell`
/// returns the cell bound to a parameter id). Returns `None` if `function` is
/// not a core node.
pub(crate) fn tick_core_node(
    function: Uuid,
    node: Uuid,
    cell: impl Fn(Uuid) -> Result<VariableCell, BehaviorTreeError>,
) -> Result<Option<Status>, BehaviorTreeError> {
    let read = |id: Uuid| -> Result<(VariableCell, Option<String>), BehaviorTreeError> {
        let cell = cell(id)?;
        let value = read_string(&cell, node, id)?;
        Ok((cell, value))
    };
    let status = match function {
        SET_STR_FUNCTION_ID => {
            let (variable, mut current) = read(SET_STR_VARIABLE_PARAM_ID)?;
            let (_, value) = read(SET_STR_VALUE_PARAM_ID)?;
            let before = current.clone();
            let status = set_str(&mut current, value);
            write_string(&variable, before, current);
            status
        }
        UNSET_STR_FUNCTION_ID => {
            let (variable, mut current) = read(UNSET_STR_VARIABLE_PARAM_ID)?;
            let before = current.clone();
            let status = unset_str(&mut current);
            write_string(&variable, before, current);
            status
        }
        IS_STR_SET_FUNCTION_ID => is_str_set(read(IS_STR_SET_VALUE_PARAM_ID)?.1),
        WAIT_STR_SET_FUNCTION_ID => wait_str_set(read(WAIT_STR_SET_VALUE_PARAM_ID)?.1),
        REGEX_MATCH_FUNCTION_ID => {
            let (_, value) = read(REGEX_MATCH_VALUE_PARAM_ID)?;
            let (_, matcher) = read(REGEX_MATCH_MATCHER_PARAM_ID)?;
            let (first_match, mut current) = read(REGEX_MATCH_FIRST_MATCH_PARAM_ID)?;
            let before = current.clone();
            let status = regex_match(value, matcher, &mut current);
            write_string(&first_match, before, current);
            status
        }
        _ => return Ok(None),
    };
    Ok(Some(status))
}

/// A bound cell as the `Option<String>` a core node takes: `None` when it
/// holds nothing (an unset key, an unbound local, `()` or an empty optional).
/// Any value other than a string fails the tick, as the module's argument
/// decoding did.
fn read_string(
    cell: &VariableCell,
    node: Uuid,
    parameter: Uuid,
) -> Result<Option<String>, BehaviorTreeError> {
    match cell.get() {
        None | Some(Value::Unit) | Some(Value::Option(None)) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Option(Some(inner))) => match *inner {
            Value::String(value) => Ok(Some(value)),
            other => Err(not_a_string(node, parameter, &other)),
        },
        Some(other) => Err(not_a_string(node, parameter, &other)),
    }
}

fn not_a_string(node: Uuid, parameter: Uuid, value: &Value) -> BehaviorTreeError {
    BehaviorTreeError::ConversionError(ConversionError {
        message: format!(
            "node {node}: parameter {parameter} expects a string, but is bound to {value}"
        ),
    })
}

/// Write an out-parameter back to its cell if the node changed it: `Some`
/// sets the variable (creating its store key), `None` unsets it. An unchanged
/// value is not rewritten, so a node that leaves its output alone does not
/// touch the store.
fn write_string(cell: &VariableCell, before: Option<String>, after: Option<String>) {
    if before != after {
        cell.set_opt(after.map(Value::String));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_functions_keep_the_module_semantics() {
        let mut variable = None;
        assert_eq!(set_str(&mut variable, Some("a".into())), Status::Success);
        assert_eq!(variable.as_deref(), Some("a"));
        assert_eq!(unset_str(&mut variable), Status::Success);
        assert_eq!(variable, None);

        assert_eq!(is_str_set(None), Status::Failure);
        assert_eq!(is_str_set(Some(String::new())), Status::Failure);
        assert_eq!(is_str_set(Some("x".into())), Status::Success);
        assert_eq!(wait_str_set(None), Status::Running);
        assert_eq!(wait_str_set(Some(String::new())), Status::Running);
        assert_eq!(wait_str_set(Some("x".into())), Status::Success);

        let mut first = Some("kept".to_string());
        let matcher = Some("(Ross|Victor)".to_string());
        assert_eq!(
            regex_match(Some("hi Victor!".into()), matcher.clone(), &mut first),
            Status::Success
        );
        assert_eq!(first.as_deref(), Some("Victor"));
        assert_eq!(
            regex_match(Some("hi Brad".into()), matcher.clone(), &mut first),
            Status::Failure
        );
        assert_eq!(first.as_deref(), Some("Victor"), "a failure leaves it");
        assert_eq!(regex_match(None, matcher, &mut first), Status::Failure);
        assert_eq!(
            regex_match(Some("x".into()), Some("(".into()), &mut first),
            Status::Failure,
            "an invalid regex fails"
        );
    }
}

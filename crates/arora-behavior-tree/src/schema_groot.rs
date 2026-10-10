use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
use arora_types::record::module::frozen::Parameter;
use arora_types::record::ty::FrozenTy;
use arora_types::value::Value;
use quick_xml::events::BytesStart;
use quick_xml::Writer;
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::{
    collections::HashMap,
    error::Error,
    fmt::{Display, Write},
    io::Cursor,
};
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::nodes::{
    COS_FUNCTION_ID, EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, FAIL_FUNCTION_ID,
    FALLBACK_FUNCTION_ID, INCREASE_FUNCTION_ID, PARALLEL_FUNCTION_ID, RUN_CALL_FUNCTION_ID,
    RUN_CALL_PARAM_ID, RUN_FUNCTION_ID, RUN_STATUS_FUNCTION_ID, RUN_STATUS_LATCH_PARAM_ID,
    RUN_STATUS_OUT_PARAM_ID, SEQ_FUNCTION_ID, SEQ_STAR_CURRENT_INDEX_PARAM_ID,
    SEQ_STAR_FUNCTION_ID, STATUS_IDENTITY_FUNCTION_ID, STORE_FUNCTION_ID, SUCCEED_FUNCTION_ID,
    WRITE_KEYS_FUNCTION_ID, WRITE_KEYS_KEYS_PARAM_ID, WRITE_KEYS_VALUES_PARAM_ID,
};
use crate::schema::{Expression, _RET_PARAM_ID};
use crate::tree_node::TreeNode;
use crate::ModuleFunction;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct BehaviorTree {
    pub root: Node,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Node {
    id: String,
    name: String,
    param_args: HashMap<String, String>,
    children: Vec<Node>,
}

impl Node {
    /// Convert the Groot-style behavior tree into a TreeNode one.
    pub fn try_into_tree_node(
        &self,
        index: &HashMap<Uuid, ModuleFunction>,
        variables: &mut HashMap<String, Uuid>,
    ) -> Result<TreeNode, BehaviorTreeError> {
        let mut tree_node_children = Vec::new();
        for child in &self.children {
            tree_node_children.push(child.try_into_tree_node(index, variables)?)
        }

        let arora_id = match self.id.as_str() {
            SUCCEED_GROOT_ID => SUCCEED_FUNCTION_ID,
            FAIL_GROOT_ID => FAIL_FUNCTION_ID,
            RUN_GROOT_ID => RUN_FUNCTION_ID,
            STATUS_IDENTITY_GROOT_ID => STATUS_IDENTITY_FUNCTION_ID,
            STORE_GROOT_ID => STORE_FUNCTION_ID,
            INCREASE_GROOT_ID => INCREASE_FUNCTION_ID,
            SEQ_GROOT_ID => SEQ_FUNCTION_ID,
            SEQ_STAR_GROOT_ID => SEQ_STAR_FUNCTION_ID,
            FALLBACK_GROOT_ID => FALLBACK_FUNCTION_ID,
            PARALLEL_GROOT_ID => PARALLEL_FUNCTION_ID,
            EQUAL_GROOT_ID => EQUAL_FUNCTION_ID,
            WRITE_KEYS_GROOT_ID => WRITE_KEYS_FUNCTION_ID,
            RUN_CALL_GROOT_ID => RUN_CALL_FUNCTION_ID,
            RUN_STATUS_GROOT_ID => RUN_STATUS_FUNCTION_ID,
            COS_GROOT_ID => COS_FUNCTION_ID,
            SET_STR_GROOT_ID => Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap(),
            UNSET_STR_GROOT_ID => Uuid::from_str("7dce01ed-9818-4b7d-b45a-2e7fdece3633").unwrap(),
            IS_STR_SET_GROOT_ID => Uuid::from_str("20ba3f0f-309e-4cd2-adfc-aca6cc432526").unwrap(),
            WAIT_STR_SET_GROOT_ID => {
                Uuid::from_str("3180977c-25a1-458e-ab82-11f36c654518").unwrap()
            }
            REGEX_MATCH_GROOT_ID => Uuid::from_str("8e3dbcc1-1a81-4cf6-a457-6e0c075456fd").unwrap(),
            tag => function_by_groot_tag(index, tag)?,
        };

        let children = if tree_node_children.is_empty() {
            None
        } else {
            Some(tree_node_children)
        };

        // The native nodes are not in the function index: build their TreeNode
        // directly, from their ports. seq_star's persistent current-index and a
        // run-status decorator's latch are seeded.
        if crate::is_native(arora_id) {
            let mut parameters = if arora_id == SEQ_STAR_FUNCTION_ID {
                HashMap::from([(
                    SEQ_STAR_CURRENT_INDEX_PARAM_ID,
                    Expression::Value(Value::U16(0)),
                )])
            } else {
                HashMap::new()
            };
            // A run-status decorator's latch is its own state: it starts
            // empty.
            if arora_id == RUN_STATUS_FUNCTION_ID {
                parameters.insert(RUN_STATUS_LATCH_PARAM_ID, Expression::Value(Value::Unit));
            }
            // A node with ports reads every one of them, and takes no other
            // attribute; a control node's attributes are not arguments.
            let ports = builtin_ports(arora_id);
            if !ports.is_empty() {
                for (port, text) in &self.param_args {
                    let param = ports
                        .iter()
                        .find(|(name, _)| name == port)
                        .map(|(_, id)| *id)
                        .ok_or_else(|| BehaviorTreeError::InconsistentTreeError {
                            message: format!("{} has no port \"{port}\"", self.id),
                        })?;
                    parameters.insert(param, builtin_port_expression(param, text, variables)?);
                }
                if let Some((missing, _)) = ports
                    .iter()
                    .find(|(port, _)| !self.param_args.contains_key(*port))
                {
                    return Err(BehaviorTreeError::InconsistentTreeError {
                        message: format!("{} needs its port \"{missing}\"", self.id),
                    });
                }
            }
            return Ok(TreeNode {
                function: arora_id,
                children,
                parameters,
            });
        }

        let function = index
            .get(&arora_id)
            .ok_or(BehaviorTreeError::InternalError {
                message: format!("function {} is missing from index", arora_id),
            })?;
        let mut parameters = HashMap::new();
        for param_arg in &self.param_args {
            let (param, arg) = groot_param_arg_to_arora(param_arg, function, variables)?;
            parameters.insert(param, arg);
        }
        Ok(TreeNode {
            function: function.function_id,
            children,
            parameters,
        })
    }

    /// Converts a TreeNode into a Groot Node.
    pub fn try_from_tree_node(
        tree_node: &TreeNode,
        index: &HashMap<Uuid, ModuleFunction>,
        variables: &mut HashMap<Uuid, String>,
    ) -> Result<Node, BehaviorTreeError> {
        let mut groot_children = Vec::new();
        if let Some(tree_node_children) = &tree_node.children {
            for child in tree_node_children {
                groot_children.push(Self::try_from_tree_node(child, index, variables)?)
            }
        }
        let groot_id = match tree_node.function {
            SUCCEED_FUNCTION_ID => SUCCEED_GROOT_ID,
            FAIL_FUNCTION_ID => FAIL_GROOT_ID,
            RUN_FUNCTION_ID => RUN_GROOT_ID,
            STATUS_IDENTITY_FUNCTION_ID => STATUS_IDENTITY_GROOT_ID,
            STORE_FUNCTION_ID => STORE_GROOT_ID,
            INCREASE_FUNCTION_ID => INCREASE_GROOT_ID,
            SEQ_FUNCTION_ID => SEQ_GROOT_ID,
            SEQ_STAR_FUNCTION_ID => SEQ_STAR_GROOT_ID,
            FALLBACK_FUNCTION_ID => FALLBACK_GROOT_ID,
            PARALLEL_FUNCTION_ID => PARALLEL_GROOT_ID,
            EQUAL_FUNCTION_ID => EQUAL_GROOT_ID,
            WRITE_KEYS_FUNCTION_ID => WRITE_KEYS_GROOT_ID,
            RUN_CALL_FUNCTION_ID => RUN_CALL_GROOT_ID,
            RUN_STATUS_FUNCTION_ID => RUN_STATUS_GROOT_ID,
            COS_FUNCTION_ID => COS_GROOT_ID,
            // The string/regex helper nodes use ad-hoc function ids (see `nodes.rs`)
            // instead of named constants, so they are matched via guards here.
            id if id == Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap() => {
                SET_STR_GROOT_ID
            }
            id if id == Uuid::from_str("7dce01ed-9818-4b7d-b45a-2e7fdece3633").unwrap() => {
                UNSET_STR_GROOT_ID
            }
            id if id == Uuid::from_str("20ba3f0f-309e-4cd2-adfc-aca6cc432526").unwrap() => {
                IS_STR_SET_GROOT_ID
            }
            id if id == Uuid::from_str("3180977c-25a1-458e-ab82-11f36c654518").unwrap() => {
                WAIT_STR_SET_GROOT_ID
            }
            id if id == Uuid::from_str("8e3dbcc1-1a81-4cf6-a457-6e0c075456fd").unwrap() => {
                REGEX_MATCH_GROOT_ID
            }
            // Any other function is written under its own name — the form the
            // importer resolves against the index.
            id => index
                .get(&id)
                .map(|function| function.function_name.as_str())
                .ok_or(BehaviorTreeError::InconsistentTreeError {
                    message: format!("node refers to function {} that could not be resolved", id),
                })?,
        }
        .to_string();

        // The native nodes are dispatched natively and not in the function
        // index. Their ports are the arguments they read — seq_star's persistent
        // current-index and a run-status decorator's latch are internal state,
        // not Groot ports.
        if crate::is_native(tree_node.function) {
            let mut param_args = HashMap::new();
            for (param, expression) in &tree_node.parameters {
                let Some((port, _)) = builtin_ports(tree_node.function)
                    .iter()
                    .find(|(_, id)| id == param)
                else {
                    continue;
                };
                param_args.insert(
                    port.to_string(),
                    builtin_port_text(*param, expression, variables)?,
                );
            }
            return Ok(Node {
                id: groot_id,
                name: Uuid::new_v4().to_string(),
                param_args,
                children: groot_children,
            });
        }

        let function =
            index
                .get(&tree_node.function)
                .ok_or(BehaviorTreeError::InconsistentTreeError {
                    message: format!(
                        "node refers to function {} that could not be resolved",
                        tree_node.function
                    ),
                })?;
        let mut param_args = HashMap::new();
        for (param, arg) in &tree_node.parameters {
            let param_arg = arora_param_to_groot((param, arg), function, variables)?;
            param_args.insert(param_arg.0, param_arg.1);
        }
        Ok(Node {
            id: groot_id,
            name: Uuid::new_v4().to_string(),
            param_args,
            children: groot_children,
        })
    }
}

/// The indexed function a Groot tag names, for the tags that are not built in
/// or in the palette above: any module function is reachable from a Groot tree
/// under its own name, so a palette written for a module needs no entry here.
///
/// A tag matches a function's name exactly, or as the PascalCase spelling of
/// its snake_case name (`Walk` for `walk`, `PlayClip` for `play_clip`), the
/// convention the palette's own tags follow. The palette is matched first, so
/// a module export spelled like a palette entry (`cos`, `store`, `status`,
/// `increase`, `equal`, `write_keys`, `run_call`, `run_status`, …) is
/// reachable only under its exact snake_case name. The index
/// spans every loaded module, host and guest alike, so a name two modules both
/// export is refused as ambiguous rather than resolved to whichever the index
/// happens to yield first. The interpreter module's own functions are not
/// reachable from a tree.
fn function_by_groot_tag(
    index: &HashMap<Uuid, ModuleFunction>,
    tag: &str,
) -> Result<Uuid, BehaviorTreeError> {
    let mut matches: Vec<&ModuleFunction> = index
        .values()
        .filter(|function| {
            !crate::is_interpreter_function(function)
                && (function.function_name == tag || pascal_case(&function.function_name) == tag)
        })
        .collect();
    match matches.len() {
        1 => Ok(matches.remove(0).function_id),
        0 => Err(BehaviorTreeError::InconsistentTreeError {
            message: format!("unexpected node id: {tag} (no loaded module exports it)"),
        }),
        _ => Err(BehaviorTreeError::InconsistentTreeError {
            message: format!(
                "ambiguous node id: {tag} is exported by several modules ({})",
                matches
                    .iter()
                    .map(|function| function.module_id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

/// `snake_case` → `PascalCase`: each underscore-separated word capitalized.
fn pascal_case(name: &str) -> String {
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// A native node's Groot ports: each port's name and the parameter it sets.
fn builtin_ports(function: Uuid) -> &'static [(&'static str, Uuid)] {
    match function {
        EQUAL_FUNCTION_ID => &[("a", EQUAL_A_PARAM_ID), ("b", EQUAL_B_PARAM_ID)],
        WRITE_KEYS_FUNCTION_ID => &[
            ("keys", WRITE_KEYS_KEYS_PARAM_ID),
            ("values", WRITE_KEYS_VALUES_PARAM_ID),
        ],
        RUN_CALL_FUNCTION_ID => &[("call", RUN_CALL_PARAM_ID)],
        RUN_STATUS_FUNCTION_ID => &[("status", RUN_STATUS_OUT_PARAM_ID)],
        _ => &[],
    }
}

/// The prefix of a port text holding a value as JSON — the value's serde form,
/// e.g. `json:{"u64":1}` — so a typed literal survives Groot. It is
/// BehaviorTree.CPP's convention for structured port values.
const JSON_PREFIX: &str = "json:";

/// A native node's port text as an argument: `{name}` is the variable of that
/// name; `json:…` is a value in its serde form; a `WriteKeys` key table is its
/// keys joined by `;` (BehaviorTree.CPP's form for a list); anything else is a
/// string.
fn builtin_port_expression(
    param: Uuid,
    text: &str,
    variables: &mut HashMap<String, Uuid>,
) -> Result<Expression, BehaviorTreeError> {
    if let Some(name) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
        let id = *variables
            .entry(name.to_string())
            .or_insert_with(Uuid::new_v4);
        return Ok(Expression::VariableId(id));
    }
    if let Some(json) = text.strip_prefix(JSON_PREFIX) {
        let value: Value =
            serde_json::from_str(json).map_err(|e| BehaviorTreeError::ParsingError {
                message: format!("port value '{text}' is not a value in JSON: {e}"),
            })?;
        return Ok(Expression::Value(value));
    }
    if param == WRITE_KEYS_KEYS_PARAM_ID {
        let keys = if text.is_empty() {
            Vec::new()
        } else {
            text.split(';').map(str::to_string).collect()
        };
        return Ok(Expression::Value(Value::ArrayString(keys)));
    }
    Ok(Expression::Value(Value::String(text.to_string())))
}

/// Whether `text`, written as a port's plain text, reads back as itself: not
/// empty, not a `{name}` variable, not `json:`, and with no tab or line break
/// (an attribute value turns them into spaces).
fn reads_back_as_plain(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(JSON_PREFIX)
        && !(text.starts_with('{') && text.ends_with('}'))
        && !text.contains(['\t', '\n', '\r'])
}

/// A native node's argument as port text: the inverse of
/// [`builtin_port_expression`].
fn builtin_port_text(
    param: Uuid,
    expression: &Expression,
    variables: &mut HashMap<Uuid, String>,
) -> Result<String, BehaviorTreeError> {
    Ok(match expression {
        Expression::VariableId(id) => {
            let name = variables
                .entry(*id)
                .or_insert_with(|| id.to_string())
                .clone();
            format!("{{{name}}}")
        }
        Expression::Value(Value::ArrayString(keys))
            if param == WRITE_KEYS_KEYS_PARAM_ID
                && keys.iter().all(|key| !key.contains(';'))
                && reads_back_as_plain(&keys.join(";")) =>
        {
            keys.join(";")
        }
        Expression::Value(Value::String(text))
            if param != WRITE_KEYS_KEYS_PARAM_ID && reads_back_as_plain(text) =>
        {
            text.clone()
        }
        Expression::Value(value) => {
            let json = serde_json::to_string(value).map_err(|e| {
                BehaviorTreeError::InconsistentTreeError {
                    message: format!("a value does not convert to JSON: {e}"),
                }
            })?;
            // A value JSON cannot hold (a non-finite float) has no Groot form.
            if serde_json::from_str::<Value>(&json).ok().as_ref() != Some(value) {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: format!("{value} has no exact JSON form"),
                });
            }
            format!("{JSON_PREFIX}{json}")
        }
        other => {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: format!("a native node's argument has no Groot form: {other:?}"),
            })
        }
    })
}

pub fn seq(children: Vec<Node>) -> Node {
    Node {
        id: SEQ_GROOT_ID.to_string(),
        name: Uuid::new_v4().to_string(),
        children,
        param_args: HashMap::new(),
    }
}

pub fn action(type_name: &str, param_args: HashMap<&str, &str>) -> Node {
    Node {
        id: type_name.to_string(),
        name: Uuid::new_v4().to_string(),
        children: Vec::new(),
        param_args: to_string_map(param_args),
    }
}

#[macro_export]
macro_rules! param_args {
  ($( $key: expr => $val: expr ),*) => {{
       let mut map = ::std::collections::HashMap::new();
       $( map.insert($key, $val); )*
       map
  }}
}

const SUCCEED_GROOT_ID: &str = "Succeed";
const FAIL_GROOT_ID: &str = "Fail";
const RUN_GROOT_ID: &str = "Run";
const STATUS_IDENTITY_GROOT_ID: &str = "Status";
const STORE_GROOT_ID: &str = "Store";
const INCREASE_GROOT_ID: &str = "Increase";
const SEQ_GROOT_ID: &str = "Sequence";
const SEQ_STAR_GROOT_ID: &str = "SequenceStar";
const FALLBACK_GROOT_ID: &str = "Fallback";
const PARALLEL_GROOT_ID: &str = "Parallel";
const COS_GROOT_ID: &str = "Cos";
const EQUAL_GROOT_ID: &str = "Equal";
const WRITE_KEYS_GROOT_ID: &str = "WriteKeys";
const RUN_CALL_GROOT_ID: &str = "RunCall";
const RUN_STATUS_GROOT_ID: &str = "RunStatus";
const SET_STR_GROOT_ID: &str = "SetString";
const UNSET_STR_GROOT_ID: &str = "UnsetString";
const IS_STR_SET_GROOT_ID: &str = "IsStringSet";
const WAIT_STR_SET_GROOT_ID: &str = "WaitStringSet";
const REGEX_MATCH_GROOT_ID: &str = "RegexMatch";

/// UUID for behavior_tree.Status type
const STATUS_TYPE_ID: Uuid = Uuid::from_bytes([
    0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
]);

/// Check if a function returns Status (vs some other type that needs _ret binding)
fn returns_status(return_ty: &FrozenTy) -> bool {
    match return_ty {
        FrozenTy::FrozenScalar(scalar) => scalar.reference.id == STATUS_TYPE_ID,
        _ => false,
    }
}

/// Converts a Groot parameter into an Arora one.
/// Requires some context to do so:
/// - the function to which the parameter belongs,
/// - a local mapping of names and variable IDs.
/// If the argument is surrounded by {}, the result will be a variable expression.
/// Otherwise, the result will be a value expression.
#[allow(clippy::doc_lazy_continuation)]
fn groot_param_arg_to_arora(
    param_arg: (&String, &String),
    module_function: &ModuleFunction,
    variables: &mut HashMap<String, Uuid>,
) -> Result<(Uuid, Expression), BehaviorTreeError> {
    let param_matches: Vec<&Uuid> = module_function
        .function
        .parameter_ordering
        .iter()
        .filter(|parameter_id| {
            let parameter = module_function
                .function
                .parameters
                .get(parameter_id)
                .unwrap();
            parameter.name == *param_arg.0
        })
        .collect();
    match param_matches.len() {
        0 => {
            // Behavior tree layer: if parameter not found but function has a return value,
            // treat this as the return value binding (_RET_PARAM_ID)
            if !returns_status(&module_function.function.return_ty) {
                let expression = if param_arg.1.starts_with("{") && param_arg.1.ends_with("}") {
                    let variable_name = &param_arg.1[1..param_arg.1.len() - 1];
                    let maybe_id = variables.get(variable_name);
                    let id = if let Some(id) = maybe_id {
                        id.to_owned()
                    } else {
                        let id = Uuid::new_v4();
                        variables.insert(variable_name.to_owned(), id.to_owned());
                        id
                    };
                    // A variable reference, so the graph lowering binds the
                    // return to the named store slot rather than to a literal.
                    Expression::VariableId(id)
                } else {
                    Expression::Value(Value::String(param_arg.1.to_owned()))
                };
                Ok((_RET_PARAM_ID, expression))
            } else {
                Err(BehaviorTreeError::InternalError {
                    message: format!(
                        "no such parameter \"{}\" in function \"{}\"",
                        param_arg.0, module_function.function_name
                    ),
                })
            }
        }
        1 => {
            let expression = if param_arg.1.starts_with("{") && param_arg.1.ends_with("}") {
                let variable_name = &param_arg.1[1..param_arg.1.len() - 1];
                let maybe_id = variables.get(variable_name);
                let id = if let Some(id) = maybe_id {
                    id.to_owned()
                } else {
                    let id = Uuid::new_v4();
                    variables.insert(variable_name.to_owned(), id.to_owned());
                    id
                };
                Expression::VariableId(id)
            } else {
                Expression::Value(Value::String(param_arg.1.to_owned()))
            };
            let parameter_id = param_matches.first().unwrap();
            Ok((*parameter_id.to_owned(), expression))
        }
        _ => Err(BehaviorTreeError::InternalError {
            message: format!(
                "several parameters found \"{}\" in function \"{}\"",
                param_arg.0, module_function.function_name
            ),
        }),
    }
}

fn arora_param_to_groot(
    param_arg: (&Uuid, &Expression),
    module_function: &ModuleFunction,
    variables: &mut HashMap<Uuid, String>,
) -> Result<(String, String), BehaviorTreeError> {
    // Behavior tree layer: handle _RET_PARAM_ID specially
    if *param_arg.0 == _RET_PARAM_ID {
        let value = match param_arg.1 {
            Expression::Uuid(id) | Expression::VariableId(id) => {
                let maybe_name = variables.get(id);
                let name = if let Some(name) = maybe_name {
                    name.to_owned()
                } else {
                    let id = Uuid::new_v4();
                    variables.insert(id.to_owned(), id.to_string());
                    id.to_string()
                };
                format!("{{{}}}", name)
            }
            Expression::Value(value) => value.to_string(),
            _ => {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: "unsupported expression type for Groot conversion".to_string(),
                })
            }
        };
        return Ok(("_ret".to_string(), value));
    }

    let function = &module_function.function;
    let param_matches: Vec<&Parameter> = function
        .parameter_ordering
        .iter()
        .filter_map(|parameter_id| {
            let parameter = function.parameters.get(parameter_id).unwrap();
            if *parameter_id == *param_arg.0 {
                Some(parameter)
            } else {
                None
            }
        })
        .collect();
    match param_matches.len() {
        0 => Err(BehaviorTreeError::InternalError {
            message: format!(
                "no such parameter \"{}\" in function \"{}\"",
                param_arg.0, module_function.function_name
            ),
        }),
        1 => {
            let function_parameter = param_matches.first().unwrap();
            let value = match param_arg.1 {
                Expression::Uuid(id) | Expression::VariableId(id) => {
                    let maybe_name = variables.get(id);
                    let name = if let Some(name) = maybe_name {
                        name.to_owned()
                    } else {
                        let id = Uuid::new_v4();
                        variables.insert(id.to_owned(), id.to_string());
                        id.to_string()
                    };
                    format!("{{{}}}", name)
                }
                Expression::Value(value) => value.to_string(),
                _ => {
                    return Err(BehaviorTreeError::InconsistentTreeError {
                        message: format!(
                            "param {} of function {} has a value of an unsupported type: {:?}",
                            param_arg.0, module_function.function_name, param_arg.1
                        ),
                    })
                }
            };
            Ok((function_parameter.name.to_owned(), value))
        }
        _ => Err(BehaviorTreeError::InternalError {
            message: format!(
                "several parameters found \"{}\" in function \"{}\"",
                param_arg.0, module_function.function_name
            ),
        }),
    }
}

impl BehaviorTree {
    pub fn try_from_groot_xml(xml_str: &str) -> Result<BehaviorTree, BehaviorTreeError> {
        parse_groot_xml(xml_str)
    }

    pub fn to_groot_xml(&self) -> Vec<u8> {
        serialize_behavior_to_groot_xml(self)
    }

    /// Lower this Groot tree into the shared [`Graph`], resolving action
    /// parameters against `index`.
    ///
    /// The Groot front-end already lowers to a [`TreeNode`] (name → arora ids,
    /// `{var}` → variable ids); this then assigns node ids and turns each node's
    /// parameters into graph [`Link`]s. Named `{var}`s become
    /// [`Graph::variables`], so an interpreter can bind them to store slots (the
    /// Direct convention). This is the promoted import path — the arora runtime
    /// builds an editable interpreter from the returned graph.
    pub fn into_graph(
        &self,
        index: &HashMap<Uuid, ModuleFunction>,
    ) -> Result<Graph, BehaviorTreeError> {
        let mut names: HashMap<String, Uuid> = HashMap::new();
        let tree_node = self.root.try_into_tree_node(index, &mut names)?;
        let mut graph = Graph::empty();
        let root_id = lower_tree_node_to_graph(tree_node, &mut graph)?;
        graph.root = Some(root_id);
        graph.variables = names.into_iter().map(|(name, id)| (id, name)).collect();
        Ok(graph)
    }
}

/// Lower a [`TreeNode`] (and its subtree) into `graph`, returning the assigned id
/// of the node. Each parameter becomes a graph [`Link`] feeding this node's input
/// slot; children recurse.
fn lower_tree_node_to_graph(
    tree_node: TreeNode,
    graph: &mut Graph,
) -> Result<Uuid, BehaviorTreeError> {
    let node_id = Uuid::new_v4();
    let children = match tree_node.children {
        Some(children) => {
            let mut ids = Vec::with_capacity(children.len());
            for child in children {
                ids.push(lower_tree_node_to_graph(child, graph)?);
            }
            Some(ids)
        }
        None => None,
    };
    let mut inputs = Vec::with_capacity(tree_node.parameters.len());
    for (param_id, expression) in &tree_node.parameters {
        inputs.push(Io::new(*param_id));
        let source = groot_expression_to_link_source(expression)?;
        graph
            .links
            .push(Link::new(Port::new(node_id, *param_id), source));
    }
    graph.nodes.insert(
        node_id,
        GraphNode {
            id: node_id,
            function: tree_node.function,
            inputs,
            outputs: Vec::new(),
            children,
        },
    );
    Ok(node_id)
}

/// A Groot-lowered [`Expression`] as a graph [`LinkSource`]. Groot only ever
/// produces literals, `{var}` references and (for a `_ret` binding) a bare uuid;
/// a `Uuid` seeds a literal holding its bytes, matching how the tree seeds a
/// `Uuid` argument cell.
fn groot_expression_to_link_source(
    expression: &Expression,
) -> Result<LinkSource, BehaviorTreeError> {
    Ok(match expression {
        Expression::Value(value) => LinkSource::Literal(value.clone()),
        Expression::Uuid(uuid) => LinkSource::Literal(Value::ArrayU8(uuid.as_bytes().to_vec())),
        Expression::VariableId(id) => LinkSource::Variable(*id),
        Expression::NodeArgument(np) => LinkSource::Port(Port::new(np.node, np.parameter)),
        Expression::Select { source, path } => LinkSource::Select {
            source: Box::new(groot_expression_to_link_source(source)?),
            path: path.clone(),
        },
        Expression::Variable(_) | Expression::Call(_) => {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: "Groot lowering does not support runtime variable cells or nested \
                          call expressions"
                    .to_string(),
            })
        }
    })
}

fn parse_groot_xml(xml_str: &str) -> Result<BehaviorTree, BehaviorTreeError> {
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text_start = true;
    reader.config_mut().trim_text_end = true;
    let mut buf = Vec::new();
    let root = parse_groot_root(&mut reader, &mut buf)?;
    // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
    buf.clear();
    Ok(BehaviorTree { root })
}

fn parse_groot_root(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Node, BehaviorTreeError> {
    let root = match reader.read_event() {
        Ok(Event::Decl(_)) => parse_groot_root(reader, buf)?,
        Ok(Event::Start(ref root_start)) => {
            if root_start.name().as_ref() != b"root" {
                return Err(BehaviorTreeError::ParsingError {
                    message: "root tag is not \"root\"".to_string(),
                });
            }
            parse_groot_behavior_tree_node(reader, buf)?
        }
        Err(e) => forward_parsing_error("Error parsing XML", reader, e)?,
        _ => new_parsing_error_result("XML does not start with a valid root tag", reader)?,
    };
    Ok(root)
}

fn parse_groot_behavior_tree_node(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Node, BehaviorTreeError> {
    match reader.read_event() {
        Ok(Event::Start(ref node_start)) => {
            if node_start.name().as_ref() != b"BehaviorTree" {
                return Err(BehaviorTreeError::ParsingError {
                    message: "found node that is not a \"BehaviorTree\"".to_string(),
                });
            }
            parse_groot_node(reader, buf)?
                .ok_or(new_parsing_error("behavior tree has no root node", reader))
        }
        Err(e) => forward_parsing_error("Error parsing XML", reader, e)?,
        Ok(Event::Comment(_)) => parse_groot_behavior_tree_node(reader, buf),
        _ => new_parsing_error_result("XML does not contain a \"BehaviorTree\" node", reader)?,
    }
}

// `buf` is threaded through the recursive descent to satisfy the reader API.
#[allow(clippy::only_used_in_recursion)]
fn parse_groot_node(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Option<Node>, BehaviorTreeError> {
    match reader.read_event() {
        Ok(Event::Start(ref node_start)) => {
            let id = String::from_utf8(node_start.name().as_ref().to_vec());
            let id = map_parsing_error(id, "invalid utf8 in action ID", reader)?;

            let mut attributes = collect_action_attributes(node_start, reader)?;
            if !attributes.remove(ID_ATTRIBUTE_KEY).is_none() {
                new_parsing_error_result("redundant ID attribute for action", reader)?
            }
            let name = attributes.remove(NAME_ATTRIBUTE_KEY);

            let mut children = Vec::new();
            loop {
                let child = parse_groot_node(reader, buf)?;
                match child {
                    Some(child) => children.push(child),
                    None => break,
                }
            }
            Ok(Some(Node {
                id,
                name: name.unwrap_or(Uuid::new_v4().to_string()),
                param_args: attributes,
                children,
            }))
        }
        Ok(Event::Empty(ref node_empty)) => match node_empty.name().as_ref() {
            b"Action" => {
                let mut attributes = collect_action_attributes(node_empty, reader)?;
                let id = attributes
                    .remove(ID_ATTRIBUTE_KEY)
                    .ok_or(new_parsing_error("missing ID attribute of action", reader))?;
                let name = attributes.remove(NAME_ATTRIBUTE_KEY);
                Ok(Some(Node {
                    id,
                    name: name.unwrap_or(Uuid::new_v4().to_string()),
                    param_args: attributes,
                    children: Vec::new(),
                }))
            }
            tag => {
                let id = String::from_utf8(tag.to_vec());
                let id = map_parsing_error(id, "invalid utf8 in action ID", reader)?;
                let mut attributes = collect_action_attributes(node_empty, reader)?;
                if !attributes.remove(ID_ATTRIBUTE_KEY).is_none() {
                    new_parsing_error_result("redundant ID attribute for action", reader)?
                }
                let name = attributes.remove(NAME_ATTRIBUTE_KEY);
                Ok(Some(Node {
                    id,
                    name: name.unwrap_or(Uuid::new_v4().to_string()),
                    param_args: attributes,
                    children: Vec::new(),
                }))
            }
        },
        Ok(Event::End(_)) => Ok(None),
        Ok(Event::Eof) => {
            new_parsing_error_result("XML file ends before the root node is closed", reader)?
        }
        Ok(event) => new_parsing_error_result(
            format!("unexpected XML element: {:?}", event).as_str(),
            reader,
        )?,
        Err(e) => forward_parsing_error("Error", reader, e)?,
    }
}

/// Collects XML node attributes.
fn collect_action_attributes(
    node: &BytesStart,
    reader: &mut Reader<&[u8]>,
) -> Result<HashMap<String, String>, BehaviorTreeError> {
    let mut attributes = HashMap::new();
    for attr in node.attributes() {
        let attr = map_parsing_error(attr, "cannot get attribute", reader)?;
        let key = String::from_utf8(attr.key.as_ref().to_vec());
        let key = map_parsing_error(key, "invalid utf8 in attribute key", reader)?;
        let value = attr.normalized_value(quick_xml::XmlVersion::Explicit1_1);
        let value = map_parsing_error(
            value,
            format!("error unescaping value of attribute {}", key).as_str(),
            reader,
        )?;
        let value = value.to_string();
        if attributes.insert(key.clone(), value).is_some() {
            new_parsing_error_result(
                format!("error unescaping value of attribute {}", key).as_str(),
                reader,
            )?
        };
    }
    Ok(attributes)
}

fn new_parsing_error(preamble: &str, reader: &Reader<&[u8]>) -> BehaviorTreeError {
    BehaviorTreeError::ParsingError {
        message: format!("{} at position {}", preamble, reader.buffer_position()),
    }
}

fn new_parsing_error_result<T>(
    preamble: &str,
    reader: &Reader<&[u8]>,
) -> Result<T, BehaviorTreeError> {
    Err(BehaviorTreeError::ParsingError {
        message: format!("{} at position {}", preamble, reader.buffer_position()),
    })
}

fn forward_parsing_error<T>(
    preamble: &str,
    reader: &Reader<&[u8]>,
    error: quick_xml::Error,
) -> Result<T, BehaviorTreeError> {
    Err(BehaviorTreeError::ParsingError {
        message: format!(
            "{} at position {}: {:?}",
            preamble,
            reader.buffer_position(),
            error
        )
        .to_string(),
    })
}

fn map_parsing_error<T, E: Error>(
    result: Result<T, E>,
    preamble: &str,
    reader: &Reader<&[u8]>,
) -> Result<T, BehaviorTreeError> {
    result.map_err(|error| BehaviorTreeError::ParsingError {
        message: format!(
            "{} at position {}: {:?}",
            preamble,
            reader.buffer_position(),
            error
        )
        .to_string(),
    })
}

fn serialize_behavior_to_groot_xml(behavior: &BehaviorTree) -> Vec<u8> {
    use quick_xml::events::BytesEnd;

    let mut writer = Writer::new(Cursor::new(Vec::new()));

    const ROOT_NAME: &str = "root";
    let mut root_elem = BytesStart::new(ROOT_NAME);
    root_elem.push_attribute(("main_tree_to_execute", "MainTree"));
    writer.write_event(Event::Start(root_elem)).unwrap();

    const BEHAVIOR_TREE_NAME: &str = "BehaviorTree";
    let mut behavior_elem = BytesStart::new(BEHAVIOR_TREE_NAME);
    behavior_elem.push_attribute((ID_ATTRIBUTE_KEY, "MainTree"));
    writer.write_event(Event::Start(behavior_elem)).unwrap();

    serialize_node_to_groot_xml(&behavior.root, &mut writer);

    writer
        .write_event(Event::End(BytesEnd::new(BEHAVIOR_TREE_NAME)))
        .unwrap();
    writer
        .write_event(Event::End(BytesEnd::new(ROOT_NAME)))
        .unwrap();
    writer.into_inner().into_inner()
}

fn serialize_node_to_groot_xml(node: &Node, writer: &mut Writer<Cursor<Vec<u8>>>) {
    use quick_xml::events::BytesEnd;

    let mut elem = BytesStart::new(node.id.as_str());
    elem.push_attribute((NAME_ATTRIBUTE_KEY, node.name.as_str()));
    for (param, arg) in &node.param_args {
        elem.push_attribute((param.as_str(), arg.as_str()));
    }
    if node.children.is_empty() {
        writer.write_event(Event::Empty(elem)).unwrap();
    } else {
        writer.write_event(Event::Start(elem)).unwrap();
        for child in &node.children {
            serialize_node_to_groot_xml(child, writer);
        }
        writer
            .write_event(Event::End(BytesEnd::new(node.id.as_str())))
            .unwrap();
    }
}

const ID_ATTRIBUTE_KEY: &str = "ID";
const NAME_ATTRIBUTE_KEY: &str = "name";

impl Display for BehaviorTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.root))
    }
}

impl Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}(", self.id))?;
        display_param_args(&self.param_args);
        f.write_char(')')?;
        if !self.children.is_empty() {
            f.write_fmt(format_args!(":"))?;
            display_children(f, &self.children)?;
        }
        Ok(())
    }
}

fn display_children(f: &mut std::fmt::Formatter<'_>, children: &[Node]) -> std::fmt::Result {
    for child in children {
        f.write_fmt(format_args!("\n- {}", child))?
    }
    Ok(())
}

fn display_param_args(param_args: &HashMap<String, String>) -> String {
    param_args
        .iter()
        .map(|(key, value)| format!("{}=\"{}\"", key, value))
        .collect::<Vec<String>>()
        .join(", ")
}

fn to_string_map(m: HashMap<&str, &str>) -> HashMap<String, String> {
    HashMap::from_iter(m.into_iter().map(|(k, v)| (k.to_string(), v.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::record::module::frozen::Function;
    use arora_types::record::ty::{FrozenScalar, PrimitiveKind};
    use arora_types::record::FrozenReference;

    /// Index a `Status`-returning function `name(speed: f32)` under `module`,
    /// the way a loaded module's export is indexed. Returns its function id.
    fn index_status_leaf(
        index: &mut HashMap<Uuid, ModuleFunction>,
        module: Uuid,
        name: &str,
    ) -> Uuid {
        let function_id = Uuid::new_v4();
        let speed = Uuid::new_v4();
        index.insert(
            function_id,
            ModuleFunction {
                module_id: module,
                function_id,
                function_name: name.to_string(),
                function: Function {
                    parameters: HashMap::from([(
                        speed,
                        Parameter {
                            name: "speed".to_string(),
                            ty: FrozenTy::from(PrimitiveKind::F32),
                            mutable: false,
                        },
                    )]),
                    parameter_ordering: vec![speed],
                    return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                        reference: FrozenReference {
                            id: STATUS_TYPE_ID,
                            version: arora_behavior::STATUS_ENUMERATION_VERSION.into(),
                        },
                    }),
                },
            },
        );
        function_id
    }

    fn groot(body: &str) -> BehaviorTree {
        let xml = format!(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">{body}</BehaviorTree></root>"#
        );
        BehaviorTree::try_from_groot_xml(&xml).expect("well-formed Groot XML")
    }

    /// A tag outside the palette resolves to the indexed function of that
    /// name — spelled as declared or in PascalCase — with its arguments bound
    /// like any palette node's.
    #[test]
    fn a_module_function_is_reachable_under_its_name_or_its_pascal_case() {
        let mut index = HashMap::new();
        let walk = index_status_leaf(&mut index, Uuid::new_v4(), "walk");
        let play_clip = index_status_leaf(&mut index, Uuid::new_v4(), "play_clip");

        let tree = groot(r#"<Sequence><walk speed="{cmd}"/><PlayClip speed="0.5f32"/></Sequence>"#);
        let mut variables = HashMap::new();
        let root = tree
            .root
            .try_into_tree_node(&index, &mut variables)
            .expect("both tags resolve");
        let children = root.children.expect("the sequence keeps its children");
        assert_eq!(children[0].function, walk);
        assert_eq!(children[1].function, play_clip);
        assert!(
            variables.contains_key("cmd"),
            "the {{cmd}} argument became a named variable"
        );
    }

    /// A tag no loaded module exports is refused, as before.
    #[test]
    fn a_tag_no_module_exports_is_refused() {
        let mut index = HashMap::new();
        index_status_leaf(&mut index, Uuid::new_v4(), "walk");
        let tree = groot(r#"<Fly speed="1.0f32"/>"#);
        let error = tree
            .root
            .try_into_tree_node(&index, &mut HashMap::new())
            .map(|_| ())
            .expect_err("Fly is exported by no module");
        assert!(format!("{error:?}").contains("Fly"));
    }

    /// A name two modules both export is refused rather than resolved to
    /// whichever the index yields first.
    #[test]
    fn a_name_two_modules_export_is_ambiguous() {
        let mut index = HashMap::new();
        index_status_leaf(&mut index, Uuid::new_v4(), "walk");
        index_status_leaf(&mut index, Uuid::new_v4(), "walk");
        let tree = groot(r#"<Walk speed="1.0f32"/>"#);
        let error = tree
            .root
            .try_into_tree_node(&index, &mut HashMap::new())
            .map(|_| ())
            .expect_err("two modules export walk");
        assert!(format!("{error:?}").contains("ambiguous"));
    }

    /// The export writes a module function under its declared name — the
    /// spelling the importer accepts — so a tree round-trips.
    #[test]
    fn a_module_function_exports_under_its_name_and_round_trips() {
        let mut index = HashMap::new();
        let walk = index_status_leaf(&mut index, Uuid::new_v4(), "walk");
        let node = TreeNode::action_node(walk);
        let mut names = HashMap::new();
        let exported = Node::try_from_tree_node(&node, &index, &mut names).expect("exported");
        assert_eq!(exported.id, "walk");
        let back = exported
            .try_into_tree_node(&index, &mut HashMap::new())
            .expect("the exported tag resolves again");
        assert_eq!(back.function, walk);
    }

    /// `Equal`, `WriteKeys`, `RunCall` and `RunStatus` read from Groot with
    /// their ports, export back to the same ports, and the tree runs.
    #[test]
    fn native_data_and_run_nodes_round_trip_through_groot() {
        let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Sequence>
      <Equal a="{revision}" b='json:{"u64":1}'/>
      <WriteKeys keys="out/a;;out/c" values='json:{"f64s":[1.0,2.0,3.0]}'/>
      <RunStatus status="{run/status}">
        <Succeed/>
      </RunStatus>
    </Sequence>
  </BehaviorTree>
</root>"#;
        let groot = BehaviorTree::try_from_groot_xml(xml).expect("parse");
        let mut names = HashMap::new();
        let tree = groot
            .root
            .try_into_tree_node(&HashMap::new(), &mut names)
            .expect("the tags resolve");
        let children = tree.children.as_ref().expect("a sequence");
        let equal = &children[0];
        assert_eq!(equal.function, EQUAL_FUNCTION_ID);
        assert_eq!(
            equal.parameters[&EQUAL_B_PARAM_ID],
            Expression::Value(Value::U64(1))
        );
        assert_eq!(
            equal.parameters[&EQUAL_A_PARAM_ID],
            Expression::VariableId(names["revision"])
        );
        let write = &children[1];
        assert_eq!(
            write.parameters[&WRITE_KEYS_KEYS_PARAM_ID],
            Expression::Value(Value::ArrayString(vec![
                "out/a".to_string(),
                String::new(),
                "out/c".to_string()
            ]))
        );
        assert_eq!(
            write.parameters[&WRITE_KEYS_VALUES_PARAM_ID],
            Expression::Value(Value::ArrayF64(vec![1.0, 2.0, 3.0]))
        );
        assert_eq!(children[2].function, RUN_STATUS_FUNCTION_ID);

        // Export and import again: the same nodes and arguments.
        let mut by_id: HashMap<Uuid, String> =
            names.iter().map(|(name, id)| (*id, name.clone())).collect();
        let exported =
            Node::try_from_tree_node(&tree, &HashMap::new(), &mut by_id).expect("exported");
        let xml = String::from_utf8(BehaviorTree { root: exported }.to_groot_xml()).unwrap();
        let mut names_again = HashMap::new();
        let again = BehaviorTree::try_from_groot_xml(&xml)
            .expect("the export parses")
            .root
            .try_into_tree_node(&HashMap::new(), &mut names_again)
            .expect("the export resolves");
        let again_children = again.children.as_ref().unwrap();
        for (before, after) in children.iter().zip(again_children) {
            assert_eq!(before.function, after.function);
            for (param, expression) in &before.parameters {
                match expression {
                    Expression::VariableId(id) => {
                        let name = &by_id[id];
                        assert_eq!(
                            after.parameters[param],
                            Expression::VariableId(names_again[name])
                        );
                    }
                    other => assert_eq!(&after.parameters[param], other, "{xml}"),
                }
            }
        }

        // An unknown port is refused.
        let bad = r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
    <Equal a="{x}" c="1"/></BehaviorTree></root>"#;
        assert!(BehaviorTree::try_from_groot_xml(bad)
            .unwrap()
            .root
            .try_into_tree_node(&HashMap::new(), &mut HashMap::new())
            .is_err());

        // The imported tree builds.
        let graph = groot
            .into_graph(&HashMap::new())
            .expect("lowers to a graph");
        crate::graph::build_behavior_tree(&graph, &|_| None).expect("the tree builds");
    }

    /// Export writes plain text only where it reads back as the same value, and
    /// import refuses an unknown or missing port on a node with ports, while a
    /// control node's attributes are not arguments.
    #[test]
    fn groot_ports_read_back_as_written() {
        let export = |param: Uuid, value: Value| {
            builtin_port_text(param, &Expression::Value(value), &mut HashMap::new())
        };
        let import = |param: Uuid, text: &str| {
            builtin_port_expression(param, text, &mut HashMap::new()).unwrap()
        };
        for keys in [
            vec![],
            vec![String::new()],
            vec!["{a}".to_string()],
            vec!["json:x".to_string()],
            vec!["a\nb".to_string(), "c".to_string()],
            vec!["a;b".to_string()],
            vec!["out/a".to_string(), String::new(), "out/c".to_string()],
        ] {
            let value = Value::ArrayString(keys);
            let text = export(WRITE_KEYS_KEYS_PARAM_ID, value.clone()).unwrap();
            assert_eq!(
                import(WRITE_KEYS_KEYS_PARAM_ID, &text),
                Expression::Value(value),
                "{text}"
            );
        }
        for text in ["", "{a}", "json:x", "a\tb", "plain"] {
            let value = Value::String(text.to_string());
            let written = export(EQUAL_B_PARAM_ID, value.clone()).unwrap();
            assert_eq!(import(EQUAL_B_PARAM_ID, &written), Expression::Value(value));
        }
        assert!(export(EQUAL_B_PARAM_ID, Value::F64(f64::NAN)).is_err());

        let tree = |xml: &str| {
            BehaviorTree::try_from_groot_xml(&format!(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">{xml}</BehaviorTree></root>"#
            ))
            .unwrap()
            .root
            .try_into_tree_node(&HashMap::new(), &mut HashMap::new())
        };
        assert!(tree(r#"<Equal a="{x}"/>"#).is_err(), "a missing port");
        assert!(
            tree(r#"<RunStatus><Succeed/></RunStatus>"#).is_err(),
            "no status"
        );
        assert!(
            tree(r#"<Parallel success_count="1"><Succeed/></Parallel>"#).is_ok(),
            "a control node's attribute is not an argument"
        );
    }
}

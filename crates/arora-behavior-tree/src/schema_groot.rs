use arora_types::value::Value;
use quick_xml::events::BytesStart;
use quick_xml::Writer;
use quick_xml::{events::Event, Reader};
use semio_record::module::v0::frozen::Parameter;
use semio_record::ty::FrozenTy;
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
    COS_FUNCTION_ID, FAIL_FUNCTION_ID, FALLBACK_FUNCTION_ID, INCREASE_FUNCTION_ID,
    PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SEQ_STAR_FUNCTION_ID,
    STATUS_IDENTITY_FUNCTION_ID, STORE_FUNCTION_ID, SUCCEED_FUNCTION_ID,
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
            COS_GROOT_ID => COS_FUNCTION_ID,
            SET_STR_GROOT_ID => Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap(),
            UNSET_STR_GROOT_ID => Uuid::from_str("7dce01ed-9818-4b7d-b45a-2e7fdece3633").unwrap(),
            IS_STR_SET_GROOT_ID => Uuid::from_str("20ba3f0f-309e-4cd2-adfc-aca6cc432526").unwrap(),
            WAIT_STR_SET_GROOT_ID => {
                Uuid::from_str("3180977c-25a1-458e-ab82-11f36c654518").unwrap()
            }
            REGEX_MATCH_GROOT_ID => Uuid::from_str("8e3dbcc1-1a81-4cf6-a457-6e0c075456fd").unwrap(),
            id => {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: format!("unexpected node id: {}", id),
                })
            }
        };
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
            children: if tree_node_children.is_empty() {
                None
            } else {
                Some(tree_node_children)
            },
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
            id => {
                return Err(BehaviorTreeError::InconsistentTreeError {
                    message: format!("unexpected node id: {}", id),
                })
            }
        }
        .to_string();
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
                    Expression::Uuid(id)
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

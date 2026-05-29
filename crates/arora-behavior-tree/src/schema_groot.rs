use arora_types::value::Value;
use quick_xml::events::BytesStart;
use quick_xml::Writer;
use quick_xml::{events::Event, Reader};
use semio_record::module::v0::frozen::Parameter;
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
use crate::schema::Expression;
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
  children: Vec<Box<Node>>,
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
      tree_node_children.push(child.as_ref().try_into_tree_node(index, variables)?)
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
      WAIT_STR_SET_GROOT_ID => Uuid::from_str("3180977c-25a1-458e-ab82-11f36c654518").unwrap(),
      REGEX_MATCH_GROOT_ID => Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap(),
      id => {
        return Err(BehaviorTreeError::InconsistentTreeError {
          message: format!("unexpected node id: {}", id.to_string()),
        })
      }
    };
    let function = index
      .get(&arora_id)
      .ok_or(BehaviorTreeError::InternalError {
        message: format!("function {} is missing from index", arora_id.to_string()),
      })?;
    let mut parameters = HashMap::new();
    for param_arg in &self.param_args {
      let (param, arg) = groot_param_arg_to_arora(param_arg, &function, variables)?;
      parameters.insert(param, arg);
    }
    Ok(TreeNode {
      function: function.function_id,
      children: None,
      parameters,
      return_binding: None,
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
        groot_children.push(Box::new(Self::try_from_tree_node(child, index, variables)?))
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
      // Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap() => SET_STR_GROOT_ID,
      id => {
        return Err(BehaviorTreeError::InconsistentTreeError {
          message: format!("unexpected node id: {}", id.to_string()),
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
            tree_node.function.to_string()
          ),
        })?;
    let mut param_args = HashMap::new();
    for (param, arg) in &tree_node.parameters {
      let param_arg = arora_param_to_groot((param, arg), &function, variables)?;
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
    children: to_boxed_vec(children),
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

const SUCCEED_GROOT_ID: &'static str = "Succeed";
const FAIL_GROOT_ID: &'static str = "Fail";
const RUN_GROOT_ID: &'static str = "Run";
const STATUS_IDENTITY_GROOT_ID: &'static str = "Status";
const STORE_GROOT_ID: &'static str = "Store";
const INCREASE_GROOT_ID: &'static str = "Increase";
const SEQ_GROOT_ID: &'static str = "Sequence";
const SEQ_STAR_GROOT_ID: &'static str = "SequenceStar";
const FALLBACK_GROOT_ID: &'static str = "Fallback";
const PARALLEL_GROOT_ID: &'static str = "Parallel";
const COS_GROOT_ID: &'static str = "Cos";
const SET_STR_GROOT_ID: &'static str = "SetString";
const UNSET_STR_GROOT_ID: &'static str = "UnsetString";
const IS_STR_SET_GROOT_ID: &'static str = "IsStringSet";
const WAIT_STR_SET_GROOT_ID: &'static str = "WaitStringSet";
const REGEX_MATCH_GROOT_ID: &'static str = "RegexMatch";

/// Converts a Groot parameter into an Arora one.
/// Requires some context to do so:
/// - the function to which the parameter belongs,
/// - a local mapping of names and variable IDs.
/// If the argument is surrounded by {}, the result will be a variable expression.
/// Otherwise, the result will be a value expression.
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
    0 => Err(BehaviorTreeError::InternalError {
      message: format!(
        "no such parameter \"{}\" in function \"{}\"",
        param_arg.0, module_function.function_name
      ),
    }),
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
        Expression::Uuid(id) => {
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
    serialize_behavior_to_groot_xml(&self)
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
    Err(e) => forward_parsing_error("Error parsing XML", &reader, e)?,
    _ => new_parsing_error_result("XML does not start with a valid root tag", &reader)?,
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
        .ok_or(new_parsing_error("behavior tree has no root node", &reader))
    }
    Err(e) => forward_parsing_error("Error parsing XML", &reader, e)?,
    Ok(Event::Comment(_)) => parse_groot_behavior_tree_node(reader, buf),
    _ => new_parsing_error_result("XML does not contain a \"BehaviorTree\" node", &reader)?,
  }
}

fn parse_groot_node(
  reader: &mut Reader<&[u8]>,
  buf: &mut Vec<u8>,
) -> Result<Option<Node>, BehaviorTreeError> {
  match reader.read_event() {
    Ok(Event::Start(ref node_start)) => {
      let id = String::from_utf8(node_start.name().as_ref().to_vec());
      let id = map_parsing_error(id, "invalid utf8 in action ID", reader)?;

      let mut attributes = collect_action_attributes(node_start, reader)?;
      if !attributes.remove(&ID_ATTRIBUTE_KEY.to_string()).is_none() {
        new_parsing_error_result("redundant ID attribute for action", reader)?
      }
      let name = attributes.remove(&NAME_ATTRIBUTE_KEY.to_string());

      let mut children = Vec::new();
      loop {
        let child = parse_groot_node(reader, buf)?;
        match child {
          Some(child) => children.push(Box::new(child)),
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
          .remove(&ID_ATTRIBUTE_KEY.to_string())
          .ok_or(new_parsing_error("missing ID attribute of action", reader))?;
        let name = attributes.remove(&NAME_ATTRIBUTE_KEY.to_string());
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
        if !attributes.remove(&ID_ATTRIBUTE_KEY.to_string()).is_none() {
          new_parsing_error_result("redundant ID attribute for action", reader)?
        }
        let name = attributes.remove(&NAME_ATTRIBUTE_KEY.to_string());
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
      new_parsing_error_result("XML file ends before the root node is closed", &reader)?
    }
    Ok(event) => new_parsing_error_result(
      format!("unexpected XML element: {:?}", event).as_str(),
      &reader,
    )?,
    Err(e) => forward_parsing_error("Error", &reader, e)?,
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
    let value = attr.unescape_value();
    let value = map_parsing_error(
      value,
      format!("error unescaping value of attribute {}", key).as_str(),
      reader,
    )?;
    let value = value.to_string();
    match attributes.insert(key.clone(), value) {
      Some(_) => new_parsing_error_result(
        format!("error unescaping value of attribute {}", key).as_str(),
        reader,
      )?,
      None => (),
    };
  }
  Ok(attributes)
}

fn new_parsing_error(preamble: &str, reader: &Reader<&[u8]>) -> BehaviorTreeError {
  BehaviorTreeError::ParsingError {
    message: format!(
      "{} at position {}",
      preamble.to_string(),
      reader.buffer_position()
    ),
  }
}

fn new_parsing_error_result<T>(
  preamble: &str,
  reader: &Reader<&[u8]>,
) -> Result<T, BehaviorTreeError> {
  Err(BehaviorTreeError::ParsingError {
    message: format!(
      "{} at position {}",
      preamble.to_string(),
      reader.buffer_position()
    ),
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
      preamble.to_string(),
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
      preamble.to_string(),
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
      serialize_node_to_groot_xml(&child, writer);
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

fn display_children(
  f: &mut std::fmt::Formatter<'_>,
  children: &Vec<Box<Node>>,
) -> std::fmt::Result {
  for child in children {
    f.write_fmt(format_args!("\n- {}", child.as_ref()))?
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

fn to_boxed_vec<T>(v: Vec<T>) -> Vec<Box<T>> {
  v.into_iter().map(|child| Box::new(child)).collect()
}

fn to_string_map(m: HashMap<&str, &str>) -> HashMap<String, String> {
  HashMap::from_iter(m.into_iter().map(|(k, v)| (k.to_string(), v.to_string())))
}

#[cfg(test)]
pub mod tests {
  use super::{action, parse_groot_xml, seq, BehaviorTree, Node};
  use crate::{
    schema::Expression,
    tests::tests::{crate_root_path, read_header_to_index, BASE_MODULE_NAMES},
    ModuleFunction,
  };
  use anyhow::Result;
  use arora_registry::{local::LocalRegistry, local_yaml::load_records_from_yaml_dir};
  use arora_types::value::Value;
  use std::{collections::HashMap, path::Path};
  use tokio::fs::read_to_string;
  use uuid::Uuid;

  #[test]
  pub fn parse_simple_compact_groot_xml() -> Result<()> {
    let behavior = BehaviorTree::try_from_groot_xml(SIMPLE_COMPACT_GROOT_XML)?;
    assert_eq!(2, behavior.root.children.len());
    for child in behavior.root.children {
      assert_eq!(child.id.as_str(), "SaySomething");
    }
    Ok(())
  }

  #[test]
  pub fn parse_simple_explicit_groot_xml() -> Result<()> {
    let behavior = BehaviorTree::try_from_groot_xml(SIMPLE_EXPLICIT_GROOT_XML)?;
    assert_eq!(2, behavior.root.children.len());
    for child in behavior.root.children {
      assert_eq!(child.id.as_str(), "SaySomething");
    }
    Ok(())
  }

  #[test]
  pub fn serialize_simple_explicit_groot_xml() -> Result<()> {
    let behavior = BehaviorTree {
      root: seq(vec![
        action("SaySomething", param_args!["message" => "Hello"]),
        action("SaySomething", param_args!["message" => "{my_message}"]),
      ]),
    };
    println!("{}", String::from_utf8(behavior.to_groot_xml()).unwrap());
    Ok(())
  }

  const SIMPLE_COMPACT_GROOT_XML: &str = r#"<root main_tree_to_execute = "MainTree" >
    <BehaviorTree ID="MainTree">
      <Sequence name="root_sequence">
          <SaySomething message="Hello"/>
          <SaySomething message="{my_message}"/>
      </Sequence>
    </BehaviorTree>
  </root>"#;

  const SIMPLE_EXPLICIT_GROOT_XML: &str = r#"<root main_tree_to_execute = "MainTree" >
  <BehaviorTree ID="MainTree">
    <Sequence name="root_sequence">
        <Action ID="SaySomething" message="Hello"/>
        <Action ID="SaySomething" message="{my_message}"/>
    </Sequence>
  </BehaviorTree>
</root>"#;

  #[tokio::test]
  async fn tree_node_to_groot() -> Result<()> {
    use crate::nodes::{cos, increase, seq};
    let angle_variable = Uuid::new_v4();
    let cos_variable = Uuid::new_v4();
    let behavior = seq(vec![
      increase(
        Expression::Uuid(angle_variable.to_owned()),
        Expression::Value(Value::F32(0.1f32)),
      ),
      cos(
        Expression::Uuid(angle_variable.to_owned()),
        Expression::Uuid(cos_variable.to_owned()),
      ),
    ]);
    let index = setup_index().await;
    let mut variables = HashMap::new();
    variables.insert(angle_variable.to_owned(), "angle".to_string());
    variables.insert(cos_variable.to_owned(), "cos".to_string());
    let behavior = BehaviorTree {
      root: Node::try_from_tree_node(&behavior, &index, &mut variables)?,
    };

    println!("{}", String::from_utf8(behavior.to_groot_xml()).unwrap());
    Ok(())
  }

  #[tokio::test]
  async fn tree_node_from_groot() -> Result<()> {
    let xml_path = std::env::var("CARGO_MANIFEST_DIR")
      .map_or_else(
        |_| {
          Path::new(file!()) // crates/arora-behavior-tree/src/schema_groot.rs
            .parent() // crates/arora-behavior-tree/src
            .unwrap()
            .parent() // crates/arora-behavior-tree
            .unwrap()
            .to_owned()
        },
        |dir| Path::new(dir.as_str()).to_owned(),
      )
      .join("groot")
      .join("cosine_tree_groot_edited.xml");
    let xml_str = read_to_string(xml_path.to_owned())
      .await
      .expect(format!("failed to read XML file {:?}", xml_path).as_str());
    let behavior = parse_groot_xml(xml_str.as_str())?;
    println!("{}", String::from_utf8(behavior.to_groot_xml()).unwrap());

    let angle_variable = Uuid::new_v4();
    let cos_variable = Uuid::new_v4();
    let index = setup_index().await;
    let mut variables = HashMap::new();
    variables.insert("angle".to_string(), angle_variable.to_owned());
    variables.insert("cos".to_string(), cos_variable.to_owned());
    let tree = behavior.root.try_into_tree_node(&index, &mut variables)?;
    let _behavior: crate::BehaviorTree = tree.try_into()?;
    Ok(())
  }

  pub async fn setup_index() -> HashMap<Uuid, ModuleFunction> {
    let mut registry = LocalRegistry::new();
    let behavior_tree_types_yaml_dir =
      crate_root_path("arora-behavior-tree-types-yaml").join("records");
    load_records_from_yaml_dir(behavior_tree_types_yaml_dir, &mut registry)
      .await
      .unwrap();
    let mut index = HashMap::new();
    for module_name in &*BASE_MODULE_NAMES {
      read_header_to_index(module_name, &mut index, &mut registry).await;
    }
    index
  }
}

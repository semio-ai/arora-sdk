use quick_xml::events::BytesStart;
use quick_xml::Writer;
use quick_xml::{escape::unescape, events::Event, Reader};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::Write;
use std::io::Cursor;
use std::{collections::HashMap, fmt::Display};
use uuid::Uuid;

use crate::error::BehaviorTreeError;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct BehaviorTree {
  pub root: Node,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Node {
  Sequence {
    name: String,
    children: Vec<Box<Node>>,
  },
  Action {
    id: String,
    name: String,
    param_args: HashMap<String, String>,
  },
}

pub fn seq(children: Vec<Node>) -> Node {
  Node::Sequence {
    name: Uuid::new_v4().to_string(),
    children: to_boxed_vec(children),
  }
}

pub fn action(type_name: &str, param_args: HashMap<&str, &str>) -> Node {
  Node::Action {
    id: type_name.to_string(),
    name: Uuid::new_v4().to_string(),
    param_args: to_string_map(param_args),
  }
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Children {
  children: Vec<Box<Node>>,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Args {
  param_args: HashMap<String, String>,
}

macro_rules! param_args {
  ($( $key: expr => $val: expr ),*) => {{
       let mut map = ::std::collections::HashMap::new();
       $( map.insert($key, $val); )*
       map
  }}
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
  reader.trim_text(true);
  let mut buf = Vec::new();

  let root = match reader.read_event(&mut buf) {
    Ok(Event::Start(ref root_start)) => {
      if root_start.name() != b"root" {
        return Err(BehaviorTreeError::ParsingError {
          message: "root tag is not \"root\"".to_string(),
        });
      }
      match reader.read_event(&mut buf) {
        Ok(Event::Start(ref node_start)) => {
          if node_start.name() != b"BehaviorTree" {
            return Err(BehaviorTreeError::ParsingError {
              message: "found node that is not a \"BehaviorTree\"".to_string(),
            });
          }
          parse_groot_node(&mut reader, &mut buf)?
            .ok_or(new_parsing_error("behavior tree has no root node", &reader))?
        }
        Err(e) => forward_parsing_error("Error parsing XML", &reader, e)?,
        _ => new_parsing_error_result("XML does contain a \"BehaviorTree\" node", &reader)?,
      }
    }
    Err(e) => forward_parsing_error("Error parsing XML", &reader, e)?,
    _ => new_parsing_error_result("XML does not start with a valid root tag", &reader)?,
  };

  // if we don't keep a borrow elsewhere, we can clear the buffer to keep memory usage low
  buf.clear();
  Ok(BehaviorTree { root })
}

fn parse_groot_node(
  reader: &mut Reader<&[u8]>,
  buf: &mut Vec<u8>,
) -> Result<Option<Node>, BehaviorTreeError> {
  let mut depth = 0usize;
  match reader.read_event(buf) {
    Ok(Event::Start(ref node_start)) => {
      depth += 1;
      match node_start.name() {
        b"Sequence" => {
          let mut children = Vec::new();
          loop {
            let child = parse_groot_node(reader, buf)?;
            match child {
              Some(child) => children.push(child),
              None => break,
            }
          }
          Ok(Some(seq(children)))
        }
        tag => new_parsing_error_result(
          format!(
            "unexpected control node type {}",
            String::from_utf8_lossy(tag)
          )
          .as_str(),
          reader,
        )?,
      }
    }
    Ok(Event::Empty(ref node_empty)) => match node_empty.name() {
      b"Action" => {
        let mut attributes = collect_action_attributes(node_empty, reader)?;
        let id = attributes
          .remove(&ID_ATTRIBUTE_KEY.to_string())
          .ok_or(new_parsing_error("missing ID attribute of action", reader))?;
        let name = attributes.remove(&NAME_ATTRIBUTE_KEY.to_string());
        Ok(Some(Node::Action {
          id,
          name: name.unwrap_or(Uuid::new_v4().to_string()),
          param_args: attributes,
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
        Ok(Some(Node::Action {
          id,
          name: name.unwrap_or(Uuid::new_v4().to_string()),
          param_args: attributes,
        }))
      }
    },
    Ok(Event::End(_)) => {
      assert!(depth <= 1usize);
      Ok(None)
    }
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
    let key = String::from_utf8(attr.key.to_vec());
    let key = map_parsing_error(key, "invalid utf8 in attribute key", reader)?;
    let value = unescape(attr.value.as_ref());
    let value = map_parsing_error(
      value,
      format!("error unescaping value of attribute {}", key).as_str(),
      reader,
    )?;
    let value = String::from_utf8(value.to_vec());
    let value = map_parsing_error(
      value,
      format!("invalid utf-8 in value of attribute {}", key).as_str(),
      reader,
    )?;
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

  let xml = r#"<this_tag k1="v1" k2="v2"><child>text</child></this_tag>"#;
  let mut reader = Reader::from_str(xml);
  reader.trim_text(true);
  let mut writer = Writer::new(Cursor::new(Vec::new()));

  const ROOT_NAME: &[u8; 4] = b"root";
  let mut root_elem = BytesStart::owned(ROOT_NAME.to_vec(), ROOT_NAME.len());
  root_elem.push_attribute(("main_tree_to_execute", "MainTree"));
  writer.write_event(Event::Start(root_elem)).unwrap();

  const BEHAVIOR_TREE_NAME: &[u8; 12] = b"BehaviorTree";
  let mut behavior_elem = BytesStart::owned(BEHAVIOR_TREE_NAME.to_vec(), BEHAVIOR_TREE_NAME.len());
  behavior_elem.push_attribute((ID_ATTRIBUTE_KEY, "MainTree"));
  writer.write_event(Event::Start(behavior_elem)).unwrap();

  serialize_node_to_groot_xml(&behavior.root, &mut writer);

  writer
    .write_event(Event::End(BytesEnd::owned(BEHAVIOR_TREE_NAME.to_vec())))
    .unwrap();
  writer
    .write_event(Event::End(BytesEnd::owned(ROOT_NAME.to_vec())))
    .unwrap();
  writer.into_inner().into_inner()
}

fn serialize_node_to_groot_xml(node: &Node, writer: &mut Writer<Cursor<Vec<u8>>>) {
  use quick_xml::events::BytesEnd;

  match node {
    Node::Sequence { name, children } => {
      const SEQUENCE_NAME: &[u8; 8] = b"Sequence";
      let mut elem = BytesStart::owned(SEQUENCE_NAME.to_vec(), SEQUENCE_NAME.len());
      elem.push_attribute((NAME_ATTRIBUTE_KEY, name.as_str()));
      writer.write_event(Event::Start(elem)).unwrap();
      for child in children {
        serialize_node_to_groot_xml(&child, writer);
      }
      writer
        .write_event(Event::End(BytesEnd::owned(SEQUENCE_NAME.to_vec())))
        .unwrap();
    }
    Node::Action {
      id,
      name,
      param_args,
    } => {
      let mut elem = BytesStart::owned(id.as_bytes().to_vec(), id.as_bytes().len());
      elem.push_attribute((NAME_ATTRIBUTE_KEY, name.as_str()));
      for (param, arg) in param_args {
        elem.push_attribute((param.as_str(), arg.as_str()));
      }
      writer.write_event(Event::Empty(elem)).unwrap();
    }
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
    match self {
      Node::Sequence { name: _, children } => {
        f.write_fmt(format_args!("sequence:"))?;
        display_children(f, children)
      }
      Node::Action {
        id,
        name: _,
        param_args,
      } => {
        f.write_fmt(format_args!("{}(", id))?;
        display_param_args(param_args);
        f.write_char(')')
      }
    }
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
  use super::{action, seq, BehaviorTree, Node};
  use anyhow::{bail, Result};

  #[test]
  pub fn parse_simple_compact_groot_xml() -> Result<()> {
    let behavior = BehaviorTree::try_from_groot_xml(SIMPLE_COMPACT_GROOT_XML)?;
    if let Node::Sequence { name: _, children } = behavior.root {
      assert_eq!(2, children.len());
      for child in children {
        if let Node::Action {
          id,
          name: _,
          param_args: _,
        } = child.as_ref()
        {
          assert_eq!(id.as_str(), "SaySomething")
        }
      }
    } else {
      bail!("root node is not a sequence");
    }
    Ok(())
  }

  #[test]
  pub fn parse_simple_explicit_groot_xml() -> Result<()> {
    let behavior = BehaviorTree::try_from_groot_xml(SIMPLE_EXPLICIT_GROOT_XML)?;
    if let Node::Sequence { name: _, children } = behavior.root {
      assert_eq!(2, children.len());
      for child in children {
        if let Node::Action {
          id,
          name: _,
          param_args: _,
        } = child.as_ref()
        {
          assert_eq!(id.as_str(), "SaySomething")
        }
      }
    } else {
      bail!("root node is not a sequence");
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
}

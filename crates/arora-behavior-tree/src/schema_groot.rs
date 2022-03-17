use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct BehaviorTree {
  pub root: Node,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Node {
  Sequence(Children),
  Action(String, Args),
}

pub fn seq(children: Vec<Node>) -> Node {
  Node::Sequence(Children {
    children: children.into_iter().map(|child| Box::new(child)).collect(),
  })
}

pub fn action(name: &str, param_args: HashMap<&str, &str>) -> Node {
  Node::Action(
    name.to_string(),
    Args {
      param_args: HashMap::from_iter(
        param_args
          .into_iter()
          .map(|(k, v)| (k.to_string(), v.to_string())),
      ),
    },
  )
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

impl Display for BehaviorTree {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("{}", self.root))
  }
}

impl Display for Node {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Node::Sequence(children) => f.write_fmt(format_args!("sequence:{}", children)),
      Node::Action(id, args) => f.write_fmt(format_args!("{}({})", id, args)),
    }
  }
}

impl Display for Children {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for child in &self.children {
      f.write_fmt(format_args!("\n- {}", child.as_ref()))?
    }
    Ok(())
  }
}

impl Display for Args {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(display_param_args(&self.param_args).as_str())
  }
}

fn display_param_args(param_args: &HashMap<String, String>) -> String {
  param_args
    .iter()
    .map(|(key, value)| format!("{}=\"{}\"", key, value))
    .collect::<Vec<String>>()
    .join(", ")
}

#[cfg(test)]
pub mod tests {
  use crate::schema_groot::{action, seq, BehaviorTree};
  use anyhow::Result;
  use serde_xml_rs;

  #[test]
  pub fn serialize_simple_tree() -> Result<()> {
    let behavior = BehaviorTree {
      root: seq(vec![
        action("SaySomething", param_args!["message" => "Hello"]),
        action("SaySomething", param_args!["message" => "{my_message}"])
      ])
    };
    println!("{}", &behavior);
    println!("{}", serde_xml_rs::to_string(&behavior).unwrap());
    Ok(())
  }
}

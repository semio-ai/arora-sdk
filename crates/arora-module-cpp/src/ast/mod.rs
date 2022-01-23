// C++ AST description

use derive_more::From;

pub trait ToPrettyString {
  fn to_pretty_string(&self, indent: usize) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeRef {
  pub reference: bool,
  pub constant: bool,
  pub ty: String,
}

impl ToPrettyString for TypeRef {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if self.constant {
      s.push_str("const ");
    }
    s.push_str(&self.ty);
    if self.reference {
      s.push_str(" &");
    }
    s
  }
}

fn indent_string(indent: usize) -> String {
  let mut s = String::new();
  for _ in 0..indent {
    s.push_str("  ");
  }
  s
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Parameter {
  pub name: String,
  pub type_ref: TypeRef,
}

impl ToPrettyString for Parameter {
  fn to_pretty_string(&self, indent: usize) -> String {
    format!("{}{} {}", indent_string(indent), &self.type_ref.to_pretty_string(0), self.name)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionPrototype {
  pub name: String,
  pub parameters: Vec<Parameter>,
  pub ret: TypeRef,
}

impl ToPrettyString for FunctionPrototype {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&self.ret.to_pretty_string(indent));
    s.push_str(" ");
    s.push_str(&self.name);
    s.push_str("(");
    for (i, parameter) in self.parameters.iter().enumerate() {
      if i > 0 {
        s.push_str(", ");
      }
      s.push_str(&format!("{} {}", parameter.type_ref.to_pretty_string(0), parameter.name));
    }
    s.push_str(");\n");
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]

pub struct Block {
  pub statements: Vec<Declaration>,
}

impl ToPrettyString for Block {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    for statement in &self.statements {
      s.push_str(&statement.to_pretty_string(indent + 1));
    }
    s.push_str(&indent_string(indent));
    s.push_str("}");
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IncludeStyle {
  System,
  Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PreprocessorDirective {
  Ifndef(String),
  Ifdef(String),
  If(String),
  Define(String),
  Else,
  Endif,
  Include(String, IncludeStyle),
}

impl ToPrettyString for PreprocessorDirective {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&match self {
      PreprocessorDirective::Ifndef(name) => format!("#ifndef {}\n", name),
      PreprocessorDirective::Ifdef(name) => format!("#ifdef {}\n", name),
      PreprocessorDirective::If(name) => format!("#if {}\n", name),
      PreprocessorDirective::Define(name) => format!("#define {}\n", name),
      PreprocessorDirective::Else => "#else\n".to_string(),
      PreprocessorDirective::Endif => "#endif\n".to_string(),
      PreprocessorDirective::Include(name, style) => match style {
        IncludeStyle::System => format!("#include <{}>\n", name),
        IncludeStyle::Local => format!("#include \"{}\"\n", name),
      },
    });
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Namespace {
  pub name: String,
  pub declarations: Vec<Declaration>,
}

impl ToPrettyString for Namespace {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&format!("namespace {} {{\n", self.name));
    for declaration in &self.declarations {
      s.push_str(&declaration.to_pretty_string(indent + 1));
    }
    s.push_str(&indent_string(indent));
    s.push_str("}\n");

    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, From)]
pub enum Declaration {
  Function(FunctionPrototype),
  Block(Block),
  PreprocessorDirective(PreprocessorDirective),
  Namespace(Namespace),
  NewLine(NewLine)
}

impl ToPrettyString for Declaration {
  fn to_pretty_string(&self, indent: usize) -> String {
    match self {
      Declaration::Function(function) => function.to_pretty_string(indent),
      Declaration::Block(block) => block.to_pretty_string(indent),
      Declaration::PreprocessorDirective(directive) => directive.to_pretty_string(indent),
      Declaration::Namespace(namespace) => namespace.to_pretty_string(indent),
      Declaration::NewLine(newline) => newline.to_pretty_string(indent),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NewLine {
  pub count: usize,
}

impl ToPrettyString for NewLine {
  fn to_pretty_string(&self, _indent: usize) -> String {
    let mut s = String::new();
    for _ in 0..self.count {
      s.push_str("\n");
    }
    s
  }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TranslationUnit {
  pub declarations: Vec<Declaration>,
}

impl ToPrettyString for TranslationUnit {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    for declaration in &self.declarations {
      s.push_str(&declaration.to_pretty_string(indent));
    }
    s
  }
}

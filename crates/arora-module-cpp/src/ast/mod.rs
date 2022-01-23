// C++ AST description

use derive_more::From;

pub trait ToPrettyString {
  fn to_pretty_string(&self, indent: usize) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeRef {
  pub reference: bool,
  pub constant: bool,
  pub pointer: bool,
  pub ty: String,
  pub arguments: Option<Vec<TypeRef>>,
}

impl Default for TypeRef {
  fn default() -> Self {
    Self {
      reference: false,
      constant: false,
      pointer: false,
      arguments: None,
      ty: String::new(),
    }
  }
}

impl ToPrettyString for TypeRef {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if self.constant {
      s.push_str("const ");
    }
    s.push_str(&self.ty);
    if let Some(arguments) = &self.arguments {
      s.push_str("<");
      for (i, argument) in arguments.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&argument.to_pretty_string(0));
      }
      s.push_str(">");
    }
    if self.reference {
      s.push_str(" &");
    }
    if self.pointer {
      s.push_str(" *");
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
pub struct FunctionImplementation {
  pub name: String,
  pub parameters: Vec<Parameter>,
  pub ret: TypeRef,
  pub body: Block,
}

impl ToPrettyString for FunctionImplementation {
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
    s.push_str(")\n");
    s.push_str(&self.body.to_pretty_string(indent));
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
    s.push_str(&indent_string(indent));
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
  FunctionPrototype(FunctionPrototype),
  FunctionImplementation(FunctionImplementation),
  Block(Block),
  PreprocessorDirective(PreprocessorDirective),
  Namespace(Namespace),
  NewLine(NewLine),
  Statement(Statement),
  Variable(Variable),
}

impl ToPrettyString for Declaration {
  fn to_pretty_string(&self, indent: usize) -> String {
    match self {
      Declaration::FunctionPrototype(function) => function.to_pretty_string(indent),
      Declaration::FunctionImplementation(function) => function.to_pretty_string(indent),
      Declaration::Block(block) => block.to_pretty_string(indent),
      Declaration::PreprocessorDirective(directive) => directive.to_pretty_string(indent),
      Declaration::Namespace(namespace) => namespace.to_pretty_string(indent),
      Declaration::NewLine(newline) => newline.to_pretty_string(indent),
      Declaration::Statement(statement) => statement.to_pretty_string(indent),
      Declaration::Variable(variable) => variable.to_pretty_string(indent),
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
pub enum Statement {
  Expression(Expression),
  Return(Expression),
  Break,
  Continue,
  If(Expression, Block, Option<Block>),
  While(Expression, Block),
  Case(Expression),
  Default,
  Goto(String),
  Label(String),
}

impl ToPrettyString for Statement {
  fn to_pretty_string(&self, indent: usize) -> String {
    match self {
      Statement::Expression(expression) => format!("{};\n", expression.to_pretty_string(indent)),
      Statement::Return(expression) => format!("return {};\n", expression.to_pretty_string(0)),
      Statement::Break => "break;\n".to_string(),
      Statement::Continue => "continue;\n".to_string(),
      Statement::If(expression, block, else_block) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("if (");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(")\n");
        s.push_str(&block.to_pretty_string(indent + 1));
        if let Some(else_block) = else_block {
          s.push_str(&indent_string(indent));
          s.push_str("else\n");
          s.push_str(&else_block.to_pretty_string(indent + 1));
        }
        s
      }
      Statement::While(expression, block) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("while (");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(")\n");
        s.push_str(&block.to_pretty_string(indent + 1));
        s
      }
      Statement::Case(expression) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("case ");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(":\n");
        s
      }
      Statement::Default => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("default:\n");
        s
      },
      Statement::Goto(label) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("goto ");
        s.push_str(&label);
        s.push_str(";\n");
        s
      },
      Statement::Label(label) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str(&label);
        s.push_str(":\n");
        s
      },
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expression {
  Identifier(String),
  Equal(Box<Expression>, Box<Expression>),
  NotEqual(Box<Expression>, Box<Expression>),
  LessThan(Box<Expression>, Box<Expression>),
  LessThanOrEqual(Box<Expression>, Box<Expression>),
  GreaterThan(Box<Expression>, Box<Expression>),
  Arrow(Box<Expression>, Box<Expression>),
  Dot(Box<Expression>, Box<Expression>),
  Call(Box<Expression>, Vec<Expression>),
  ColonColon(Box<Expression>, Box<Expression>),
  Parenthesized(Box<Expression>),
  ArrayAccess(Box<Expression>, Box<Expression>),
}

impl ToPrettyString for Expression {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&match self {
      Expression::Identifier(identifier) => identifier.to_string(),
      Expression::Equal(left, right) => format!("{} == {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::NotEqual(left, right) => format!("{} != {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::LessThan(left, right) => format!("{} < {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::LessThanOrEqual(left, right) => format!("{} <= {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::GreaterThan(left, right) => format!("{} > {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::Arrow(left, right) => format!("{}->{}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::Dot(left, right) => format!("{}.{}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::Call(left, args) => format!("{}({})", left.to_pretty_string(0), args.iter().map(|arg| arg.to_pretty_string(0)).collect::<Vec<String>>().join(", ")),
      Expression::ColonColon(left, right) => format!("{}::{}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::Parenthesized(expr) => format!("({})", expr.to_pretty_string(0)),
      Expression::ArrayAccess(left, right) => format!("{}[{}]", left.to_pretty_string(0), right.to_pretty_string(0)),
    });
    s
  }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable {
  pub name: String,
  pub ty: TypeRef,
  pub value: Option<Expression>,
}

impl ToPrettyString for Variable {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&self.ty.to_pretty_string(indent));
    s.push_str(" ");
    s.push_str(&self.name);
    if let Some(value) = &self.value {
      s.push_str(" = ");
      s.push_str(&value.to_pretty_string(0));
    }
    s.push_str(";\n");
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

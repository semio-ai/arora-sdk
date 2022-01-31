// C++ AST description

use derive_more::From;

pub trait ToPrettyString {
  fn to_pretty_string(&self, indent: usize) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArrayKind {
  None,
  Fixed(Expression),
  Dynamic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeRef {
  pub reference: bool,
  pub move_: bool,
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
      move_: false,
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
    if self.move_ {
      s.push_str(" &&");
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
  pub ret: Option<TypeRef>,
  pub template_arguments: Option<Vec<String>>,
  pub specialization: Option<Vec<String>>,
  pub operator: bool,
  pub static_: bool,
  pub constant: bool,
  pub noexcept: bool,
  pub attributes: Option<Vec<String>>,
}

impl Default for FunctionPrototype {
  fn default() -> Self {
    Self {
      name: String::new(),
      parameters: Vec::new(),
      ret: None,
      template_arguments: None,
      specialization: None,
      operator: false,
      static_: false,
      constant: false,
      noexcept: false,
      attributes: None,
    }
  }
}

impl ToPrettyString for FunctionPrototype {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if let Some(attributes) = &self.attributes {
      s.push_str("__attribute__((");
      for attribute in attributes {
        s.push_str(&format!("{} ", attribute));
      }
      s.push_str(")) ");
    }
    if let Some(template_arguments) = &self.template_arguments {
      s.push_str("template<");
      for (i, template_argument) in template_arguments.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&template_argument);
      }
      s.push_str("> ");
    }

    if self.static_ {
      s.push_str("static ");
    }

    if let Some(ret) = &self.ret {
      s.push_str(&ret.to_pretty_string(0));
      s.push_str(" ");
    }
    s.push_str(&self.name);

    if let Some(specialization) = &self.specialization {
      s.push_str("<");
      for (i, specialization) in specialization.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&specialization);
      }
      s.push_str(">");
    }

    s.push_str("(");
    for (i, parameter) in self.parameters.iter().enumerate() {
      if i > 0 {
        s.push_str(", ");
      }
      s.push_str(&format!("{} {}", parameter.type_ref.to_pretty_string(0), parameter.name));
    }
    s.push_str(")");
    if self.constant {
      s.push_str(" const");
    }
    if self.noexcept {
      s.push_str(" noexcept");
    }
    s.push_str(";\n");
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionImplementation {
  pub name: String,
  pub parameters: Vec<Parameter>,
  pub ret: Option<TypeRef>,
  pub body: Block,
  pub attributes: Option<Vec<String>>,
  pub template_arguments: Option<Vec<String>>,
  pub specialization: Option<Vec<String>>,
  pub operator: bool,
  pub static_: bool,
  pub constant: bool,
  pub noexcept: bool,
  pub inline: bool,
}

impl Default for FunctionImplementation {
  fn default() -> Self {
    Self {
      name: String::new(),
      parameters: Vec::new(),
      ret: None,
      body: Block::default(),
      attributes: None,
      template_arguments: None,
      specialization: None,
      operator: false,
      static_: false,
      constant: false,
      noexcept: false,
      inline: false,
    }
  }
}

impl ToPrettyString for FunctionImplementation {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if let Some(attributes) = &self.attributes {
      s.push_str("__attribute__((");
      for attribute in attributes {
        s.push_str(&format!("{} ", attribute));
      }
      s.push_str(")) ");
    }
    if let Some(template_arguments) = &self.template_arguments {
      s.push_str("template<");
      for (i, template_argument) in template_arguments.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&template_argument);
      }
      s.push_str(">\n");
    }

    if self.inline {
      s.push_str("inline ");
    }

    if self.static_ {
      s.push_str("static ");
    }

    if let Some(ret) = &self.ret {
      s.push_str(&ret.to_pretty_string(0));
      s.push_str(" ");
    }
    s.push_str(&self.name);

    if let Some(specialization) = &self.specialization {
      s.push_str("<");
      for (i, specialization) in specialization.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&specialization);
      }
      s.push_str(">");
    }

    s.push_str("(");
    for (i, parameter) in self.parameters.iter().enumerate() {
      if i > 0 {
        s.push_str(", ");
      }
      s.push_str(&format!("{} {}", parameter.type_ref.to_pretty_string(0), parameter.name));
    }
    s.push_str(")");
    if self.constant {
      s.push_str(" const");
    }
    if self.noexcept {
      s.push_str(" noexcept");
    }
    s.push_str("\n");
    s.push_str(&self.body.to_pretty_string(indent));
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]

pub struct Block {
  pub statements: Vec<Declaration>,
  pub semicolon: bool
}

impl Default for Block {
  fn default() -> Self {
    Self {
      statements: Vec::new(),
      semicolon: false,
    }
  }
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
    if self.semicolon {
      s.push_str("};\n");
    } else {
      s.push_str("}\n");
    }
    s
  }
}

impl ToExpression for u8 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for u16 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for u32 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for u64 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for i8 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for i16 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for i32 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
  }
}

impl ToExpression for i64 {
  fn to_expression(&self) -> Expression {
    Expression::IntegerLiteral(*self as i64)
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
pub struct Extern {
  pub name: String,
  pub block: Block,
}

impl ToPrettyString for Extern {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&format!("extern \"{}\" ", self.name));
    s.push_str(&self.block.to_pretty_string(indent));
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Struct {
  pub name: String,
  pub block: Block,
  pub template_arguments: Option<Vec<String>>,
  pub specialization: Option<Vec<String>>,
}

impl ToPrettyString for Struct {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if let Some(template_arguments) = &self.template_arguments {
      s.push_str("template<");
      for (i, template_argument) in template_arguments.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&template_argument);
      }
      s.push_str(">\n");
    }
    s.push_str(&format!("struct {}", self.name));
    if let Some(specialization) = &self.specialization {
      s.push_str("<");
      for (i, specialization) in specialization.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&specialization);
      }
      s.push_str(">");
    }
    s.push_str("\n");
    s.push_str(&self.block.to_pretty_string(indent));
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Class {
  pub name: String,
  pub block: Block,
  pub template_arguments: Option<Vec<String>>,
  pub specialization: Option<Vec<String>>,
}

impl ToPrettyString for Class {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    if let Some(template_arguments) = &self.template_arguments {
      s.push_str("template<");
      for (i, template_argument) in template_arguments.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&template_argument);
      }
      s.push_str("> ");
    }
    s.push_str(&format!("class {} ", self.name));
    if let Some(specialization) = &self.specialization {
      s.push_str("<");
      for (i, specialization) in specialization.iter().enumerate() {
        if i > 0 {
          s.push_str(", ");
        }
        s.push_str(&specialization);
      }
      s.push_str(">");
    }
    s.push_str(&self.block.to_pretty_string(indent));
    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Enum {
  pub name: String,
  pub members: Vec<String>,
}

impl ToPrettyString for Enum {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&format!("enum class {} {{\n", self.name));
    for member in &self.members {
      s.push_str(&indent_string(indent + 1));
      s.push_str(&format!("{},\n", member));
    }
    s.push_str(&indent_string(indent));
    s.push_str("};\n");

    s
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Union {
  pub name: String,
  pub block: Block,
}

impl ToPrettyString for Union {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&format!("union {} ", self.name));
    s.push_str(&self.block.to_pretty_string(indent));
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
  Struct(Struct),
  Class(Class),
  Enum(Enum),
  Union(Union),
  Extern(Extern),
  Public,
  Private,
  Protected,
}

impl Declaration {
  pub fn new_line(count: usize) -> Declaration {
    NewLine { count }.into()
  }

  pub fn include<P: Into<String>>(path: P, style: IncludeStyle) -> Declaration {
    PreprocessorDirective::Include(path.into(), style).into()
  }

  pub fn include_system<P: Into<String>>(path: P) -> Declaration {
    Self::include(path, IncludeStyle::System)
  }

  pub fn include_local<P: Into<String>>(path: P) -> Declaration {
    Self::include(path, IncludeStyle::Local)
  }

  pub fn endif() -> Declaration {
    PreprocessorDirective::Endif.into()
  }
  pub fn ifndef<P: Into<String>>(name: P) -> Declaration {
    PreprocessorDirective::Ifndef(name.into()).into()
  }

  pub fn ifdef<P: Into<String>>(name: P) -> Declaration {
    PreprocessorDirective::Ifdef(name.into()).into()
  }

  pub fn define<P: Into<String>>(name: P) -> Declaration {
    PreprocessorDirective::Define(name.into()).into()
  }

  pub fn public() -> Declaration {
    Declaration::Public.into()
  }

  pub fn private() -> Declaration {
    Declaration::Private.into()
  }

  pub fn protected() -> Declaration {
    Declaration::Protected.into()
  }
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
      Declaration::Extern(extern_) => extern_.to_pretty_string(indent),
      Declaration::Struct(struct_) => struct_.to_pretty_string(indent),
      Declaration::Class(class_) => class_.to_pretty_string(indent),
      Declaration::Enum(enum_) => enum_.to_pretty_string(indent),
      Declaration::Union(union_) => union_.to_pretty_string(indent),
      Declaration::Public => format!("{}public:\n", indent_string(indent - 1)),
      Declaration::Private => format!("{}private:\n", indent_string(indent - 1)),
      Declaration::Protected => format!("{}protected:\n", indent_string(indent - 1)),
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
  Switch(Expression, Vec<(Expression, Block)>),
  Case(Expression),
  Default,
  Goto(String),
  Label(String),
}

impl ToPrettyString for Statement {
  fn to_pretty_string(&self, indent: usize) -> String {
    match self {
      Statement::Expression(expression) => format!("{};\n", expression.to_pretty_string(indent)),
      Statement::Return(expression) => format!("{}return {};\n", &indent_string(indent), expression.to_pretty_string(0)),
      Statement::Break => format!("{}break;\n", &indent_string(indent)),
      Statement::Continue => format!("{}continue;\n", &indent_string(indent)),
      Statement::If(expression, block, else_block) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("if (");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(")\n");
        s.push_str(&block.to_pretty_string(indent));
        if let Some(else_block) = else_block {
          s.push_str(&indent_string(indent));
          s.push_str("else\n");
          s.push_str(&else_block.to_pretty_string(indent));
        }
        s
      }
      Statement::While(expression, block) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("while (");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(")\n");
        s.push_str(&block.to_pretty_string(indent));
        s
      },
      Statement::Switch(expression, cases) => {
        let mut s = String::new();
        s.push_str(&indent_string(indent));
        s.push_str("switch (");
        s.push_str(&expression.to_pretty_string(0));
        s.push_str(")\n");
        s.push_str(&indent_string(indent));
        s.push_str("{\n");
        for (case, block) in cases {
          s.push_str(&indent_string(indent + 1));
          s.push_str("case ");
          s.push_str(&case.to_pretty_string(0));
          s.push_str(":\n");
          s.push_str(&block.to_pretty_string(indent + 2));
        }
        s.push_str(&indent_string(indent));
        s.push_str("}\n");
        s
      },
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

pub trait ToExpression {
  fn to_expression(&self) -> Expression;
}

impl ToExpression for String {
  fn to_expression(&self) -> Expression {
    Expression::Identifier(self.clone())
  }
}

impl ToExpression for &str {
  fn to_expression(&self) -> Expression {
    Expression::Identifier(self.to_string())
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expression {
  Identifier(String),
  IntegerLiteral(i64),
  StringLiteral(String),
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
  InitializerList(Vec<Expression>),
  LogicalAnd(Box<Expression>, Box<Expression>),
  LogicalOr(Box<Expression>, Box<Expression>),
  PreIncrement(Box<Expression>),
  PreDecrement(Box<Expression>),
  Assign(Box<Expression>, Box<Expression>),
  AddAssign(Box<Expression>, Box<Expression>),
}

impl ToExpression for Expression {
  fn to_expression(&self) -> Expression {
    self.clone()
  }
}

impl Expression {
  pub fn into_statement(self) -> Statement {
    Statement::Expression(self)
  }

  pub fn call<E: ToExpression, I: IntoIterator<Item = E>>(&self, args: I) -> Expression {
    Expression::Call(Box::new(self.clone()), args.into_iter().map(|a| a.to_expression()).collect())
  }

  pub fn dot<E: ToExpression>(&self, member: E) -> Expression {
    Expression::Dot(Box::new(self.clone()), Box::new(member.to_expression()))
  }

  pub fn array_access(&self, index: Expression) -> Expression {
    Expression::ArrayAccess(Box::new(self.clone()), Box::new(index))
  }

  pub fn arrow<E: ToExpression>(&self, member: E) -> Expression {
    Expression::Arrow(Box::new(self.clone()), Box::new(member.to_expression()))
  }

  pub fn colon_colon<E: ToExpression>(&self, member: E) -> Expression {
    Expression::ColonColon(Box::new(self.clone()), Box::new(member.to_expression()))
  }

  pub fn parenthesized(&self) -> Expression {
    Expression::Parenthesized(Box::new(self.clone()))
  }

  pub fn equal<E: ToExpression>(&self, other: E) -> Expression {
    Expression::Equal(Box::new(self.clone()), Box::new(other.to_expression()))
  }

  pub fn not_equal<E: ToExpression>(&self, other: E) -> Expression {
    Expression::NotEqual(Box::new(self.clone()), Box::new(other.to_expression()))
  }

  pub fn less_than(&self, other: Expression) -> Expression {
    Expression::LessThan(Box::new(self.clone()), Box::new(other))
  }

  pub fn less_than_or_equal(&self, other: Expression) -> Expression {
    Expression::LessThanOrEqual(Box::new(self.clone()), Box::new(other))
  }

  pub fn greater_than(&self, other: Expression) -> Expression {
    Expression::GreaterThan(Box::new(self.clone()), Box::new(other))
  }

  pub fn logical_and(&self, other: Expression) -> Expression {
    Expression::LogicalAnd(Box::new(self.clone()), Box::new(other))
  }

  pub fn logical_or(&self, other: Expression) -> Expression {
    Expression::LogicalOr(Box::new(self.clone()), Box::new(other))
  }

  pub fn pre_increment(&self) -> Expression {
    Expression::PreIncrement(Box::new(self.clone()))
  }

  pub fn pre_decrement(&self) -> Expression {
    Expression::PreDecrement(Box::new(self.clone()))
  }

  pub fn assign<E: ToExpression>(&self, other: E) -> Expression {
    Expression::Assign(Box::new(self.clone()), Box::new(other.to_expression()))
  }

  pub fn add_assign<E: ToExpression>(&self, other: Expression) -> Expression {
    Expression::AddAssign(Box::new(self.clone()), Box::new(other.to_expression()))
  }
}

impl ToPrettyString for Expression {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    s.push_str(&indent_string(indent));
    s.push_str(&match self {
      Expression::Identifier(identifier) => identifier.to_string(),
      Expression::IntegerLiteral(value) => value.to_string(),
      Expression::StringLiteral(string) => format!("\"{}\"", string),
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
      Expression::InitializerList(exprs) => format!("{{ {} }}", exprs.iter().map(|expr| expr.to_pretty_string(0)).collect::<Vec<String>>().join(", ")),
      Expression::LogicalAnd(left, right) => format!("{} && {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::LogicalOr(left, right) => format!("{} || {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::PreIncrement(expr) => format!("++{}", expr.to_pretty_string(0)),
      Expression::PreDecrement(expr) => format!("--{}", expr.to_pretty_string(0)),
      Expression::Assign(left, right) => format!("{} = {}", left.to_pretty_string(0), right.to_pretty_string(0)),
      Expression::AddAssign(left, right) => format!("{} += {}", left.to_pretty_string(0), right.to_pretty_string(0)),
    });
    s
  }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable {
  pub name: String,
  pub ty: TypeRef,
  pub value: Option<Expression>,
  pub array: ArrayKind,
  pub extern_: bool,
}

impl ToPrettyString for Variable {
  fn to_pretty_string(&self, indent: usize) -> String {
    let mut s = String::new();
    if self.extern_ {
      s.push_str("extern ");
    }
    s.push_str(&self.ty.to_pretty_string(indent));
    s.push_str(" ");
    s.push_str(&self.name);
    match &self.array {
      ArrayKind::None => (),
      ArrayKind::Fixed(c) => s.push_str(&format!("[{}]", c.to_pretty_string(0))),
      ArrayKind::Dynamic => s.push_str("[]"),
    }
    if let Some(value) = &self.value {
      s.push_str(" = ");
      s.push_str(&value.to_pretty_string(0));
    }
    s.push_str(";\n");
    s
  }
}

impl Default for Variable {
  fn default() -> Self {
    Variable {
      name: String::new(),
      ty: TypeRef::default(),
      value: None,
      array: ArrayKind::None,
      extern_: false,
    }
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

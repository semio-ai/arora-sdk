//! A small ROS 2 `.msg` parser.
//!
//! It reads the subset of the `.msg` grammar the bundled packages
//! (std/geometry/sensor/hri + builtin_interfaces) actually use: fields, integer
//! constants, primitive and nested types, and `[]` / `[N]` arrays. It has no
//! dependencies beyond `std`, so the same parser runs in `build.rs` (to generate
//! the message structs) and at runtime (to [`define`](crate::Ros2Registry) a
//! type from a `.msg`-shaped blob that never entered the crate).
//!
//! Deliberately out of scope (absent from the bundled set): bounded strings and
//! bounded sequences (`<=N`) are read as their unbounded form, and field default
//! values are parsed then ignored — none of these changes the wire layout or the
//! arora type.

/// A ROS 2 primitive field type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
}

impl Primitive {
    /// The primitive named by a `.msg` type token, if it is one. `char`/`byte`
    /// are the deprecated aliases for `uint8`; `wstring` is treated as `string`.
    pub fn from_token(token: &str) -> Option<Self> {
        // A bounded string (`string<=10`) is still a string.
        if token.starts_with("string") || token.starts_with("wstring") {
            return Some(Primitive::String);
        }
        Some(match token {
            "bool" => Primitive::Bool,
            "int8" => Primitive::I8,
            "int16" => Primitive::I16,
            "int32" => Primitive::I32,
            "int64" => Primitive::I64,
            "uint8" | "char" | "byte" => Primitive::U8,
            "uint16" => Primitive::U16,
            "uint32" => Primitive::U32,
            "uint64" => Primitive::U64,
            "float32" => Primitive::F32,
            "float64" => Primitive::F64,
            _ => return None,
        })
    }

    /// The Rust type this primitive maps to (`float64` -> `f64`).
    pub fn rust_name(self) -> &'static str {
        match self {
            Primitive::Bool => "bool",
            Primitive::I8 => "i8",
            Primitive::I16 => "i16",
            Primitive::I32 => "i32",
            Primitive::I64 => "i64",
            Primitive::U8 => "u8",
            Primitive::U16 => "u16",
            Primitive::U32 => "u32",
            Primitive::U64 => "u64",
            Primitive::F32 => "f32",
            Primitive::F64 => "f64",
            Primitive::String => "String",
        }
    }
}

/// The element type of a field: a primitive, or a named (nested) message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    Primitive(Primitive),
    /// A nested message type, ROS-qualified as `package` + `name`.
    Named { package: String, name: String },
}

/// A field's array shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arr {
    /// A single value.
    Unit,
    /// A `T[]` sequence.
    Unbounded,
    /// A `T[N]` fixed array.
    Fixed(usize),
}

/// A field type: an element base and its array shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosType {
    pub base: Base,
    pub arr: Arr,
}

/// A message field: a name and its type. (Any `.msg` default value is dropped.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: RosType,
}

/// A message constant: a primitive-typed named value, emitted as a Rust `const`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constant {
    pub ty: Primitive,
    pub name: String,
    /// The literal value, verbatim from the `.msg` (e.g. `0`, `-2`).
    pub value: String,
}

/// A parsed `.msg`: its package, type name, fields (in declared order) and
/// constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSpec {
    pub package: String,
    pub name: String,
    pub fields: Vec<Field>,
    pub constants: Vec<Constant>,
}

impl MessageSpec {
    /// The REP-2016 qualified name, `package/msg/Name` — what a message carries
    /// in its arora `low::Type.name` and what the type hash uses.
    pub fn rep2016_name(&self) -> String {
        format!("{}/msg/{}", self.package, self.name)
    }
}

/// Parse the text of a `.msg` for the message `package/name`.
pub fn parse_msg(package: &str, name: &str, text: &str) -> Result<MessageSpec, String> {
    let mut fields = Vec::new();
    let mut constants = Vec::new();

    for (line_no, raw) in text.lines().enumerate() {
        // Strip a trailing comment and surrounding whitespace. (No bundled
        // message has a `#` inside a string literal, so this is safe here.)
        let line = match raw.split_once('#') {
            Some((code, _comment)) => code,
            None => raw,
        }
        .trim();
        if line.is_empty() {
            continue;
        }

        let at = |msg: &str| format!("{package}/{name}.msg:{}: {msg}", line_no + 1);

        // A constant has `=`; a field does not.
        if let Some((decl, value)) = line.split_once('=') {
            let mut it = decl.split_whitespace();
            let type_tok = it.next().ok_or_else(|| at("constant without a type"))?;
            let const_name = it.next().ok_or_else(|| at("constant without a name"))?;
            let ty = Primitive::from_token(type_tok)
                .ok_or_else(|| at(&format!("constant of non-primitive type `{type_tok}`")))?;
            constants.push(Constant {
                ty,
                name: const_name.to_string(),
                value: value.trim().to_string(),
            });
            continue;
        }

        // A field: `TYPE NAME [default…]` — the default (if any) is ignored.
        let mut it = line.split_whitespace();
        let type_tok = it.next().ok_or_else(|| at("field without a type"))?;
        let field_name = it.next().ok_or_else(|| at("field without a name"))?;
        let ty = parse_type(type_tok, package);
        fields.push(Field {
            name: field_name.to_string(),
            ty,
        });
    }

    Ok(MessageSpec {
        package: package.to_string(),
        name: name.to_string(),
        fields,
        constants,
    })
}

/// Parse a `.msg` type token (`float64`, `float64[36]`, `geometry_msgs/Point`,
/// `Header`, `NormalizedPointOfInterest2D[]`) in the context of `package`.
fn parse_type(token: &str, package: &str) -> RosType {
    // Split off any `[...]` array suffix.
    let (base_tok, arr) = match token.split_once('[') {
        Some((base, rest)) => {
            let inside = rest.trim_end_matches(']');
            let arr = if inside.is_empty() || inside.starts_with("<=") {
                // `[]` or a bounded sequence `[<=N]` — both variable length here.
                Arr::Unbounded
            } else {
                match inside.parse::<usize>() {
                    Ok(n) => Arr::Fixed(n),
                    // An unrecognised bound degrades to a sequence rather than
                    // failing — no bundled message hits this.
                    Err(_) => Arr::Unbounded,
                }
            };
            (base, arr)
        }
        None => (token, Arr::Unit),
    };

    let base = if let Some(primitive) = Primitive::from_token(base_tok) {
        Base::Primitive(primitive)
    } else if base_tok == "Header" {
        // ROS resolves a bare `Header` to `std_msgs/Header`.
        Base::Named {
            package: "std_msgs".to_string(),
            name: "Header".to_string(),
        }
    } else if let Some((pkg, ty)) = base_tok.split_once('/') {
        Base::Named {
            package: pkg.to_string(),
            name: ty.to_string(),
        }
    } else {
        // A bare name refers to a type in the same package.
        Base::Named {
            package: package.to_string(),
            name: base_tok.to_string(),
        }
    };

    RosType { base, arr }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fields_constants_and_arrays() {
        let text = "\
# a comment
Header header       # inline comment
float64 x
float64 w 1         # a field with a default (ignored)
uint8 NOSE = 0
uint8 LEFT_EAR=16
float64[36] covariance
NormalizedPointOfInterest2D[] skeleton
geometry_msgs/Point point
";
        let spec = parse_msg("hri_msgs", "Sample", text).unwrap();

        assert_eq!(spec.rep2016_name(), "hri_msgs/msg/Sample");
        assert_eq!(spec.constants.len(), 2);
        assert_eq!(spec.constants[0].name, "NOSE");
        assert_eq!(spec.constants[0].value, "0");
        assert_eq!(spec.constants[1].value, "16");

        let names: Vec<&str> = spec.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["header", "x", "w", "covariance", "skeleton", "point"]);

        // `Header` resolves to std_msgs/Header.
        assert_eq!(
            spec.fields[0].ty,
            RosType {
                base: Base::Named {
                    package: "std_msgs".into(),
                    name: "Header".into()
                },
                arr: Arr::Unit
            }
        );
        // The default value on `w` is dropped; it stays a plain f64 field.
        assert_eq!(spec.fields[2].ty.base, Base::Primitive(Primitive::F64));
        assert_eq!(spec.fields[2].ty.arr, Arr::Unit);
        // float64[36] -> fixed array of 36.
        assert_eq!(spec.fields[3].ty.arr, Arr::Fixed(36));
        // NormalizedPointOfInterest2D[] -> unbounded, same-package nested.
        assert_eq!(spec.fields[4].ty.arr, Arr::Unbounded);
        assert_eq!(
            spec.fields[4].ty.base,
            Base::Named {
                package: "hri_msgs".into(),
                name: "NormalizedPointOfInterest2D".into()
            }
        );
        // geometry_msgs/Point -> qualified nested.
        assert_eq!(
            spec.fields[5].ty.base,
            Base::Named {
                package: "geometry_msgs".into(),
                name: "Point".into()
            }
        );
    }
}

//! A registry of the ROS 2 message types Arora knows.
//!
//! Every ROS message reduces to one triple: an arora [`low::Type`], the
//! dependency types it nests, and its ROS-qualified name (REP-2016 form,
//! `geometry_msgs/msg/Point`, kept in [`low::Type::name`]). [`Ros2Registry`]
//! holds those triples keyed by type id, and indexes them by name so a name can
//! be turned into a type — the operation the node editor needs to type a key.
//!
//! The bundled messages (the generated `msgs` modules) register themselves here;
//! the same registry also accepts types defined at runtime — from a `.msg` file
//! that never entered this crate, or from a JSON blob authored in a behavior —
//! so a new ROS type becomes usable without recompiling.

use std::collections::HashMap;

use arora_types::module::low::TypeRef;
use arora_types::ty::{low, TypeRegistry};
use arora_types::Uuid;

use crate::schema::{Arr, Base, MessageSpec, Primitive};

/// The ROS 2 message types known to a running Arora, keyed by type id and
/// indexed by ROS name.
#[derive(Debug, Clone, Default)]
pub struct Ros2Registry {
    /// The arora walk registry the codec and hash traverse.
    types: TypeRegistry,
    /// ROS name → type id, holding both the stored REP-2016 name
    /// (`geometry_msgs/msg/Point`) and its short form (`geometry_msgs/Point`).
    by_name: HashMap<String, Uuid>,
}

impl Ros2Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one type, indexing it under its ROS name. Nested types it
    /// references must be inserted too (see [`define`](Self::define) /
    /// [`extend`](Self::extend)); this call does not pull them.
    pub fn insert(&mut self, ty: low::Type) -> Uuid {
        let id = ty.id;
        for name in name_aliases(&ty.name) {
            self.by_name.insert(name, id);
        }
        self.types.insert(id, ty);
        id
    }

    /// Insert every type in an arora [`TypeRegistry`] (e.g. the one
    /// [`AroraType::arora_type_with_registry`](arora_types::AroraType) yields),
    /// indexing each by name.
    pub fn extend(&mut self, registry: TypeRegistry) {
        for (_, ty) in registry {
            self.insert(ty);
        }
    }

    /// Define a type together with its dependency closure, returning its id.
    /// This is the runtime path a ROS developer uses to add a type that was
    /// never a `.msg` in this crate: hand it the type and the registry of what
    /// it nests, and it is immediately encodable and hashable.
    pub fn define(&mut self, ty: low::Type, deps: TypeRegistry) -> Uuid {
        self.extend(deps);
        self.insert(ty)
    }

    /// Define types from a JSON array of [`low::Type`] — the type and every
    /// type it nests. This is the "define a type on the fly" path: a behavior
    /// saves the closure as JSON (each [`low::Type`] carries its ROS name), and
    /// loading it here makes the topic speakable to ROS clients. Returns the ids
    /// in the order given (the first is conventionally the message type).
    pub fn define_from_json(&mut self, json: &str) -> Result<Vec<Uuid>, serde_json::Error> {
        let types: Vec<low::Type> = serde_json::from_str(json)?;
        Ok(types.into_iter().map(|ty| self.insert(ty)).collect())
    }

    /// Register a type known at compile time — a generated message struct, or
    /// any `#[derive(AroraType)]` type — together with the types it nests. This
    /// is what the generated per-package `register` functions call.
    pub fn register_arora<T: arora_types::AroraType>(&mut self) {
        let mut deps = TypeRegistry::new();
        T::register_types(&mut deps);
        self.extend(deps);
    }

    /// Define a type from the text of a `.msg` for `package/name`, returning its
    /// id. This is the path a ROS developer uses for a type that never entered
    /// the crate: the type joins the registry with exactly the id it would have
    /// as a bundled message (the ROS name hashed), so it is immediately
    /// encodable, hashable and — with a ROS bridge enabled — speakable to ROS
    /// clients. Types it nests (e.g. `std_msgs/Header`) must already be present.
    pub fn define_from_msg(
        &mut self,
        package: &str,
        name: &str,
        text: &str,
    ) -> Result<Uuid, String> {
        let spec = crate::schema::parse_msg(package, name, text)?;
        Ok(self.insert(message_spec_to_low_type(&spec)))
    }

    /// The type with this id, if known.
    pub fn get(&self, id: &Uuid) -> Option<&low::Type> {
        self.types.get(id)
    }

    /// The type with this ROS name — either form, `geometry_msgs/msg/Point` or
    /// `geometry_msgs/Point`.
    pub fn get_by_name(&self, name: &str) -> Option<&low::Type> {
        self.by_name.get(name).and_then(|id| self.types.get(id))
    }

    /// The id of the type with this ROS name (either form).
    pub fn id_of(&self, name: &str) -> Option<Uuid> {
        self.by_name.get(name).copied()
    }

    pub fn contains(&self, id: &Uuid) -> bool {
        self.types.contains_key(id)
    }

    /// The REP-2016 names of every known message, sorted — the list a node
    /// editor offers when a user types a key.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .types
            .values()
            .map(|t| t.name.as_str())
            .filter(|n| !n.is_empty())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The underlying arora walk registry, to hand to
    /// [`crate::cdr::encode`]/[`decode`](crate::cdr::decode) and
    /// [`crate::hash::rihs01`].
    pub fn types(&self) -> &TypeRegistry {
        &self.types
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

/// Split a ROS message name into `(package, type)` — accepting both the
/// REP-2016 form `geometry_msgs/msg/Point` and the short form
/// `geometry_msgs/Point`. This is what a caller feeds
/// `ros2_client::MessageTypeName::new(package, type)`.
pub fn package_and_type(name: &str) -> Option<(&str, &str)> {
    if let Some((package, rest)) = name.split_once("/msg/") {
        return Some((package, rest));
    }
    name.rsplit_once('/')
}

/// The names a type is indexed under: its stored name, plus the short
/// `package/Type` form when the stored name is REP-2016 `package/msg/Type`.
fn name_aliases(name: &str) -> Vec<String> {
    if name.is_empty() {
        return Vec::new();
    }
    let mut aliases = vec![name.to_string()];
    if let Some((package, ty)) = name.split_once("/msg/") {
        aliases.push(format!("{package}/{ty}"));
    }
    aliases
}

/// Build the arora [`low::Type`] for a parsed message. The type id and field ids
/// are `gen_uuid_from_str` of the ROS-qualified name and the field names — the
/// exact scheme the generated `#[derive(AroraType)]` structs use — so a message
/// defined from a `.msg` at runtime carries the same ids as the bundled one, and
/// both stay stable across builds (the id is a pure function of the name).
pub fn message_spec_to_low_type(spec: &MessageSpec) -> low::Type {
    let name = spec.rep2016_name();
    let fields = spec.fields.iter().map(|field| {
        (
            arora_types::gen_uuid_from_str(&field.name),
            low::StructureField {
                name: field.name.clone(),
                type_ref: message_type_ref(&field.ty.base, field.ty.arr),
            },
        )
    });
    low::Type {
        id: arora_types::gen_uuid_from_str(&name),
        name,
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    }
}

/// The [`TypeRef`] for a message field: its element id (a well-known primitive,
/// or the nested message's name hashed) wrapped by its array shape.
fn message_type_ref(base: &Base, arr: Arr) -> TypeRef {
    let element_id = match base {
        Base::Primitive(primitive) => primitive_id(*primitive),
        Base::Named { package, name } => {
            arora_types::gen_uuid_from_str(&format!("{package}/msg/{name}"))
        }
    };
    match arr {
        Arr::Unit => TypeRef::Scalar { id: element_id },
        Arr::Unbounded => TypeRef::Array { id: element_id },
        Arr::Fixed(len) => TypeRef::FixedArray {
            id: element_id,
            len,
        },
    }
}

/// The well-known arora id a ROS primitive maps to.
fn primitive_id(primitive: Primitive) -> Uuid {
    use arora_types::ty;
    match primitive {
        Primitive::Bool => *ty::BOOLEAN_ID,
        Primitive::I8 => *ty::I8_ID,
        Primitive::I16 => *ty::I16_ID,
        Primitive::I32 => *ty::I32_ID,
        Primitive::I64 => *ty::I64_ID,
        Primitive::U8 => *ty::U8_ID,
        Primitive::U16 => *ty::U16_ID,
        Primitive::U32 => *ty::U32_ID,
        Primitive::U64 => *ty::U64_ID,
        Primitive::F32 => *ty::F32_ID,
        Primitive::F64 => *ty::F64_ID,
        Primitive::String => *ty::STRING_ID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::ty::low::{Structure, StructureField, Type, TypeKind};
    use arora_types::{module::low::TypeRef, ty};

    fn point() -> Type {
        Type {
            name: "geometry_msgs/msg/Point".to_string(),
            id: Uuid::from_u128(0x33),
            description: String::new(),
            kind: TypeKind::Structure(Structure::from_fields(vec![(
                Uuid::from_u128(0x331),
                StructureField {
                    name: "x".to_string(),
                    type_ref: TypeRef::Scalar { id: *ty::F64_ID },
                },
            )])),
        }
    }

    #[test]
    fn a_type_is_found_by_either_name_form() {
        let mut reg = Ros2Registry::new();
        let id = reg.insert(point());
        assert_eq!(reg.id_of("geometry_msgs/msg/Point"), Some(id));
        assert_eq!(reg.id_of("geometry_msgs/Point"), Some(id));
        assert_eq!(reg.get_by_name("geometry_msgs/Point").unwrap().id, id);
    }

    #[test]
    fn package_and_type_handles_both_forms() {
        assert_eq!(
            package_and_type("geometry_msgs/msg/Point"),
            Some(("geometry_msgs", "Point"))
        );
        assert_eq!(
            package_and_type("geometry_msgs/Point"),
            Some(("geometry_msgs", "Point"))
        );
    }

    #[test]
    fn names_lists_and_sorts_the_rep2016_form() {
        let mut reg = Ros2Registry::new();
        reg.insert(point());
        assert_eq!(reg.names(), vec!["geometry_msgs/msg/Point"]);
    }

    #[test]
    fn define_from_json_round_trips_a_saved_type() {
        let mut reg = Ros2Registry::new();
        let json = serde_json::to_string(&vec![point()]).unwrap();
        let ids = reg.define_from_json(&json).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(reg.id_of("geometry_msgs/Point"), Some(ids[0]));
    }
}

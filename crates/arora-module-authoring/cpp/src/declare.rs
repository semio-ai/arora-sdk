use crate::{
    ast::{
        ArrayKind, Block, Declaration, Enum, Expression, FunctionImplementation, FunctionPrototype,
        Parameter, Statement, Struct, ToExpression, ToPrettyString, TypeRef, Variable,
    },
    constant, func, id, identifier_name, identifier_uuid, ty, Context,
};
use arora_registry::{EnumerationFrozen, StructureFrozen, TypeDefinitionFrozen};
use arora_types::record::{
    structure::frozen::StructureField,
    ty::{FrozenTy, PrimitiveKind},
};
use uuid::Uuid;

fn uuid_initializer_list(uuid: &Uuid) -> Expression {
    let id_bytes = uuid.as_bytes();
    Expression::InitializerList(vec![
        Expression::IntegerLiteral(id_bytes[0] as i64),
        Expression::IntegerLiteral(id_bytes[1] as i64),
        Expression::IntegerLiteral(id_bytes[2] as i64),
        Expression::IntegerLiteral(id_bytes[3] as i64),
        Expression::IntegerLiteral(id_bytes[4] as i64),
        Expression::IntegerLiteral(id_bytes[5] as i64),
        Expression::IntegerLiteral(id_bytes[6] as i64),
        Expression::IntegerLiteral(id_bytes[7] as i64),
        Expression::IntegerLiteral(id_bytes[8] as i64),
        Expression::IntegerLiteral(id_bytes[9] as i64),
        Expression::IntegerLiteral(id_bytes[10] as i64),
        Expression::IntegerLiteral(id_bytes[11] as i64),
        Expression::IntegerLiteral(id_bytes[12] as i64),
        Expression::IntegerLiteral(id_bytes[13] as i64),
        Expression::IntegerLiteral(id_bytes[14] as i64),
        Expression::IntegerLiteral(id_bytes[15] as i64),
    ])
}

pub fn uuid_variable(name: String, uuid: &Uuid) -> Variable {
    Variable {
        name,
        ty: ty::U8_CONST.clone(),
        value: uuid_initializer_list(uuid).into(),
        array: ArrayKind::Fixed(16.to_expression()),
        ..Default::default()
    }
}

pub fn arora_buffer_writer() -> Variable {
    Variable {
        name: "writer".to_string(),
        ty: ty::ARORA_BUFFER_WRITER_PTR.clone(),
        value: Some(func::ARORA_BUFFER_WRITER_NEW.call::<String, _>([])),
        ..Default::default()
    }
}

pub fn arora_buffer_reader(identifier: &str) -> Variable {
    Variable {
        name: "reader".to_string(),
        ty: ty::ARORA_BUFFER_READER_PTR.clone(),
        value: Some(func::ARORA_BUFFER_READER_NEW.call([identifier])),
        ..Default::default()
    }
}

pub fn arora_buffer_writer_free() -> Statement {
    func::ARORA_BUFFER_WRITER_FREE
        .call(["writer"])
        .into_statement()
}

pub fn arora_buffer_writer_finalize() -> Expression {
    func::ARORA_BUFFER_WRITER_FINALIZE.call(["writer".to_expression(), 0u8.to_expression()])
}

pub fn arora_buffer_reader_free() -> Statement {
    func::ARORA_BUFFER_READER_FREE
        .call(["reader"])
        .into_statement()
}

pub fn arora_buffer_writer_begin_structure(
    uuid_identifier: &str,
    field_count: Expression,
) -> Statement {
    func::ARORA_BUFFER_WRITER_BEGIN_STRUCTURE
        .call([
            "writer".to_expression(),
            uuid_identifier.to_expression(),
            field_count,
        ])
        .into_statement()
}

pub fn arora_buffer_writer_add_structure_field(field_identifier: Expression) -> Statement {
    func::ARORA_BUFFER_WRITER_ADD_STRUCTURE_FIELD
        .call(["writer".to_expression(), field_identifier])
        .into_statement()
}

pub fn arora_buffer_skip() -> Expression {
    func::ARORA_BUFFER_SKIP.call(["reader", "variant"])
}

pub fn arora_buffer_skip_array() -> Expression {
    func::ARORA_BUFFER_SKIP_ARRAY.call(["reader", "array_type"])
}

pub fn serialize(type_name: &str, value: &Expression) -> Statement {
    format!(
        "arora::buffer::serialize<{}>(writer, {})",
        type_name,
        value.to_pretty_string(0)
    )
    .to_expression()
    .into_statement()
}

pub fn deserialize(type_name: &str) -> Expression {
    format!("arora::buffer::deserialize<{}>(reader)", type_name).to_expression()
}

// ARORA_BUFFER_READER_GET_STRUCTURE_FIELD
pub fn arora_buffer_reader_get_structure_field() -> Expression {
    func::ARORA_BUFFER_READER_GET_STRUCTURE_FIELD.call(["reader"])
}

// arora_buffer_reader_next_type
pub fn arora_buffer_reader_next_type() -> Expression {
    func::ARORA_BUFFER_READER_NEXT_TYPE.call(["reader"])
}

pub fn assert(expression: Expression) -> Statement {
    func::ASSERT.call([expression]).into_statement()
}

// ARORA_BUFFER_READER_NEW
pub fn arora_buffer_reader_new(identifier: Expression) -> Expression {
    func::ARORA_BUFFER_READER_NEW.call([identifier])
}

// ARORA_BUFFER_READER_GET_STRUCTURE
pub fn arora_buffer_reader_get_structure() -> Expression {
    func::ARORA_BUFFER_READER_GET_STRUCTURE.call(["reader"])
}

pub fn structure(context: &Context, name: &str, ty: &StructureFrozen) -> Struct {
    let mut declarations = Vec::new();
    for (_, field) in &ty.fields {
        declarations.push(
            FunctionPrototype {
                name: field.name.clone(),
                parameters: vec![],
                ret: Some(ty::optional_const_ref(&TypeRef {
                    ty: ty::type_name(context, &field.ty),
                    ..Default::default()
                })),
                constant: true,
                noexcept: true,
                ..Default::default()
            }
            .into(),
        );

        declarations.push(
            FunctionPrototype {
                name: format!("set_{}", field.name),
                parameters: vec![Parameter {
                    name: "value".to_string(),
                    type_ref: ty::optional_const_ref(&TypeRef {
                        ty: ty::type_name(context, &field.ty),
                        ..Default::default()
                    }),
                }],
                ret: Some(ty::VOID.clone()),
                ..Default::default()
            }
            .into(),
        );

        declarations.push(
            FunctionPrototype {
                name: format!("set_{}", field.name),
                parameters: vec![Parameter {
                    name: "value".to_string(),
                    type_ref: ty::optional_move(&TypeRef {
                        ty: ty::type_name(context, &field.ty),
                        ..Default::default()
                    }),
                }],
                ret: Some(ty::VOID.clone()),
                ..Default::default()
            }
            .into(),
        );
    }

    declarations.push(Declaration::private());

    for (id, field) in &ty.fields {
        declarations.push(
            Variable {
                name: structure_private_field_variable_name(id, field),
                ty: ty::optional(&TypeRef {
                    ty: ty::type_name(context, &field.ty),
                    ..Default::default()
                }),
                ..Default::default()
            }
            .into(),
        );
    }

    Struct {
        block: Block {
            statements: declarations,
            semicolon: true,
        },
        name: name.to_string(),
        specialization: None,
        template_arguments: None,
    }
}

pub fn enumeration_constants(_: &Uuid, name: &str, ty: &EnumerationFrozen) -> Vec<Declaration> {
    let mut ret = Vec::new();

    ret.push(
        Variable {
            name: id::type_uuid(name),
            ty: ty::U8_CONST.clone(),
            extern_: true,
            array: ArrayKind::Fixed(16u64.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    for (_, variant) in ty.variants.iter() {
        ret.push(
            Variable {
                name: id::value_uuid(name, &variant.name),
                ty: ty::U8_CONST.clone(),
                extern_: true,
                array: ArrayKind::Fixed(16u64.to_expression()),
                ..Default::default()
            }
            .into(),
        );
    }

    ret
}

pub fn structure_constants(_: &Uuid, name: &str, ty: &StructureFrozen) -> Vec<Declaration> {
    let mut ret = Vec::new();

    ret.push(
        Variable {
            name: id::type_uuid(name),
            ty: ty::U8_CONST.clone(),
            extern_: true,
            array: ArrayKind::Fixed(16u64.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    for (_, value) in ty.fields.iter() {
        ret.push(
            Variable {
                name: id::field_uuid(name, &value.name),
                ty: ty::U8_CONST.clone(),
                extern_: true,
                array: ArrayKind::Fixed(16u64.to_expression()),
                ..Default::default()
            }
            .into(),
        );
    }

    ret
}

pub fn type_constants(id: &Uuid, ty: &TypeDefinitionFrozen) -> Vec<Declaration> {
    match &ty {
        TypeDefinitionFrozen::Structure(v) => structure_constants(id, &ty.name(), v),
        TypeDefinitionFrozen::Enumeration(v) => enumeration_constants(id, &ty.name(), v),
        TypeDefinitionFrozen::Primitive(_) => {
            panic!("forbidden to define primitive type {}", id)
        }
    }
}

pub fn enumeration_constants_impl(
    id: &Uuid,
    name: &str,
    ty: &EnumerationFrozen,
) -> Vec<Declaration> {
    let mut ret = Vec::new();

    ret.push(
        Variable {
            name: id::type_uuid(name),
            ty: ty::U8_CONST.clone(),
            value: Some(uuid_initializer_list(id)),
            array: ArrayKind::Fixed(16u64.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    for (id, variant) in ty.variants.iter() {
        ret.push(
            Variable {
                name: id::value_uuid(name, &variant.name),
                ty: ty::U8_CONST.clone(),
                value: Some(uuid_initializer_list(id)),
                array: ArrayKind::Fixed(16u64.to_expression()),
                ..Default::default()
            }
            .into(),
        );
    }

    ret
}

pub fn structure_constants_impl(id: &Uuid, name: &str, ty: &StructureFrozen) -> Vec<Declaration> {
    let mut ret = Vec::new();

    ret.push(
        Variable {
            name: id::type_uuid(name),
            ty: ty::U8_CONST.clone(),
            value: Some(uuid_initializer_list(id)),
            array: ArrayKind::Fixed(16u64.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    for (id, value) in ty.fields.iter() {
        ret.push(
            Variable {
                name: id::field_uuid(name, &value.name),
                ty: ty::U8_CONST.clone(),
                value: Some(uuid_initializer_list(id)),
                array: ArrayKind::Fixed(16u64.to_expression()),
                ..Default::default()
            }
            .into(),
        );
    }

    ret
}

pub fn type_constants_impl(id: &Uuid, ty: &TypeDefinitionFrozen) -> Vec<Declaration> {
    match ty {
        TypeDefinitionFrozen::Structure(v) => structure_constants_impl(id, &v.name, v),
        TypeDefinitionFrozen::Enumeration(v) => enumeration_constants_impl(id, &v.name, v),
        TypeDefinitionFrozen::Primitive(v) => {
            panic!("forbidden to define primitive type {}", v)
        }
    }
}

pub fn is_unit(ty: &FrozenTy) -> bool {
    match ty {
        FrozenTy::Primitive(as_primitive) => as_primitive.kind == PrimitiveKind::Unit,
        _ => false,
    }
}

pub fn enumeration(context: &Context, name: &str, ty: &EnumerationFrozen) -> Struct {
    let mut enumeration_values = Vec::new();
    for (_, variant) in ty.variants.iter() {
        enumeration_values.push(variant.name.clone());
    }

    let enumeration = Enum {
        name: "Variant".to_string(),
        members: enumeration_values,
    };

    let mut data_size_args = Vec::new();
    for (_, variant) in ty.variants.iter() {
        if is_unit(&variant.ty) {
            continue;
        }

        data_size_args.push(func::SIZEOF.call([ty::type_name(context, &variant.ty)]));
    }

    let mut struct_statements: Vec<Declaration> = Vec::new();

    struct_statements.push(enumeration.into());
    struct_statements.push(Declaration::new_line(1));

    struct_statements.push(
        FunctionPrototype {
            name: "get_variant".to_string(),
            ret: Some(TypeRef {
                ty: "Variant".to_string(),
                ..Default::default()
            }),
            constant: true,
            noexcept: true,
            ..Default::default()
        }
        .into(),
    );

    for (_, variant) in ty.variants.iter() {
        struct_statements.push(Declaration::new_line(1));

        if variant.ty.is_scalar()
            || variant.ty.is_primitive()
                && variant.ty.as_primitive().unwrap().is_scalar()
                && variant.ty.as_primitive().unwrap().kind != PrimitiveKind::Unit
        {
            // Static method to create an instance of the variant.
            struct_statements.push(
                FunctionPrototype {
                    name: variant.name.to_lowercase().to_string(),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    parameters: vec![Parameter {
                        name: "value".to_string(),
                        type_ref: TypeRef {
                            ty: ty::type_name(context, &variant.ty),
                            reference: true,
                            constant: true,
                            ..Default::default()
                        },
                    }],
                    static_: true,
                    noexcept: true,
                    ..Default::default()
                }
                .into(),
            );

            struct_statements.push(
                FunctionPrototype {
                    name: variant.name.to_lowercase().to_string(),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    parameters: vec![Parameter {
                        name: "value".to_string(),
                        type_ref: TypeRef {
                            ty: ty::type_name(context, &variant.ty),
                            rvalue_reference: true,
                            ..Default::default()
                        },
                    }],
                    static_: true,
                    noexcept: true,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            struct_statements.push(
                FunctionPrototype {
                    name: variant.name.to_lowercase().to_string(),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    static_: true,
                    noexcept: true,
                    ..Default::default()
                }
                .into(),
            );
        }

        struct_statements.push(
            FunctionPrototype {
                name: format!("is_{}", variant.name.to_lowercase()),
                ret: Some(ty::BOOL.clone()),
                constant: true,
                noexcept: true,
                ..Default::default()
            }
            .into(),
        );

        if variant.ty.is_scalar()
            || variant.ty.is_primitive()
                && variant.ty.as_primitive().unwrap().is_scalar()
                && variant.ty.as_primitive().unwrap().kind != PrimitiveKind::Unit
        {
            struct_statements.push(
                FunctionPrototype {
                    name: format!("as_{}", variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: ty::type_name(context, &variant.ty),
                        reference: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .into(),
            );

            struct_statements.push(
                FunctionPrototype {
                    name: format!("as_{}", variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: ty::type_name(context, &variant.ty),
                        reference: true,
                        constant: true,
                        ..Default::default()
                    }),
                    constant: true,
                    ..Default::default()
                }
                .into(),
            );
        }
    }

    struct_statements.push(Declaration::new_line(1));
    struct_statements.push(Declaration::private());
    struct_statements.push(
        FunctionPrototype {
            name: "destroy_".to_string(),
            ret: Some(ty::VOID.clone()),
            ..Default::default()
        }
        .into(),
    );

    struct_statements.push(
        Variable {
            name: "variant_".to_string(),
            ty: TypeRef {
                ty: "Variant".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
        .into(),
    );

    if !data_size_args.is_empty() {
        struct_statements.push(
            Variable {
                name: "data_".to_string(),
                ty: ty::U8.clone(),
                array: ArrayKind::Fixed(func::STD_MAX.call(data_size_args)),
                ..Default::default()
            }
            .into(),
        );
    }

    Struct {
        block: Block {
            statements: struct_statements,
            semicolon: true,
        },
        name: name.to_string(),
        specialization: None,
        template_arguments: None,
    }
}

pub fn ty(context: &Context, ty: &TypeDefinitionFrozen) -> Struct {
    match ty {
        TypeDefinitionFrozen::Enumeration(value) => enumeration(context, &ty.name(), value),
        TypeDefinitionFrozen::Structure(value) => structure(context, &ty.name(), value),
        TypeDefinitionFrozen::Primitive(_) => {
            panic!("forbidden to define primitive type {}", ty.name())
        }
    }
}

pub fn enumeration_impl(
    context: &Context,
    _: &Uuid,
    name: &str,
    ty: &EnumerationFrozen,
) -> Vec<Declaration> {
    let mut ret = Vec::new();

    ret.push(Declaration::new_line(1));

    ret.push(
        FunctionImplementation {
            name: format!("{}::get_variant", name),
            ret: Some(TypeRef {
                ty: format!("{}::Variant", name),
                ..Default::default()
            }),
            constant: true,
            noexcept: true,
            body: Block {
                statements: vec![Statement::Return("variant_".to_expression()).into()],
                semicolon: false,
            },
            ..Default::default()
        }
        .into(),
    );

    ret.push(
        FunctionImplementation {
            name: format!("{}::destroy_", name),
            ret: Some(ty::VOID.clone()),
            ..Default::default()
        }
        .into(),
    );

    for (_, variant) in ty.variants.iter() {
        ret.push(Declaration::new_line(1));

        if !is_unit(&variant.ty) {
            ret.push(
                FunctionImplementation {
                    name: format!("{}::{}", name, variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    parameters: vec![Parameter {
                        name: "value".to_string(),
                        type_ref: TypeRef {
                            ty: ty::type_name(context, &variant.ty),
                            reference: true,
                            constant: true,
                            ..Default::default()
                        },
                    }],
                    noexcept: true,
                    body: Block {
                        statements: vec![],
                        semicolon: false,
                    },
                    ..Default::default()
                }
                .into(),
            );

            ret.push(
                FunctionImplementation {
                    name: format!("{}::{}", name, variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    parameters: vec![Parameter {
                        name: "value".to_string(),
                        type_ref: TypeRef {
                            ty: ty::type_name(context, &variant.ty),
                            rvalue_reference: true,
                            ..Default::default()
                        },
                    }],
                    noexcept: true,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            ret.push(
                FunctionImplementation {
                    name: format!("{}::{}", name, variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: name.to_string(),
                        ..Default::default()
                    }),
                    body: Block {
                        statements: vec![
                            Variable {
                                name: "ret".to_string(),
                                ty: TypeRef {
                                    ty: name.to_string(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            }
                            .into(),
                            "ret"
                                .to_expression()
                                .dot("variant_")
                                .assign(
                                    "Variant".to_expression().colon_colon(variant.name.as_str()),
                                )
                                .into_statement()
                                .into(),
                            Statement::Return("ret".to_expression()).into(),
                        ],
                        semicolon: false,
                    },
                    noexcept: true,
                    ..Default::default()
                }
                .into(),
            );
        }

        ret.push(
            FunctionImplementation {
                name: format!("{}::is_{}", name, variant.name.to_lowercase()),
                ret: Some(ty::BOOL.clone()),
                constant: true,
                noexcept: true,
                body: Block {
                    statements: vec![Statement::Return(
                        "variant_"
                            .to_expression()
                            .equal("Variant".to_expression().colon_colon(variant.name.as_str())),
                    )
                    .into()],
                    semicolon: false,
                },
                ..Default::default()
            }
            .into(),
        );

        if !is_unit(&variant.ty) {
            // Generate Enumeration::as_value()
            ret.push(
                FunctionImplementation {
                    name: format!("{}::as_{}", name, variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: ty::type_name(context, &variant.ty),
                        reference: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                }
                .into(),
            );

            // Generate Enumeration::as_value()
            ret.push(
                FunctionImplementation {
                    name: format!("{}::as_{}", name, variant.name.to_lowercase()),
                    ret: Some(TypeRef {
                        ty: ty::type_name(context, &variant.ty),
                        reference: true,
                        constant: true,
                        ..Default::default()
                    }),
                    constant: true,
                    ..Default::default()
                }
                .into(),
            );
        }
    }

    ret
}

fn structure_private_field_variable_name(id: &Uuid, field: &StructureField) -> String {
    format!(
        "{}_{}",
        identifier_name(field.name.as_str()),
        identifier_uuid(id)
    )
}

fn structure_private_field_variable(id: &Uuid, field: &StructureField) -> Expression {
    structure_private_field_variable_name(id, field).to_expression()
}

pub fn structure_impl(context: &Context, name: &str, ty: &StructureFrozen) -> Vec<Declaration> {
    let mut ret = Vec::new();

    for (id, field) in ty.fields.iter() {
        ret.push(
            FunctionImplementation {
                name: format!("{}::{}", name, field.name.to_lowercase()),
                ret: Some(ty::optional_const_ref(&TypeRef {
                    ty: ty::type_name(context, &field.ty),
                    ..Default::default()
                })),
                body: Block {
                    statements: vec![Statement::Return(structure_private_field_variable(
                        id, field,
                    ))
                    .into()],
                    semicolon: false,
                },
                constant: true,
                noexcept: true,
                ..Default::default()
            }
            .into(),
        );

        ret.push(
            FunctionImplementation {
                name: format!("{}::set_{}", name, field.name.to_lowercase()),
                parameters: vec![Parameter {
                    name: "value".to_string(),
                    type_ref: ty::optional_const_ref(&TypeRef {
                        ty: ty::type_name(context, &field.ty),
                        ..Default::default()
                    }),
                }],
                ret: Some(ty::VOID.clone()),
                body: Block {
                    statements: vec![structure_private_field_variable(id, field)
                        .assign("value")
                        .into_statement()
                        .into()],
                    semicolon: false,
                },
                ..Default::default()
            }
            .into(),
        );
        ret.push(
            FunctionImplementation {
                name: format!("{}::set_{}", name, field.name.to_lowercase()),
                parameters: vec![Parameter {
                    name: "value".to_string(),
                    type_ref: ty::optional_move(&TypeRef {
                        ty: ty::type_name(context, &field.ty),
                        ..Default::default()
                    }),
                }],
                ret: Some(ty::VOID.clone()),
                body: Block {
                    statements: vec![structure_private_field_variable(id, field)
                        .assign("value")
                        .into_statement()
                        .into()],
                    semicolon: false,
                },
                ..Default::default()
            }
            .into(),
        );
    }

    ret
}

pub fn ty_impl(context: &Context, id: &Uuid, ty: &TypeDefinitionFrozen) -> Vec<Declaration> {
    match ty {
        TypeDefinitionFrozen::Enumeration(value) => {
            enumeration_impl(context, id, &value.name, value)
        }
        TypeDefinitionFrozen::Structure(value) => structure_impl(context, &value.name, value),
        TypeDefinitionFrozen::Primitive(_) => {
            panic!("forbidden to define primitive type {}", id)
        }
    }
}

pub fn structure_deserializer(
    context: &Context,
    name: &str,
    ty: &StructureFrozen,
) -> FunctionImplementation {
    let mut function_statements = Vec::<Declaration>::new();

    function_statements.push(
        Statement::If(
            arora_buffer_reader_next_type().equal(constant::ARORA_BUFFER_TYPE_STRUCTURE.clone()),
            Block {
                statements: vec![Statement::Return(constant::NULL_OPTION.clone()).into()],
                semicolon: false,
            },
            None,
        )
        .into(),
    );

    let structure_metadata = "structure_metadata".to_expression();
    let field_count = "field_count".to_expression();

    function_statements.push(
        Variable {
            name: "structure_metadata".to_string(),
            ty: ty::ARORA_GET_STRUCTURE_RESULT.clone(),
            value: Some(arora_buffer_reader_get_structure()),
            ..Default::default()
        }
        .into(),
    );

    function_statements.push(
        Statement::If(
            structure_metadata
                .dot(field_count.clone())
                .greater_than("0".to_expression())
                .logical_and(
                    func::ARORA_UUID_COMPARE
                        .call([
                            Expression::Dot(
                                "structure_metadata".to_expression().into(),
                                "id".to_expression().into(),
                            ),
                            id::type_uuid(name).to_expression(),
                        ])
                        .not_equal("0".to_expression()),
                ),
            Block {
                statements: vec![Statement::Return(constant::NULL_OPTION.clone()).into()],
                ..Default::default()
            },
            None,
        )
        .into(),
    );

    function_statements.push(
        Variable {
            name: "__arora_result__".to_string(),
            ty: TypeRef {
                ty: name.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
        .into(),
    );

    let mut sorted_field_ids = ty.fields.keys().collect::<Vec<_>>();
    sorted_field_ids.sort();

    function_statements.push(
        Variable {
            name: "field_index".to_string(),
            ty: ty::U32.clone(),
            value: Some("0".to_expression()),
            ..Default::default()
        }
        .into(),
    );

    function_statements.push(
        Variable {
            name: "field".to_string(),
            ty: ty::U8_CONST_PTR.clone(),
            value: Some(arora_buffer_reader_get_structure_field()),
            ..Default::default()
        }
        .into(),
    );

    function_statements.push(
        Variable {
            name: "current_res".to_string(),
            ty: ty::U8.clone(),
            value: Some(0u64.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    let field_value = "field".to_expression();
    let field_index = "field_index".to_expression();
    let current_res = "current_res".to_expression();

    for (i, field_id) in sorted_field_ids.iter().enumerate() {
        let field = ty.fields.get(*field_id).unwrap();

        let mut field_declarations: Vec<Declaration> = Vec::new();

        let type_name = ty::type_name(context, &field.ty);

        field_declarations.push(
            "__arora_result__"
                .to_expression()
                .dot(format!("set_{}", field.name))
                .call([deserialize(&type_name)])
                .into_statement()
                .into(),
        );

        field_declarations.push(field_index.clone().pre_increment().into_statement().into());

        if i < sorted_field_ids.len() - 1 {
            field_declarations.push(
                field_value
                    .clone()
                    .assign(arora_buffer_reader_get_structure_field())
                    .into_statement()
                    .into(),
            );
        }

        function_statements.push(
            Statement::While(
                field_index
                    .less_than(structure_metadata.dot(field_count.clone()))
                    .logical_and(
                        current_res
                            .assign(func::ARORA_UUID_COMPARE.call([
                                field_value.clone(),
                                id::field_uuid(name, &field.name).to_expression(),
                            ]))
                            .parenthesized(),
                    )
                    .less_than("0".to_expression()),
                Block {
                    statements: vec![
                        field_index.clone().pre_increment().into_statement().into(),
                        field_value
                            .clone()
                            .assign(arora_buffer_reader_get_structure_field())
                            .into_statement()
                            .into(),
                    ],
                    semicolon: false,
                },
            )
            .into(),
        );

        function_statements.push(
            Statement::If(
                field_index
                    .less_than(structure_metadata.dot(field_count.clone()))
                    .logical_and(current_res.equal("0".to_expression())),
                Block {
                    statements: field_declarations,
                    ..Default::default()
                },
                None,
            )
            .into(),
        );
    }

    function_statements.push(Statement::Return("__arora_result__".to_expression()).into());

    FunctionImplementation {
        name: "arora::buffer::deserialize".to_string(),
        ret: Some(ty::optional(&TypeRef {
            ty: name.to_string(),
            ..Default::default()
        })),
        parameters: vec![Parameter {
            name: "reader".to_string(),
            type_ref: ty::ARORA_BUFFER_READER_PTR.clone(),
        }],
        noexcept: true,
        body: Block {
            statements: function_statements,
            semicolon: false,
        },
        template_arguments: Some(vec![]),
        specialization: Some(vec![name.to_string()]),
        inline: true,
        ..Default::default()
    }
}

pub fn enumeration_deserializer(
    context: &Context,
    _: &Uuid,
    name: &str,
    ty: &EnumerationFrozen,
) -> FunctionImplementation {
    let mut function_statements: Vec<Declaration> = vec![
        Variable {
            name: "variant".to_string(),
            ty: ty::U8_CONST.clone(),
            value: Some(arora_buffer_reader_next_type()),
            ..Default::default()
        }
        .into(),
        Statement::If(
            "variant"
                .to_expression()
                .not_equal(constant::ARORA_BUFFER_TYPE_ENUMERATION.clone()),
            Block {
                statements: vec![
                    arora_buffer_skip().into_statement().into(),
                    Statement::Return(constant::NULL_OPTION.clone()).into(),
                ],
                semicolon: false,
            },
            None,
        )
        .into(),
        Variable {
            name: "res".to_string(),
            ty: ty::ARORA_GET_ENUMERATION_VALUE_RESULT.clone(),
            value: Some(func::ARORA_BUFFER_READER_GET_ENUMERATION_VALUE.call(["reader"])),
            ..Default::default()
        }
        .into(),
        Statement::If(
            func::ARORA_UUID_COMPARE
                .call([
                    "res".to_expression().dot("id"),
                    id::type_uuid(name).to_expression(),
                ])
                .not_equal(0u8.to_expression()),
            Block {
                statements: vec![
                    func::ARORA_BUFFER_SKIP
                        .call(["reader".to_expression(), arora_buffer_reader_next_type()])
                        .into_statement()
                        .into(),
                    Statement::Return(constant::NULL_OPTION.clone()).into(),
                ],
                semicolon: false,
            },
            None,
        )
        .into(),
    ];

    for (_, variant) in ty.variants.iter() {
        let ret = if variant.ty.is_scalar()
            || variant.ty.is_primitive()
                && variant.ty.as_primitive().unwrap().is_scalar()
                && variant.ty.as_primitive().unwrap().kind != PrimitiveKind::Unit
        {
            Statement::Return(
                name.to_expression()
                    .colon_colon(variant.name.to_lowercase().to_expression())
                    .call([deserialize(ty::type_name(context, &variant.ty).as_str())]),
            )
        } else {
            Statement::Return(
                name.to_expression()
                    .colon_colon(variant.name.to_lowercase().to_expression())
                    .call::<String, _>([]),
            )
        };
        function_statements.push(
            Statement::If(
                func::ARORA_UUID_COMPARE
                    .call([
                        "res".to_expression().dot("value_id"),
                        id::value_uuid(name, &variant.name).to_expression(),
                    ])
                    .equal(0u8.to_expression()),
                Block {
                    statements: vec![ret.into()],
                    semicolon: false,
                },
                None,
            )
            .into(),
        );
    }

    function_statements.push(Statement::Return(constant::NULL_OPTION.clone()).into());

    FunctionImplementation {
        name: "arora::buffer::deserialize".to_string(),
        ret: Some(ty::optional(&TypeRef {
            ty: name.to_string(),
            ..Default::default()
        })),
        parameters: vec![Parameter {
            name: "reader".to_string(),
            type_ref: ty::ARORA_BUFFER_READER_PTR.clone(),
        }],
        noexcept: true,
        body: Block {
            statements: function_statements,
            semicolon: false,
        },
        template_arguments: Some(vec![]),
        specialization: Some(vec![name.to_string()]),
        inline: true,
        ..Default::default()
    }
}

pub fn type_of(ty: &TypeDefinitionFrozen) -> FunctionImplementation {
    let buffer_type_constant = match ty {
        TypeDefinitionFrozen::Structure(_) => &*constant::ARORA_BUFFER_TYPE_STRUCTURE,
        TypeDefinitionFrozen::Enumeration(_) => &*constant::ARORA_BUFFER_TYPE_ENUMERATION,
        TypeDefinitionFrozen::Primitive(_) => {
            panic!("forbidden to define primitive type {}", ty.name())
        }
    };

    FunctionImplementation {
        template_arguments: Some(vec![]),
        inline: true,
        ret: Some(TypeRef {
            ty: "int".to_string(),
            ..Default::default()
        }),
        name: "arora::buffer::arora_buffer_type_of".to_string(),
        specialization: Some(vec![ty.name().clone()]),
        noexcept: true,
        body: Block {
            statements: vec![Declaration::Statement(Statement::Return(
                buffer_type_constant.clone(),
            ))],
            semicolon: false,
        },
        ..Default::default()
    }
}

pub fn deserializer(
    context: &Context,
    type_id: &Uuid,
    ty: &TypeDefinitionFrozen,
) -> FunctionImplementation {
    match ty {
        TypeDefinitionFrozen::Structure(ref structure) => {
            structure_deserializer(context, &ty.name(), structure)
        }
        TypeDefinitionFrozen::Enumeration(ref enumeration) => {
            enumeration_deserializer(context, type_id, &ty.name(), enumeration)
        }
        _ => panic!("deserializer: not implemented for {:?}", ty),
    }
}

pub fn structure_serializer(
    context: &Context,
    name: &str,
    ty: &StructureFrozen,
) -> FunctionImplementation {
    let value_name = "value".to_string();
    let field_count = "field_count".to_string();

    let mut function_statements = Vec::<Declaration>::new();

    let mut sorted_field_ids = ty.fields.keys().collect::<Vec<_>>();
    sorted_field_ids.sort();

    function_statements.push(
        Variable {
            name: field_count.clone(),
            ty: ty::U32.clone(),
            value: Some(0u32.to_expression()),
            ..Default::default()
        }
        .into(),
    );

    // Count fields that are available
    for field_id in sorted_field_ids.iter() {
        let field = ty.fields.get(*field_id).unwrap();
        function_statements.push(
            Statement::If(
                value_name
                    .to_expression()
                    .dot(field.name.as_str())
                    .call::<String, _>([])
                    .logical_not()
                    .logical_not(),
                Block {
                    statements: vec![field_count
                        .to_expression()
                        .pre_increment()
                        .into_statement()
                        .into()],
                    semicolon: false,
                },
                None,
            )
            .into(),
        );
    }

    function_statements.push(
        arora_buffer_writer_begin_structure(&id::type_uuid(name), field_count.to_expression())
            .into(),
    );

    for field_id in sorted_field_ids {
        let field = ty.fields.get(field_id).unwrap();
        let value_accessor = value_name.to_expression().dot(field.name.as_str());
        function_statements.push(
            Statement::If(
                value_accessor
                    .call::<String, _>([])
                    .logical_not()
                    .logical_not(),
                Block {
                    statements: vec![
                        arora_buffer_writer_add_structure_field(
                            id::field_uuid(name, &field.name).to_expression(),
                        )
                        .into(),
                        format!(
                            "arora::buffer::serialize<{}>",
                            ty::type_name(context, &field.ty)
                        )
                        .to_expression()
                        .call([
                            "writer".to_expression(),
                            value_accessor.call::<String, _>([]).dereference(),
                        ])
                        .into_statement()
                        .into(),
                    ],
                    semicolon: false,
                },
                None,
            )
            .into(),
        );
    }

    FunctionImplementation {
        name: "arora::buffer::serialize".to_string(),
        ret: Some(ty::VOID.clone()),
        parameters: vec![
            Parameter {
                name: "writer".to_string(),
                type_ref: ty::ARORA_BUFFER_WRITER_PTR.clone(),
            },
            Parameter {
                name: value_name,
                type_ref: TypeRef {
                    ty: name.to_string(),
                    reference: true,
                    constant: true,
                    ..Default::default()
                },
            },
        ],
        noexcept: true,
        body: Block {
            statements: function_statements,
            semicolon: false,
        },
        template_arguments: Some(vec![]),
        specialization: Some(vec![name.to_string()]),
        inline: true,
        ..Default::default()
    }
}

pub fn enumeration_serializer(
    _: &Context,
    _: &Uuid,
    enum_type_name: &str,
    enum_type: &EnumerationFrozen,
) -> FunctionImplementation {
    let writer_name = "writer".to_string();
    let value_name = "value".to_string();
    let writer = writer_name.to_expression();
    let value = value_name.to_expression();
    let enum_type_enum = enum_type_name.to_expression().colon_colon("Variant");
    let mut switch_cases = Vec::<(Expression, Block)>::new();
    for (_, variant) in &enum_type.variants {
        let case_statements: Vec<Declaration> = vec![
            Declaration::Statement(Statement::Expression(
                func::ARORA_BUFFER_WRITER_ADD_ENUMERATION_VALUE.call([
                    writer.clone(),
                    id::type_uuid(enum_type_name).to_expression(),
                    id::value_uuid(enum_type_name, &variant.name).to_expression(),
                ]),
            )),
            Declaration::Statement(Statement::Expression(
                func::ARORA_BUFFER_WRITER_ADD_UNIT.call([writer.clone()]),
            )),
            Declaration::Statement(Statement::Break),
        ];
        switch_cases.push((
            enum_type_enum.colon_colon(variant.name.to_expression()),
            Block {
                statements: case_statements,
                semicolon: false,
            },
        ));
    }

    let function_statements: Vec<Declaration> = vec![Statement::Switch(
        value.dot("get_variant").call::<String, _>(vec![]),
        switch_cases,
    )
    .into()];

    FunctionImplementation {
        name: "arora::buffer::serialize".to_string(),
        ret: Some(ty::VOID.clone()),
        parameters: vec![
            Parameter {
                name: writer_name,
                type_ref: ty::ARORA_BUFFER_WRITER_PTR.clone(),
            },
            Parameter {
                name: value_name,
                type_ref: TypeRef {
                    ty: enum_type_name.to_string(),
                    reference: true,
                    constant: true,
                    ..Default::default()
                },
            },
        ],
        noexcept: true,
        body: Block {
            statements: function_statements,
            semicolon: false,
        },
        template_arguments: Some(vec![]),
        specialization: Some(vec![enum_type_name.to_string()]),
        inline: true,
        ..Default::default()
    }
}

pub fn serializer(
    context: &Context,
    type_id: &Uuid,
    ty: &TypeDefinitionFrozen,
) -> FunctionImplementation {
    match ty {
        TypeDefinitionFrozen::Structure(ref structure) => {
            structure_serializer(context, &ty.name(), structure)
        }
        TypeDefinitionFrozen::Enumeration(ref enumeration) => {
            enumeration_serializer(context, type_id, &ty.name(), enumeration)
        }
        _ => panic!("deserializer: not implemented for {:?}", ty),
    }
}

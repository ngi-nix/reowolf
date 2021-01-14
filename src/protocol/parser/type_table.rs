// TODO: @fix PrimitiveType for enums/unions
use crate::protocol::ast::*;
use crate::protocol::parser::symbol_table::*;

use std::collections::HashMap;


enum DefinedType {
    Enum(EnumType),
    Union(UnionType),
    Struct(StructType),
    Function(FunctionType),
    Component(ComponentType)
}

// TODO: Also support maximum u64 value
struct EnumVariant {
    identifier: Identifier,
    value: i64,
}

/// `EnumType` is the classical C/C++ enum type. It has various variants with
/// an assigned integer value. The integer values may be user-defined,
/// compiler-defined, or a mix of the two. If a user assigns the same enum
/// value multiple times, we assume the user is an expert and we consider both
/// variants to be equal to one another.
struct EnumType {
    definition: DefinitionId,
    variants: Vec<EnumVariant>,
    representation: PrimitiveType,
}

struct UnionVariant {
    identifier: Identifier,
    embedded_type: Option<DefinitionId>,
    tag_value: i64,
}

struct UnionType {
    definition: DefinitionId,
    variants: Vec<EnumVariant>,
    tag_representation: PrimitiveType
}

struct StructMemberType {
    identifier: Identifier,

}

struct StructType {

}

struct FunctionType {

}

struct ComponentType {

}

struct TypeTable {
    lookup: HashMap<DefinitionId, DefinedType>,
}
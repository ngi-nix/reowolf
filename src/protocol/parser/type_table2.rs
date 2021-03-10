/**
TypeTable

Contains the type table: a datastructure that, when compilation succeeds,
contains a concrete type definition for each AST type definition. In general
terms the type table will go through the following phases during the compilation
process:

    1. The base type definitions are resolved after the parser phase has
        finished. This implies that the AST is fully constructed, but not yet
        annotated.
    2. With the base type definitions resolved, the validation/linker phase will
        use the type table (together with the symbol table) to disambiguate
        terms (e.g. does an expression refer to a variable, an enum, a constant,
        etc.)
    3. During the type checking/inference phase the type table is used to ensure
        that the AST contains valid use of types in expressions and statements.
        At the same time type inference will find concrete instantiations of
        polymorphic types, these will be stored in the type table as monomorphed
        instantiations of a generic type.
    4. After type checking and inference (and possibly when constructing byte
        code) the type table will construct a type graph and solidify each
        non-polymorphic type and monomorphed instantiations of polymorphic types
        into concrete types.

So a base type is defined by its (optionally polymorphic) representation in the
AST. A concrete type replaces all polymorphic arguments with inferred types. A
struct, enum or union may have polymorphic arguments but not actually be a
polymorphic type. This happens when the polymorphic arguments are not used in
the type definition itself.

NOTE: for now a polymorphic definition of a function/component is illegal if the
    polymorphic arguments are not used in the arguments/return type. It should
    be legal, but we disallow it for now.

TODO: Allow potentially cyclic datatypes and reject truly cyclic datatypes.
TODO: Allow for the full potential of polymorphism
TODO: Detect "true" polymorphism: for datatypes like structs/enum/unions this
    is simple. For functions we need to check the entire body. Do it here? Or
    do it somewhere else?
TODO: Do we want to check fn argument collision here, or in validation phase?
*/

use std::fmt::{Formatter, Result as FmtResult};
use std::collections::HashMap;

use crate::protocol::ast::*;
use crate::protocol::parser::symbol_table::{SymbolTable, Symbol};
use crate::protocol::inputsource::*;
use crate::protocol::parser::*;
use crate::protocol::parser::type_table2::ResolveResult::Resolved;

//------------------------------------------------------------------------------
// Defined Types
//------------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum TypeClass {
    Enum,
    Union,
    Struct,
    Function,
    Component
}

impl TypeClass {
    pub(crate) fn display_name(&self) -> &'static str {
        match self {
            TypeClass::Enum => "enum",
            TypeClass::Union => "enum",
            TypeClass::Struct => "struct",
            TypeClass::Function => "function",
            TypeClass::Component => "component",
        }
    }
}

impl std::fmt::Display for TypeClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.display_name())
    }
}

/// Struct wrapping around a potentially polymorphic type. If the type does not
/// have any polymorphic arguments then it will not have any monomorphs and
/// `is_polymorph` will be set to `false`. A type with polymorphic arguments
/// only has `is_polymorph` set to `true` if the polymorphic arguments actually
/// appear in the types associated types (function return argument, struct
/// field, enum variant, etc.). Otherwise the polymorphic argument is just a
/// marker and does not influence the bytesize of the type.
pub struct DefinedType {
    ast_definition: DefinitionId,
    definition: DefinedTypeVariant,
    poly_args: Vec<PolyArg>,
    is_polymorph: bool,
    is_pointerlike: bool,
    monomorphs: Vec<u32>, // TODO: ?
}

pub enum DefinedTypeVariant {
    Enum(EnumType),
    Union(UnionType),
    Struct(StructType),
    Function(FunctionType),
    Component(ComponentType)
}

pub struct PolyArg {
    identifier: Identifier,
    is_phantom: bool,
}

impl DefinedTypeVariant {
    pub(crate) fn type_class(&self) -> TypeClass {
        match self {
            DefinedTypeVariant::Enum(_) => TypeClass::Enum,
            DefinedTypeVariant::Union(_) => TypeClass::Union,
            DefinedTypeVariant::Struct(_) => TypeClass::Struct,
            DefinedTypeVariant::Function(_) => TypeClass::Function,
            DefinedTypeVariant::Component(_) => TypeClass::Component
        }
    }
}

/// `EnumType` is the classical C/C++ enum type. It has various variants with
/// an assigned integer value. The integer values may be user-defined,
/// compiler-defined, or a mix of the two. If a user assigns the same enum
/// value multiple times, we assume the user is an expert and we consider both
/// variants to be equal to one another.
pub struct EnumType {
    variants: Vec<EnumVariant>,
    representation: PrimitiveType,
}

// TODO: Also support maximum u64 value
pub struct EnumVariant {
    identifier: Identifier,
    value: i64,
}

/// `UnionType` is the algebraic datatype (or sum type, or discriminated union).
/// A value is an element of the union, identified by its tag, and may contain
/// a single subtype.
pub struct UnionType {
    variants: Vec<UnionVariant>,
    tag_representation: PrimitiveType
}

pub struct UnionVariant {
    identifier: Identifier,
    parser_type: Option<ParserTypeId>,
    tag_value: i64,
}

pub struct StructType {
    fields: Vec<StructField>,
}

pub struct StructField {
    identifier: Identifier,
    parser_type: ParserTypeId,
}

pub struct FunctionType {
    return_type: ParserTypeId,
    arguments: Vec<FunctionArgument>
}

pub struct ComponentType {
    variant: ComponentVariant,
    arguments: Vec<FunctionArgument>
}

pub struct FunctionArgument {
    identifier: Identifier,
    parser_type: ParserTypeId,
}

//------------------------------------------------------------------------------
// Type table
//------------------------------------------------------------------------------

// TODO: @cleanup Do I really need this, doesn't make the code that much cleaner
struct TypeIterator {
    breadcrumbs: Vec<(RootId, DefinitionId)>
}

impl TypeIterator {
    fn new() -> Self {
        Self{ breadcrumbs: Vec::with_capacity(32) }
    }

    fn reset(&mut self, root_id: RootId, definition_id: DefinitionId) {
        self.breadcrumbs.clear();
        self.breadcrumbs.push((root_id, definition_id))
    }

    fn push(&mut self, root_id: RootId, definition_id: DefinitionId) {
        self.breadcrumbs.push((root_id, definition_id));
    }

    fn contains(&self, root_id: RootId, definition_id: DefinitionId) -> bool {
        for (stored_root_id, stored_definition_id) in self.breadcrumbs.iter() {
            if stored_root_id == root_id && stored_definition_id == definition_id { return true; }
        }

        return false
    }

    fn top(&self) -> Option<(RootId, DefinitionId)> {
        self.breadcrumbs.last().map(|(r, d)| (*r, *d))
    }

    fn pop(&mut self) {
        debug_assert!(!self.breadcrumbs.is_empty());
        self.breadcrumbs.pop();
    }
}

#[derive(PartialEq, Eq)]
enum SpecifiedTypeVariant {
    // No subtypes
    Message,
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // Always one subtype
    ArrayOf,
    InputOf,
    OutputOf,
    // Variable number of subtypes, depending on the polymorphic arguments on
    // the definition
    InstanceOf(DefinitionId, usize)
}

#[derive(Eq)]
struct SpecifiedType {
    /// Definition ID, may not be enough as the type may be polymorphic
    definition: DefinitionId,
    /// The polymorphic types for the definition. These are encoded in a list,
    /// which we interpret as the depth-first serialization of the type tree.
    poly_vars: Vec<SpecifiedTypeVariant>
}

impl PartialEq for SpecifiedType {
    fn eq(&self, other: &Self) -> bool {
        // Should point to same definition and have the same polyvars
        if self.definition.index != other.definition.index { return false; }
        if self.poly_vars.len() != other.poly_vars.len() { return false; }
        for (my_var, other_var) in self.poly_vars.iter().zip(other.poly_vars.iter()) {
            if my_var != other_var { return false; }
        }

        return true
    }
}

impl SpecifiedType {
    fn new_non_polymorph(definition: DefinitionId) -> Self {
        Self{ definition, poly_vars: Vec::new() }
    }

    fn new_polymorph(definition: DefinitionId, heap: &Heap, parser_type_id: ParserTypeId) -> Self {
        // Serialize into concrete types
        let mut poly_vars = Vec::new();
        Self::construct_poly_vars(&mut poly_vars, heap, parser_type_id);
        Self{ definition, poly_vars }
    }

    fn construct_poly_vars(poly_vars: &mut Vec<SpecifiedTypeVariant>, heap: &Heap, parser_type_id: ParserTypeId) {
        // Depth-first construction of poly vars
        let parser_type = &heap[parser_type_id];
        match &parser_type.variant {
            ParserTypeVariant::Message => { poly_vars.push(SpecifiedTypeVariant::Message); },
            ParserTypeVariant::Bool => { poly_vars.push(SpecifiedTypeVariant::Bool); },
            ParserTypeVariant::Byte => { poly_vars.push(SpecifiedTypeVariant::Byte); },
            ParserTypeVariant::Short => { poly_vars.push(SpecifiedTypeVariant::Short); },
            ParserTypeVariant::Int => { poly_vars.push(SpecifiedTypeVariant::Int); },
            ParserTypeVariant::Long => { poly_vars.push(SpecifiedTypeVariant::Long); },
            ParserTypeVariant::String => { poly_vars.push(SpecifiedTypeVariant::String); },
            ParserTypeVariant::Array(subtype_id) => {
                poly_vars.push(SpecifiedTypeVariant::ArrayOf);
                Self::construct_poly_vars(poly_vars, heap, *subtype_id);
            },
            ParserTypeVariant::Input(subtype_id) => {
                poly_vars.push(SpecifiedTypeVariant::InputOf);
                Self::construct_poly_vars(poly_vars, heap, *subtype_id);
            },
            ParserTypeVariant::Output(subtype_id) => {
                poly_vars.push(SpecifiedTypeVariant::OutputOf);
                Self::construct_poly_vars(poly_vars, heap, *subtype_id);
            },
            ParserTypeVariant::Symbolic(symbolic) => {
                let definition_id = match symbolic.variant {
                    SymbolicParserTypeVariant::Definition(definition_id) => definition_id,
                    SymbolicParserTypeVariant::PolyArg(_) => {
                        // When construct entries in the type table, we no longer allow the
                        // unspecified types in the AST, we expect them to be fully inferred.
                        debug_assert!(false, "Encountered 'PolyArg' symbolic type. Expected fully inferred types");
                        unreachable!();
                    }
                };

                poly_vars.push(SpecifiedTypeVariant::InstanceOf(definition_id, symbolic.poly_args.len()));
                for subtype_id in &symbolic.poly_args {
                    Self::construct_poly_vars(poly_vars, heap, *subtype_id);
                }
            },
            ParserTypeVariant::IntegerLiteral => {
                debug_assert!(false, "Encountered 'IntegerLiteral' symbolic type. Expected fully inferred types");
                unreachable!();
            },
            ParserTypeVariant::Inferred => {
                debug_assert!(false, "Encountered 'Inferred' symbolic type. Expected fully inferred types");
                unreachable!();
            }
        }
    }
}

enum ResolveResult {
    BuiltIn,
    PolyArg,
    Resolved((RootId, DefinitionId)),
    Unresolved((RootId, DefinitionId))
}

pub(crate) struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Iterator over (module, definition) tuples used as workspace to make sure
    /// that each base definition of all a type's subtypes are resolved.
    iter: TypeIterator,
}

pub(crate) struct TypeCtx<'a> {
    symbols: &'a SymbolTable,
    heap: &'a Heap,
    modules: &'a [LexedModule]
}

impl TypeTable {
    /// Construct a new type table without any resolved types. Types will be
    /// resolved on-demand.
    pub(crate) fn new(ctx: &TypeCtx) -> Self {
        // Use context to guess hashmap size
        let reserve_size = ctx.heap.definitions.len();
        Self{
            lookup: HashMap::with_capacity(reserve_size),
            iter: TypeIterator::new(),
        }
    }

    pub(crate) fn resolve_type(&mut self, ctx: &TypeCtx, specified_type: &SpecifiedType) {
        // Make sure we're allowed to cast root_id to index into ctx.modules
        if cfg!(debug_assertions) {
            for (index, module) in ctx.modules.iter().enumerate() {
                debug_assert_eq!(index, module.root_id.index as usize);
            }
        }

        // Check if we already have the definition
        let definition = self.get_definition(definition_id);

    }

    /// This function will resolve just the basic definition of the type, it
    /// will not handle any of the monomorphized instances of the type.
    fn resolve_base_definition(&mut self, ctx: &TypeCtx, definition_id: DefinitionId) -> &DefinedType {
        // Check if we have already resolved the base definition
        if let Some(definition) = self.lookup.get(&definition_id) {
            return definition;
        }

        let root_id = Self::find_root_id(ctx, definition_id);
        self.iter.reset(root_id, definition_id);

        while let Some((root_id, definition_id)) = self.iter.top() {
            // We have a type to resolve
            let definition = &ctx.heap[definition_id];

            let can_pop_breadcrumb = match definition {
                Definition::Enum(definition) => self.resolve_base_enum_definition(ctx, root_id, definition),
                Definition::Struct(definition) => self.resolve_base_struct_definition(ctx, root_id, definition),
                Definition::Component(definition) => self.resolve_base_component_definition(ctx, root_id, definition),
                Definition::Function(definition) => self.resolve_base_function_definition(ctx, root_id, definition),
            }?;

            // Otherwise: `ingest_resolve_result` has pushed a new breadcrumb
            // that we must follow before we can resolve the current type
            if can_pop_breadcrumb {
                self.iter.pop();
            }
        }

        // We must have resolved the type
        debug_assert!(self.lookup.contains_key(&definition_id), "base type not resolved");
        self.lookup.get(&definition_id).unwrap()
    }

    /// Resolve the basic enum definition to an entry in the type table. It will
    /// not instantiate any monomorphized instances of polymorphic enum
    /// definitions. If a subtype has to be resolved first then this function
    /// will return `false` after calling `ingest_resolve_result`.
    fn resolve_base_enum_definition(&mut self, ctx: &TypeCtx, root_id: RootId, definition: &EnumDefinition) -> Result<bool, ParseError2> {
        debug_assert!(!self.lookup.contains_key(&definition.this.upcast()), "base enum already resolved");

        // Check if the enum should be implemented as a classic enumeration or
        // a tagged union. Keep track of variant index for error messages. Make
        // sure all embedded types are resolved.
        let mut first_tag_value = None;
        let mut first_int_value = None;
        for variant in &definition.variants {
            match &variant.value {
                EnumVariantValue::None => {},
                EnumVariantValue::Integer(_) => if first_int_value.is_none() {
                    first_int_value = Some(variant.position);
                },
                EnumVariantValue::Type(variant_type_id) => {
                    if first_tag_value.is_none() {
                        first_tag_value = Some(variant.position);
                    }

                    // Check if the embedded type needs to be resolved
                    let resolve_result = self.resolve_base_parser_type(ctx, &definition.poly_vars, root_id, *variant_type_id)?;
                    if !self.ingest_resolve_result(ctx, resolve_result) {
                        return Ok(false)
                    }
                }
            }
        }

        if first_tag_value.is_some() && first_int_value.is_some() {
            // Not illegal, but useless and probably a programmer mistake
            let module_source = &ctx.modules[root_id.index as usize].source;
            let tag_pos = first_tag_value.unwrap();
            let int_pos = first_int_value.unwrap();
            return Err(
                ParseError2::new_error(
                    module_source, definition.position,
                    "Illegal combination of enum integer variant(s) and enum union variant(s)"
                )
                    .with_postfixed_info(module_source, int_pos, "Assigning an integer value here")
                    .with_postfixed_info(module_source, tag_pos, "Embedding a type in a union variant here")
            );
        }

        // Enumeration is legal
        if first_tag_value.is_some() {
            // Implement as a tagged union

            // Determine the union variants
            let mut tag_value = -1;
            let mut variants = Vec::with_capacity(definition.variants.len());
            for variant in &definition.variants {
                tag_value += 1;
                let parser_type = match &variant.value {
                    EnumVariantValue::None => {
                        None
                    },
                    EnumVariantValue::Type(parser_type_id) => {
                        // Type should be resolvable, we checked this above
                        Some(*parser_type_id)
                    },
                    EnumVariantValue::Integer(_) => {
                        debug_assert!(false, "Encountered `Integer` variant after asserting enum is a discriminated union");
                        unreachable!();
                    }
                };

                variants.push(UnionVariant{
                    identifier: variant.identifier.clone(),
                    parser_type,
                    tag_value,
                })
            }

            // Ensure union names and polymorphic args do not conflict
            self.check_identifier_collision(
                ctx, root_id, &variants, |variant| &variant.identifier, "enum variant"
            )?;
            self.check_poly_args_collision(ctx, root_id, &definition.poly_args)?;

            // Insert base definition in type table
            let definition_id = definition.this.upcast();
            self.lookup.insert(definition_id, DefinedType {
                ast_definition: definition_id,
                definition: DefinedTypeVariant::Union(UnionType{
                    variants,
                    tag_representation: Self::enum_tag_type(-1, tag_value),
                }),
                poly_args: Vec::new(),
                is_polymorph: self.poly_args_in_use.iter().any(|v| *v),
                is_pointerlike: false, // TODO: @cyclic_types
                monomorphs: Vec::new()
            });
        } else {
            // Implement as a regular enum
            debug_assert!(self.poly_args_in_use.iter().all(|v| !*v));
            let mut enum_value = -1;
            let mut min_enum_value = 0;
            let mut max_enum_value = 0;
            let mut variants = Vec::with_capacity(definition.variants.len());
            for variant in &definition.variants {
                enum_value += 1;
                match &variant.value {
                    EnumVariantValue::None => {
                        variants.push(EnumVariant{
                            identifier: variant.identifier.clone(),
                            value: enum_value,
                        });
                    },
                    EnumVariantValue::Integer(override_value) => {
                        enum_value = *override_value;
                        variants.push(EnumVariant{
                            identifier: variant.identifier.clone(),
                            value: enum_value,
                        });
                    },
                    EnumVariantValue::Type(_) => {
                        debug_assert!(false, "Encountered `Type` variant after asserting enum is not a discriminated union");
                        unreachable!();
                    }
                }
                if enum_value < min_enum_value { min_enum_value = enum_value; }
                else if enum_value > max_enum_value { max_enum_value = enum_value; }
            }

            // Ensure enum names and polymorphic args do not conflict
            self.check_identifier_collision(
                ctx, root_id, &variants, |variant| &variant.identifier, "enum variant"
            )?;
            self.check_poly_args_collision(ctx, root_id, &definition.poly_vars)?;

            let definition_id = definition.this.upcast();
            self.lookup.insert(definition_id, DefinedType {
                ast_definition: definition_id,
                definition: DefinedTypeVariant::Enum(EnumType{
                    variants,
                    representation: Self::enum_tag_type(min_enum_value, max_enum_value)
                }),
                poly_args: Vec::new(),
                is_polymorph: false,
                is_pointerlike: false,
                monomorphs: Vec::new()
            });
        }

        Ok(true)
    }

    /// Resolves the basic struct definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic struct
    /// definitions.
    fn resolve_base_struct_definition(&mut self, ctx: &TypeCtx, root_id: RootId, definition: &StructDefinition) -> Result<bool, ParseError2> {
        debug_assert!(!self.lookup.contains_key(&definition.this.upcast()), "base struct already resolved");

        // Make sure all fields point to resolvable types
        for field_definition in &definition.fields {
            let resolve_result = self.resolve_base_parser_type(ctx, &definition.poly_vars, root_id, field_definition.parser_type)?;
            if !self.ingest_resolve_result(ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // All fields types are resolved, construct base type
        let mut fields = Vec::with_capacity(definition.fields.len());
        for field_definition in &definition.fields {
            fields.push(StructField{
                identifier: field_definition.field.clone(),
                parser_type: field_definition.parser_type,
            })
        }

        // And make sure no conflicts exist in field names and/or polymorphic args
        self.check_identifier_collision(
            ctx, root_id, &fields, |field| &field.identifier, "struct field"
        )?;
        self.check_poly_args_collision(ctx, root_id, &definition.poly_vars)?;

        let definition_id = definition.this.upcast();
        self.lookup.insert(definition_id, DefinedType{
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Struct(StructType{
                fields,
            }),
            poly_args: Vec::new(),
            is_polymorph: false, // TODO: @polymorphism
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
        });

        Ok(true)
    }

    /// Resolves the basic function definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic function
    /// definitions.
    fn resolve_base_function_definition(&mut self, ctx: &TypeCtx, root_id: RootId, definition: &Function) -> Result<bool, ParseError2> {
        debug_assert!(!self.lookup.contains_key(&definition.this.upcast()), "base function already resolved");

        // Check the return type
        let resolve_result = self.resolve_base_parser_type(
            ctx, &definition.poly_vars, root_id, definition.return_type
        )?;
        if !self.ingest_resolve_result(ctx, resolve_result)? {
            return Ok(false)
        }

        // Check the argument types
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            let resolve_result = self.resolve_base_parser_type(
                ctx, &definition.poly_vars, root_id, param.parser_type
            )?;
            if !self.ingest_resolve_result(ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // Construct arguments to function
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            arguments.push(FunctionArgument{
                identifier: param.identifier.clone(),
                parser_type: param.parser_type,
            })
        }

        // Check conflict of argument and polyarg identifiers
        self.check_identifier_collision(
            ctx, root_id, &arguments, |arg| &arg.identifier, "function argument"
        )?;
        self.check_poly_args_collision(ctx, root_id, &definition.poly_vars)?;

        // Construct entry in type table
        let definition_id = definition.this.upcast();
        self.lookup.insert(definition_id, DefinedType{
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Function(FunctionType{
                return_type: definition.return_type,
                arguments,
            }),
            poly_args: Vec::new(),
            is_polymorph: false, // TODO: @polymorphism
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
        });

        Ok(true)
    }

    /// Resolves the basic component definition to an entry in the type table.
    /// It will not instantiate any monomorphized instancees of polymorphic
    /// component definitions.
    fn resolve_base_component_definition(&mut self, ctx: &TypeCtx, root_id: RootId, definition: &Component) -> Result<bool, ParseError2> {
        debug_assert!(self.lookup.contains_key(&definition.this.upcast()), "base component already resolved");

        // Check argument types
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            let resolve_result = self.resolve_base_parser_type(
                ctx, &definition.poly_vars, root_id, param.parser_type
            )?;
            if !self.ingest_resolve_result(ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // Construct argument types
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            arguments.push(FunctionArgument{
                identifier: param.identifier.clone(),
                parser_type: param.parser_type
            })
        }

        // Check conflict of argument and polyarg identifiers
        self.check_identifier_collision(
            ctx, root_id, &arguments, |arg| &arg.identifier, "component argument"
        )?;
        self.check_poly_args_collision(ctx, root_id, &definition.poly_vars)?;

        // Construct entry in type table
        let definition_id = definition.this.upcast();
        self.lookup.insert(definition_id, DefinedType{
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Component(ComponentType{
                variant: definition.variant,
                arguments,
            }),
            poly_args: Vec::new(),
            is_polymorph: false, // TODO: @polymorphism,
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
        });

        Ok(true)
    }

    /// Takes a ResolveResult and returns `true` if the caller can happily
    /// continue resolving its current type, or `false` if the caller must break
    /// resolving the current type and exit to the outer resolving loop. In the
    /// latter case the `result` value was `ResolveResult::Unresolved`, implying
    /// that the type must be resolved first.
    fn ingest_resolve_result(&mut self, ctx: &TypeCtx, result: ResolveResult) -> Result<bool, ParseError2> {
        match result {
            ResolveResult::BuiltIn | ResolveResult::PolyArg => Ok(true),
            ResolveResult::Resolved(_) => Ok(true),
            ResolveResult::Unresolved((root_id, definition_id)) => {
                if self.iter.contains(root_id, definition_id) {
                    // Cyclic dependency encountered
                    // TODO: Allow this
                    let mut error = ParseError2::new_error(
                        &ctx.modules[root_id.index as usize].source, ctx.heap[definition_id].position(),
                        "Evaluating this type definition results in a cyclic type"
                    );

                    for (breadcrumb_idx, (root_id, definition_id)) in self.iter.breadcrumbs.iter().enumerate() {
                        let msg = if breadcrumb_idx == 0 {
                            "The cycle started with this definition"
                        } else {
                            "Which depends on this definition"
                        };

                        error = error.with_postfixed_info(
                            &ctx.modules[root_id.index as usize].source,
                            ctx.heap[*definition_id].position(), msg
                        );
                    }

                    Err(error)
                } else {
                    // Type is not yet resolved, so push IDs on iterator and
                    // continue the resolving loop
                    self.iter.push(root_id, definition_id);
                    Ok(false)
                }
            }
        }
    }

    /// Each type definition may consist of several subtypes (the type
    /// associated with a union variant, or the type associated with a struct
    /// field). This function goes through the subtype tree and determines if
    /// each reference type is resolved or not (yet).
    fn resolve_base_parser_type(&mut self, ctx: &TypeCtx, poly_vars: &Vec<Identifier>, root_id: RootId, mut parser_type_id: ParserTypeId) -> Result<ResolveResult, ParseError2> {
        use ParserTypeVariant as PTV;

        loop {
            let parser_type = &ctx.heap[parser_type_id];
            match &parser_type.variant {
                // Builtin types. An array is a builtin as it is implemented as a
                // couple of pointers, so we do not require the subtype to be fully
                // resolved.
                PTV::Array(_) |
                PTV::Message |
                PTV::Bool |
                PTV::Byte |
                PTV::Short |
                PTV::Int |
                PTV::Long |
                PTV::String => {
                    return Ok(ResolveResult::BuiltIn);
                },
                // IntegerLiteral types and the inferred marker are not allowed in
                // definitions of types
                PTV::IntegerLiteral |
                PTV::Inferred => {
                    debug_assert!(false, "Encountered illegal ParserTypeVariant within type definition");
                    unreachable!();
                },
                // Input/Output port
                // TODO: @unclear Come back to this, do these need to be fully
                //  resolved? Or do we implement these as the port identifiers?
                PTV::Input(subtype_id) |
                PTV::Output(subtype_id) => {
                    parser_type_id = *subtype_id;
                },
                PTV::Symbolic(symbolic) => {
                    // Check if the symbolic type is one of the definition's
                    // polymorphic arguments. If so then we can halt the
                    // execution
                    for poly_arg in poly_vars.iter() {
                        if poly_arg.value == symbolic.identifier.value {
                            return Ok(ResolveResult::PolyArg)
                        }
                    }

                    // Lookup the definition in the symbol table
                    let symbol = ctx.symbols.resolve_namespaced_symbol(root_id, &symbolic.identifier);
                    if symbol.is_none() {
                        return Err(ParseError2::new_error(
                            &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                            "Could not resolve type"
                        ))
                    }

                    let (symbol_value, mut ident_iter) = symbol.unwrap();
                    match symbol_value.symbol {
                        Symbol::Namespace(_) => {
                            // Reference to a namespace instead of a type
                            return if ident_iter.num_remaining() == 0 {
                                Err(ParseError2::new_error(
                                    &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                                    "Expected a type, got a module name"
                                ))
                            } else {
                                let next_identifier = ident_iter.next().unwrap();
                                Err(ParseError2::new_error(
                                    &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                                    &format!("Could not find symbol '{}' with this module", String::from_utf8_lossy(next_identifier))
                                ))
                            }
                        },
                        Symbol::Definition((root_id, definition_id)) => {
                            let definition = &ctx.heap[definition_id];
                            if ident_iter.num_remaining > 0 {
                                // Namespaced identifier is longer than the type
                                // we found. Return the appropriate message
                                return if definition.is_struct() || definition.is_enum() {
                                    Err(ParseError2::new_error(
                                        &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                                        &format!(
                                            "Unknown type '{}', did you mean to use '{}'?",
                                            String::from_utf8_lossy(&symbolic.identifier.value),
                                            String::from_utf8_lossy(&definition.identifier().value)
                                        )
                                    ))
                                } else {
                                    Err(ParseError2::new_error(
                                        &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                                        "Unknown type"
                                    ))
                                }
                            }

                            // Found a match, make sure it is a datatype
                            if !(definition.is_struct() || definition.is_enum()) {
                                return Err(ParseError2::new_error(
                                    &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                                    "Embedded types must be datatypes (structs or enums)"
                                ))
                            }

                            // Found a struct/enum definition
                            return if self.lookup.contains_key(&definition_id) {
                                Ok(ResolveResult::Resolved((root_id, definition_id)))
                            } else {
                                Ok(ResolveResult::Unresolved((root_id, definition_id)))
                            }
                        }
                    }
                }
            }
        }
    }

    /// Go through a list of identifiers and ensure that all identifiers have
    /// unique names
    fn check_identifier_collision<T: Sized, F: Fn(&T) -> &Identifier>(
        &self, ctx: &TypeCtx, root_id: RootId, items: &[T], getter: F, item_name: &'static str
    ) -> Result<(), ParseError2> {
        for (item_idx, item) in items.iter().enumerate() {
            let item_ident = getter(item);
            for other_item in &items[0..item_idx] {
                let other_item_ident = getter(other_item);
                if item_ident.value == other_item_ident {
                    let module_source = &ctx.modules[root_id.index as usize].source;
                    return Err(ParseError2::new_error(
                        module_source, item_ident.position, &format!("This {} is defined more than once", item_name)
                    ).with_postfixed_info(
                        module_source, other_item_ident.position, &format!("The other {} is defined here", item_name)
                    ));
                }
            }
        }

        Ok(())
    }

    /// Go through a list of polymorphic arguments and make sure that the
    /// arguments all have unique names, and the arguments do not conflict with
    /// any symbols defined at the module scope.
    fn check_poly_args_collision(
        &self, ctx: &TypeCtx, root_id: RootId, poly_args: &[Identifier]
    ) -> Result<(), ParseError2> {
        // Make sure polymorphic arguments are unique and none of the
        // identifiers conflict with any imported scopes
        for (arg_idx, poly_arg) in poly_args.iter().enumerate() {
            for other_poly_arg in &poly_args[..arg_idx] {
                if poly_arg.value == other_poly_arg.value {
                    let module_source = &ctx.modules[root_id.index as usize].source;
                    return Err(ParseError2::new_error(
                        module_source, poly_arg.position,
                        "This polymorphic argument is defined more than once"
                    ).with_postfixed_info(
                        module_source, other_poly_arg.position,
                        "It conflicts with this polymorphic argument"
                    ));
                }
            }

            // Check if identifier conflicts with a symbol defined or imported
            // in the current module
            if let Some(symbol) = ctx.symbols.resolve_symbol(root_id, &poly_arg.value) {
                // We have a conflict
                let module_source = &ctx.modules[root_id.index as usize].source;
                return Err(ParseError2::new_error(
                    module_source, poly_arg.position,
                    "This polymorphic argument conflicts with another symbol"
                ).with_postfixed_info(
                    module_source, symbol.position,
                    "It conflicts due to this symbol"
                ));
            }
        }

        // All arguments are fine
        Ok(())
    }

    //--------------------------------------------------------------------------
    // Small utilities
    //--------------------------------------------------------------------------

    fn enum_tag_type(min_tag_value: i64, max_tag_value: i64) -> PrimitiveType {
        // TODO: @consistency tag values should be handled correctly
        debug_assert!(min_tag_value < max_tag_value);
        let abs_max_value = min_tag_value.abs().max(max_tag_value.abs());
        if abs_max_value <= u8::max_value() as i64 {
            PrimitiveType::Byte
        } else if abs_max_value <= u16::max_value() as i64 {
            PrimitiveType::Short
        } else if abs_max_value <= u32::max_value() as i64 {
            PrimitiveType::Int
        } else {
            PrimitiveType::Long
        }
    }

    fn find_root_id(ctx: &TypeCtx, definition_id: DefinitionId) -> RootId {
        // TODO: Keep in lookup or something
        for module in ctx.modules {
            let root_id = module.root_id;
            let root = &ctx.heap[root_id];
            for module_definition_id in root.definitions.iter() {
                if module_definition_id == definition_id {
                    return root_id
                }
            }
        }

        debug_assert!(false, "DefinitionId without corresponding RootId");
        unreachable!();
    }
}
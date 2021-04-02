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
AST. A concrete type has concrete types for each of the polymorphic arguments. A
struct, enum or union may have polymorphic arguments but not actually be a
polymorphic type. This happens when the polymorphic arguments are not used in
the type definition itself. Similarly for functions/components: but here we just
check the arguments/return type of the signature.

Apart from base types and concrete types, we also use the term "embedded type"
for types that are embedded within another type, such as a type of a struct
struct field or of a union variant. Embedded types may themselves have
polymorphic arguments and therefore form an embedded type tree.

NOTE: for now a polymorphic definition of a function/component is illegal if the
    polymorphic arguments are not used in the arguments/return type. It should
    be legal, but we disallow it for now.

TODO: Allow potentially cyclic datatypes and reject truly cyclic datatypes.
TODO: Allow for the full potential of polymorphism
TODO: Detect "true" polymorphism: for datatypes like structs/enum/unions this
    is simple. For functions we need to check the entire body. Do it here? Or
    do it somewhere else?
TODO: Do we want to check fn argument collision here, or in validation phase?
TODO: Make type table an on-demand thing instead of constructing all base types.
TODO: Cleanup everything, feels like a lot can be written cleaner and with less
    assumptions on each function call.
// TODO: Review all comments
*/

use std::fmt::{Formatter, Result as FmtResult};
use std::collections::{HashMap, VecDeque};

use crate::protocol::ast::*;
use crate::protocol::parser::symbol_table::{SymbolTable, Symbol};
use crate::protocol::inputsource::*;
use crate::protocol::parser::*;

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

    pub(crate) fn is_data_type(&self) -> bool {
        *self == TypeClass::Enum || *self == TypeClass::Union || *self == TypeClass::Struct
    }

    pub(crate) fn is_proc_type(&self) -> bool {
        *self == TypeClass::Function || *self == TypeClass::Component
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
    pub(crate) ast_root: RootId,
    pub(crate) ast_definition: DefinitionId,
    pub(crate) definition: DefinedTypeVariant,
    pub(crate) poly_vars: Vec<PolyVar>,
    pub(crate) is_polymorph: bool,
    pub(crate) is_pointerlike: bool,
    // TODO: @optimize
    pub(crate) monomorphs: Vec<Vec<ConcreteType>>,
}

impl DefinedType {
    fn add_monomorph(&mut self, types: Vec<ConcreteType>) {
        debug_assert!(!self.has_monomorph(&types), "monomorph already exists");
        self.monomorphs.push(types);
    }

    pub(crate) fn has_any_monomorph(&self) -> bool {
        !self.monomorphs.is_empty()
    }

    pub(crate) fn has_monomorph(&self, types: &Vec<ConcreteType>) -> bool {
        debug_assert_eq!(self.poly_vars.len(), types.len(), "mismatch in number of polymorphic types");
        for monomorph in &self.monomorphs {
            if monomorph == types { return true; }
        }

        return false;
    }
}

pub enum DefinedTypeVariant {
    Enum(EnumType),
    Union(UnionType),
    Struct(StructType),
    Function(FunctionType),
    Component(ComponentType)
}

pub struct PolyVar {
    identifier: Identifier,
    /// Whether the polymorphic variables is used directly in the definition of
    /// the type (not including bodies of function/component types)
    is_in_use: bool,
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

    pub(crate) fn as_struct(&self) -> &StructType {
        match self {
            DefinedTypeVariant::Struct(v) => v,
            _ => unreachable!("Cannot convert {} to struct variant", self.type_class())
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
    pub(crate) fields: Vec<StructField>,
}

pub struct StructField {
    pub(crate) identifier: Identifier,
    pub(crate) parser_type: ParserTypeId,
}

pub struct FunctionType {
    pub return_type: ParserTypeId,
    pub arguments: Vec<FunctionArgument>
}

pub struct ComponentType {
    pub variant: ComponentVariant,
    pub arguments: Vec<FunctionArgument>
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
            if *stored_root_id == root_id && *stored_definition_id == definition_id { return true; }
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

/// Result from attempting to resolve a `ParserType` using the symbol table and
/// the type table.
enum ResolveResult {
    /// ParserType is a builtin type
    BuiltIn,
    /// ParserType points to a polymorphic argument, contains the index of the
    /// polymorphic argument in the outermost definition (e.g. we may have 
    /// structs nested three levels deep, but in the innermost struct we can 
    /// only use the polyargs that are specified in the type definition of the
    /// outermost struct).
    PolyArg(usize),
    /// ParserType points to a user-defined type that is already resolved in the
    /// type table.
    Resolved((RootId, DefinitionId)),
    /// ParserType points to a user-defined type that is not yet resolved into
    /// the type table.
    Unresolved((RootId, DefinitionId))
}

pub(crate) struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Iterator over `(module, definition)` tuples used as workspace to make sure
    /// that each base definition of all a type's subtypes are resolved.
    iter: TypeIterator,
    /// Iterator over `parser type`s during the process where `parser types` are
    /// resolved into a `(module, definition)` tuple.
    parser_type_iter: VecDeque<ParserTypeId>,
}

pub(crate) struct TypeCtx<'a> {
    symbols: &'a SymbolTable,
    heap: &'a mut Heap,
    modules: &'a [LexedModule]
}

impl<'a> TypeCtx<'a> {
    pub(crate) fn new(symbols: &'a SymbolTable, heap: &'a mut Heap, modules: &'a [LexedModule]) -> Self {
        Self{ symbols, heap, modules }
    }
}

impl TypeTable {
    /// Construct a new type table without any resolved types.
    pub(crate) fn new() -> Self {
        Self{ 
            lookup: HashMap::new(), 
            iter: TypeIterator::new(), 
            parser_type_iter: VecDeque::with_capacity(64), 
        }
    }

    pub(crate) fn build_base_types(&mut self, ctx: &mut TypeCtx) -> Result<(), ParseError2> {
        // Make sure we're allowed to cast root_id to index into ctx.modules
        debug_assert!(self.lookup.is_empty());
        debug_assert!(self.iter.top().is_none());
        debug_assert!(self.parser_type_iter.is_empty());

        if cfg!(debug_assertions) {
            for (index, module) in ctx.modules.iter().enumerate() {
                debug_assert_eq!(index, module.root_id.index as usize);
            }
        }

        // Use context to guess hashmap size
        let reserve_size = ctx.heap.definitions.len();
        self.lookup.reserve(reserve_size);

        // TODO: @cleanup Rework this hack
        for root_idx in 0..ctx.modules.len() {
            let last_definition_idx = ctx.heap[ctx.modules[root_idx].root_id].definitions.len();
            for definition_idx in 0..last_definition_idx {
                let definition_id = ctx.heap[ctx.modules[root_idx].root_id].definitions[definition_idx];
                self.resolve_base_definition(ctx, definition_id)?;
            }
        }

        debug_assert_eq!(self.lookup.len(), reserve_size, "mismatch in reserved size of type table");

        Ok(())
    }

    /// Retrieves base definition from type table. We must be able to retrieve
    /// it as we resolve all base types upon type table construction (for now).
    /// However, in the future we might do on-demand type resolving, so return
    /// an option anyway
    pub(crate) fn get_base_definition(&self, definition_id: &DefinitionId) -> Option<&DefinedType> {
        self.lookup.get(&definition_id)
    }

    /// Instantiates a monomorph for a given base definition.
    pub(crate) fn add_monomorph(&mut self, definition_id: &DefinitionId, types: Vec<ConcreteType>) {
        debug_assert!(
            self.lookup.contains_key(definition_id),
            "attempting to instantiate monomorph of definition unknown to type table"
        );

        let definition = self.lookup.get_mut(definition_id).unwrap();
        definition.add_monomorph(types);
    }

    /// Checks if a given definition already has a specific monomorph
    pub(crate) fn has_monomorph(&mut self, definition_id: &DefinitionId, types: &Vec<ConcreteType>) -> bool {
        debug_assert!(
            self.lookup.contains_key(definition_id),
            "attempting to check monomorph existence of definition unknown to type table"
        );

        let definition = self.lookup.get(definition_id).unwrap();
        definition.has_monomorph(types)
    }

    /// This function will resolve just the basic definition of the type, it
    /// will not handle any of the monomorphized instances of the type.
    fn resolve_base_definition<'a>(&'a mut self, ctx: &mut TypeCtx, definition_id: DefinitionId) -> Result<(), ParseError2> {
        // Check if we have already resolved the base definition
        if self.lookup.contains_key(&definition_id) { return Ok(()); }

        let root_id = Self::find_root_id(ctx, definition_id);
        self.iter.reset(root_id, definition_id);

        while let Some((root_id, definition_id)) = self.iter.top() {
            // We have a type to resolve
            let definition = &ctx.heap[definition_id];

            let can_pop_breadcrumb = match definition {
                // TODO: @cleanup Borrow rules hax
                Definition::Enum(_) => self.resolve_base_enum_definition(ctx, root_id, definition_id),
                Definition::Struct(_) => self.resolve_base_struct_definition(ctx, root_id, definition_id),
                Definition::Component(_) => self.resolve_base_component_definition(ctx, root_id, definition_id),
                Definition::Function(_) => self.resolve_base_function_definition(ctx, root_id, definition_id),
            }?;

            // Otherwise: `ingest_resolve_result` has pushed a new breadcrumb
            // that we must follow before we can resolve the current type
            if can_pop_breadcrumb {
                self.iter.pop();
            }
        }

        // We must have resolved the type
        debug_assert!(self.lookup.contains_key(&definition_id), "base type not resolved");
        Ok(())
    }

    /// Resolve the basic enum definition to an entry in the type table. It will
    /// not instantiate any monomorphized instances of polymorphic enum
    /// definitions. If a subtype has to be resolved first then this function
    /// will return `false` after calling `ingest_resolve_result`.
    fn resolve_base_enum_definition(&mut self, ctx: &mut TypeCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError2> {
        debug_assert!(ctx.heap[definition_id].is_enum());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base enum already resolved");
        
        let definition = ctx.heap[definition_id].as_enum();

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
                    if !self.ingest_resolve_result(ctx, resolve_result)? {
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
            self.check_poly_args_collision(ctx, root_id, &definition.poly_vars)?;

            let mut poly_args = self.create_initial_poly_vars(&definition.poly_vars);
            for variant in &variants {
                if let Some(embedded) = variant.parser_type {
                    self.check_and_resolve_embedded_type_and_modify_poly_args(ctx, definition_id, &mut poly_args, root_id, embedded)?;
                }
            }
            let is_polymorph = poly_args.iter().any(|arg| arg.is_in_use);

            // Insert base definition in type table
            self.lookup.insert(definition_id, DefinedType {
                ast_root: root_id,
                ast_definition: definition_id,
                definition: DefinedTypeVariant::Union(UnionType{
                    variants,
                    tag_representation: Self::enum_tag_type(-1, tag_value),
                }),
                poly_vars: poly_args,
                is_polymorph,
                is_pointerlike: false, // TODO: @cyclic_types
                monomorphs: Vec::new()
            });
        } else {
            // Implement as a regular enum
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

            // Note: although we cannot have embedded type dependent on the
            // polymorphic variables, they might still be present as tokens
            let definition_id = definition.this.upcast();
            self.lookup.insert(definition_id, DefinedType {
                ast_root: root_id,
                ast_definition: definition_id,
                definition: DefinedTypeVariant::Enum(EnumType{
                    variants,
                    representation: Self::enum_tag_type(min_enum_value, max_enum_value)
                }),
                poly_vars: self.create_initial_poly_vars(&definition.poly_vars),
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
    fn resolve_base_struct_definition(&mut self, ctx: &mut TypeCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError2> {
        debug_assert!(ctx.heap[definition_id].is_struct());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base struct already resolved");

        let definition = ctx.heap[definition_id].as_struct();

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

        // Construct representation of polymorphic arguments
        let mut poly_args = self.create_initial_poly_vars(&definition.poly_vars);
        for field in &fields {
            self.check_and_resolve_embedded_type_and_modify_poly_args(ctx, definition_id, &mut poly_args, root_id, field.parser_type)?;
        }

        let is_polymorph = poly_args.iter().any(|arg| arg.is_in_use);

        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Struct(StructType{
                fields,
            }),
            poly_vars: poly_args,
            is_polymorph,
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
        });

        Ok(true)
    }

    /// Resolves the basic function definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic function
    /// definitions.
    fn resolve_base_function_definition(&mut self, ctx: &mut TypeCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError2> {
        debug_assert!(ctx.heap[definition_id].is_function());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base function already resolved");

        let definition = ctx.heap[definition_id].as_function();
        let return_type = definition.return_type;

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

        // Construct polymorphic arguments
        let mut poly_args = self.create_initial_poly_vars(&definition.poly_vars);
        let return_type_id = definition.return_type;
        self.check_and_resolve_embedded_type_and_modify_poly_args(ctx, definition_id, &mut poly_args, root_id, return_type_id)?;
        for argument in &arguments {
            self.check_and_resolve_embedded_type_and_modify_poly_args(ctx, definition_id, &mut poly_args, root_id, argument.parser_type)?;
        }

        let is_polymorph = poly_args.iter().any(|arg| arg.is_in_use);

        // Construct entry in type table
        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Function(FunctionType{
                return_type,
                arguments,
            }),
            poly_vars: poly_args,
            is_polymorph,
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
        });

        Ok(true)
    }

    /// Resolves the basic component definition to an entry in the type table.
    /// It will not instantiate any monomorphized instancees of polymorphic
    /// component definitions.
    fn resolve_base_component_definition(&mut self, ctx: &mut TypeCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError2> {
        debug_assert!(ctx.heap[definition_id].is_component());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base component already resolved");

        let definition = ctx.heap[definition_id].as_component();
        let component_variant = definition.variant;

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

        // Construct polymorphic arguments
        let mut poly_args = self.create_initial_poly_vars(&definition.poly_vars);
        for argument in &arguments {
            self.check_and_resolve_embedded_type_and_modify_poly_args(ctx, definition_id, &mut poly_args, root_id, argument.parser_type)?;
        }

        let is_polymorph = poly_args.iter().any(|v| v.is_in_use);

        // Construct entry in type table
        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Component(ComponentType{
                variant: component_variant,
                arguments,
            }),
            poly_vars: poly_args,
            is_polymorph,
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
            ResolveResult::BuiltIn | ResolveResult::PolyArg(_) => Ok(true),
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

    /// Each type definition may consist of several embedded subtypes. This
    /// function checks whether that embedded type is a builtin, a direct
    /// reference to a polymorphic argument, or an (un)resolved type definition.
    /// If the embedded type's symbol cannot be found then this function returns
    /// an error.
    ///
    /// If the embedded type is resolved, then one always receives the type's
    /// (module, definition) tuple. If any of the types in the embedded type's
    /// tree is not yet resolved, then one may receive a (module, definition)
    /// tuple that does not correspond to the `parser_type_id` passed into this
    /// function.
    fn resolve_base_parser_type(&mut self, ctx: &TypeCtx, poly_vars: &Vec<Identifier>, root_id: RootId, parser_type_id: ParserTypeId) -> Result<ResolveResult, ParseError2> {
        use ParserTypeVariant as PTV;

        // Prepping iterator
        self.parser_type_iter.clear();
        self.parser_type_iter.push_back(parser_type_id);

        // Result for the very first time we resolve a
        let mut resolve_result = None;
        let mut set_resolve_result = |v: ResolveResult| {
            if resolve_result.is_none() { resolve_result = Some(v); }
        };

        'resolve_loop: while let Some(parser_type_id) = self.parser_type_iter.pop_back() {
            let parser_type = &ctx.heap[parser_type_id];

            match &parser_type.variant {
                // Builtin types. An array is a builtin as it is implemented as a
                // couple of pointers, so we do not require the subtype to be fully
                // resolved. Similar for input/output ports.
                PTV::Array(_) | PTV::Input(_) | PTV::Output(_) | PTV::Message |
                PTV::Bool | PTV::Byte | PTV::Short | PTV::Int | PTV::Long |
                PTV::String => {
                    set_resolve_result(ResolveResult::BuiltIn);
                },
                // IntegerLiteral types and the inferred marker are not allowed in
                // definitions of types
                PTV::IntegerLiteral |
                PTV::Inferred => {
                    debug_assert!(false, "Encountered illegal ParserTypeVariant within type definition");
                    unreachable!();
                },
                // Symbolic type, make sure its base type, and the base types
                // of all members of the embedded type tree are resolved. We
                // don't care about monomorphs yet.
                PTV::Symbolic(symbolic) => {
                    // Check if the symbolic type is one of the definition's
                    // polymorphic arguments. If so then we can halt the
                    // execution
                    for (poly_arg_idx, poly_arg) in poly_vars.iter().enumerate() {
                        if symbolic.identifier.matches_identifier(poly_arg) {
                            set_resolve_result(ResolveResult::PolyArg(poly_arg_idx));
                            continue 'resolve_loop;
                        }
                    }

                    // Lookup the definition in the symbol table
                    let (symbol, mut ident_iter) = ctx.symbols.resolve_namespaced_identifier(root_id, &symbolic.identifier);
                    if symbol.is_none() {
                        return Err(ParseError2::new_error(
                            &ctx.modules[root_id.index as usize].source, symbolic.identifier.position,
                            "Could not resolve type"
                        ))
                    }

                    let symbol_value = symbol.unwrap();
                    let module_source = &ctx.modules[root_id.index as usize].source;

                    match symbol_value.symbol {
                        Symbol::Namespace(_) => {
                            // Reference to a namespace instead of a type
                            let last_ident = ident_iter.prev();
                            return if ident_iter.num_remaining() == 0 {
                                // Could also have polymorphic args, but we 
                                // don't care, just throw this error: 
                                Err(ParseError2::new_error(
                                    module_source, symbolic.identifier.position,
                                    "Expected a type, got a module name"
                                ))
                            } else if last_ident.is_some() && last_ident.map(|(_, poly_args)| poly_args.is_some()).unwrap() {
                                // Halted at a namespaced because we encountered
                                // polymorphic arguments
                                Err(ParseError2::new_error(
                                    module_source, symbolic.identifier.position,
                                    "Illegal specification of polymorphic arguments to a module name"
                                ))
                            } else {
                                // Impossible (with the current implementation 
                                // of the symbol table)
                                unreachable!(
                                    "Got namespace symbol with {} returned symbols from {}",
                                    ident_iter.num_returned(),
                                    &String::from_utf8_lossy(&symbolic.identifier.value)
                                );
                            }
                        },
                        Symbol::Definition((root_id, definition_id)) => {
                            let definition = &ctx.heap[definition_id];
                            if ident_iter.num_remaining() > 0 {
                                // Namespaced identifier is longer than the type
                                // we found. Return the appropriate message
                                return if definition.is_struct() || definition.is_enum() {
                                    Err(ParseError2::new_error(
                                        module_source, symbolic.identifier.position,
                                        &format!(
                                            "Unknown type '{}', did you mean to use '{}'?",
                                            String::from_utf8_lossy(&symbolic.identifier.value),
                                            String::from_utf8_lossy(&definition.identifier().value)
                                        )
                                    ))
                                } else {
                                    Err(ParseError2::new_error(
                                        module_source, symbolic.identifier.position,
                                        "Unknown datatype"
                                    ))
                                }
                            }

                            // Found a match, make sure it is a datatype
                            if !(definition.is_struct() || definition.is_enum()) {
                                return Err(ParseError2::new_error(
                                    module_source, symbolic.identifier.position,
                                    "Embedded types must be datatypes (structs or enums)"
                                ))
                            }

                            // Found a struct/enum definition
                            if !self.lookup.contains_key(&definition_id) {
                                // Type is not yet resoled, immediately return
                                // this
                                return Ok(ResolveResult::Unresolved((root_id, definition_id)));
                            }

                            // Type is resolved, so set as result, but continue
                            // iterating over the parser types in the embedded
                            // type's tree
                            set_resolve_result(ResolveResult::Resolved((root_id, definition_id)));

                            // Note: because we're resolving parser types, not
                            // embedded types, we're parsing a tree, so we can't
                            // get stuck in a cyclic loop.
                            let last_ident = ident_iter.prev();
                            if let Some((_, Some(poly_args))) = last_ident {
                                for poly_arg_type_id in poly_args {
                                    self.parser_type_iter.push_back(*poly_arg_type_id);
                                }
                            }
                        }
                    }
                }
            }
        }

        // If here then all types in the embedded type's tree were resolved.
        debug_assert!(resolve_result.is_some(), "faulty logic in ParserType resolver");
        return Ok(resolve_result.unwrap())
    }

    fn create_initial_poly_vars(&self, poly_args: &[Identifier]) -> Vec<PolyVar> {
        poly_args
            .iter()
            .map(|v| PolyVar{ identifier: v.clone(), is_in_use: false })
            .collect()
    }

    /// This function modifies the passed `poly_args` by checking the embedded
    /// type tree. This should be called after `resolve_base_parser_type` is
    /// called on each node in this tree: we assume that each symbolic type was
    /// resolved to either a polymorphic arg or a definition.
    ///
    /// This function will also make sure that if the embedded type has
    /// polymorphic variables itself, that the number of polymorphic variables
    /// matches the number of arguments in the associated definition.
    ///
    /// Finally, for all embedded types (which includes function/component 
    /// arguments and return types) in type definitions we will modify the AST
    /// when the embedded type is a polymorphic variable or points to another
    /// user-defined type.
    fn check_and_resolve_embedded_type_and_modify_poly_args(
        &mut self, ctx: &mut TypeCtx, 
        type_definition_id: DefinitionId, poly_args: &mut [PolyVar], 
        root_id: RootId, embedded_type_id: ParserTypeId,
    ) -> Result<(), ParseError2> {
        use ParserTypeVariant as PTV;

        self.parser_type_iter.clear();
        self.parser_type_iter.push_back(embedded_type_id);

        'type_loop: while let Some(embedded_type_id) = self.parser_type_iter.pop_back() {
            let embedded_type = &mut ctx.heap[embedded_type_id];

            match &mut embedded_type.variant {
                PTV::Message | PTV::Bool | 
                PTV::Byte | PTV::Short | PTV::Int | PTV::Long |
                PTV::String => {
                    // Builtins, no modification/iteration required
                },
                PTV::IntegerLiteral | PTV::Inferred => {
                    // TODO: @hack Allowed for now so we can continue testing 
                    //  the parser/lexer
                    // debug_assert!(false, "encountered illegal parser type during ParserType/PolyArg modification");
                    // unreachable!();
                },
                PTV::Array(subtype_id) |
                PTV::Input(subtype_id) |
                PTV::Output(subtype_id) => {
                    // Outer type is fixed, but inner type might be symbolic
                    self.parser_type_iter.push_back(*subtype_id);
                },
                PTV::Symbolic(symbolic) => {
                    for (poly_arg_idx, poly_arg) in poly_args.iter_mut().enumerate() {
                        if symbolic.identifier.matches_identifier(&poly_arg.identifier) {
                            poly_arg.is_in_use = true;
                            // TODO: If we allow higher-kinded types in the future,
                            //  then we can't continue here, but must resolve the
                            //  polyargs as well
                            debug_assert!(symbolic.identifier.get_poly_args().is_none(), "got polymorphic arguments to a polymorphic variable");
                            debug_assert!(symbolic.variant.is_none(), "symbolic parser type's variant already resolved");
                            symbolic.variant = Some(SymbolicParserTypeVariant::PolyArg(type_definition_id, poly_arg_idx));
                            continue 'type_loop;
                        }
                    }

                    // Must match a definition
                    let (symbol, ident_iter) = ctx.symbols.resolve_namespaced_identifier(root_id, &symbolic.identifier);
                    debug_assert!(symbol.is_some(), "could not resolve symbolic parser type when determining poly args");
                    let symbol = symbol.unwrap();
                    debug_assert_eq!(ident_iter.num_remaining(), 0, "no exact symbol match when determining poly args");
                    let (_root_id, definition_id) = symbol.as_definition().unwrap();
    
                    // Must be a struct, enum, or union, we checked this
                    let defined_type = self.lookup.get(&definition_id).unwrap();
                    let (_, poly_args) = ident_iter.prev().unwrap();
                    let poly_args = poly_args.unwrap_or_default();

                    if cfg!(debug_assertions) {
                        // Everything here should already be checked in 
                        // `resolve_base_parser_type`.
                        let type_class = defined_type.definition.type_class();
                        debug_assert!(
                            type_class == TypeClass::Struct || type_class == TypeClass::Enum || type_class == TypeClass::Union,
                            "embedded type's class is not struct, enum or union"
                        );
                        debug_assert_eq!(poly_args.len(), symbolic.identifier.poly_args.len());
                    }
    
                    if poly_args.len() != defined_type.poly_vars.len() {
                        // Mismatch in number of polymorphic arguments. This is 
                        // not allowed in type definitions (no inference is 
                        // allowed within type definitions, only in bodies of
                        // functions/components).
                        let module_source = &ctx.modules[root_id.index as usize].source;
                        let number_args_msg = if defined_type.poly_vars.is_empty() {
                            String::from("is not polymorphic")
                        } else {
                            format!("accepts {} polymorphic arguments", defined_type.poly_vars.len())
                        };
    
                        return Err(ParseError2::new_error(
                            module_source, symbolic.identifier.position,
                            &format!(
                                "The type '{}' {}, but {} polymorphic arguments were provided",
                                String::from_utf8_lossy(&symbolic.identifier.strip_poly_args()),
                                number_args_msg, poly_args.len()
                            )
                        ));
                    }
    
                    self.parser_type_iter.extend(poly_args);
                    debug_assert!(symbolic.variant.is_none(), "symbolic parser type's variant already resolved");
                    symbolic.variant = Some(SymbolicParserTypeVariant::Definition(definition_id));
                }
            }
        }

        // All nodes in the embedded type tree were valid
        Ok(())
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
                if item_ident == other_item_ident {
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
                if poly_arg == other_poly_arg {
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
                if *module_definition_id == definition_id {
                    return root_id
                }
            }
        }

        debug_assert!(false, "DefinitionId without corresponding RootId");
        unreachable!();
    }
}
/**
 * type_table.rs
 *
 * The type table is a lookup from AST definition (which contains just what the
 * programmer typed) to a type with additional information computed (e.g. the
 * byte size and offsets of struct members). The type table should be considered
 * the authoritative source of information on types by the compiler (not the
 * AST itself!).
 *
 * The type table operates in two modes: one is where we just look up the type,
 * check its fields for correctness and mark whether it is polymorphic or not.
 * The second one is where we compute byte sizes, alignment and offsets.
 *
 * The basic algorithm for type resolving and computing byte sizes is to
 * recursively try to lay out each member type of a particular type. This is
 * done in a stack-like fashion, where each embedded type pushes a breadcrumb
 * unto the stack. We may discover a cycle in embedded types (we call this a
 * "type loop"). After which the type table attempts to break the type loop by
 * making specific types heap-allocated. Upon doing so we know their size
 * because their stack-size is now based on pointers. Hence breaking the type
 * loop required for computing the byte size of types.
 *
 * The reason for these type shenanigans is because PDL is a value-based
 * language, but we would still like to be able to express recursively defined
 * types like trees or linked lists. Hence we need to insert pointers somewhere
 * to break these cycles.
 *
 * As a final bit of global documentation: non-polymorphic types will always
 * have one "monomorph" entry. This contains the non-polymorphic type's memory
 * layout.
 */

use std::fmt::{Formatter, Result as FmtResult};
use std::collections::HashMap;

use crate::protocol::ast::*;
use crate::protocol::parser::symbol_table::SymbolScope;
use crate::protocol::input_source::ParseError;
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
            TypeClass::Union => "union",
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
    pub(crate) ast_root: RootId,
    pub(crate) ast_definition: DefinitionId,
    pub(crate) definition: DefinedTypeVariant,
    pub(crate) poly_vars: Vec<PolymorphicVariable>,
    pub(crate) is_polymorph: bool,
}

pub enum DefinedTypeVariant {
    Enum(EnumType),
    Union(UnionType),
    Struct(StructType),
    Function(FunctionType),
    Component(ComponentType)
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

    pub(crate) fn as_struct_mut(&mut self) -> &mut StructType {
        match self {
            DefinedTypeVariant::Struct(v) => v,
            _ => unreachable!("Cannot convert {} to struct variant", self.type_class())
        }
    }

    pub(crate) fn as_enum(&self) -> &EnumType {
        match self {
            DefinedTypeVariant::Enum(v) => v,
            _ => unreachable!("Cannot convert {} to enum variant", self.type_class())
        }
    }

    pub(crate) fn as_enum_mut(&mut self) -> &mut EnumType {
        match self {
            DefinedTypeVariant::Enum(v) => v,
            _ => unreachable!("Cannot convert {} to enum variant", self.type_class())
        }
    }

    pub(crate) fn as_union(&self) -> &UnionType {
        match self {
            DefinedTypeVariant::Union(v) => v,
            _ => unreachable!("Cannot convert {} to union variant", self.type_class())
        }
    }

    pub(crate) fn as_union_mut(&mut self) -> &mut UnionType {
        match self {
            DefinedTypeVariant::Union(v) => v,
            _ => unreachable!("Cannot convert {} to union variant", self.type_class())
        }
    }

    pub(crate) fn procedure_monomorphs(&self) -> &Vec<ProcedureMonomorph> {
        use DefinedTypeVariant::*;

        match self {
            Function(v) => &v.monomorphs,
            Component(v) => &v.monomorphs,
            _ => unreachable!("cannot get procedure monomorphs from {}", self.type_class()),
        }
    }

    pub(crate) fn procedure_monomorphs_mut(&mut self) -> &mut Vec<ProcedureMonomorph> {
        use DefinedTypeVariant::*;

        match self {
            Function(v) => &mut v.monomorphs,
            Component(v) => &mut v.monomorphs,
            _ => unreachable!("cannot get procedure monomorphs from {}", self.type_class()),
        }
    }
}

pub struct PolymorphicVariable {
    identifier: Identifier,
    is_in_use: bool, // a polymorphic argument may be defined, but not used by the type definition
}

/// Data associated with a monomorphized datatype
pub struct DataMonomorph {
    pub poly_args: Vec<ConcreteType>,
}

/// Data associated with a monomorphized procedure type. Has the wrong name,
/// because it will also be used to store expression data for a non-polymorphic
/// procedure. (in that case, there will only ever be one)
pub struct ProcedureMonomorph {
    // Expression data for one particular monomorph
    pub poly_args: Vec<ConcreteType>,
    pub expr_data: Vec<MonomorphExpression>,
}

/// `EnumType` is the classical C/C++ enum type. It has various variants with
/// an assigned integer value. The integer values may be user-defined,
/// compiler-defined, or a mix of the two. If a user assigns the same enum
/// value multiple times, we assume the user is an expert and we consider both
/// variants to be equal to one another.
pub struct EnumType {
    pub variants: Vec<EnumVariant>,
    pub monomorphs: Vec<EnumMonomorph>,
    pub minimum_tag_value: i64,
    pub maximum_tag_value: i64,
    pub tag_type: ConcreteType,
    pub size: usize,
    pub alignment: usize,
}

// TODO: Also support maximum u64 value
pub struct EnumVariant {
    pub identifier: Identifier,
    pub value: i64,
}

pub struct EnumMonomorph {
    pub poly_args: Vec<ConcreteType>
}

/// `UnionType` is the algebraic datatype (or sum type, or discriminated union).
/// A value is an element of the union, identified by its tag, and may contain
/// a single subtype.
/// For potentially infinite types (i.e. a tree, or a linked list) only unions
/// can break the infinite cycle. So when we lay out these unions in memory we
/// will reserve enough space on the stack for all union variants that do not
/// cause "type loops" (i.e. a union `A` with a variant containing a struct
/// `B`). And we will reserve enough space on the heap (and store a pointer in
/// the union) for all variants which do cause type loops (i.e. a union `A`
/// with a variant to a struct `B` that contains the union `A` again).
pub struct UnionType {
    pub variants: Vec<UnionVariant>,
    pub monomorphs: Vec<UnionMonomorph>,
    pub requires_allocation: bool,
    pub contains_unallocated_variant: bool,
}

pub struct UnionVariant {
    pub identifier: Identifier,
    pub embedded: Vec<ParserType>, // zero-length does not have embedded values
    pub tag_value: i64,
}

pub struct UnionMonomorph {
    pub poly_args: Vec<ConcreteType>,
    pub variants: Vec<UnionMonomorphVariant>,
    pub alignment: usize,
    // stack_byte_size is the size of the union on the stack, includes the tag
    pub stack_byte_size: usize,
    // heap_byte_size contains the allocated size of the union in the case it
    // is used to break a type loop. If it is 0, then it doesn't require
    // allocation and lives entirely on the stack.
    pub heap_byte_size: usize,
}

pub struct UnionMonomorphVariant {
    pub lives_on_heap: bool,
    pub embedded: Vec<UnionMonomorphEmbedded>,
}

pub struct UnionMonomorphEmbedded {
    pub concrete_type: ConcreteType,
    pub size: usize,
    pub offset: usize,
}

/// `StructType` is a generic C-like struct type (or record type, or product
/// type) type.
pub struct StructType {
    pub fields: Vec<StructField>,
    pub monomorphs: Vec<DataMonomorph>,
}

pub struct StructField {
    pub identifier: Identifier,
    pub parser_type: ParserType,
}

pub struct StructMonomorph {
    pub poly_args: Vec<ConcreteType>,
    pub fields: Vec<StructMonomorphField>,
}

pub struct StructMonomorphField {
    pub concrete_type: ConcreteType,
    pub size: usize,
    pub offset: usize,
}

/// `FunctionType` is what you expect it to be: a particular function's
/// signature.
pub struct FunctionType {
    pub return_types: Vec<ParserType>,
    pub arguments: Vec<FunctionArgument>,
    pub monomorphs: Vec<ProcedureMonomorph>,
}

pub struct ComponentType {
    pub variant: ComponentVariant,
    pub arguments: Vec<FunctionArgument>,
    pub monomorphs: Vec<ProcedureMonomorph>
}

pub struct FunctionArgument {
    identifier: Identifier,
    parser_type: ParserType,
}

/// Represents the data associated with a single expression after type inference
/// for a monomorph (or just the normal expression types, if dealing with a
/// non-polymorphic function/component).
pub struct MonomorphExpression {
    // The output type of the expression. Note that for a function it is not the
    // function's signature but its return type
    pub(crate) expr_type: ConcreteType,
    // Has multiple meanings: the field index for select expressions, the
    // monomorph index for polymorphic function calls or literals. Negative
    // values are never used, but used to catch programming errors.
    pub(crate) field_or_monomorph_idx: i32,
}

//------------------------------------------------------------------------------
// Type table
//------------------------------------------------------------------------------

struct LayoutBreadcrumb {
    root_id: RootId,
    definition_id: DefinitionId,
    monomorph_idx: usize,
    next_member: usize,
    next_embedded: usize, // for unions, the next embedded value inside the member
}

/// Result from attempting to progress a breadcrumb
enum LayoutResult {
    PopBreadcrumb,
    PushBreadcrumb(RootId, DefinitionId, Vec<ConcreteType>),
    TypeLoop(RootId, DefinitionId, Vec<ConcreteType>),
}

enum LookupResult {
    Exists(ConcreteType), // type was looked up and exists (or is a builtin)
    Missing(DefinitionId, Vec<ConcreteType>), // type was looked up and doesn't exist, vec contains poly args
}

pub struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Breadcrumbs left behind while resolving embedded types
    breadcrumbs: Vec<LayoutBreadcrumb>,
    infinite_unions: Vec<(RootId, DefinitionId)>,
}

impl TypeTable {
    /// Construct a new type table without any resolved types.
    pub(crate) fn new() -> Self {
        Self{ 
            lookup: HashMap::new(), 
            breadcrumbs: Vec::with_capacity(32),
            infinite_unions: Vec::with_capacity(16),
        }
    }

    /// Iterates over all defined types (polymorphic and non-polymorphic) and
    /// add their types in two passes. In the first pass we will just add the
    /// base types (we will not consider monomorphs, and we will not compute
    /// byte sizes). In the second pass we will compute byte sizes of
    /// non-polymorphic types, and potentially the monomorphs that are embedded
    /// in those types.
    pub(crate) fn build_base_types(&mut self, modules: &mut [Module], ctx: &mut PassCtx) -> Result<(), ParseError> {
        // Make sure we're allowed to cast root_id to index into ctx.modules
        debug_assert!(modules.iter().all(|m| m.phase >= ModuleCompilationPhase::DefinitionsParsed));
        debug_assert!(self.lookup.is_empty());

        if cfg!(debug_assertions) {
            for (index, module) in modules.iter().enumerate() {
                debug_assert_eq!(index, module.root_id.index as usize);
            }
        }

        // Use context to guess hashmap size of the base types
        let reserve_size = ctx.heap.definitions.len();
        self.lookup.reserve(reserve_size);

        // Resolve all base types
        for definition_idx in 0..ctx.heap.definitions.len() {
            let definition_id = ctx.heap.definitions.get_id(definition_idx);
            let definition = &ctx.heap[definition_id];

            match definition {
                Definition::Enum(_) => self.build_base_enum_definition(modules, ctx, definition_id)?,
                Definition::Union(_) => self.build_base_union_definition(modules, ctx, definition_id)?,
                Definition::Struct(_) => self.build_base_struct_definition(modules, ctx, definition_id)?,
                Definition::Function(_) => self.build_base_function_definition(modules, ctx, definition_id)?,
                Definition::Component(_) => self.build_base_component_definition(modules, ctx, definition_id)?,
            }
        }

        debug_assert_eq!(self.lookup.len(), reserve_size, "mismatch in reserved size of type table"); // NOTE: Temp fix for builtin functions
        for module in modules {
            module.phase = ModuleCompilationPhase::TypesAddedToTable;
        }

        // Go through all types again, lay out all types that are not
        // polymorphic. This might cause us to lay out types that are monomorphs
        // of polymorphic types.
        for definition_idx in 0..ctx.heap.definitions.len() {
            let definition_id = ctx.heap.definitions.get_id(definition_idx);
            let base_type = self.lookup.get(&definition_id).unwrap();
            if !base_type.is_polymorph {

            }
        }

        Ok(())
    }

    /// Retrieves base definition from type table. We must be able to retrieve
    /// it as we resolve all base types upon type table construction (for now).
    /// However, in the future we might do on-demand type resolving, so return
    /// an option anyway
    pub(crate) fn get_base_definition(&self, definition_id: &DefinitionId) -> Option<&DefinedType> {
        self.lookup.get(&definition_id)
    }

    /// Returns the index into the monomorph type array if the procedure type
    /// already has a (reserved) monomorph.
    pub(crate) fn get_procedure_monomorph_index(&self, definition_id: &DefinitionId, types: &Vec<ConcreteType>) -> Option<i32> {
        let def = self.lookup.get(definition_id).unwrap();
        if def.is_polymorph {
            let monos = def.definition.procedure_monomorphs();
            return monos.iter()
                .position(|v| v.poly_args == *types)
                .map(|v| v as i32);
        } else {
            // We don't actually care about the types
            let monos = def.definition.procedure_monomorphs();
            if monos.is_empty() {
                return None
            } else {
                return Some(0)
            }
        }
    }

    /// Returns a mutable reference to a procedure's monomorph expression data.
    /// Used by typechecker to fill in previously reserved type information
    pub(crate) fn get_procedure_expression_data_mut(&mut self, definition_id: &DefinitionId, monomorph_idx: i32) -> &mut ProcedureMonomorph {
        debug_assert!(monomorph_idx >= 0);
        let def = self.lookup.get_mut(definition_id).unwrap();
        let monomorphs = def.definition.procedure_monomorphs_mut();
        return &mut monomorphs[monomorph_idx as usize];
    }

    pub(crate) fn get_procedure_expression_data(&self, definition_id: &DefinitionId, monomorph_idx: i32) -> &ProcedureMonomorph {
        debug_assert!(monomorph_idx >= 0);
        let def = self.lookup.get(definition_id).unwrap();
        let monomorphs = def.definition.procedure_monomorphs();
        return &monomorphs[monomorph_idx as usize];
    }

    /// Reserves space for a monomorph of a polymorphic procedure. The index
    /// will point into a (reserved) slot of the array of expression types. The
    /// monomorph may NOT exist yet (because the reservation implies that we're
    /// going to be performing typechecking on it, and we don't want to
    /// check the same monomorph twice)
    pub(crate) fn reserve_procedure_monomorph_index(&mut self, definition_id: &DefinitionId, types: Option<Vec<ConcreteType>>) -> i32 {
        let def = self.lookup.get_mut(definition_id).unwrap();
        if let Some(types) = types {
            // Expecting a polymorphic procedure
            let monos = def.definition.procedure_monomorphs_mut();
            debug_assert!(def.is_polymorph);
            debug_assert!(def.poly_vars.len() == types.len());
            debug_assert!(monos.iter().find(|v| v.poly_args == types).is_none());

            let mono_idx = monos.len();
            monos.push(ProcedureMonomorph{ poly_args: types, expr_data: Vec::new() });

            return mono_idx as i32;
        } else {
            // Expecting a non-polymorphic procedure
            let monos = def.definition.procedure_monomorphs_mut();
            debug_assert!(!def.is_polymorph);
            debug_assert!(def.poly_vars.is_empty());
            debug_assert!(monos.is_empty());

            monos.push(ProcedureMonomorph{ poly_args: Vec::new(), expr_data: Vec::new() });

            return 0;
        }
    }

    /// Adds a datatype polymorph to the type table. Will not add the
    /// monomorph if it is already present, or if the type's polymorphic
    /// variables are all unused.
    pub(crate) fn add_data_monomorph(&mut self, definition_id: &DefinitionId, types: Vec<ConcreteType>) -> i32 {
        let def = self.lookup.get_mut(definition_id).unwrap();
        if !def.is_polymorph {
            // Not a polymorph, or polyvars are not used in type definition
            return 0;
        }

        let monos = def.definition.data_monomorphs_mut();
        if let Some(index) = monos.iter().position(|v| v.poly_args == types) {
            // We already know about this monomorph
            return index as i32;
        }

        let index = monos.len();
        monos.push(DataMonomorph{ poly_args: types });
        return index as i32;
    }

    //--------------------------------------------------------------------------
    // Building base types
    //--------------------------------------------------------------------------

    /// Builds the base type for an enum. Will not compute byte sizes
    fn build_base_enum_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(!self.lookup.contains_key(&definition_id), "base enum already built");
        let definition = ctx.heap[definition_id].as_enum();
        let root_id = definition.defined_in;

        // Determine enum variants
        let mut enum_value = -1;
        let mut variants = Vec::with_capacity(definition.variants.len());

        for variant in &definition.variants {
            if enum_value == i64::MAX {
                let source = &modules[definition.defined_in.index as usize].source;
                return Err(ParseError::new_error_str_at_span(
                    source, variant.identifier.span,
                    "this enum variant has an integer value that is too large"
                ));
            }

            enum_value += 1;
            if let EnumVariantValue::Integer(explicit_value) = variant.value {
                enum_value = explicit_value;
            }

            variants.push(EnumVariant{
                identifier: variant.identifier.clone(),
                value: enum_value,
            });
        }

        // Determine tag size
        let mut min_enum_value = 0;
        let mut max_enum_value = 0;
        if !variants.is_empty() {
            min_enum_value = variants[0].value;
            max_enum_value = variants[0].value;
            for variant in variants.iter().skip(1) {
                min_enum_value = min_enum_value.min(variant.value);
                max_enum_value = max_enum_value.max(variant.value);
            }
        }

        let (tag_type, size_and_alignment) = Self::variant_tag_type_from_values(min_enum_value, max_enum_value);

        // Enum names and polymorphic args do not conflict
        Self::check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "enum variant"
        )?;

        // Polymorphic arguments cannot appear as embedded types, because
        // they can only consist of integer variants.
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;
        let poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);

        self.lookup.insert(definition_id, DefinedType {
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Enum(EnumType{
                variants,
                monomorphs: Vec::new(),
                minimum_tag_value: min_enum_value,
                maximum_tag_value: max_enum_value,
                tag_type,
                size: size_and_alignment,
                alignment: size_and_alignment
            }),
            poly_vars,
            is_polymorph: false,
        });

        return Ok(());
    }

    /// Builds the base type for a union. Will compute byte sizes.
    fn build_base_union_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(!self.lookup.contains_key(&definition_id), "base union already built");
        let definition = ctx.heap[definition_id].as_union();
        let root_id = definition.defined_in;

        // Check all variants and their embedded types
        let mut variants = Vec::with_capacity(definition.variants.len());
        let tag_counter = 0;
        for variant in &definition.variants {
            for embedded in &variant.value {
                Self::check_member_parser_type(
                    modules, ctx, root_id, embedded, false
                )?;
            }

            let has_embedded = !variant.value.is_empty();
            variants.push(UnionVariant{
                identifier: variant.identifier.clone(),
                embedded: variant.value.clone(),
                tag_value: tag_counter,
            });
        }

        // Make sure there are no conflicts in identifiers
        Self::check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "union variant"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars);

        // Construct internal representation of union
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for variant in &definition.variants {
            for embedded in &variant.value {
                Self::mark_used_polymorphic_variables(&mut poly_vars, embedded);
            }
        }

        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Union(UnionType{
                variants,
                monomorphs: Vec::new(),
                requires_allocation: false,
                contains_unallocated_variant: false
            }),
            poly_vars,
            is_polymorph
        });

        return Ok(());
    }

    /// Builds base struct type. Will not compute byte sizes.
    fn build_base_struct_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(!self.lookup.contains_key(&definition_id), "base struct already built");
        let definition = ctx.heap[definition_id].as_struct();
        let root_id = definition.defined_in;

        // Check all struct fields and construct internal representation
        let mut fields = Vec::with_capacity(definition.fields.len());

        for field in &definition.fields {
            Self::check_member_parser_type(
                modules, ctx, root_id, &field.parser_type, false
            )?;

            fields.push(StructField{
                identifier: field.field.clone(),
                parser_type: field.parser_type.clone(),
            });
        }

        // Make sure there are no conflicting variables
        Self::check_identifier_collision(
            modules, root_id, &fields, |field| &field.identifier, "struct field"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct base type in table
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for field in &fields {
            Self::mark_used_polymorphic_variables(&mut poly_vars, &field.parser_type);
        }

        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Struct(StructType{
                fields,
                monomorphs: Vec::new(),
            }),
            poly_vars,
            is_polymorph
        });

        return Ok(())
    }

    /// Builds base function type.
    fn build_base_function_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(!self.lookup.contains_key(&definition_id), "base function already built");
        let definition = ctx.heap[definition_id].as_function();
        let root_id = definition.defined_in;

        // Check and construct return types and argument types.
        debug_assert_eq!(definition.return_types.len(), 1, "not one return type"); // TODO: @ReturnValues
        for return_type in &definition.return_types {
            Self::check_member_parser_type(
                modules, ctx, root_id, return_type, definition.builtin
            )?;
        }

        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for parameter_id in &definition.parameters {
            let parameter = &ctx.heap[*parameter_id];
            Self::check_member_parser_type(
                modules, ctx, root_id, &parameter.parser_type, definition.builtin
            )?;

            arguments.push(FunctionArgument{
                identifier: parameter.identifier.clone(),
                parser_type: parameter.parser_type.clone(),
            });
        }

        // Check conflict of identifiers
        Self::check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "function argument"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct internal representation of function type
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for return_type in &definition.return_types {
            Self::mark_used_polymorphic_variables(&mut poly_vars, return_type);
        }
        for argument in &arguments {
            Self::mark_used_polymorphic_variables(&mut poly_vars, &argument.parser_type);
        }

        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Function(FunctionType{
                return_types: definition.return_types.clone(),
                arguments,
                monomorphs: Vec::new(),
            }),
            poly_vars,
            is_polymorph
        });

        return Ok(());
    }

    /// Builds base component type.
    fn build_base_component_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(self.lookup.contains_key(&definition_id), "base component already built");

        let definition = &ctx.heap[definition_id].as_component();
        let root_id = definition.defined_in;

        // Check the argument types
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for parameter_id in &definition.parameters {
            let parameter = &ctx.heap[*parameter_id];
            Self::check_member_parser_type(
                modules, ctx, root_id, &parameter.parser_type, false
            )?;

            arguments.push(FunctionArgument{
                identifier: parameter.identifier.clone(),
                parser_type: parameter.parser_type.clone(),
            });
        }

        // Check conflict of identifiers
        Self::check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "connector argument"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct internal representation of component
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for argument in &arguments {
            Self::mark_used_polymorphic_variables(&mut poly_vars, &argument.parser_type);
        }

        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Component(ComponentType{
                variant: definition.variant,
                arguments,
                monomorphs: Vec::new()
            }),
            poly_vars,
            is_polymorph
        });

        Ok(())
    }

    /// Will check if the member type (field of a struct, embedded type in a
    /// union variant) is valid.
    fn check_member_parser_type(
        modules: &[Module], ctx: &PassCtx, base_definition_root_id: RootId,
        member_parser_type: &ParserType, allow_special_compiler_types: bool
    ) -> Result<(), ParseError> {
        use ParserTypeVariant as PTV;

        for element in &member_parser_type.elements {
            match element.variant {
                // Special cases
                PTV::Void | PTV::InputOrOutput | PTV::ArrayLike | PTV::IntegerLike => {
                    if !allow_special_compiler_types {
                        unreachable!("compiler-only ParserTypeVariant in member type");
                    }
                },
                // Builtin types, always valid
                PTV::Message | PTV::Bool |
                PTV::UInt8 | PTV::UInt16 | PTV::UInt32 | PTV::UInt64 |
                PTV::SInt8 | PTV::SInt16 | PTV::SInt32 | PTV::SInt64 |
                PTV::Character | PTV::String |
                PTV::Array | PTV::Input | PTV::Output |
                // Likewise, polymorphic variables are always valid
                PTV::PolymorphicArgument(_, _) => {},
                // Types that are not constructable, or types that are not
                // allowed (and checked earlier)
                PTV::IntegerLiteral | PTV::Inferred => {
                    unreachable!("illegal ParserTypeVariant within type definition");
                },
                // Finally, user-defined types
                PTV::Definition(definition_id, _) => {
                    let definition = &ctx.heap[definition_id];
                    if !(definition.is_struct() || definition.is_enum() || definition.is_union()) {
                        let source = &modules[base_definition_root_id.index as usize].source;
                        return Err(ParseError::new_error_str_at_span(
                            source, element.element_span, "expected a datatype (a struct, enum or union)"
                        ));
                    }

                    // Otherwise, we're fine
                }
            }
        }

        // If here, then all elements check out
        return Ok(());
    }

    /// Go through a list of identifiers and ensure that all identifiers have
    /// unique names
    fn check_identifier_collision<T: Sized, F: Fn(&T) -> &Identifier>(
        modules: &[Module], root_id: RootId, items: &[T], getter: F, item_name: &'static str
    ) -> Result<(), ParseError> {
        for (item_idx, item) in items.iter().enumerate() {
            let item_ident = getter(item);
            for other_item in &items[0..item_idx] {
                let other_item_ident = getter(other_item);
                if item_ident == other_item_ident {
                    let module_source = &modules[root_id.index as usize].source;
                    return Err(ParseError::new_error_at_span(
                        module_source, item_ident.span, format!("This {} is defined more than once", item_name)
                    ).with_info_at_span(
                        module_source, other_item_ident.span, format!("The other {} is defined here", item_name)
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
        modules: &[Module], ctx: &PassCtx, root_id: RootId, poly_args: &[Identifier]
    ) -> Result<(), ParseError> {
        // Make sure polymorphic arguments are unique and none of the
        // identifiers conflict with any imported scopes
        for (arg_idx, poly_arg) in poly_args.iter().enumerate() {
            for other_poly_arg in &poly_args[..arg_idx] {
                if poly_arg == other_poly_arg {
                    let module_source = &modules[root_id.index as usize].source;
                    return Err(ParseError::new_error_str_at_span(
                        module_source, poly_arg.span,
                        "This polymorphic argument is defined more than once"
                    ).with_info_str_at_span(
                        module_source, other_poly_arg.span,
                        "It conflicts with this polymorphic argument"
                    ));
                }
            }

            // Check if identifier conflicts with a symbol defined or imported
            // in the current module
            if let Some(symbol) = ctx.symbols.get_symbol_by_name(SymbolScope::Module(root_id), poly_arg.value.as_bytes()) {
                // We have a conflict
                let module_source = &modules[root_id.index as usize].source;
                let introduction_span = symbol.variant.span_of_introduction(ctx.heap);
                return Err(ParseError::new_error_str_at_span(
                    module_source, poly_arg.span,
                    "This polymorphic argument conflicts with another symbol"
                ).with_info_str_at_span(
                    module_source, introduction_span,
                    "It conflicts due to this symbol"
                ));
            }
        }

        // All arguments are fine
        Ok(())
    }

    //--------------------------------------------------------------------------
    // Determining memory layout for types
    //--------------------------------------------------------------------------

    pub(crate) fn layout_for_top_enum_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx, poly_args: Vec<ConcreteType>) -> Result<LayoutResult, ParseError> {
        // Enums are a bit special, because they never use their polymorphic
        // variables. So they only have to be layed out once. And this was
        // already done when laying out the base type.
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let definition = self.lookup.get_mut(&breadcrumb.definition_id).unwrap().definition.as_enum_mut();
        if definition.monomorphs.iter().any(|v| v.poly_args == poly_args) {
            return Ok(LayoutResult::PopBreadcrumb);
        }

        definition.monomorphs.push(EnumMonomorph{ poly_args });
        return Ok(LayoutResult::PopBreadcrumb);
    }

    pub(crate) fn layout_for_top_union_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx, poly_args: Vec<ConcreteType>) -> Result<LayoutResult, ParseError> {
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let definition = self.lookup.get_mut(&breadcrumb.definition_id).unwrap();
        debug_assert!(definition.poly_vars.len() == poly_args.len() || !definition.is_polymorph);
        let type_poly = definition.definition.as_union_mut();
        let type_mono = &mut type_poly.monomorphs[breadcrumb.monomorph_idx];

        let num_variants = type_poly.variants.len();
        while breadcrumb.next_member < num_variants {
            let poly_variant = &type_poly.variants[breadcrumb.next_member];
            let num_embedded = poly_variant.embedded.len();

            while breadcrumb.next_embedded < num_embedded {
                let poly_embedded = &poly_variant.embedded[breadcrumb.next_embedded];


                breadcrumb.next_embedded += 1;
            }

            breadcrumb.next_embedded = 0;
            breadcrumb.next_member += 1;
        }

        return Ok(LayoutResult::PopBreadcrumb);
    }

    /// Returns tag concrete type (always a builtin integer type), the size of
    /// that type in bytes (and implicitly, its alignment)
    pub(crate) fn variant_tag_type_from_values(min_val: i64, max_val: i64) -> (ConcreteType, usize) {
        debug_assert!(min_val <= max_val);

        let (part, size) = if min_val >= 0 {
            // Can be an unsigned integer
            if max_val <= (u32::MAX as i64) {
                (ConcreteTypePart::UInt32, 4)
            } else {
                (ConcreteTypePart::UInt64, 8)
            }
        } else {
            // Must be a signed integer
            if min_val >= (i32::MIN as i64) && max_val <= (i32::MAX as i64) {
                (ConcreteTypePart::SInt32, 4)
            } else {
                (ConcreteTypePart::SInt64, 8)
            }
        };

        return (ConcreteType{ parts: vec![part] }, size);
    }

    pub(crate) fn prepare_layout_for_member_type(
        &self, definition_id: DefinitionId, parser_type: &ParserType, poly_args: &Vec<ConcreteType>
    ) -> LookupResult {
        use ParserTypeVariant as PTV;
        use ConcreteTypePart as CTP;

        // Helper for direct translation of parser type to concrete type
        fn parser_to_concrete_part(part: &ParserTypeVariant) -> Option<ConcreteTypePart> {
            match part {
                PTV::Void      => Some(CTP::Void),
                PTV::Message   => Some(CTP::Message),
                PTV::Bool      => Some(CTP::Bool),
                PTV::UInt8     => Some(CTP::UInt8),
                PTV::UInt16    => Some(CTP::UInt16),
                PTV::UInt32    => Some(CTP::UInt32),
                PTV::UInt64    => Some(CTP::UInt64),
                PTV::SInt8     => Some(CTP::SInt8),
                PTV::SInt16    => Some(CTP::SInt16),
                PTV::SInt32    => Some(CTP::SInt32),
                PTV::SInt64    => Some(CTP::SInt64),
                PTV::Character => Some(CTP::Character),
                PTV::String    => Some(CTP::String),
                PTV::Array     => Some(CTP::Array),
                PTV::Input     => Some(CTP::Input),
                PTV::Output    => Some(CTP::Output),
                PTV::Definition(definition_id, num) => Some(CTP::Instance(*definition_id, *num)),
                _              => None
            }
        }

        // Construct the concrete type from the parser type and its polymorphic
        // arguments. Make a rough estimation of the total number of parts:
        // TODO: @Optimize
        debug_assert!(!parser_type.elements.is_empty());
        let mut concrete_parts = Vec::with_capacity(parser_type.elements.len());

        for parser_part in &parser_type.elements {
            if let Some(concrete_part) = parser_to_concrete_part(&parser_part.variant) {
                concrete_parts.push(concrete_part);
            } else if let PTV::PolymorphicArgument(_part_of_id, poly_idx) = parser_part.variant {
                concrete_parts.extend_from_slice(&poly_args[poly_idx as usize].parts);
            } else {
                unreachable!("unexpected parser part {:?} in {:?}", parser_part, parser_type);
            }
        }

        // Check if the type is an instance of a user-defined type, and if so,
        // whether the particular monomorph is already instantiated.
        if let CTP::Instance(definition_id, _) = concrete_parts[0] {
            let target_type = self.lookup.get(&definition_id).unwrap();
            // TODO: Continue here
        } else {

        }
    }

    //--------------------------------------------------------------------------
    // Breadcrumb management
    //--------------------------------------------------------------------------

    /// Pushes a breadcrumb for a particular definition. The field in the
    /// breadcrumb tracking progress in defining the type will be preallocated
    /// as best as possible.
    fn push_breadcrumb_for_definition(&mut self, ctx: &PassCtx, definition_id: DefinitionId) {
        let definition = &ctx.heap[definition_id];

        let (root_id, progression) = match definition {
            Definition::Struct(definition) => {
                (
                    definition.defined_in,
                    DefinedTypeVariant::Struct(StructType{
                        fields: Vec::with_capacity(definition.fields.len()),
                        monomorphs: Vec::new(),
                    })
                )
            },
            Definition::Enum(definition) => {
                (
                    definition.defined_in,
                    DefinedTypeVariant::Enum(EnumType{
                        variants: Vec::with_capacity(definition.variants.len()),
                        monomorphs: Vec::new(),
                    })
                )
            },
            Definition::Union(definition) => {
                (
                    definition.defined_in,
                    DefinedTypeVariant::Union(UnionType{
                        variants: Vec::with_capacity(definition.variants.len()),
                        monomorphs: Vec::new(),
                        requires_allocation: false,
                        contains_unallocated_variant: false
                    })
                )
            },
            Definition::Component(definition) => {
                (
                    definition.defined_in,
                    DefinedTypeVariant::Component(ComponentType{
                        variant: definition.variant,
                        arguments: Vec::with_capacity(definition.parameters.len()),
                        monomorphs: Vec::new(),
                    })
                )
            },
            Definition::Function(definition) => {
                (
                    definition.defined_in,
                    DefinedTypeVariant::Function(FunctionType{
                        return_types: Vec::with_capacity(1),
                        arguments: Vec::with_capacity(definition.parameters.len()),
                        monomorphs: Vec::new(),
                    })
                )
            }
        };

        self.breadcrumbs.push(ResolveBreadcrumb{
            root_id,
            definition_id,
            next_member: 0,
            next_embedded: 0,
        });
    }

    //--------------------------------------------------------------------------
    // Small utilities
    //--------------------------------------------------------------------------

    fn create_polymorphic_variables(variables: &[Identifier]) -> Vec<PolymorphicVariable> {
        let mut result = Vec::with_capacity(variables.len());
        for variable in variables.iter() {
            result.push(PolymorphicVariable{ identifier: variable.clone(), is_in_use: false });
        }

        result
    }

    fn mark_used_polymorphic_variables(poly_vars: &mut Vec<PolymorphicVariable>, parser_type: &ParserType) {
        for element in &parser_type.elements {
            if let ParserTypeVariant::PolymorphicArgument(_, idx) = &element.variant {
                poly_vars[*idx as usize].is_in_use = true;
            }
        }
    }
}
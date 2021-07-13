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
 * unto the stack. We may discover a cycle in embedded types (called a "type
 * loop"). After which the type table attempts to break the type loop by making
 * specific types heap-allocated. Upon doing so we know their size because their
 * stack-size is now based on pointers.
 *
 * The reason for these type shenanigans is because PDL is a value-based
 * language, but we would still like to be able to express recursively defined
 * types like trees or linked lists. Hence we need to insert pointers somewhere
 * to break these cycles.
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

    pub(crate) fn as_enum(&self) -> &EnumType {
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

    pub(crate) fn data_monomorphs(&self) -> &Vec<DataMonomorph> {
        use DefinedTypeVariant::*;

        match self {
            Enum(v) => &v.monomorphs,
            Union(v) => &v.monomorphs,
            Struct(v) => &v.monomorphs,
            _ => unreachable!("cannot get data monomorphs from {}", self.type_class()),
        }
    }

    pub(crate) fn data_monomorphs_mut(&mut self) -> &mut Vec<DataMonomorph> {
        use DefinedTypeVariant::*;

        match self {
            Enum(v) => &mut v.monomorphs,
            Union(v) => &mut v.monomorphs,
            Struct(v) => &mut v.monomorphs,
            _ => unreachable!("cannot get data monomorphs from {}", self.type_class()),
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
    pub monomorphs: Vec<DataMonomorph>,
}

// TODO: Also support maximum u64 value
pub struct EnumVariant {
    pub identifier: Identifier,
    pub value: i64,
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
    pub monomorphs: Vec<DataMonomorph>,
    pub requires_allocation: bool,
    pub contains_unallocated_variant: bool,
}

pub struct UnionVariant {
    pub identifier: Identifier,
    pub embedded: Vec<ParserType>, // zero-length does not have embedded values
    pub tag_value: i64,
    pub exists_in_heap: bool,
}

pub struct StructType {
    pub fields: Vec<StructField>,
    pub monomorphs: Vec<DataMonomorph>,
}

pub struct StructField {
    pub identifier: Identifier,
    pub parser_type: ParserType,
}

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

struct ResolveBreadcrumb {
    root_id: RootId,
    definition_id: DefinitionId,
    next_member: u32,
    next_embedded: u32, // for unions, the next embedded value inside the member
    progression: DefinedTypeVariant
}

/// Result from attempting to resolve a `ParserType` using the symbol table and
/// the type table.
enum ResolveResult {
    Builtin,
    PolymoprhicArgument,
    /// ParserType points to a user-defined type that is already resolved in the
    /// type table.
    Resolved(RootId, DefinitionId),
    /// ParserType points to a user-defined type that is not yet resolved into
    /// the type table.
    Unresolved(RootId, DefinitionId)
}

/// Result from attempting to progress a breadcrumb
enum ProgressResult {
    PopBreadcrumb,
    PushBreadcrumb(RootId, DefinitionId),
    TypeLoop(RootId, DefinitionId),
}

pub struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Breadcrumbs left behind while resolving embedded types
    breadcrumbs: Vec<ResolveBreadcrumb>,
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

        // Go through all types again, now try to m

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

    /// This function will resolve just the basic definition of the type, it
    /// will not handle any of the monomorphized instances of the type.
    fn resolve_base_definition<'a>(&'a mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        // Check if we have already resolved the base definition
        if self.lookup.contains_key(&definition_id) { return Ok(()); }

        // We haven't, push the first breadcrumb and start resolving
        self.push_breadcrumb_for_definition(ctx, definition_id);

        while let Some(breadcrumb) = self.breadcrumbs.last() {
            // We have a type to resolve
            let definition = &ctx.heap[breadcrumb.definition_id];

            let can_pop_breadcrumb = match definition {
                // Bit ugly, since we already have the definition, but we need
                // to work around rust borrowing rules...
                Definition::Enum(_) => self.resolve_base_enum_definition(modules, ctx),
                Definition::Union(_) => self.resolve_base_union_definition(modules, ctx),
                Definition::Struct(_) => self.resolve_base_struct_definition(modules, ctx),
                Definition::Component(_) => self.resolve_base_component_definition(modules, ctx),
                Definition::Function(_) => self.resolve_base_function_definition(modules, ctx),
            }?;

            // Otherwise: `ingest_resolve_result` has pushed a new breadcrumb
            // that we must follow before we can resolve the current type
            if can_pop_breadcrumb {
                self.breadcrumbs.pop();
            }
        }

        // We must have resolved the type
        debug_assert!(self.lookup.contains_key(&definition_id), "base type not resolved");
        Ok(())
    }

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

        // TODO: Tag size determination, here or when laying out?

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
            }),
            poly_vars,
            is_polymorph: false,
        });

        return Ok(());
    }

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
                exists_in_heap: false
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

    fn build_base_component_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        debug_assert!(self.lookup.contains_key(&definition_id), "base component already built");

        let definition = &ctx.heap[definition_id].as_component();
        let root_id = definition.defined_in;

        // Check the argument types
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for parameter_id in &definition.parameters {
            let parameter = &ctx.heap[*parameter_id];
            Self::check_member_parser_type(
                modules, ctx, root_id, &parameter.parser_type
            )?;

            arguments.push(FunctionArgument{
                identifier: parameter.identifier.clone(),
                parser_type: parameter.parser_type.clone(),
            });
        }

        // Check conflict of identifiers
        Self::check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier
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


    /// Resolve the basic enum definition to an entry in the type table. It will
    /// not instantiate any monomorphized instances of polymorphic enum
    /// definitions. If a subtype has to be resolved first then this function
    /// will return `false` after calling `ingest_resolve_result`.
    fn resolve_base_enum_definition(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<bool, ParseError> {
        // Retrieve breadcrumb and perform some basic checking
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let root_id = breadcrumb.root_id;
        let definition_id = breadcrumb.definition_id;

        debug_assert!(ctx.heap[definition_id].is_enum());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base enum already resolved");
        
        let definition = ctx.heap[definition_id].as_enum();

        // Since we're dealing with an enum's base definition, we're not going
        // to check any embedded types and should finish layout out the enum in
        // one go.
        debug_assert!(breadcrumb.progression.is_none());

        // Determine enum variants
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
                }
            }
            if enum_value < min_enum_value { min_enum_value = enum_value; }
            else if enum_value > max_enum_value { max_enum_value = enum_value; }
        }

        // Ensure enum names and polymorphic args do not conflict
        Self::check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "enum variant"
        )?;

        // Because we're parsing an enum, the programmer cannot put the
        // polymorphic variables inside the variants. But the polymorphic
        // variables might still be present as "marker types"
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;
        let poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);

        self.lookup.insert(definition_id, DefinedType {
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Enum(EnumType{
                variants,
                monomorphs: Vec::new(),
            }),
            poly_vars,
            is_polymorph: false,
        });

        Ok(true)
    }

    /// Resolves the basic union definiton to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic union
    /// definitions. If a subtype has to be resolved first then this function
    /// will return `false` after calling `ingest_resolve_result`.
    fn resolve_base_union_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_union());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base union already resolved");

        let definition = ctx.heap[definition_id].as_union();

        // Make sure all embedded types are resolved
        for variant in &definition.variants {
            match &variant.value {
                UnionVariantValue::None => {},
                UnionVariantValue::Embedded(embedded) => {
                    for parser_type in embedded {
                        let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, parser_type, false)?;
                        if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
                            return Ok(false)
                        }
                    }
                }
            }
        }

        // If here then all embedded types are resolved

        // Determine the union variants
        let mut tag_value = -1;
        let mut variants = Vec::with_capacity(definition.variants.len());
        for variant in &definition.variants {
            tag_value += 1;
            let embedded = match &variant.value {
                UnionVariantValue::None => { Vec::new() },
                UnionVariantValue::Embedded(embedded) => {
                    // Type should be resolvable, we checked this above
                    embedded.clone()
                },
            };

            variants.push(UnionVariant{
                identifier: variant.identifier.clone(),
                embedded,
                tag_value,
                exists_in_heap: false
            })
        }

        // Ensure union names and polymorphic args do not conflict
        Self::check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "union variant"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct polymorphic variables and mark the ones that are in use
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for variant in &variants {
            for parser_type in &variant.embedded {
                Self::mark_used_polymorphic_variables(&mut poly_vars, parser_type);
            }
        }
        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        // Insert base definition in type table
        self.lookup.insert(definition_id, DefinedType {
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Union(UnionType{
                variants,
                monomorphs: Vec::new(),
                requires_allocation: false,
                contains_unallocated_variant: true
            }),
            poly_vars,
            is_polymorph,
        });

        Ok(true)
    }

    /// Resolves the basic struct definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic struct
    /// definitions.
    fn resolve_base_struct_definition(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_struct());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base struct already resolved");

        let definition = ctx.heap[definition_id].as_struct();

        // Make sure all fields point to resolvable types
        for field_definition in &definition.fields {
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &field_definition.parser_type, false)?;
            if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // All fields types are resolved, construct base type
        let mut fields = Vec::with_capacity(definition.fields.len());
        for field_definition in &definition.fields {
            fields.push(StructField{
                identifier: field_definition.field.clone(),
                parser_type: field_definition.parser_type.clone(),
            })
        }

        // And make sure no conflicts exist in field names and/or polymorphic args
        Self::check_identifier_collision(
            modules, root_id, &fields, |field| &field.identifier, "struct field"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct representation of polymorphic arguments
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
            is_polymorph,
        });

        Ok(true)
    }

    /// Resolves the basic function definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic function
    /// definitions.
    fn resolve_base_function_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_function());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base function already resolved");

        let definition = ctx.heap[definition_id].as_function();

        // Check the return type
        debug_assert_eq!(definition.return_types.len(), 1, "not one return type"); // TODO: @ReturnValues
        let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &definition.return_types[0], definition.builtin)?;
        if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
            return Ok(false)
        }

        // Check the argument types
        let mut arguments = Vec::with_capacity(definition.parameters.len());

        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &param.parser_type, definition.builtin)?;
            if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
                return Ok(false)
            }

            arguments.push(FunctionArgument{
                identifier: param.identifier.clone(),
                parser_type: param.parser_type.clone(),
            });
        }

        // Check conflict of argument and polyarg identifiers
        Self::check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "function argument"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct polymorphic arguments
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        Self::mark_used_polymorphic_variables(&mut poly_vars, &definition.return_types[0]);
        for argument in &arguments {
            Self::mark_used_polymorphic_variables(&mut poly_vars, &argument.parser_type);
        }
        let is_polymorph = poly_vars.iter().any(|arg| arg.is_in_use);

        // Construct entry in type table
        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Function(FunctionType{
                return_types: definition.return_types.clone(),
                arguments,
                monomorphs: Vec::new(),
            }),
            poly_vars,
            is_polymorph,
        });

        Ok(true)
    }

    /// Resolves the basic component definition to an entry in the type table.
    /// It will not instantiate any monomorphized instancees of polymorphic
    /// component definitions.
    fn resolve_base_component_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_component());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base component already resolved");

        let definition = ctx.heap[definition_id].as_component();
        let component_variant = definition.variant;

        // Check argument types
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &param.parser_type, false)?;
            if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // Construct argument types
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            arguments.push(FunctionArgument{
                identifier: param.identifier.clone(),
                parser_type: param.parser_type.clone()
            })
        }

        // Check conflict of argument and polyarg identifiers
        Self::check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "component argument"
        )?;
        Self::check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

        // Construct polymorphic arguments
        let mut poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);
        for argument in &arguments {
            Self::mark_used_polymorphic_variables(&mut poly_vars, &argument.parser_type);
        }

        let is_polymorph = poly_vars.iter().any(|v| v.is_in_use);

        // Construct entry in type table
        self.lookup.insert(definition_id, DefinedType{
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Component(ComponentType{
                variant: component_variant,
                arguments,
                monomorphs: Vec::new(),
            }),
            poly_vars,
            is_polymorph,
        });

        Ok(true)
    }

    /// Takes a ResolveResult and returns `true` if the caller can happily
    /// continue resolving its current type, or `false` if the caller must break
    /// resolving the current type and exit to the outer resolving loop. In the
    /// latter case the `result` value was `ResolveResult::Unresolved`, implying
    /// that the type must be resolved first.
    fn ingest_resolve_result(&mut self, modules: &[Module], ctx: &PassCtx, result: ResolveResult) -> Result<bool, ParseError> {
        match result {
            ResolveResult::Builtin | ResolveResult::PolymoprhicArgument => Ok(true),
            ResolveResult::Resolved(_, _) => Ok(true),
            ResolveResult::Unresolved(root_id, definition_id) => {
                if self.iter.contains(root_id, definition_id) {
                    // Cyclic dependency encountered

                } else {
                    // Type is not yet resolved, so push IDs on iterator and
                    // continue the resolving loop
                    self.iter.push(root_id, definition_id);
                    Ok(false)
                }
            }
        }
    }

    fn construct_cyclic_type_error(
        &self, modules: &[Module], ctx: &PassCtx, type_root_id: RootId, type_definition_id: DefinitionId
    ) -> ParseError {
        debug_assert!(self.iter.contains(type_root_id, type_definition_id));

        // Construct error message put at the top
        let module_source = &modules[type_root_id.index as usize].source;
        let mut error = ParseError::new_error_str_at_span(
            module_source, ctx.heap[type_definition_id].identifier().span,
            "Evaluating this type definition results in a cyclic type"
        );

        // Show a listing of all dependent types.
        // TODO: Make more correct later
        for (breadcrumb_idx, (root_id, definition_id)) in self.iter.breadcrumbs.iter().enumerate() {
            let msg = if breadcrumb_idx == 0 {
                "The cycle started with this definition"
            } else {
                "Which depends on this definition"
            };

            let module_source = &modules[root_id.index as usize].source;
            error = error.with_info_str_at_span(module_source, ctx.heap[*definition_id].identifier().span, msg);
        }

        return error;
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

    /// Each type may consist of embedded types. If this type does not have a
    /// fixed implementation (e.g. an input port may have an embedded type
    /// indicating the type of messages, but it always exists in the runtime as
    /// a port identifier, so it has a fixed implementation) then this function
    /// will traverse the embedded types to ensure all of them are resolved.
    ///
    /// Hence if one checks a particular parser type for being resolved, one may
    /// get back a result value indicating an embedded type (with a different
    /// DefinitionId) is unresolved.
    fn resolve_base_parser_type(
        &mut self, modules: &[Module], ctx: &PassCtx, root_id: RootId,
        parser_type: &ParserType, allow_special_compiler_types: bool
    ) -> Result<ResolveResult, ParseError> {
        use ParserTypeVariant as PTV;

        // Result for the very first time we resolve a type (i.e the outer type
        // that we're actually looking up)
        let mut resolve_result = None;
        let mut set_resolve_result = |v: ResolveResult| {
            if resolve_result.is_none() { resolve_result = Some(v); }
        };

        for element in parser_type.elements.iter() {
            match element.variant {
                PTV::Void | PTV::InputOrOutput | PTV::ArrayLike | PTV::IntegerLike => {
                    if allow_special_compiler_types {
                        set_resolve_result(ResolveResult::Builtin);
                    } else {
                        unreachable!("compiler-only ParserTypeVariant within type definition");
                    }
                },
                PTV::Message | PTV::Bool |
                PTV::UInt8 | PTV::UInt16 | PTV::UInt32 | PTV::UInt64 |
                PTV::SInt8 | PTV::SInt16 | PTV::SInt32 | PTV::SInt64 |
                PTV::Character | PTV::String |
                PTV::Array | PTV::Input | PTV::Output => {
                    // Nothing to do: these are builtin types or types with a
                    // fixed implementation
                    set_resolve_result(ResolveResult::Builtin);
                },
                PTV::IntegerLiteral | PTV::Inferred => {
                    // As we're parsing the type definitions where these kinds
                    // of types are impossible/disallowed to express:
                    unreachable!("illegal ParserTypeVariant within type definition");
                },
                PTV::PolymorphicArgument(_, _) => {
                    set_resolve_result(ResolveResult::PolymoprhicArgument);
                },
                PTV::Definition(embedded_id, _) => {
                    let definition = &ctx.heap[embedded_id];
                    if !(definition.is_struct() || definition.is_enum() || definition.is_union()) {
                        let module_source = &modules[root_id.index as usize].source;
                        return Err(ParseError::new_error_str_at_span(
                            module_source, element.element_span, "expected a datatype (struct, enum or union)"
                        ))
                    }

                    if self.lookup.contains_key(&embedded_id) {
                        set_resolve_result(ResolveResult::Resolved(definition.defined_in(), embedded_id))
                    } else {
                        return Ok(ResolveResult::Unresolved(definition.defined_in(), embedded_id))
                    }
                }
            }
        }

        // If here then all types in the embedded type's tree were resolved.
        debug_assert!(resolve_result.is_some(), "faulty logic in ParserType resolver");
        return Ok(resolve_result.unwrap())
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
            progression
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
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
 * We will insert these pointers into the variants of unions. However note that
 * we can only compute the stack size of a union until we've looked at *all*
 * variants. Hence we perform an initial pass where we detect type loops, a
 * second pass where we compute the stack sizes of everything, and a third pass
 * where we actually compute the size of the heap allocations for unions.
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

impl DefinedType {
    /// Returns the number of monomorphs that are instantiated. Remember that
    /// during the type loop detection, and the memory layout phase we will
    /// pre-allocate monomorphs which are not yet fully laid out in memory.
    pub(crate) fn num_monomorphs(&self) -> usize {
        use DefinedTypeVariant as DTV;
        match &self.definition {
            DTV::Enum(def) => def.monomorphs.len(),
            DTV::Union(def) => def.monomorphs.len(),
            DTV::Struct(def) => def.monomorphs.len(),
            DTV::Function(_) | DTV::Component(_) => unreachable!(),
        }
    }
    /// Returns the index at which a monomorph occurs. Will only check the
    /// polymorphic arguments that are in use (none of the, in rust lingo,
    /// phantom types). If the type is not polymorphic and its memory has been
    /// layed out, then this will always return `Some(0)`.
    pub(crate) fn get_monomorph_index(&self, concrete_type: &ConcreteType) -> Option<usize> {
        use DefinedTypeVariant as DTV;

        // Helper to compare two types, while disregarding the polymorphic
        // variables that are not in use.
        let concrete_types_match = |type_a: &ConcreteType, type_b: &ConcreteType| -> bool {
            let mut a_iter = type_a.embedded_iter(0).enumerate();
            let mut b_iter = type_b.embedded_iter(0);

            while let Some((section_idx, a_section)) = a_iter.next() {
                let b_section = b_iter.next().unwrap();

                if !self.poly_vars[section_idx].is_in_use {
                    continue;
                }

                if a_section != b_section {
                    return false;
                }
            }

            return true;
        };

        // Check check if type is polymorphic to some degree at all
        if cfg!(debug_assertions) {
            if let ConcreteTypePart::Instance(definition_id, num_poly_args) = concrete_type.parts[0] {
                assert_eq!(definition_id, self.ast_definition);
                assert_eq!(num_poly_args as usize, self.poly_vars.len());
            } else {
                assert!(false, "concrete type {:?} is not a user-defined type", concrete_type);
            }
        }

        match &self.definition {
            DTV::Enum(definition) => {
                // Special case, enum is never a "true polymorph"
                debug_assert!(!self.is_polymorph);
                if definition.monomorphs.is_empty() {
                    return None
                } else {
                    return Some(0)
                }
            },
            DTV::Union(definition) => {
                for (monomorph_idx, monomorph) in definition.monomorphs.iter().enumerate() {
                    if concrete_types_match(&monomorph.concrete_type, concrete_type) {
                        return Some(monomorph_idx);
                    }
                }
            },
            DTV::Struct(definition) => {
                for (monomorph_idx, monomorph) in definition.monomorphs.iter().enumerate() {
                    if concrete_types_match(&monomorph.concrete_type, concrete_type) {
                        return Some(monomorph_idx);
                    }
                }
            },
            DTV::Function(_) | DTV::Component(_) => {
                unreachable!("called get_monomorph_index on a procedure type");
            }
        }

        // Nothing matched
        return None;
    }

    /// Retrieves size and alignment of the particular type's monomorph if it
    /// has been layed out in memory.
    pub(crate) fn get_monomorph_size_alignment(&self, idx: usize) -> Option<(usize, usize)> {
        use DefinedTypeVariant as DTV;
        let (size, alignment) = match &self.definition {
            DTV::Enum(def) => {
                debug_assert!(idx == 0);
                (def.size, def.alignment)
            },
            DTV::Union(def) => {
                let monomorph = &def.monomorphs[idx];
                (monomorph.stack_size, monomorph.stack_alignment)
            },
            DTV::Struct(def) => {
                let monomorph = &def.monomorphs[idx];
                (monomorph.size, monomorph.alignment)
            },
            DTV::Function(_) | DTV::Component(_) => {
                // Type table should never be able to arrive here during layout
                // of types. Types may only contain function prototypes.
                unreachable!("retrieving size and alignment of procedure type");
            }
        };

        if size == 0 && alignment == 0 {
            // The "marker" for when the type has not been layed out yet. Even
            // for zero-size types we will set alignment to `1` to simplify
            // alignment calculations.
            return None;
        } else {
            return Some((size, alignment));
        }
    }
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
    pub concrete_type: ConcreteType,
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
    pub tag_type: ConcreteType,
    pub tag_size: usize,
}

pub struct UnionVariant {
    pub identifier: Identifier,
    pub embedded: Vec<ParserType>, // zero-length does not have embedded values
    pub tag_value: i64,
}

pub struct UnionMonomorph {
    pub concrete_type: ConcreteType,
    pub variants: Vec<UnionMonomorphVariant>,
    // stack_size is the size of the union on the stack, includes the tag
    pub stack_size: usize,
    pub stack_alignment: usize,
    // heap_size contains the allocated size of the union in the case it
    // is used to break a type loop. If it is 0, then it doesn't require
    // allocation and lives entirely on the stack.
    pub heap_size: usize,
    pub heap_alignment: usize,
}

pub struct UnionMonomorphVariant {
    pub lives_on_heap: bool,
    pub embedded: Vec<UnionMonomorphEmbedded>,
}

pub struct UnionMonomorphEmbedded {
    pub concrete_type: ConcreteType,
    // Note that the meaning of the offset (and alignment) depend on whether or
    // not the variant lives on the stack/heap. If it lives on the stack then
    // they refer to the offset from the start of the union value (so the first
    // embedded type lives at a non-zero offset, because the union tag sits in
    // the front). If it lives on the heap then it refers to the offset from the
    // allocated memory region (so the first embedded type lives at a 0 offset).
    pub size: usize,
    pub alignment: usize,
    pub offset: usize,
}

/// `StructType` is a generic C-like struct type (or record type, or product
/// type) type.
pub struct StructType {
    pub fields: Vec<StructField>,
    pub monomorphs: Vec<StructMonomorph>,
}

pub struct StructField {
    pub identifier: Identifier,
    pub parser_type: ParserType,
}

pub struct StructMonomorph {
    pub concrete_type: ConcreteType,
    pub fields: Vec<StructMonomorphField>,
    pub size: usize,
    pub alignment: usize,
}

pub struct StructMonomorphField {
    pub concrete_type: ConcreteType,
    pub size: usize,
    pub alignment: usize,
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

struct TypeLoopBreadcrumb {
    definition_id: DefinitionId,
    monomorph_idx: usize,
    next_member: usize,
    next_embedded: usize, // for unions, the index into the variant's embedded types
}

#[derive(Debug, PartialEq, Eq)]
enum BreadcrumbResult {
    TypeExists,
    PushedBreadcrumb,
    TypeLoop(usize), // index into vec of breadcrumbs at which the type matched
}

enum MemoryLayoutResult {
    TypeExists(usize, usize), // (size, alignment)
    PushBreadcrumb(TypeLoopBreadcrumb),
}

// TODO: @Optimize, initial memory-unoptimized implementation
struct TypeLoopEntry {
    definition_id: DefinitionId,
    monomorph_idx: usize,
    is_union: bool,
}

struct TypeLoop {
    members: Vec<TypeLoopEntry>
}

pub struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Breadcrumbs left behind while trying to find type loops. Also used to
    /// determine sizes of types when all type loops are detected.
    breadcrumbs: Vec<TypeLoopBreadcrumb>,
    type_loops: Vec<TypeLoop>,
    encountered_types: Vec<TypeLoopEntry>,
}

impl TypeTable {
    /// Construct a new type table without any resolved types.
    pub(crate) fn new() -> Self {
        Self{ 
            lookup: HashMap::new(), 
            breadcrumbs: Vec::with_capacity(32),
            type_loops: Vec::with_capacity(8),
            encountered_types: Vec::with_capacity(32),
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
        let empty_concrete_type = ConcreteType{ parts: Vec::new() };
        for definition_idx in 0..ctx.heap.definitions.len() {
            let definition_id = ctx.heap.definitions.get_id(definition_idx);
            let poly_type = self.lookup.get(&definition_id).unwrap();

            // Here we explicitly want to instantiate types which have no
            // polymorphic arguments (even if it has phantom polymorphic
            // arguments) because otherwise the user will see very weird
            // error messages.
            if poly_type.poly_vars.is_empty() && poly_type.num_monomorphs() == 0 {
                self.detect_and_resolve_type_loops_for(
                    modules, ctx,
                    ConcreteType{
                        parts: vec![ConcreteTypePart::Instance(definition_id, 0)]
                    },
                )?;
                self.lay_out_memory_for_encountered_types(ctx);
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
    pub(crate) fn add_data_monomorph(
        &mut self, modules: &[Module], ctx: &PassCtx, definition_id: &DefinitionId, concrete_type: ConcreteType
    ) -> Result<i32, ParseError> {
        debug_assert_eq!(*definition_id, get_concrete_type_definition(&concrete_type));

        // Check if the monomorph already exists
        let poly_type = self.lookup.get_mut(definition_id).unwrap();
        if let Some(idx) = poly_type.get_monomorph_index(&concrete_type) {
            return idx as i32;
        }

        // Doesn't exist, so instantiate a monomorph and determine its memory
        // layout.
        self.detect_and_resolve_type_loops_for(modules, ctx, concrete_type)?;
        debug_assert_eq!(self.encountered_types[0].definition_id, definition_id);
        let mono_idx = self.encountered_types[0].monomorph_idx;
        self.lay_out_memory_for_encountered_types(ctx);

        return mono_idx as i32;
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
        let mut tag_counter = 0;
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
            tag_counter += 1;
        }

        let mut max_tag_value = 0;
        if tag_counter != 0 {
            max_tag_value = tag_counter - 1
        }

        let (tag_type, tag_size) = Self::variant_tag_type_from_values(0, max_tag_value);

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
                tag_type,
                tag_size,
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
    // Detecting type loops
    //--------------------------------------------------------------------------

    /// Internal function that will detect type loops and check if they're
    /// resolvable. If so then the appropriate union variants will be marked as
    /// "living on heap". If not then a `ParseError` will be returned
    fn detect_and_resolve_type_loops_for(&mut self, modules: &[Module], ctx: &PassCtx, concrete_type: ConcreteType) -> Result<(), ParseError> {
        use DefinedTypeVariant as DTV;

        debug_assert!(self.breadcrumbs.is_empty());
        debug_assert!(self.type_loops.is_empty());
        debug_assert!(self.encountered_types.is_empty());

        // Push the initial breadcrumb
        let _initial_result = self.push_breadcrumb_for_type_loops(get_concrete_type_definition(&concrete_type), &concrete_type);
        debug_assert_eq!(_initial_result, BreadcrumbResult::PushedBreadcrumb);

        // Enter into the main resolving loop
        while !self.breadcrumbs.is_empty() {
            let breadcrumb_idx = self.breadcrumbs.len() - 1;
            let breadcrumb = self.breadcrumbs.last_mut().unwrap();

            let poly_type = self.lookup.get(&breadcrumb.definition_id).unwrap();

            // TODO: Misuse of BreadcrumbResult enum?
            let resolve_result = match &poly_type.definition {
                DTV::Enum(_) => {
                    BreadcrumbResult::TypeExists
                },
                DTV::Union(definition) => {
                    let monomorph = &definition.monomorphs[breadcrumb.monomorph_idx];
                    let num_variants = monomorph.variants.len();

                    let mut union_result = BreadcrumbResult::TypeExists;

                    'member_loop: while breadcrumb.next_member < num_variants {
                        let mono_variant = &monomorph.variants[breadcrumb.next_member];
                        let num_embedded = mono_variant.embedded.len();

                        while breadcrumb.next_embedded < num_embedded {
                            let mono_embedded = &mono_variant.embedded[breadcrumb.next_embedded];
                            union_result = self.push_breadcrumb_for_type_loops(poly_type.ast_definition, &mono_embedded.concrete_type);

                            if union_result != BreadcrumbResult::TypeExists {
                                // In type loop or new breadcrumb pushed, so
                                // break out of the resolving loop
                                break 'member_loop;
                            }

                            breadcrumb.next_embedded += 1;
                        }

                        breadcrumb.next_embedded = 0;
                        breadcrumb.next_member += 1
                    }

                    union_result
                },
                DTV::Struct(definition) => {
                    let monomorph = &definition.monomorphs[breadcrumb.monomorph_idx];
                    let num_fields = monomorph.fields.len();

                    let mut struct_result = BreadcrumbResult::TypeExists;
                    while breadcrumb.next_member < num_fields {
                        let mono_field = &monomorph.fields[breadcrumb.next_member];
                        struct_result = self.push_breadcrumb_for_type_loops(poly_type.ast_definition, &mono_field.concrete_type);

                        if struct_result != BreadcrumbResult::TypeExists {
                            // Type loop or breadcrumb pushed, so break out of
                            // the resolving loop
                            break;
                        }

                        breadcrumb.next_member += 1;
                    }

                    struct_result
                },
                DTV::Function(_) | DTV::Component(_) => unreachable!(),
            };

            // Handle the result of attempting to resolve the current breadcrumb
            match resolve_result {
                BreadcrumbResult::TypeExists => {
                    // We finished parsing the type
                    self.breadcrumbs.pop();
                },
                BreadcrumbResult::PushedBreadcrumb => {
                    // We recurse into the member type, since the breadcrumb is
                    // already pushed we don't take any action here.
                },
                BreadcrumbResult::TypeLoop(first_idx) => {
                    // We're in a type loop. Add the type loop
                    let mut loop_members = Vec::with_capacity(self.breadcrumbs.len() - first_idx);
                    for breadcrumb_idx in first_idx..self.breadcrumbs.len() {
                        let breadcrumb = &mut self.breadcrumbs[breadcrumb_idx];
                        let mut is_union = false;

                        let entry = self.lookup.get_mut(&breadcrumb.definition_id).unwrap();
                        match &mut entry.definition {
                            DTV::Union(definition) => {
                                // Mark the currently processed variant as requiring heap
                                // allocation, then advance the *embedded* type. The loop above
                                // will then take care of advancing it to the next *member*.
                                let monomorph = &mut definition.monomorphs[breadcrumb.monomorph_idx];
                                let variant = &mut monomorph.variants[breadcrumb.next_member];
                                variant.lives_on_heap = true;
                                breadcrumb.next_embedded += 1;
                                is_union = true;
                            },
                            _ => {}, // else: we don't care for now
                        }

                        loop_members.push(TypeLoopEntry{
                            definition_id: breadcrumb.definition_id,
                            monomorph_idx: breadcrumb.monomorph_idx,
                            is_union
                        });
                    }

                    self.type_loops.push(TypeLoop{ members: loop_members });
                }
            }
        }

        // All breadcrumbs have been cleared. So now `type_loops` contains all
        // of the encountered type loops, and `encountered_types` contains a
        // list of all unique monomorphs we encountered.

        // The next step is to figure out if all of the type loops can be
        // broken. A type loop can be broken if at least one union exists in the
        // loop and that union ended up having variants that are not part of
        // a type loop.
        fn type_loop_source_span_and_message<'a>(
            modules: &'a [Module], ctx: &PassCtx, defined_type: &DefinedType, monomorph_idx: usize, index_in_loop: usize
        ) -> (&'a InputSource, InputSpan, String) {
            // Note: because we will discover the type loop the *first* time we
            // instantiate a monomorph with the provided polymorphic arguments
            // (not all arguments are actually used in the type). We don't have
            // to care about a second instantiation where certain unused
            // polymorphic arguments are different.
            let monomorph_type = match &defined_type.definition {
                DTV::Union(definition) => &definition.monomorphs[monomorph_idx].concrete_type,
                DTV::Struct(definition) => &definition.monomorphs[monomorph_idx].concrete_type,
                DTV::Enum(_) | DTV::Function(_) | DTV::Component(_) =>
                    unreachable!(), // impossible to have an enum/procedure in a type loop
            };

            let type_name = monomorph_type.display_name(&ctx.heap);
            let message = if index_in_loop == 0 {
                format!(
                    "encountered an infinitely large type for '{}' (which can be fixed by \
                    introducing a union type that has a variant whose embedded types are \
                    not part of a type loop, or do not have embedded types)",
                    type_name
                )
            } else if index_in_loop == 1 {
                format!("because it depends on the type '{}'", type_name)
            } else {
                format!("which depends on the type '{}'", type_name)
            };

            let ast_definition = &ctx.heap[defined_type.ast_definition];
            let ast_root_id = ast_definition.defined_in();

            return (
                &modules[ast_root_id.index as usize].source,
                ast_definition.identifier().span,
                message
            );
        }

        for type_loop in &self.type_loops {
            let mut can_be_broken = false;
            debug_assert!(!type_loop.members.is_empty());

            for entry in &type_loop.members {
                if entry.is_union {
                    let base_type = self.lookup.get(&entry.definition_id).unwrap();
                    let monomorph = &base_type.definition.as_union().monomorphs[entry.monomorph_idx];

                    debug_assert!(!monomorph.variants.is_empty()); // otherwise it couldn't be part of the type loop
                    let has_stack_variant = monomorph.variants.iter().any(|variant| !variant.lives_on_heap);
                    if has_stack_variant {
                        can_be_broken = true;
                    }
                }
            }

            if !can_be_broken {
                // Construct a type loop error
                let first_entry = &type_loop.members[0];
                let first_type = self.lookup.get(&first_entry.definition_id).unwrap();
                let (first_module, first_span, first_message) = type_loop_source_span_and_message(
                    modules, ctx, first_type, first_entry.monomorph_idx, 0
                );
                let mut parse_error = ParseError::new_error_at_span(first_module, first_span, first_message);

                for member_idx in 1..type_loop.members.len() {
                    let entry = &type_loop.members[member_idx];
                    let entry_type = self.lookup.get(&first_entry.definition_id).unwrap();
                    let (module, span, message) = type_loop_source_span_and_message(
                        modules, ctx, entry_type, entry.monomorph_idx, member_idx
                    );
                    parse_error = parse_error.with_info_at_span(module, span, message);
                }

                return Err(parse_error);
            }
        }

        // If here, then all type loops have been resolved and we can lay out
        // all of the members
        self.type_loops.clear();

        return Ok(());
    }

    // TODO: Pass in definition_type by value?
    fn push_breadcrumb_for_type_loops(&mut self, definition_id: DefinitionId, definition_type: &ConcreteType) -> BreadcrumbResult {
        use DefinedTypeVariant as DTV;

        let mut base_type = self.lookup.get_mut(&definition_id).unwrap();
        if let Some(mono_idx) = base_type.get_monomorph_index(&definition_type) {
            // Monomorph is already known. Check if it is present in the
            // breadcrumbs. If so, then we are in a type loop
            for (breadcrumb_idx, breadcrumb) in self.breadcrumbs.iter().enumerate() {
                if breadcrumb.definition_id == definition_id && breadcrumb.monomorph_idx == mono_idx {
                    return BreadcrumbResult::TypeLoop(breadcrumb_idx);
                }
            }

            return BreadcrumbResult::TypeExists;
        }

        // Type is not yet known, so we need to insert it into the lookup and
        // push a new breadcrumb.
        let mut is_union = false;
        let monomorph_idx = match &mut base_type.definition {
            DTV::Enum(definition) => {
                debug_assert!(definition.monomorphs.is_empty());
                definition.monomorphs.push(EnumMonomorph{
                    concrete_type: definition_type.clone(),
                });
                0
            },
            DTV::Union(definition) => {
                // Create all the variants with their concrete types
                let mut mono_variants = Vec::with_capacity(definition.variants.len());
                for poly_variant in &definition.variants {
                    let mut mono_embedded = Vec::with_capacity(poly_variant.embedded.len());
                    for poly_embedded in &poly_variant.embedded {
                        let mono_concrete = Self::construct_concrete_type(poly_embedded, definition_type);
                        mono_embedded.push(UnionMonomorphEmbedded{
                            concrete_type: mono_concrete,
                            size: 0,
                            alignment: 0,
                            offset: 0
                        });
                    }

                    mono_variants.push(UnionMonomorphVariant{
                        lives_on_heap: false,
                        embedded: mono_embedded,
                    })
                }

                let mono_idx = definition.monomorphs.len();
                definition.monomorphs.push(UnionMonomorph{
                    concrete_type: definition_type.clone(),
                    variants: mono_variants,
                    stack_size: 0,
                    stack_alignment: 0,
                    heap_size: 0,
                    heap_alignment: 0
                });

                is_union = true;
                mono_idx
            },
            DTV::Struct(definition) => {
                let mut mono_fields = Vec::with_capacity(definition.fields.len());
                for poly_field in &definition.fields {
                    let mono_concrete = Self::construct_concrete_type(&poly_field.parser_type, definition_type);
                    mono_fields.push(StructMonomorphField{
                        concrete_type: mono_concrete,
                        size: 0,
                        alignment: 0,
                        offset: 0
                    })
                }

                let mono_idx = definition.monomorphs.len();
                definition.monomorphs.push(StructMonomorph{
                    concrete_type: definition_type.clone(),
                    fields: mono_fields,
                    size: 0,
                    alignment: 0
                });

                mono_idx
            },
            DTV::Function(_) | DTV::Component(_) => {
                unreachable!("pushing type resolving breadcrumb for procedure type")
            },
        };

        self.breadcrumbs.push(TypeLoopBreadcrumb{
            definition_id,
            monomorph_idx,
            next_member: 0,
            next_embedded: 0
        });

        // With the breadcrumb constructed we know this is a new type, so we
        // also need to add an entry in the list of all encountered types
        self.encountered_types.push(TypeLoopEntry{
            definition_id,
            monomorph_idx,
            is_union,
        });

        return BreadcrumbResult::PushedBreadcrumb;
    }

    /// Constructs a concrete type out of a parser type for a struct field or
    /// union embedded type. It will do this by looking up the polymorphic
    /// variables in the supplied concrete type. The assumption is that the
    /// polymorphic variable's indices correspond to the subtrees in the
    /// concrete type.
    fn construct_concrete_type(member_type: &ParserType, container_type: &ConcreteType) -> ConcreteType {
        use ParserTypeVariant as PTV;
        use ConcreteTypePart as CTP;

        // TODO: Combine with code in pass_typing.rs
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

        let mut parts = Vec::with_capacity(member_type.elements.len()); // usually a correct estimation, might not be
        for member_part in &member_type.elements {
            // Check if we have a regular builtin type
            if let Some(part) = parser_to_concrete_part(&member_part.variant) {
                parts.push(part);
                continue;
            }

            // Not builtin, but if all code is working correctly, we only care
            // about the polymorphic argument at this point.
            if let PTV::PolymorphicArgument(_container_definition_id, poly_arg_idx) = member_part.variant {
                debug_assert_eq!(_container_definition_id, get_concrete_type_definition(container_type));

                let mut container_iter = container_type.embedded_iter(0);
                for _ in 0..poly_arg_idx {
                    container_iter.next();
                }

                let poly_section = container_iter.next().unwrap();
                parts.extend(poly_section);

                continue;
            }

            unreachable!("unexpected type part {:?} from {:?}", member_part, member_type);
        }

        return ConcreteType{ parts };
    }

    //--------------------------------------------------------------------------
    // Determining memory layout for types
    //--------------------------------------------------------------------------

    fn lay_out_memory_for_encountered_types(&mut self, ctx: &PassCtx) {
        use DefinedTypeVariant as DTV;

        // Just finished type loop detection, so we're left with the encountered
        // types only
        debug_assert!(self.breadcrumbs.is_empty());
        debug_assert!(self.type_loops.is_empty());
        debug_assert!(!self.encountered_types.is_empty());

        // Push the first entry (the type we originally started with when we
        // were detecting type loops)
        let first_entry = &self.encountered_types[0];
        self.breadcrumbs.push(TypeLoopBreadcrumb{
            definition_id: first_entry.definition_id,
            monomorph_idx: first_entry.monomorph_idx,
            next_member: 0,
            next_embedded: 0,
        });

        // Enter the main resolving loop
        'breadcrumb_loop: while !self.breadcrumbs.is_empty() {
            let breadcrumb_idx = self.breadcrumbs.len() - 1;
            let breadcrumb = &mut self.breadcrumbs[breadcrumb_idx];

            let poly_type = self.lookup.get_mut(&breadcrumb.definition_id).unwrap();
            match &mut poly_type.definition {
                DTV::Enum(definition) => {
                    // Size should already be computed
                    debug_assert!(definition.size != 0 && definition.alignment != 0);
                },
                DTV::Union(definition) => {
                    // Retrieve size/alignment of each embedded type. We do not
                    // compute the offsets or total type sizes yet.
                    let mono_type = &mut definition.monomorphs[breadcrumb.monomorph_idx];
                    let num_variants = mono_type.variants.len();
                    while breadcrumb.next_member < num_variants {
                        let mono_variant = &mut mono_type.variants[breadcrumb.next_member];

                        if mono_variant.lives_on_heap {
                            // To prevent type loops we made this a heap-
                            // allocated variant. This implies we cannot
                            // compute sizes of members at this point.
                        } else {
                            let num_embedded = mono_variant.embedded.len();
                            while breadcrumb.next_embedded < num_embedded {
                                let mono_embedded = &mut mono_variant.embedded[breadcrumb.next_embedded];
                                match self.get_memory_layout_or_breadcrumb(ctx, &mono_embedded.concrete_type) {
                                    MemoryLayoutResult::TypeExists(size, alignment) => {
                                        mono_embedded.size = size;
                                        mono_embedded.alignment = alignment;
                                    },
                                    MemoryLayoutResult::PushBreadcrumb(new_breadcrumb) => {
                                        self.breadcrumbs.push(new_breadcrumb);
                                        continue 'breadcrumb_loop;
                                    }
                                }

                                breadcrumb.next_embedded += 1;
                            }
                        }

                        breadcrumb.next_member += 1;
                        breadcrumb.next_embedded = 0;
                    }

                    // If here then we can at least compute the stack size of
                    // the type, we'll have to come back at the very end to
                    // fill in the heap size/alignment/offset of each heap-
                    // allocated variant.
                    let mut max_size = definition.tag_size;
                    let mut max_alignment = definition.tag_size;

                    for variant in &mut mono_type.variants {
                        // We're doing stack computations, so always start with
                        // the tag size/alignment.
                        let mut variant_offset = definition.tag_size;
                        let mut variant_alignment = definition.tag_size;

                        if variant.lives_on_heap {
                            // Variant lives on heap, so just a pointer
                            let (ptr_size, ptr_align) = ctx.arch.pointer_size_alignment;
                            align_offset_to(&mut variant_offset, ptr_align);

                            variant_offset += ptr_size;
                            variant_alignment = variant_alignment.max(ptr_align);
                        } else {
                            // Variant lives on stack, so walk all embedded
                            // types.
                            for embedded in &mut variant.embedded {
                                align_offset_to(&mut variant_offset, embedded.alignment);
                                embedded.offset = variant_offset;

                                variant_offset += embedded.size;
                                variant_alignment = variant_alignment.max(embedded.alignment);
                            }
                        };

                        max_size = max_size.max(variant_offset);
                        max_alignment = max_alignment.max(variant_alignment);
                    }

                    mono_type.stack_size = max_size;
                    mono_type.stack_alignment = max_alignment;
                },
                DTV::Struct(definition) => {
                    // Retrieve size and alignment of each struct member. We'll
                    // compute the offsets once all of those are known
                    let mono_type = &mut definition.monomorphs[breadcrumb.monomorph_idx];
                    let num_fields = mono_type.fields.len();
                    while breadcrumb.next_member < num_fields {
                        let mono_field = &mut mono_type.fields[breadcrumb.next_member];

                        match self.get_memory_layout_or_breadcrumb(ctx, &mono_field.concrete_type) {
                            MemoryLayoutResult::TypeExists(size, alignment) => {
                                mono_field.size = size;
                                mono_field.alignment = alignment;
                            },
                            MemoryLayoutResult::PushBreadcrumb(new_breadcrumb) => {
                                self.breadcrumbs.push(new_breadcrumb);
                                continue 'breadcrumb_loop;
                            },
                        }

                        breadcrumb.next_member += 1;
                    }

                    // Compute offsets and size of total type
                    let mut cur_offset = 0;
                    let mut max_alignment = 1;
                    for field in &mut mono_type.fields {
                        align_offset_to(&mut cur_offset, field.alignment);
                        field.offset = cur_offset;

                        cur_offset += field.size;
                        max_alignment = max_alignment.max(field.alignment);
                    }

                    mono_type.size = cur_offset;
                    mono_type.alignment = max_alignment;
                },
                DTV::Function(_) | DTV::Component(_) => {
                    unreachable!();
                }
            }

            // If here, then we completely layed out the current type. So move
            // to the next breadcrumb
            self.breadcrumbs.pop();
        }

        // If here then all types have been layed out. What remains is to
        // compute the sizes/alignment/offsets of the heap variants of the
        // unions we have encountered.
        for entry in &self.encountered_types {
            if !entry.is_union {
                continue;
            }

            let poly_type = self.lookup.get_mut(&entry.definition_id).unwrap();
            match &mut poly_type.definition {
                DTV::Union(definition) => {
                    let mono_type = &mut definition.monomorphs[entry.monomorph_idx];
                    let mut max_size = 0;
                    let mut max_alignment = 1;

                    for variant in &mut mono_type.variants {
                        if !variant.lives_on_heap {
                            continue;
                        }

                        let mut variant_offset = 0;
                        let mut variant_alignment = 1;
                        debug_assert!(!variant.embedded.is_empty());

                        for embedded in &mut variant.embedded {
                            match self.get_memory_layout_or_breadcrumb(ctx, &embedded.concrete_type) {
                                MemoryLayoutResult::TypeExists(size, alignment) => {
                                    embedded.size = size;
                                    embedded.alignment = alignment;

                                    align_offset_to(&mut variant_offset, alignment);
                                    embedded.alignment = variant_offset;

                                    variant_offset += size;
                                    variant_alignment = variant_alignment.max(alignment);
                                },
                                _ => unreachable!(),
                            }
                        }

                        // Update heap size/alignment
                        max_size = max_size.max(variant_offset);
                        max_alignment = max_alignment.max(variant_alignment);
                    }

                    if max_size != 0 {
                        // At least one entry lives on the heap
                        mono_type.heap_size = max_size;
                        mono_type.heap_alignment = max_alignment;
                    }
                },
                _ => unreachable!(),
            }
        }

        // And now, we're actually, properly, done
        self.encountered_types.clear();
    }

    fn get_memory_layout_or_breadcrumb(&self, ctx: &PassCtx, concrete_type: &ConcreteType) -> MemoryLayoutResult {
        use ConcreteTypePart as CTP;

        // Before we do any fancy shenanigans, we need to check if the concrete
        // type actually requires laying out memory.
        debug_assert!(!concrete_type.parts.is_empty());
        let (builtin_size, builtin_alignment) = match concrete_type.parts[0] {
            CTP::Void   => (0, 1),
            CTP::Message => ctx.arch.array_size_alignment,
            CTP::Bool   => (1, 1),
            CTP::UInt8  => (1, 1),
            CTP::UInt16 => (2, 2),
            CTP::UInt32 => (4, 4),
            CTP::UInt64 => (8, 8),
            CTP::SInt8  => (1, 1),
            CTP::SInt16 => (2, 2),
            CTP::SInt32 => (4, 4),
            CTP::SInt64 => (8, 8),
            CTP::Character => (4, 4),
            CTP::String => ctx.arch.string_size_alignment,
            CTP::Array => ctx.arch.array_size_alignment,
            CTP::Slice => ctx.arch.array_size_alignment,
            CTP::Input => ctx.arch.port_size_alignment,
            CTP::Output => ctx.arch.port_size_alignment,
            CTP::Instance(definition_id, _) => {
                // Special case where we explicitly return to simplify the
                // return case for the builtins.
                let entry = self.lookup.get(&definition_id).unwrap();
                let monomorph_idx = entry.get_monomorph_index(concrete_type).unwrap();

                if let Some((size, alignment)) = entry.get_monomorph_size_alignment(monomorph_idx) {
                    // Type has been layed out in memory
                    return MemoryLayoutResult::TypeExists(size, alignment);
                } else {
                    return MemoryLayoutResult::PushBreadcrumb(TypeLoopBreadcrumb {
                        definition_id,
                        monomorph_idx,
                        next_member: 0,
                        next_embedded: 0,
                    });
                }
            }
        };

        return MemoryLayoutResult::TypeExists(builtin_size, builtin_alignment);
    }

    /// Returns tag concrete type (always a builtin integer type), the size of
    /// that type in bytes (and implicitly, its alignment)
    fn variant_tag_type_from_values(min_val: i64, max_val: i64) -> (ConcreteType, usize) {
        debug_assert!(min_val <= max_val);

        let (part, size) = if min_val >= 0 {
            // Can be an unsigned integer
            if max_val <= (u8::MAX as i64) {
                (ConcreteTypePart::UInt8, 1)
            } else if max_val <= (u16::MAX as i64) {
                (ConcreteTypePart::UInt16, 2)
            } else if max_val <= (u32::MAX as i64) {
                (ConcreteTypePart::UInt32, 4)
            } else {
                (ConcreteTypePart::UInt64, 8)
            }
        } else {
            // Must be a signed integer
            if min_val >= (i8::MIN as i64) && max_val <= (i8::MAX as i64) {
                (ConcreteTypePart::SInt8, 1)
            } else if min_val >= (i16::MIN as i64) && max_val <= (i16::MAX as i64) {
                (ConcreteTypePart::SInt16, 2)
            } else if min_val >= (i32::MIN as i64) && max_val <= (i32::MAX as i64) {
                (ConcreteTypePart::SInt32, 4)
            } else {
                (ConcreteTypePart::SInt64, 8)
            }
        };

        return (ConcreteType{ parts: vec![part] }, size);
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

#[inline] fn align_offset_to(offset: &mut usize, alignment: usize) {
    debug_assert!(alignment > 0);
    let alignment_min_1 = alignment - 1;
    *offset += alignment_min_1;
    *offset &= !(alignment_min_1);
}

#[inline] fn get_concrete_type_definition(concrete: &ConcreteType) -> DefinitionId {
    if let ConcreteTypePart::Instance(definition_id, _) = concrete.parts[0] {
        return definition_id;
    } else {
        debug_assert!(false, "passed {:?} to the type table", concrete);
        return DefinitionId::new_invalid()
    }
}
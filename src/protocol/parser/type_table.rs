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

impl DefinedType {
    /// Checks if the monomorph at the specified index has polymorphic arguments
    /// that match the provided polymorphic arguments. Only the polymorphic
    /// variables that are in use by the type are checked.
    pub(crate) fn monomorph_matches_poly_args(&self, monomorph_idx: usize, checking_poly_args: &[ConcreteType]) -> bool {
        use DefinedTypeVariant as DTV;

        debug_assert!(self.poly_vars.len(), checking_poly_args.len());
        if !self.is_polymorph {
            return true;
        }

        let existing_poly_args = match &self.definition {
            DTV::Enum(def) => &def.monomorphs[monomorph_idx].poly_args,
            DTV::Union(def) => &def.monomorphs[monomorph_idx].poly_args,
            DTV::Struct(def) => &def.monomorphs[monomorph_idx].poly_args,
            DTV::Function(def) => &def.monomorphs[monomorph_idx].poly_args,
            DTV::Component(def) => &def.monomorphs[monomorph_idx].poly_args
        };

        for poly_idx in 0..num_args {
            let poly_var = &self.poly_vars[poly_idx];
            if !poly_var.is_in_use {
                continue;
            }

            let existing_arg = &existing_poly_args[poly_idx];
            let checking_arg = &checking_poly_args[poly_idx];
            if existing_arg != checking_arg {
                return false;
            }
        }

        return true;
    }

    /// Checks if a monomorph with the provided polymorphic arguments already
    /// exists. The polymorphic variables that are not in use by the type will
    /// not contribute to the checking (e.g. an `enum` type will always return
    /// `true`, because it cannot embed its polymorphic variables). Returns the
    /// index of the matching monomorph.
    pub(crate) fn has_monomorph(&self, poly_args: &[ConcreteType]) -> Option<usize> {
        use DefinedTypeVariant as DTV;

        // Quick check if type is polymorphic at all
        let num_args = poly_args.len();
        let num_monomorphs = match &self.definition {
            DTV::Enum(def) => def.monomorphs.len(),
            DTV::Union(def) => def.monomorphs.len(),
            DTV::Struct(def) => def.monomorphs.len(),
            DTV::Function(def) => def.monomorphs.len(),
            DTV::Component(def) => def.monomorphs.len(),
        };

        debug_assert_eq!(self.poly_vars.len(), num_args);
        if !self.is_polymorph {
            debug_assert!(num_monomorphs <= 1);
            if num_monomorphs == 0 {
                return None;
            } else {
                return Some(0);
            }
        }

        for monomorph_idx in 0..num_monomorphs {
            if self.monomorph_matches_poly_args(monomorph_idx, poly_args) {
                return Some(monomorph_idx);
            }
        }

        return None;
    }

    /// Retrieves size and alignment of the particular type's monomorph
    pub(crate) fn get_monomorph_size_alignment(&self, idx: usize) -> (usize, usize) {
        use DefinedTypeVariant as DTV;
        match &self.definition {
            DTV::Enum(def) => {
                debug_assert!(idx == 0);
                (def.size, def.alignment)
            },
            DTV::Union(def) => {
                let monomorph = &def.monomorphs[idx];
                // TODO: @Revise, probably not what I want
                (monomorph.stack_byte_size, monomorph.alignment)
            },
            DTV::Struct(def) => {
                let monomorph = &def.monomorphs[idx];
                (monomorph.size, monomorph.alignment)
            },
            DTV::Function(_) | DTV::Component(_) => {
                // Type table should never be able to arrive here during layout
                // of types. Types may only contain function prototypes.
                unreachable!("retrieving size and alignment of {:?}", self);
            }
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
    pub tag_type: ConcreteType,
    pub tag_size: usize,
}

pub struct UnionVariant {
    pub identifier: Identifier,
    pub embedded: Vec<ParserType>, // zero-length does not have embedded values
    pub tag_value: i64,
}

pub struct UnionMonomorph {
    pub poly_args: Vec<ConcreteType>,
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
    pub poly_args: Vec<ConcreteType>,
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
    PushBreadcrumb(DefinitionId, Vec<ConcreteType>),
    TypeLoop(usize),
}

enum LookupResult {
    Exists(ConcreteType, usize, usize), // type was looked up and exists (or is a builtin). Also returns size and alignment
    Missing(DefinitionId, Vec<ConcreteType>), // type was looked up and doesn't exist, vec contains poly args
    TypeLoop(usize), // type is already in the breadcrumbs, index points into the matching breadcrumb
}

macro_rules! unwrap_lookup_result {
    ($result:expr) => {
        match $result {
            LookupResult::Exists(a, b, c) => (a, b, c)
            LookupResult::Missing(a, b) => return LayoutResult::PushBreadcrumb(a, b),
            LookupResult::TypeLoop(a) => return LayoutResult::TypeLoop(a),
        }
    }
}

pub struct TypeTable {
    /// Lookup from AST DefinitionId to a defined type. Considering possible
    /// polymorphs is done inside the `DefinedType` struct.
    lookup: HashMap<DefinitionId, DefinedType>,
    /// Breadcrumbs left behind while resolving embedded types
    breadcrumbs: Vec<LayoutBreadcrumb>,
}

impl TypeTable {
    /// Construct a new type table without any resolved types.
    pub(crate) fn new() -> Self {
        Self{ 
            lookup: HashMap::new(), 
            breadcrumbs: Vec::with_capacity(32),
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
        let mut fake_poly_args = Vec::with_capacity(8);

        for definition_idx in 0..ctx.heap.definitions.len() {
            let definition_id = ctx.heap.definitions.get_id(definition_idx);
            let base_type = self.lookup.get(&definition_id).unwrap();

            if !base_type.is_polymorph {
                if base_type.poly_vars.len() != 0 {
                    // TODO: @Rewrite, I really don't like this, there should be something living
                    //       underneath this code that is cleaner.
                    fake_poly_args.clear();
                    for _ in 0..base_type.poly_vars.len() {
                        fake_poly_args.push(ConcreteType{ parts: vec![ConcreteTypePart::UInt8] });
                    }
                }

                if !base_type.has_monomorph(&fake_poly_args) {
                    self.push_breadcrumb_for_definition(ctx, definition_id, fake_poly_args.clone());
                }

                while !self.breadcrumbs.is_empty() {
                    match self.layout_for_top_breadcrumb(modules, ctx)? {
                        LayoutResult::PopBreadcrumb => {
                            self.breadcrumbs.pop()
                        },
                        LayoutResult::PushBreadcrumb(definition_id, poly_args) => {
                            self.push_breadcrumb_for_definition(ctx, definition_id, poly_args);
                        },
                        LayoutResult::TypeLoop(breadcrumb_idx) => {

                        }
                    }
                }
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
    // Determining memory layout for types
    //--------------------------------------------------------------------------

    #[inline] pub(crate) fn layout_for_top_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        use DefinedTypeVariant as DTV;

        debug_assert!(!self.breadcrumbs.is_empty());
        let top_definition_id = self.breadcrumbs.last().unwrap().definition_id;
        match self.lookup.get(&top_definition_id).unwrap().definition {
            DTV::Enum(_) => self.layout_for_top_enum_breadcrumb(modules, ctx),
            DTV::Union(_) => self.layout_for_top_union_breadcrumb(modules, ctx),
            DTV::Struct(_) => self.layout_for_top_struct_breadcrumb(modules, ctx),
            DTV::Function(_) => self.layout_for_top_function_breadcrumb(modules, ctx),
            DTV::Component(_) => self.layout_for_top_component_breadcrumb(modules, ctx),
        }
    }

    pub(crate) fn layout_for_top_enum_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        // Enums are a bit special, because they never use their polymorphic
        // variables. So they only have to be layed out once. But this is
        // already done when laying out the base type.
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let definition = self.lookup.get_mut(&breadcrumb.definition_id).unwrap().definition.as_enum_mut();

        debug_assert!(definition.monomorphs.len() == 1 && breadcrumb.monomorph_idx == 0);

        return Ok(LayoutResult::PopBreadcrumb);
    }

    pub(crate) fn layout_for_top_union_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        // Resolve each variant and embedded type until all sizes can be
        // computed.
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let definition = self.lookup.get_mut(&breadcrumb.definition_id).unwrap();
        debug_assert!(definition.poly_vars.len() == poly_args.len() || !definition.is_polymorph);
        let type_poly = definition.definition.as_union_mut();
        let type_mono = &mut type_poly.monomorphs[breadcrumb.monomorph_idx];

        let num_variants = type_poly.variants.len();
        while breadcrumb.next_member < num_variants {
            let poly_variant = &type_poly.variants[breadcrumb.next_member];
            let mono_variant = &mut type_mono.variants[breadcrumb.next_member];
            let num_embedded = poly_variant.embedded.len();

            while breadcrumb.next_embedded < num_embedded {
                let mono_embedded = &mut mono_variant.embedded[breadcrumb.next_embedded];
                let poly_embedded = &poly_variant.embedded[breadcrumb.next_embedded];
                let (concrete_type, size, alignment) = unwrap_lookup_result!(
                    self.lookup_layout_for_member_type(poly_embedded, &type_mono.poly_args)
                );

                mono_embedded.concrete_type = concrete_type;
                mono_embedded.size = size;
                mono_embedded.alignment = alignment;

                breadcrumb.next_embedded += 1;
            }

            breadcrumb.next_embedded = 0;
            breadcrumb.next_member += 1;
        }

        // If here then each variant, and each therein embedded type, has been
        // found and resolved. But during repeated iteration we might have
        // encountered a type loop. This will have forced some union members to
        // be heap allocated.

        let mut stack_size = type_poly.tag_size;
        let mut stack_alignment = type_poly.tag_size;
        let mut has_stack_variant = false;

        let mut heap_size = 0;
        let mut heap_alignment = 1;

        // Go through all variants and apply their alignment/offsets to compute
        // the stack and heap size.
        for variant in &mut type_mono.variants {
            let mut variant_offset = 0;
            let mut variant_alignment = 1;

            if !variant.lives_on_heap {
                variant_offset = type_poly.tag_size;
                variant_alignment = type_poly.tag_size;
            }

            for embedded in &mut variant.embedded {
                align_offset_to(&mut variant_offset, embedded.alignment);
                embedded.offset = variant_offset;

                variant_offset += embedded.size;
                variant_alignment = variant_alignment.max(embedded.alignment);
            }

            let variant_size = variant_offset;

            if variant.lives_on_heap {
                heap_size = heap_size.max(variant_size);
                heap_alignment = heap_alignment.max(heap_alignment);
            } else {
                stack_size = stack_size.max(variant_size);
                stack_alignment = stack_alignment.max(variant_alignment);
                has_stack_variant = true;
            }
        }

        // Store the computed sizes
        type_mono.stack_size = stack_size;
        type_mono.stack_alignment = stack_alignment;
        type_mono.heap_size = heap_size;
        type_mono.heap_alignment = heap_alignment;

        if type_mono.heap_size != 0 && !has_stack_variant {
            // Union was part of a type loop, because we have variants that live
            // on the heap. But we do not have any stack variants.
            // TODO: Think about this again, we only need one union to be
            //       non-infinite per type loop. But we may encounter multiple
            //       type loops per layed out type. So keep a vec of sets for
            //       each loop? Then at least one in each set should have a
            //       stack variant?
            let source = &modules[definition.ast_root.index as usize].source;
            return Err(ParseError::new_error_str_at_span(
                source, ctx.heap[definition.ast_definition].identifier().span,
                "This union is part of an infinite type, so should contain a variant that do not cause type loops"
            ));
        }

        return Ok(LayoutResult::PopBreadcrumb);
    }

    pub(crate) fn layout_for_top_struct_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        // Resolve each of the struct fields
        let breadcrumb = self.breadcrumbs.last_mut().unwrap();
        let definition = self.lookup.get_mut(&breadcrumb.definition_id).unwrap();
        debug_assert!(definition.poly_vars.len() == poly_args.len() || !definition.is_polymorph);
        let type_poly = definition.definition.as_struct_mut();
        let type_mono = &mut type_poly.monomorphs[breadcrumb.monomorph_idx];

        let num_members = type_poly.fields.len();
        while breadcrumb.next_member < num_members {
            let poly_member = &type_poly.fields[breadcrumb.next_member];
            let mono_member = &mut type_mono.fields[breadcrumb.next_member];

            let (concrete_type, size, alignment) = unwrap_lookup_result!(
                self.lookup_layout_for_member_type(&poly_member.parser_type, &type_mono.poly_args)
            );

            mono_member.concrete_type = concrete_type;
            mono_member.size = size;
            mono_member.alignment = alignment;

            breadcrumb.next_member += 1;
        }

        // All fields are resolved
        let mut offset = 0;
        let mut alignment = 1;

        for field in &mut type_mono.fields {
            align_offset_to(&mut offset, field.alignment);

            field.offset = offset;

            offset += field.size;
            alignment = alignment.max(field.alignment);
        }

        type_mono.size = offset;
        type_mono.alignment = alignment;

        return Ok(LayoutResult::PopBreadcrumb);
    }

    pub(crate) fn layout_for_top_function_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        unreachable!("ended up laying out function, breadcrumbs are: {:?}", self.breadcrumbs);
    }

    pub(crate) fn layout_for_top_component_breadcrumb(&mut self, modules: &[Module], ctx: &mut PassCtx) -> Result<LayoutResult, ParseError> {
        unreachable!("ended up layout out component, breadcrumbs are: {:?}", self.breadcrumbs);
    }

    /// Returns tag concrete type (always a builtin integer type), the size of
    /// that type in bytes (and implicitly, its alignment)
    pub(crate) fn variant_tag_type_from_values(min_val: i64, max_val: i64) -> (ConcreteType, usize) {
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
            } else if min_val >= (i16::MIN as i64) && max_Val <= (i16::MAX as i64) {
                (ConcreteTypePart::SInt16, 2)
            } else if min_val >= (i32::MIN as i64) && max_val <= (i32::MAX as i64) {
                (ConcreteTypePart::SInt32, 4)
            } else {
                (ConcreteTypePart::SInt64, 8)
            }
        };

        return (ConcreteType{ parts: vec![part] }, size);
    }

    pub(crate) fn lookup_layout_for_member_type(
        &self, parser_type: &ParserType, poly_args: &Vec<ConcreteType>, arch: &TargetArch
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
        let concrete_type = ConcreteType{ parts: concrete_parts };
        if let CTP::Instance(definition_id, num_poly_args) = concrete_type.parts[0] {
            // Retrieve polymorphic arguments from the full type tree
            // TODO: Rather wasteful, this screams for a different implementation
            //       @Optimize
            let mut poly_args = Vec::new();
            if num_poly_args != 0 {
                poly_args.reserve(num_poly_args as usize);

                let mut subtree_idx = 1;
                for _ in 0..num_poly_args {
                    let mut subtree_end = concrete_type.subtree_end_idx(subtree_idx);
                    poly_args.push(ConcreteType{
                        parts: Vec::from(&concrete_type.parts[subtree_idx..subtree_end])
                    });
                }

                debug_assert_eq!(subtree_idx, concrete_p)
            }

            // Always check the breadcrumbs first, because this allows us to
            // detect type loops
            for (breadcrumb_idx, breadcrumb) in self.breadcrumbs.iter().enumerate() {
                if breadcrumb.definition_id != definition_id {
                    continue;
                }

                let other_def = self.lookup.get(&breadcrumb.definition_id).unwrap();
                if other_def.monomorph_matches_poly_args(breadcrumb.monomorph_idx, &poly_args) {
                    // We're in a type loop
                    return LookupResult::TypeLoop(breadcrumb_idx);
                }
            }

            // None of the breadcrumbs matched, so we're not in a type loop, but
            // we might have still encountered this particular monomorph before
            let definition = self.lookup.get(&definition_id).unwrap();
            if let Some(monomorph_idx) = definition.has_monomorph(&poly_args) {
                let (size, alignment) = definition.get_monomorph_size_alignment(monomorph_idx);
                return LookupResult::Exists(concrete_type, size, alignment);
            }

            // If here, then the type has not been instantiated before
            return LookupResult::Missing(definition_id, poly_args);
        } else {
            // Must be some kind of builtin type. Probably the hot path, so just
            // return the type
            let (size, alignment) = match concrete_type.parts[0] {
                CTP::Void => (0, 1), // alignment is 1 to simplify alignment calculations
                CTP::Message => arch.pointer_size_alignment,
                CTP::Bool => (1, 1),
                CTP::UInt8 | CTP::SInt8 => (1, 1),
                CTP::UInt16 | CTP::SInt16 => (2, 2),
                CTP::UInt32 | CTP::SInt32 => (4, 4),
                CTP::UInt64 | CTP::SInt64 => (8, 8),
                CTP::Array => arch.array_size_alignment,
                CTP::Slice => arch.slice_size_alignment,
                CTP::Input | CTP::Output => arch.port_size_alignment,
                _ => unreachable!("unexpected builtin type in {:?}", concrete_type),
            };

            return LookupResult::Exists(concrete_type, size, alignment);
        }
    }

    fn handle_breadcrumb_type_loop(&mut self, breadcrumb_idx: usize) -> Result<(), ParseError> {
        // Starting at the breadcrumb index, walk all the way to the back and
        // mark each
    }

    //--------------------------------------------------------------------------
    // Breadcrumb management
    //--------------------------------------------------------------------------

    /// Pushes a breadcrumb for a particular definition. The field in the
    /// breadcrumb tracking progress in defining the type will be preallocated
    /// as best as possible.
    fn push_breadcrumb_for_definition(&mut self, ctx: &PassCtx, definition_id: DefinitionId, poly_args: Vec<ConcreteType>) {
        let definition = &ctx.heap[definition_id];
        debug_assert!(!self.lookup.get(&definition_id).unwrap().has_monomorph(&poly_args));

        let monomorph_idx = match definition {
            Definition::Struct(definition) => {

            },
            Definition::Enum(definition) => {

            },
            Definition::Union(definition) => {

            },
            Definition::Component(definition) => {

            },
            Definition::Function(definition) => {

            }
        };

        self.breadcrumbs.push(LayoutBreadcrumb{
            root_id,
            definition_id,
            monomorph_idx: 0,
            next_member: 0,
            next_embedded: 0
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

#[inline] fn align_offset_to(offset: &mut usize, alignment: usize) {
    debug_assert!(alignment > 0);
    let alignment_min_1 = alignment - 1;
    *offset += alignment_min_1;
    *offset &= !(alignment_min_1);
}
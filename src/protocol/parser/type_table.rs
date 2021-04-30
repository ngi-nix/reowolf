use std::fmt::{Formatter, Result as FmtResult};
use std::collections::{HashMap, VecDeque};

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
    pub(crate) poly_vars: Vec<PolymorphicVariable>,
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
}

struct PolymorphicVariable {
    identifier: Identifier,
    is_in_use: bool, // a polymorphic argument may be defined, but not used by the type definition
}

/// `EnumType` is the classical C/C++ enum type. It has various variants with
/// an assigned integer value. The integer values may be user-defined,
/// compiler-defined, or a mix of the two. If a user assigns the same enum
/// value multiple times, we assume the user is an expert and we consider both
/// variants to be equal to one another.
pub struct EnumType {
    pub(crate) variants: Vec<EnumVariant>,
    pub(crate) representation: PrimitiveType,
}

// TODO: Also support maximum u64 value
pub struct EnumVariant {
    pub(crate) identifier: Identifier,
    pub(crate) value: i64,
}

/// `UnionType` is the algebraic datatype (or sum type, or discriminated union).
/// A value is an element of the union, identified by its tag, and may contain
/// a single subtype.
pub struct UnionType {
    pub(crate) variants: Vec<UnionVariant>,
    pub(crate) tag_representation: PrimitiveType
}

pub struct UnionVariant {
    pub(crate) identifier: Identifier,
    pub(crate) embedded: Vec<ParserType>, // zero-length does not have embedded values
    pub(crate) tag_value: i64,
}

pub struct StructType {
    pub(crate) fields: Vec<StructField>,
}

pub struct StructField {
    pub(crate) identifier: Identifier,
    pub(crate) parser_type: ParserType,
}

pub struct FunctionType {
    pub return_types: Vec<ParserType>,
    pub arguments: Vec<FunctionArgument>
}

pub struct ComponentType {
    pub variant: ComponentVariant,
    pub arguments: Vec<FunctionArgument>
}

pub struct FunctionArgument {
    identifier: Identifier,
    parser_type: ParserType,
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
    Builtin,
    PolymoprhicArgument,
    /// ParserType points to a user-defined type that is already resolved in the
    /// type table.
    Resolved(RootId, DefinitionId),
    /// ParserType points to a user-defined type that is not yet resolved into
    /// the type table.
    Unresolved(RootId, DefinitionId)
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

impl TypeTable {
    /// Construct a new type table without any resolved types.
    pub(crate) fn new() -> Self {
        Self{ 
            lookup: HashMap::new(), 
            iter: TypeIterator::new(), 
            parser_type_iter: VecDeque::with_capacity(64), 
        }
    }

    pub(crate) fn build_base_types(&mut self, modules: &mut [Module], ctx: &mut PassCtx) -> Result<(), ParseError> {
        // Make sure we're allowed to cast root_id to index into ctx.modules
        debug_assert!(modules.iter().all(|m| m.phase >= ModuleCompilationPhase::DefinitionsParsed));
        debug_assert!(self.lookup.is_empty());
        debug_assert!(self.iter.top().is_none());
        debug_assert!(self.parser_type_iter.is_empty());

        if cfg!(debug_assertions) {
            for (index, module) in modules.iter().enumerate() {
                debug_assert_eq!(index, module.root_id.index as usize);
            }
        }

        // Use context to guess hashmap size
        let reserve_size = ctx.heap.definitions.len();
        self.lookup.reserve(reserve_size);

        for root_idx in 0..modules.len() {
            let last_definition_idx = ctx.heap[modules[root_idx].root_id].definitions.len();
            for definition_idx in 0..last_definition_idx {
                let definition_id = ctx.heap[modules[root_idx].root_id].definitions[definition_idx];
                self.resolve_base_definition(modules, ctx, definition_id)?;
            }
        }

        debug_assert_eq!(self.lookup.len(), reserve_size, "mismatch in reserved size of type table");
        for module in modules {
            module.phase = ModuleCompilationPhase::TypesAddedToTable;
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
    fn resolve_base_definition<'a>(&'a mut self, modules: &[Module], ctx: &mut PassCtx, definition_id: DefinitionId) -> Result<(), ParseError> {
        // Check if we have already resolved the base definition
        if self.lookup.contains_key(&definition_id) { return Ok(()); }

        let root_id = ctx.heap[definition_id].defined_in();
        self.iter.reset(root_id, definition_id);

        while let Some((root_id, definition_id)) = self.iter.top() {
            // We have a type to resolve
            let definition = &ctx.heap[definition_id];

            let can_pop_breadcrumb = match definition {
                // TODO: @cleanup Borrow rules hax
                Definition::Enum(_) => self.resolve_base_enum_definition(modules, ctx, root_id, definition_id),
                Definition::Union(_) => self.resolve_base_union_definition(modules, ctx, root_id, definition_id),
                Definition::Struct(_) => self.resolve_base_struct_definition(modules, ctx, root_id, definition_id),
                Definition::Component(_) => self.resolve_base_component_definition(modules, ctx, root_id, definition_id),
                Definition::Function(_) => self.resolve_base_function_definition(modules, ctx, root_id, definition_id),
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
    fn resolve_base_enum_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_enum());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base enum already resolved");
        
        let definition = ctx.heap[definition_id].as_enum();

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
        self.check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "enum variant"
        )?;

        // Because we're parsing an enum, the programmer cannot put the
        // polymorphic variables inside the variants. But the polymorphic
        // variables might still be present as "marker types"
        self.check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;
        let poly_vars = Self::create_polymorphic_variables(&definition.poly_vars);

        self.lookup.insert(definition_id, DefinedType {
            ast_root: root_id,
            ast_definition: definition_id,
            definition: DefinedTypeVariant::Enum(EnumType{
                variants,
                representation: Self::enum_tag_type(min_enum_value, max_enum_value)
            }),
            poly_vars,
            is_polymorph: false,
            is_pointerlike: false,
            monomorphs: Vec::new()
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
                        let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, parser_type)?;
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
            })
        }

        // Ensure union names and polymorphic args do not conflict
        self.check_identifier_collision(
            modules, root_id, &variants, |variant| &variant.identifier, "union variant"
        )?;
        self.check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

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
                tag_representation: Self::enum_tag_type(-1, tag_value),
            }),
            poly_vars,
            is_polymorph,
            is_pointerlike: false, // TODO: @cyclic_types
            monomorphs: Vec::new()
        });

        Ok(true)
    }

    /// Resolves the basic struct definition to an entry in the type table. It
    /// will not instantiate any monomorphized instances of polymorphic struct
    /// definitions.
    fn resolve_base_struct_definition(&mut self, modules: &[Module], ctx: &mut PassCtx, root_id: RootId, definition_id: DefinitionId) -> Result<bool, ParseError> {
        debug_assert!(ctx.heap[definition_id].is_struct());
        debug_assert!(!self.lookup.contains_key(&definition_id), "base struct already resolved");

        let definition = ctx.heap[definition_id].as_struct();

        // Make sure all fields point to resolvable types
        for field_definition in &definition.fields {
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &field_definition.parser_type)?;
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
        self.check_identifier_collision(
            modules, root_id, &fields, |field| &field.identifier, "struct field"
        )?;
        self.check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

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
            }),
            poly_vars,
            is_polymorph,
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
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
        let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &definition.return_types[0])?;
        if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
            return Ok(false)
        }

        // Check the argument types
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &param.parser_type)?;
            if !self.ingest_resolve_result(modules, ctx, resolve_result)? {
                return Ok(false)
            }
        }

        // Construct arguments to function
        let mut arguments = Vec::with_capacity(definition.parameters.len());
        for param_id in &definition.parameters {
            let param = &ctx.heap[*param_id];
            arguments.push(FunctionArgument{
                identifier: param.identifier.clone(),
                parser_type: param.parser_type.clone(),
            })
        }

        // Check conflict of argument and polyarg identifiers
        self.check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "function argument"
        )?;
        self.check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

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
            }),
            poly_vars,
            is_polymorph,
            is_pointerlike: false, // TODO: @cyclic
            monomorphs: Vec::new(),
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
            let resolve_result = self.resolve_base_parser_type(modules, ctx, root_id, &param.parser_type)?;
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
        self.check_identifier_collision(
            modules, root_id, &arguments, |arg| &arg.identifier, "component argument"
        )?;
        self.check_poly_args_collision(modules, ctx, root_id, &definition.poly_vars)?;

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
            }),
            poly_vars,
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
    fn ingest_resolve_result(&mut self, modules: &[Module], ctx: &PassCtx, result: ResolveResult) -> Result<bool, ParseError> {
        match result {
            ResolveResult::Builtin | ResolveResult::PolymoprhicArgument => Ok(true),
            ResolveResult::Resolved(_, _) => Ok(true),
            ResolveResult::Unresolved(root_id, definition_id) => {
                if self.iter.contains(root_id, definition_id) {
                    // Cyclic dependency encountered
                    // TODO: Allow this
                    let module_source = &modules[root_id.index as usize].source;
                    let mut error = ParseError::new_error_str_at_span(
                        module_source, ctx.heap[definition_id].identifier().span,
                        "Evaluating this type definition results in a cyclic type"
                    );

                    for (breadcrumb_idx, (root_id, definition_id)) in self.iter.breadcrumbs.iter().enumerate() {
                        let msg = if breadcrumb_idx == 0 {
                            "The cycle started with this definition"
                        } else {
                            "Which depends on this definition"
                        };

                        let module_source = &modules[root_id.index as usize].source;
                        error = error.with_info_str_at_span(module_source, ctx.heap[*definition_id].identifier().span, msg);
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

    /// Each type may consist of embedded types. If this type does not have a
    /// fixed implementation (e.g. an input port may have an embedded type
    /// indicating the type of messages, but it always exists in the runtime as
    /// a port identifier, so it has a fixed implementation) then this function
    /// will traverse the embedded types to ensure all of them are resolved.
    ///
    /// Hence if one checks a particular parser type for being resolved, one may
    /// get back a result value indicating an embedded type (with a different
    /// DefinitionId) is unresolved.
    fn resolve_base_parser_type(&mut self, modules: &[Module], ctx: &PassCtx, root_id: RootId, parser_type: &ParserType) -> Result<ResolveResult, ParseError> {
        // Note that as we iterate over the elements of a
        use ParserTypeVariant as PTV;

        // Result for the very first time we resolve a type (i.e the outer type
        // that we're actually looking up)
        let mut resolve_result = None;
        let mut set_resolve_result = |v: ResolveResult| {
            if resolve_result.is_none() { resolve_result = Some(v); }
        };

        for element in parser_type.elements.iter() {
            match element.variant {
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
                            module_source, element.full_span, "expected a datatype (struct, enum or union)"
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
        &self, modules: &[Module], root_id: RootId, items: &[T], getter: F, item_name: &'static str
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
        &self, modules: &[Module], ctx: &PassCtx, root_id: RootId, poly_args: &[Identifier]
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
        for element in & parser_type.elements {
            if let ParserTypeVariant::PolymorphicArgument(_, idx) = &element.variant {
                poly_vars[*idx].is_in_use = true;
            }
        }
    }

    fn enum_tag_type(min_tag_value: i64, max_tag_value: i64) -> PrimitiveType {
        // TODO: @consistency tag values should be handled correctly
        debug_assert!(min_tag_value <= max_tag_value);
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
}
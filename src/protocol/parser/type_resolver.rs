use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use super::type_table::*;
use super::symbol_table::*;
use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx,
    Visitor2,
    VisitorResult
};
use std::collections::HashMap;

pub(crate) enum InferredPart {
    // Unknown section of inferred type, yet to be inferred
    Unknown,
    // No subtypes
    Message,
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // One subtype
    Array,
    Slice,
    Input,
    Output,
    // One or more subtypes
    Instance(DefinitionId, usize),
}

impl From<ConcreteTypeVariant> for InferredPart {
    fn from(v: ConcreteTypeVariant) -> Self {
        use ConcreteTypeVariant as CTV;
        use InferredPart as IP;

        match v {
            CTV::Message => IP::Message,
            CTV::Bool => IP::Bool,
            CTV::Byte => IP::Byte,
            CTV::Short => IP::Short,
            CTV::Int => IP::Int,
            CTV::Long => IP::Long,
            CTV::String => IP::String,
            CTV::Array => IP::Array,
            CTV::Slice => IP::Slice,
            CTV::Input => IP::Input,
            CTV::Output => IP::Output,
            CTV::Instance(definition_id, num_sub) => IP::Instance(definition_id, num_sub),
        }
    }
}

pub(crate) struct InferenceType {
    origin: ParserTypeId,
    inferred: Vec<InferredPart>,
}

impl InferenceType {
    fn new(inferred_type: ParserTypeId) -> Self {
        Self{ origin: inferred_type, inferred: vec![InferredPart::Unknown] }
    }

    fn assign_concrete(&mut self, concrete_type: &ConcreteType) {
        self.inferred.clear();
        self.inferred.reserve(concrete_type.v.len());
        for variant in concrete_type.v {
            self.inferred.push(InferredPart::from(variant))
        }
    }
}

// TODO: @cleanup I will do a very dirty implementation first, because I have no idea
//  what I am doing.
// Very rough idea:
//  - go through entire AST first, find all places where we have inferred types
//      (which may be embedded) and store them in some kind of map.
//  - go through entire AST and visit all expressions depth-first. We will
//      attempt to resolve the return type of each expression. If we can't then
//      we store them in another lookup map and link the dependency on an
//      inferred variable to that expression.
//  - keep iterating until we have completely resolved all variables.

/// This particular visitor will recurse depth-first into the AST and ensures
/// that all expressions have the appropriate types. At the moment this implies:
///
///     - Type checking arguments to unary and binary operators.
///     - Type checking assignment, indexing, slicing and select expressions.
///     - Checking arguments to functions and component instantiations.
///
/// This will be achieved by slowly descending into the AST. At any given
/// expression we may depend on
pub(crate) struct TypeResolvingVisitor {
    // Buffers for iteration over substatements and subexpressions
    stmt_buffer: Vec<StatementId>,
    expr_buffer: Vec<ExpressionId>,

    // If instantiating a monomorph of a polymorphic proctype, then we store the
    // values of the polymorphic values here.
    polyvars: Vec<(Identifier, ConcreteTypeVariant)>,
    // Mapping from parser type to inferred type. We attempt to continue to
    // specify these types until we're stuck or we've fully determined the type.
    infer_types: HashMap<ParserTypeId, InferenceType>,
    // Mapping from variable ID to parser type, optionally inferred, so then
    var_types: HashMap<VariableId, ParserTypeId>,
}

impl TypeResolvingVisitor {
    pub(crate) fn new() -> Self {
        TypeResolvingVisitor{
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            infer_types: HashMap::new(),
            var_types: HashMap::new(),
        }
    }

    fn reset(&mut self) {
        self.stmt_buffer.clear();
        self.expr_buffer.clear();
        self.infer_types.clear();
        self.var_types.clear();
    }
}

impl Visitor2 for TypeResolvingVisitor {
    // Definitions

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        self.reset();
        let comp_def = &ctx.heap[id];
        for param_id in comp_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            self.var_types.insert(param_id.upcast(), param.parser_type);
        }

        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        self.reset();
        let func_def = &ctx.heap[id];
        for param_id in func_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            self.var_types.insert(param_id.upcast(), param.parser_type);
        }
        let body_stmt_id = ctx.heap[id].body;


        self.visit_stmt(ctx, body_stmt_id)
    }

    // Statements

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        // Transfer statements for traversal
        let block = &ctx.heap[id];

        for stmt_id in block.statements.clone() {
            self.visit_stmt(ctx, stmt_id);
        }

        Ok(())
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        let memory_stmt = &ctx.heap[id];
        let local = &ctx.heap[memory_stmt.variable];
        self.var_types.insert(memory_stmt.variable, )

        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        Ok(())
    }
}

impl TypeResolvingVisitor {
    /// Checks if the `ParserType` contains any inferred variables. If so then
    /// they will be inserted into the `infer_types` variable. Here we assume
    /// we're parsing the body of a proctype, so any reference to polymorphic
    /// variables must refer to the polymorphic arguments of the proctype's
    /// definition.
    /// TODO: @cleanup: The symbol_table -> type_table pattern appears quite
    ///     a lot, will likely need to create some kind of function for this
    fn insert_parser_type_if_needs_inference(
        &mut self, ctx: &mut Ctx, root_id: RootId, parser_type_id: ParserTypeId
    ) -> Result<(), ParseError2> {
        use ParserTypeVariant as PTV;

        let mut to_consider = vec![parser_type_id];
        while !to_consider.is_empty() {
            let parser_type_id = to_consider.pop().unwrap();
            let parser_type = &mut ctx.heap[parser_type_id];

            match &mut parser_type.variant {
                PTV::Inferred => {
                    self.env.insert(parser_type_id, InferenceType::new(parser_type_id));
                },
                PTV::Array(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Input(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Output(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Symbolic(symbolic) => {
                    // If not yet resolved, try to resolve
                    if symbolic.variant.is_none() {
                        let mut found = false;
                        for (poly_idx, (poly_var, _)) in self.polyvars.iter().enumerate() {
                            if symbolic.identifier.value == poly_var.value {
                                // Found a match
                                symbolic.variant = Some(SymbolicParserTypeVariant::PolyArg(poly_idx))
                                found = true;
                                break;
                            }
                        }

                        if !found {
                            // Attempt to find in symbol/type table
                            let symbol = ctx.symbols.resolve_namespaced_symbol(root_id, &symbolic.identifier);
                            if symbol.is_none() {
                                let module_source = &ctx.module.source;
                                return Err(ParseError2::new_error(
                                    module_source, symbolic.identifier.position,
                                    "Could not resolve symbol to a type"
                                ));
                            }

                            // Check if symbol was fully resolved
                            let (symbol, ident_iter) = symbol.unwrap();
                            if ident_iter.num_remaining() != 0 {
                                let module_source = &ctx.module.source;
                                ident_iter.
                                return Err(ParseError2::new_error(
                                    module_source, symbolic.identifier.position,
                                    "Could not resolve symbol to a type"
                                ).with_postfixed_info(
                                    module_source, symbol.position,
                                    "Could resolve part of the identifier to this symbol"
                                ));
                            }

                            // Check if symbol resolves to struct/enum
                            let definition_id = match symbol.symbol {
                                Symbol::Namespace(_) => {
                                    let module_source = &ctx.module.source;
                                    return Err(ParseError2::new_error(
                                        module_source, symbolic.identifier.position,
                                        "Symbol resolved to a module instead of a type"
                                    ));
                                },
                                Symbol::Definition((_, definition_id)) => definition_id
                            };

                            // Retrieve from type table and make sure it is a
                            // reference to a struct/enum/union
                            // TODO: @types Allow function pointers
                            let def_type = ctx.types.get_base_definition(&definition_id);
                            debug_assert!(def_type.is_some(), "Expected to resolve definition ID to type definition in type table");
                            let def_type = def_type.unwrap();

                            let def_type_class = def_type.definition.type_class();
                            if !def_type_class.is_data_type() {
                                return Err(ParseError2::new_error(
                                    &ctx.module.source, symbolic.identifier.position,
                                    &format!("Symbol refers to a {}, only data types are supported", def_type_class)
                                ));
                            }

                            // Now that we're certain it is a datatype, make
                            // sure that the number of polyargs in the symbolic
                            // type matches that of the definition, or conclude
                            // that all polyargs need to be inferred.
                            if symbolic.poly_args.len() != def_type.poly_args.len() {
                                if symbolic.poly_args.is_empty() {
                                    // Modify ParserType to have auto-inferred
                                    // polymorphic arguments
                                    symbolic.poly_args.
                                }
                            }
                        }
                    }
                },
                _ => {} // Builtin, doesn't require inference
            }
        }
    }
}
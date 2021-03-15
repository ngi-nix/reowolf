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
    done: bool,
    inferred: Vec<InferredPart>,
}

impl InferenceType {
    fn new(inferred_type: ParserTypeId) -> Self {
        Self{ origin: inferred_type, done: false, inferred: vec![InferredPart::Unknown] }
    }

    fn assign_concrete(&mut self, concrete_type: &ConcreteType) {
        self.done = true;
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
    // values of the polymorphic values here. There should be as many, and in
    // the same order as, in the definition's polyargs.
    polyvars: Vec<ConcreteType>,
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
            polyvars: Vec::new(),
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
    // We have a function that traverses the types of variable expressions. If
    // we do not know the type yet we insert it in the "infer types" list.
    // If we do know the type then we can return it and assign it in the
    // variable expression.
    // Hence although the parser types are recursive structures with nested
    // ParserType IDs, here we need to traverse the type all at once.
    fn insert_parser_type_if_needs_inference(
        &mut self, ctx: &mut Ctx, root_id: RootId, parser_type_id: ParserTypeId
    ) -> Result<(), ParseError2> {
        use ParserTypeVariant as PTV;

        let mut to_consider = vec![parser_type_id];
        while !to_consider.is_empty() {
            let parser_type_id = to_consider.pop().unwrap();
            let parser_type = &ctx.heap[parser_type_id];

            match &parser_type.variant {
                PTV::Inferred => {
                    self.env.insert(parser_type_id, InferenceType::new(parser_type_id));
                },
                PTV::Array(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Input(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Output(subtype_id) => { to_consider.push(*subtype_id); },
                PTV::Symbolic(symbolic) => {
                    // variant is resolved in type table if a type definition,
                    // or in the linker phase if in a function body.
                    debug_assert!(symbolic.variant.is_some());

                    match symbolic.variant.unwrap() {
                        SymbolicParserTypeVariant::PolyArg(_, arg_idx) => {
                            // Points to polyarg, which is resolved by definition
                            debug_assert!(arg_idx < self.polyvars.len());
                            debug_assert!(symbolic.poly_args.is_empty()); // TODO: @hkt

                            let mut inferred = InferenceType::new(parser_type_id);
                            inferred.assign_concrete(&self.polyvars[arg_idx]);

                            self.infer_types.insert(parser_type_id, inferred);
                        },
                        SymbolicParserTypeVariant::Definition(definition_id) => {
                            // Points to a type definition, but if it has poly-
                            // morphic arguments then these need to be inferred.
                        }
                    }
                },
                _ => {} // Builtin, doesn't require inference
            }
        }

        Ok(())
    }
}
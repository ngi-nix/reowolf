use crate::protocol::ast::*;
use super::type_table::{ConcreteType, ConcreteTypeVariant};
use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx,
    Visitor2,
    VisitorResult
};
use std::collections::HashMap;

enum ExprType {
    Regular, // expression statement or return statement
    Memory, // memory statement's expression
    Condition, // if/while conditional statement
    Assert, // assert statement
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
    // Tracking traversal state
    expr_type: ExprType,

    // Buffers for iteration over substatements and subexpressions
    stmt_buffer: Vec<StatementId>,
    expr_buffer: Vec<ExpressionId>,

    // Map for associating "auto"/"polyarg" variables with a concrete type where
    // it is not yet determined.
    env: HashMap<ParserTypeId, ConcreteTypeVariant>
}

impl TypeResolvingVisitor {
    pub(crate) fn new() -> Self {
        TypeResolvingVisitor{
            expr_type: ExprType::Regular,
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            env: HashMap::new(),
        }
    }

    fn reset(&mut self) {
        self.expr_type = ExprType::Regular;
    }
}

impl Visitor2 for TypeResolvingVisitor {
    // Definitions

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        let body_stmt_id = ctx.heap[id].body;

        self.visit_stmt(ctx, body_stmt_id)
    }

    // Statements

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        // Transfer statements for traversal
        let block = &ctx.heap[id];

        let old_len_stmts = self.stmt_buffer.len();
        self.stmt_buffer.extend(&block.statements);
        let new_len_stmts = self.stmt_buffer.len();

        // Traverse statements
        for stmt_idx in old_len_stmts..new_len_stmts {
            let stmt_id = self.stmt_buffer[stmt_idx];
            self.expr_type = ExprType::Regular;
            self.visit_stmt(ctx, stmt_id)?;
        }

        self.stmt_buffer.truncate(old_len_stmts);
        Ok(())
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        let memory_stmt = &ctx.heap[id];



        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        Ok(())
    }
}

impl TypeResolvingVisitor {

}
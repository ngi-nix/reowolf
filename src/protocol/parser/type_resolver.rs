use crate::protocol::ast::*;
use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx,
    Visitor2,
    VisitorResult
};

enum ExprType {
    Regular, // expression statement or return statement
    Memory, // memory statement's expression
    Condition, // if/while conditional statement
    Assert, // assert statement
}

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
}

impl TypeResolvingVisitor {
    pub(crate) fn new() -> Self {
        TypeResolvingVisitor{
            expr_type: ExprType::Regular,
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
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

        // Type of local should match the type of the initial expression


        // For now, with all variables having an explicit type, it seems we 
        // do not need to consider all expressions within a single definition in
        // order to do typechecking and type inference for numeric constants.

        // Since each expression will get an assigned type, and everything 
        // already has a type, we may traverse leaf-to-root, while assigning
        // output types. We throw an error if the types do not match. If an
        // expression's type is already assigned then they should match.

        // Note that for numerical types the information may also travel upward.
        // That is: if we have:
        //  
        //      u32 a = 5 * 2 << 3 + 8
        // 
        // Then we may infer that the expression yields a u32 type. As a result
        // All of the literals 5, 2, 3 and 8 will have type u32 as well.

        Ok(())
    }
}
use crate::protocol::ast::*;
use crate::protocol::input_source::ParseError;
use crate::protocol::parser::{type_table::*, Module};
use crate::protocol::symbol_table::{SymbolTable};

type Unit = ();
pub(crate) type VisitorResult = Result<Unit, ParseError>;

/// Globally configured vector capacity for statement buffers in visitor 
/// implementations
pub(crate) const STMT_BUFFER_INIT_CAPACITY: usize = 256;
/// Globally configured vector capacity for expression buffers in visitor
/// implementations
pub(crate) const EXPR_BUFFER_INIT_CAPACITY: usize = 256;

/// General context structure that is used while traversing the AST.
pub(crate) struct Ctx<'p> {
    pub heap: &'p mut Heap,
    pub module: &'p Module,
    pub symbols: &'p mut SymbolTable,
    pub types: &'p mut TypeTable,
}

/// Visitor is a generic trait that will fully walk the AST. The default
/// implementation of the visitors is to not recurse. The exception is the
/// top-level `visit_definition`, `visit_stmt` and `visit_expr` methods, which
/// call the appropriate visitor function.
pub(crate) trait Visitor2 {
    // Entry point
    fn visit_module(&mut self, ctx: &mut Ctx) -> VisitorResult {
        let mut def_index = 0;
        loop {
            let definition_id = {
                let root = &ctx.heap[ctx.module.root_id];
                if def_index >= root.definitions.len() {
                    return Ok(())
                }

                root.definitions[def_index]
            };

            self.visit_definition(ctx, definition_id)?;
            def_index += 1;
        }
    }

    // Definitions
    // --- enum matching
    fn visit_definition(&mut self, ctx: &mut Ctx, id: DefinitionId) -> VisitorResult {
        match &ctx.heap[id] {
            Definition::Enum(def) => {
                let def = def.this;
                self.visit_enum_definition(ctx, def)
            },
            Definition::Union(def) => {
                let def = def.this;
                self.visit_union_definition(ctx, def)
            }
            Definition::Struct(def) => {
                let def = def.this;
                self.visit_struct_definition(ctx, def)
            },
            Definition::Component(def) => {
                let def = def.this;
                self.visit_component_definition(ctx, def)
            },
            Definition::Function(def) => {
                let def = def.this;
                self.visit_function_definition(ctx, def)
            }
        }
    }

    // --- enum variant handling
    fn visit_enum_definition(&mut self, _ctx: &mut Ctx, _id: EnumDefinitionId) -> VisitorResult { Ok(()) }
    fn visit_union_definition(&mut self, _ctx: &mut Ctx, _id: UnionDefinitionId) -> VisitorResult{ Ok(()) }
    fn visit_struct_definition(&mut self, _ctx: &mut Ctx, _id: StructDefinitionId) -> VisitorResult { Ok(()) }
    fn visit_component_definition(&mut self, _ctx: &mut Ctx, _id: ComponentDefinitionId) -> VisitorResult { Ok(()) }
    fn visit_function_definition(&mut self, _ctx: &mut Ctx, _id: FunctionDefinitionId) -> VisitorResult { Ok(()) }

    // Statements
    // --- enum matching
    fn visit_stmt(&mut self, ctx: &mut Ctx, id: StatementId) -> VisitorResult {
        match &ctx.heap[id] {
            Statement::Block(stmt) => {
                let this = stmt.this;
                self.visit_block_stmt(ctx, this)
            },
            Statement::Local(stmt) => {
                let this = stmt.this();
                self.visit_local_stmt(ctx, this)
            },
            Statement::Labeled(stmt) => {
                let this = stmt.this;
                self.visit_labeled_stmt(ctx, this)
            },
            Statement::If(stmt) => {
                let this = stmt.this;
                self.visit_if_stmt(ctx, this)
            },
            Statement::EndIf(_stmt) => Ok(()),
            Statement::While(stmt) => {
                let this = stmt.this;
                self.visit_while_stmt(ctx, this)
            },
            Statement::EndWhile(_stmt) => Ok(()),
            Statement::Break(stmt) => {
                let this = stmt.this;
                self.visit_break_stmt(ctx, this)
            },
            Statement::Continue(stmt) => {
                let this = stmt.this;
                self.visit_continue_stmt(ctx, this)
            },
            Statement::Synchronous(stmt) => {
                let this = stmt.this;
                self.visit_synchronous_stmt(ctx, this)
            },
            Statement::EndSynchronous(_stmt) => Ok(()),
            Statement::Return(stmt) => {
                let this = stmt.this;
                self.visit_return_stmt(ctx, this)
            },
            Statement::Goto(stmt) => {
                let this = stmt.this;
                self.visit_goto_stmt(ctx, this)
            },
            Statement::New(stmt) => {
                let this = stmt.this;
                self.visit_new_stmt(ctx, this)
            },
            Statement::Expression(stmt) => {
                let this = stmt.this;
                self.visit_expr_stmt(ctx, this)
            }
        }
    }

    fn visit_local_stmt(&mut self, ctx: &mut Ctx, id: LocalStatementId) -> VisitorResult {
        match &ctx.heap[id] {
            LocalStatement::Channel(stmt) => {
                let this = stmt.this;
                self.visit_local_channel_stmt(ctx, this)
            },
            LocalStatement::Memory(stmt) => {
                let this = stmt.this;
                self.visit_local_memory_stmt(ctx, this)
            },
        }
    }

    // --- enum variant handling
    fn visit_block_stmt(&mut self, _ctx: &mut Ctx, _id: BlockStatementId) -> VisitorResult { Ok(()) }
    fn visit_local_memory_stmt(&mut self, _ctx: &mut Ctx, _id: MemoryStatementId) -> VisitorResult { Ok(()) }
    fn visit_local_channel_stmt(&mut self, _ctx: &mut Ctx, _id: ChannelStatementId) -> VisitorResult { Ok(()) }
    fn visit_labeled_stmt(&mut self, _ctx: &mut Ctx, _id: LabeledStatementId) -> VisitorResult { Ok(()) }
    fn visit_if_stmt(&mut self, _ctx: &mut Ctx, _id: IfStatementId) -> VisitorResult { Ok(()) }
    fn visit_while_stmt(&mut self, _ctx: &mut Ctx, _id: WhileStatementId) -> VisitorResult { Ok(()) }
    fn visit_break_stmt(&mut self, _ctx: &mut Ctx, _id: BreakStatementId) -> VisitorResult { Ok(()) }
    fn visit_continue_stmt(&mut self, _ctx: &mut Ctx, _id: ContinueStatementId) -> VisitorResult { Ok(()) }
    fn visit_synchronous_stmt(&mut self, _ctx: &mut Ctx, _id: SynchronousStatementId) -> VisitorResult { Ok(()) }
    fn visit_return_stmt(&mut self, _ctx: &mut Ctx, _id: ReturnStatementId) -> VisitorResult { Ok(()) }
    fn visit_goto_stmt(&mut self, _ctx: &mut Ctx, _id: GotoStatementId) -> VisitorResult { Ok(()) }
    fn visit_new_stmt(&mut self, _ctx: &mut Ctx, _id: NewStatementId) -> VisitorResult { Ok(()) }
    fn visit_expr_stmt(&mut self, _ctx: &mut Ctx, _id: ExpressionStatementId) -> VisitorResult { Ok(()) }

    // Expressions
    // --- enum matching
    fn visit_expr(&mut self, ctx: &mut Ctx, id: ExpressionId) -> VisitorResult {
        match &ctx.heap[id] {
            Expression::Assignment(expr) => {
                let this = expr.this;
                self.visit_assignment_expr(ctx, this)
            },
            Expression::Binding(expr) => {
                let this = expr.this;
                self.visit_binding_expr(ctx, this)
            }
            Expression::Conditional(expr) => {
                let this = expr.this;
                self.visit_conditional_expr(ctx, this)
            }
            Expression::Binary(expr) => {
                let this = expr.this;
                self.visit_binary_expr(ctx, this)
            }
            Expression::Unary(expr) => {
                let this = expr.this;
                self.visit_unary_expr(ctx, this)
            }
            Expression::Indexing(expr) => {
                let this = expr.this;
                self.visit_indexing_expr(ctx, this)
            }
            Expression::Slicing(expr) => {
                let this = expr.this;
                self.visit_slicing_expr(ctx, this)
            }
            Expression::Select(expr) => {
                let this = expr.this;
                self.visit_select_expr(ctx, this)
            }
            Expression::Literal(expr) => {
                let this = expr.this;
                self.visit_literal_expr(ctx, this)
            }
            Expression::Call(expr) => {
                let this = expr.this;
                self.visit_call_expr(ctx, this)
            }
            Expression::Variable(expr) => {
                let this = expr.this;
                self.visit_variable_expr(ctx, this)
            }
        }
    }

    fn visit_assignment_expr(&mut self, _ctx: &mut Ctx, _id: AssignmentExpressionId) -> VisitorResult { Ok(()) }
    fn visit_binding_expr(&mut self, _ctx: &mut Ctx, _id: BindingExpressionId) -> VisitorResult { Ok(()) }
    fn visit_conditional_expr(&mut self, _ctx: &mut Ctx, _id: ConditionalExpressionId) -> VisitorResult { Ok(()) }
    fn visit_binary_expr(&mut self, _ctx: &mut Ctx, _id: BinaryExpressionId) -> VisitorResult { Ok(()) }
    fn visit_unary_expr(&mut self, _ctx: &mut Ctx, _id: UnaryExpressionId) -> VisitorResult { Ok(()) }
    fn visit_indexing_expr(&mut self, _ctx: &mut Ctx, _id: IndexingExpressionId) -> VisitorResult { Ok(()) }
    fn visit_slicing_expr(&mut self, _ctx: &mut Ctx, _id: SlicingExpressionId) -> VisitorResult { Ok(()) }
    fn visit_select_expr(&mut self, _ctx: &mut Ctx, _id: SelectExpressionId) -> VisitorResult { Ok(()) }
    fn visit_literal_expr(&mut self, _ctx: &mut Ctx, _id: LiteralExpressionId) -> VisitorResult { Ok(()) }
    fn visit_call_expr(&mut self, _ctx: &mut Ctx, _id: CallExpressionId) -> VisitorResult { Ok(()) }
    fn visit_variable_expr(&mut self, _ctx: &mut Ctx, _id: VariableExpressionId) -> VisitorResult { Ok(()) }
}
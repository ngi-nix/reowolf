use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use crate::protocol::parser::{
    symbol_table::*, 
    type_table::*,
    utils::*,
};

use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    TYPE_BUFFER_INIT_CAPACITY,
    Ctx, 
    Visitor2, 
    VisitorResult
};

#[derive(PartialEq, Eq)]
enum DefinitionType {
    None,
    Primitive(ComponentId),
    Composite(ComponentId),
    Function(FunctionId)
}

impl DefinitionType {
    fn is_primitive(&self) -> bool { if let Self::Primitive(_) = self { true } else { false } }
    fn is_composite(&self) -> bool { if let Self::Composite(_) = self { true } else { false } }
    fn is_function(&self) -> bool { if let Self::Function(_) = self { true } else { false } }
}

/// This particular visitor will go through the entire AST in a recursive manner
/// and check if all statements and expressions are legal (e.g. no "return"
/// statements in component definitions), and will link certain AST nodes to
/// their appropriate targets (e.g. goto statements, or function calls).
///
/// This visitor will not perform control-flow analysis (e.g. making sure that
/// each function actually returns) and will also not perform type checking. So
/// the linking of function calls and component instantiations will be checked
/// and linked to the appropriate definitions, but the return types and/or
/// arguments will not be checked for validity.
///
/// The visitor visits each statement in a block in a breadth-first manner
/// first. We are thereby sure that we have found all variables/labels in a
/// particular block. In this phase nodes may queue statements for insertion
/// (e.g. the insertion of an `EndIf` statement for a particular `If`
/// statement). These will be inserted after visiting every node, after which
/// the visitor recurses into each statement in a block.
///
/// Because of this scheme expressions will not be visited in the breadth-first
/// pass.
pub(crate) struct ValidityAndLinkerVisitor {
    /// `in_sync` is `Some(id)` if the visitor is visiting the children of a
    /// synchronous statement. A single value is sufficient as nested
    /// synchronous statements are not allowed
    in_sync: Option<SynchronousStatementId>,
    /// `in_while` contains the last encountered `While` statement. This is used
    /// to resolve unlabeled `Continue`/`Break` statements.
    in_while: Option<WhileStatementId>,
    // Traversal state: current scope (which can be used to find the parent
    // scope), the definition variant we are considering, and whether the
    // visitor is performing breadthwise block statement traversal.
    cur_scope: Option<Scope>,
    def_type: DefinitionType,
    performing_breadth_pass: bool,
    // Parent expression (the previous stmt/expression we visited that could be
    // used as an expression parent)
    expr_parent: ExpressionParent,
    // Keeping track of relative position in block in the breadth-first pass.
    // May not correspond to block.statement[index] if any statements are
    // inserted after the breadth-pass
    relative_pos_in_block: u32,
    // Single buffer of statement IDs that we want to traverse in a block.
    // Required to work around Rust borrowing rules and to prevent constant
    // cloning of vectors.
    statement_buffer: Vec<StatementId>,
    // Another buffer, now with expression IDs, to prevent constant cloning of
    // vectors while working around rust's borrowing rules
    expression_buffer: Vec<ExpressionId>,
    // Yet another buffer, now with parser type IDs, similar to above
    parser_type_buffer: Vec<ParserTypeId>,
    // Statements to insert after the breadth pass in a single block
    insert_buffer: Vec<(u32, StatementId)>,
}

impl ValidityAndLinkerVisitor {
    pub(crate) fn new() -> Self {
        Self{
            in_sync: None,
            in_while: None,
            cur_scope: None,
            expr_parent: ExpressionParent::None,
            def_type: DefinitionType::None,
            performing_breadth_pass: false,
            relative_pos_in_block: 0,
            statement_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expression_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            parser_type_buffer: Vec::with_capacity(TYPE_BUFFER_INIT_CAPACITY),
            insert_buffer: Vec::with_capacity(32),
        }
    }

    fn reset_state(&mut self) {
        self.in_sync = None;
        self.in_while = None;
        self.cur_scope = None;
        self.expr_parent = ExpressionParent::None;
        self.def_type = DefinitionType::None;
        self.relative_pos_in_block = 0;
        self.performing_breadth_pass = false;
        self.statement_buffer.clear();
        self.expression_buffer.clear();
        self.parser_type_buffer.clear();
        self.insert_buffer.clear();
    }

    /// Debug call to ensure that we didn't make any mistakes in any of the
    /// employed buffers
    fn check_post_definition_state(&self) {
        debug_assert!(self.statement_buffer.is_empty());
        debug_assert!(self.expression_buffer.is_empty());
        debug_assert!(self.parser_type_buffer.is_empty());
        debug_assert!(self.insert_buffer.is_empty());
    }
}

impl Visitor2 for ValidityAndLinkerVisitor {
    //--------------------------------------------------------------------------
    // Definition visitors
    //--------------------------------------------------------------------------

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        self.reset_state();

        self.def_type = match &ctx.heap[id].variant {
            ComponentVariant::Primitive => DefinitionType::Primitive(id),
            ComponentVariant::Composite => DefinitionType::Composite(id),
        };
        self.cur_scope = Some(Scope::Definition(id.upcast()));
        self.expr_parent = ExpressionParent::None;

        // Visit types of parameters
        debug_assert!(self.parser_type_buffer.is_empty());
        let comp_def = &ctx.heap[id];
        self.parser_type_buffer.extend(
            comp_def.parameters
                .iter()
                .map(|id| ctx.heap[*id].parser_type)
        );

        let num_types = self.parser_type_buffer.len();
        for idx in 0..num_types {
            self.visit_parser_type(ctx, self.parser_type_buffer[idx])?;
        }

        self.parser_type_buffer.clear();

        // Visit statements in component body
        let body_id = ctx.heap[id].body;
        self.performing_breadth_pass = true;
        self.visit_stmt(ctx, body_id)?;
        self.performing_breadth_pass = false;
        self.visit_stmt(ctx, body_id)?;

        self.check_post_definition_state();
        Ok(())
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        self.reset_state();

        // Set internal statement indices
        self.def_type = DefinitionType::Function(id);
        self.cur_scope = Some(Scope::Definition(id.upcast()));
        self.expr_parent = ExpressionParent::None;

        // Visit types of parameters
        debug_assert!(self.parser_type_buffer.is_empty());
        let func_def = &ctx.heap[id];
        self.parser_type_buffer.extend(
            func_def.parameters
                .iter()
                .map(|id| ctx.heap[*id].parser_type)
        );
        self.parser_type_buffer.push(func_def.return_type);

        let num_types = self.parser_type_buffer.len();
        for idx in 0..num_types {
            self.visit_parser_type(ctx, self.parser_type_buffer[idx])?;
        }

        self.parser_type_buffer.clear();

        // Visit statements in function body
        let body_id = ctx.heap[id].body;
        self.performing_breadth_pass = true;
        self.visit_stmt(ctx, body_id)?;
        self.performing_breadth_pass = false;
        self.visit_stmt(ctx, body_id)?;

        self.check_post_definition_state();
        Ok(())
    }

    //--------------------------------------------------------------------------
    // Statement visitors
    //--------------------------------------------------------------------------

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        self.visit_block_stmt_with_hint(ctx, id, None)
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let variable_id = ctx.heap[id].variable;
            self.checked_local_add(ctx, self.relative_pos_in_block, variable_id)?;
        } else {
            let variable_id = ctx.heap[id].variable;
            let parser_type_id = ctx.heap[variable_id].parser_type;
            self.visit_parser_type(ctx, parser_type_id)?;

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        }

        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let (from_id, to_id) = {
                let stmt = &ctx.heap[id];
                (stmt.from, stmt.to)
            };
            self.checked_local_add(ctx, self.relative_pos_in_block, from_id)?;
            self.checked_local_add(ctx, self.relative_pos_in_block, to_id)?;
        } else {
            let chan_stmt = &ctx.heap[id];
            let from_type_id = ctx.heap[chan_stmt.from].parser_type;
            let to_type_id = ctx.heap[chan_stmt.to].parser_type;
            self.visit_parser_type(ctx, from_type_id)?;
            self.visit_parser_type(ctx, to_type_id)?;
        }

        Ok(())
    }

    fn visit_labeled_stmt(&mut self, ctx: &mut Ctx, id: LabeledStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            // Add label to block lookup
            self.checked_label_add(ctx, id)?;

            // Modify labeled statement itself
            let labeled = &mut ctx.heap[id];
            labeled.relative_pos_in_block = self.relative_pos_in_block;
            labeled.in_sync = self.in_sync.clone();
        }

        let body_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_id)?;

        Ok(())
    }

    fn visit_if_stmt(&mut self, ctx: &mut Ctx, id: IfStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let position = ctx.heap[id].position;
            let end_if_id = ctx.heap.alloc_end_if_statement(|this| {
                EndIfStatement {
                    this,
                    start_if: id,
                    position,
                    next: None,
                }
            });
            let stmt = &mut ctx.heap[id];
            stmt.end_if = Some(end_if_id);
            self.insert_buffer.push((self.relative_pos_in_block + 1, end_if_id.upcast()));
        } else {
            // Traverse expression and bodies
            let (test_id, true_id, false_id) = {
                let stmt = &ctx.heap[id];
                (stmt.test, stmt.true_body, stmt.false_body)
            };

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::If(id);
            self.visit_expr(ctx, test_id)?;
            self.expr_parent = ExpressionParent::None;

            self.visit_stmt(ctx, true_id)?;
            self.visit_stmt(ctx, false_id)?;
        }

        Ok(())
    }

    fn visit_while_stmt(&mut self, ctx: &mut Ctx, id: WhileStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let position = ctx.heap[id].position;
            let end_while_id = ctx.heap.alloc_end_while_statement(|this| {
                EndWhileStatement {
                    this,
                    start_while: id,
                    position,
                    next: None,
                }
            });
            let stmt = &mut ctx.heap[id];
            stmt.end_while = Some(end_while_id);
            stmt.in_sync = self.in_sync.clone();

            self.insert_buffer.push((self.relative_pos_in_block + 1, end_while_id.upcast()));
        } else {
            let (test_id, body_id) = {
                let stmt = &ctx.heap[id];
                (stmt.test, stmt.body)
            };
            let old_while = self.in_while.replace(id);
            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::While(id);
            self.visit_expr(ctx, test_id)?;
            self.expr_parent = ExpressionParent::None;

            self.visit_stmt(ctx, body_id)?;
            self.in_while = old_while;
        }

        Ok(())
    }

    fn visit_break_stmt(&mut self, ctx: &mut Ctx, id: BreakStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            // Should be able to resolve break statements with a label in the
            // breadth pass, no need to do after resolving all labels
            let target_end_while = {
                let stmt = &ctx.heap[id];
                let target_while_id = self.resolve_break_or_continue_target(ctx, stmt.position, &stmt.label)?;
                let target_while = &ctx.heap[target_while_id];
                debug_assert!(target_while.end_while.is_some());
                target_while.end_while.unwrap()
            };

            let stmt = &mut ctx.heap[id];
            stmt.target = Some(target_end_while);
        }

        Ok(())
    }

    fn visit_continue_stmt(&mut self, ctx: &mut Ctx, id: ContinueStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let target_while_id = {
                let stmt = &ctx.heap[id];
                self.resolve_break_or_continue_target(ctx, stmt.position, &stmt.label)?
            };

            let stmt = &mut ctx.heap[id];
            stmt.target = Some(target_while_id)
        }

        Ok(())
    }

    fn visit_synchronous_stmt(&mut self, ctx: &mut Ctx, id: SynchronousStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            // Check for validity of synchronous statement
            let cur_sync_position = ctx.heap[id].position;
            if self.in_sync.is_some() {
                // Nested synchronous statement
                let old_sync = &ctx.heap[self.in_sync.unwrap()];
                return Err(
                    ParseError2::new_error(&ctx.module.source, cur_sync_position, "Illegal nested synchronous statement")
                        .with_postfixed_info(&ctx.module.source, old_sync.position, "It is nested in this synchronous statement")
                );
            }

            if !self.def_type.is_primitive() {
                return Err(ParseError2::new_error(
                    &ctx.module.source, cur_sync_position,
                    "Synchronous statements may only be used in primitive components"
                ));
            }

            // Append SynchronousEnd pseudo-statement
            let sync_end_id = ctx.heap.alloc_end_synchronous_statement(|this| EndSynchronousStatement{
                this,
                position: cur_sync_position,
                start_sync: id,
                next: None,
            });
            let sync_start = &mut ctx.heap[id];
            sync_start.end_sync = Some(sync_end_id);
            self.insert_buffer.push((self.relative_pos_in_block + 1, sync_end_id.upcast()));
        } else {
            let sync_body = ctx.heap[id].body;
            let old = self.in_sync.replace(id);
            self.visit_stmt_with_hint(ctx, sync_body, Some(id))?;
            self.in_sync = old;
        }

        Ok(())
    }

    fn visit_return_stmt(&mut self, ctx: &mut Ctx, id: ReturnStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let stmt = &ctx.heap[id];
            if !self.def_type.is_function() {
                return Err(
                    ParseError2::new_error(&ctx.module.source, stmt.position, "Return statements may only appear in function bodies")
                );
            }
        } else {
            // If here then we are within a function
            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::Return(id);
            self.visit_expr(ctx, ctx.heap[id].expression)?;
            self.expr_parent = ExpressionParent::None;
        }

        Ok(())
    }

    fn visit_assert_stmt(&mut self, ctx: &mut Ctx, id: AssertStatementId) -> VisitorResult {
        let stmt = &ctx.heap[id];
        if self.performing_breadth_pass {
            if self.def_type.is_function() {
                // TODO: We probably want to allow this. Mark the function as
                //  using asserts, and then only allow calls to these functions
                //  within components. Such a marker will cascade through any
                //  functions that then call an asserting function
                return Err(
                    ParseError2::new_error(&ctx.module.source, stmt.position, "Illegal assert statement in a function")
                );
            }

            // We are in a component of some sort, but we also need to be within a
            // synchronous statement
            if self.in_sync.is_none() {
                return Err(
                    ParseError2::new_error(&ctx.module.source, stmt.position, "Illegal assert statement outside of a synchronous block")
                );
            }
        } else {
            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            let expr_id = stmt.expression;

            self.expr_parent = ExpressionParent::Assert(id);
            self.visit_expr(ctx, expr_id)?;
            self.expr_parent = ExpressionParent::None;
        }

        Ok(())
    }

    fn visit_goto_stmt(&mut self, ctx: &mut Ctx, id: GotoStatementId) -> VisitorResult {
        if !self.performing_breadth_pass {
            // Must perform goto label resolving after the breadth pass, this
            // way we are able to find all the labels in current and outer
            // scopes.
            let target_id = self.find_label(ctx, &ctx.heap[id].label)?;
            ctx.heap[id].target = Some(target_id);

            let target = &ctx.heap[target_id];
            if self.in_sync != target.in_sync {
                // We can only goto the current scope or outer scopes. Because
                // nested sync statements are not allowed so if the value does
                // not match, then we must be inside a sync scope
                debug_assert!(self.in_sync.is_some());
                let goto_stmt = &ctx.heap[id];
                let sync_stmt = &ctx.heap[self.in_sync.unwrap()];
                return Err(
                    ParseError2::new_error(&ctx.module.source, goto_stmt.position, "Goto may not escape the surrounding synchronous block")
                        .with_postfixed_info(&ctx.module.source, target.position, "This is the target of the goto statement")
                        .with_postfixed_info(&ctx.module.source, sync_stmt.position, "Which will jump past this statement")
                );
            }
        }

        Ok(())
    }

    fn visit_new_stmt(&mut self, ctx: &mut Ctx, id: NewStatementId) -> VisitorResult {
        // Link the call expression following the new statement
        if self.performing_breadth_pass {
            // TODO: Cleanup error messages, can be done cleaner
            // Make sure new statement occurs within a composite component
            let call_expr_id = ctx.heap[id].expression;
            if !self.def_type.is_composite() {
                let new_stmt = &ctx.heap[id];
                return Err(
                    ParseError2::new_error(&ctx.module.source, new_stmt.position, "Instantiating components may only be done in composite components")
                );
            }

            // We make sure that we point to a symbolic method. Checking that it
            // points to a component is done in the depth pass.
            let call_expr = &ctx.heap[call_expr_id];
            if let Method::Symbolic(_) = &call_expr.method {
                // We're fine
            } else {
                return Err(
                    ParseError2::new_error(&ctx.module.source, call_expr.position, "Must instantiate a component")
                );
            }
        } else {
            // Just call `visit_call_expr`. We do some extra work we don't have
            // to, but this prevents silly mistakes.
            let call_expr_id = ctx.heap[id].expression;

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::New(id);
            self.visit_call_expr(ctx, call_expr_id)?;
            self.expr_parent = ExpressionParent::None;
        }

        Ok(())
    }

    fn visit_expr_stmt(&mut self, ctx: &mut Ctx, id: ExpressionStatementId) -> VisitorResult {
        if !self.performing_breadth_pass {
            let expr_id = ctx.heap[id].expression;

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::ExpressionStmt(id);
            self.visit_expr(ctx, expr_id)?;
            self.expr_parent = ExpressionParent::None;
        }

        Ok(())
    }


    //--------------------------------------------------------------------------
    // Expression visitors
    //--------------------------------------------------------------------------

    fn visit_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let upcast_id = id.upcast();
        let assignment_expr = &mut ctx.heap[id];

        let left_expr_id = assignment_expr.left;
        let right_expr_id = assignment_expr.right;
        let old_expr_parent = self.expr_parent;
        assignment_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.visit_expr(ctx, left_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.visit_expr(ctx, right_expr_id)?;
        self.expr_parent = old_expr_parent;
        Ok(())
    }

    fn visit_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);
        let upcast_id = id.upcast();
        let conditional_expr = &mut ctx.heap[id];

        let test_expr_id = conditional_expr.test;
        let true_expr_id = conditional_expr.true_expression;
        let false_expr_id = conditional_expr.false_expression;

        let old_expr_parent = self.expr_parent;
        conditional_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.visit_expr(ctx, test_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.visit_expr(ctx, true_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 2);
        self.visit_expr(ctx, false_expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_binary_expr(&mut self, ctx: &mut Ctx, id: BinaryExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);
        let upcast_id = id.upcast();
        let binary_expr = &mut ctx.heap[id];
        let left_expr_id = binary_expr.left;
        let right_expr_id = binary_expr.right;

        let old_expr_parent = self.expr_parent;
        binary_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.visit_expr(ctx, left_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.visit_expr(ctx, right_expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_unary_expr(&mut self, ctx: &mut Ctx, id: UnaryExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let unary_expr = &mut ctx.heap[id];
        let expr_id = unary_expr.expression;

        let old_expr_parent = self.expr_parent;
        unary_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(id.upcast(), 0);
        self.visit_expr(ctx, expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);
        let upcast_id = id.upcast();
        let indexing_expr = &mut ctx.heap[id];

        let subject_expr_id = indexing_expr.subject;
        let index_expr_id = indexing_expr.index;

        let old_expr_parent = self.expr_parent;
        indexing_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.visit_expr(ctx, subject_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.visit_expr(ctx, index_expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_slicing_expr(&mut self, ctx: &mut Ctx, id: SlicingExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);
        let upcast_id = id.upcast();
        let slicing_expr = &mut ctx.heap[id];

        let subject_expr_id = slicing_expr.subject;
        let from_expr_id = slicing_expr.from_index;
        let to_expr_id = slicing_expr.to_index;

        let old_expr_parent = self.expr_parent;
        slicing_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.visit_expr(ctx, subject_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.visit_expr(ctx, from_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 2);
        self.visit_expr(ctx, to_expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_select_expr(&mut self, ctx: &mut Ctx, id: SelectExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let select_expr = &mut ctx.heap[id];
        let expr_id = select_expr.subject;

        let old_expr_parent = self.expr_parent;
        select_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(id.upcast(), 0);
        self.visit_expr(ctx, expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_array_expr(&mut self, ctx: &mut Ctx, id: ArrayExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let upcast_id = id.upcast();
        let array_expr = &mut ctx.heap[id];

        let old_num_exprs = self.expression_buffer.len();
        self.expression_buffer.extend(&array_expr.elements);
        let new_num_exprs = self.expression_buffer.len();

        let old_expr_parent = self.expr_parent;
        array_expr.parent = old_expr_parent;

        for field_expr_idx in old_num_exprs..new_num_exprs {
            let field_expr_id = self.expression_buffer[field_expr_idx];
            self.expr_parent = ExpressionParent::Expression(upcast_id, field_expr_idx as u32);
            self.visit_expr(ctx, field_expr_id)?;
        }

        self.expression_buffer.truncate(old_num_exprs);
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_literal_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        const FIELD_NOT_FOUND_SENTINEL: usize = usize::max_value();

        let constant_expr = &mut ctx.heap[id];
        let old_expr_parent = self.expr_parent;
        constant_expr.parent = old_expr_parent;

        match &mut constant_expr.value {
            Literal::Null | Literal::True | Literal::False |
            Literal::Character(_) | Literal::Integer(_) => {
                // Just the parent has to be set, done above
            },
            Literal::Struct(literal) => {
                let upcast_id = id.upcast();

                // Retrieve and set the literal's definition
                let definition = self.find_symbol_of_type(
                    &ctx.module.source, ctx.module.root_id, &ctx.symbols, &ctx.types,
                    &literal.identifier, TypeClass::Struct
                )?;
                literal.definition = Some(definition.ast_definition);

                let definition = definition.definition.as_struct();

                // Make sure all fields are specified, none are specified twice
                // and all fields exist on the struct definition
                let mut specified = Vec::new(); // TODO: @performance
                specified.resize(definition.fields.len(), false);

                for field in &mut literal.fields {
                    // Find field in the struct definition
                    field.field_idx = FIELD_NOT_FOUND_SENTINEL;
                    for (def_field_idx, def_field) in definition.fields.iter().enumerate() {
                        if field.identifier == def_field.identifier {
                            field.field_idx = def_field_idx;
                            break;
                        }
                    }

                    // Check if not found
                    if field.field_idx == FIELD_NOT_FOUND_SENTINEL {
                        return Err(ParseError2::new_error(
                            &ctx.module.source, field.identifier.position,
                            &format!(
                                "This field does not exist on the struct '{}'",
                                &String::from_utf8_lossy(&literal.identifier.value),
                            )
                        ));
                    }

                    // Check if specified more than once
                    if specified[field.field_idx] {
                        return Err(ParseError2::new_error(
                            &ctx.module.source, field.identifier.position,
                            "This field is specified more than once"
                        ));
                    }

                    specified[field.field_idx] = true;
                }

                if !specified.iter().all(|v| *v) {
                    // Some fields were not specified
                    let mut not_specified = String::new();
                    for (def_field_idx, is_specified) in specified.iter().enumerate() {
                        if !is_specified {
                            if !not_specified.is_empty() { not_specified.push_str(", ") }
                            let field_ident = &definition.fields[def_field_idx].identifier;
                            not_specified.push_str(&String::from_utf8_lossy(&field_ident.value));
                        }
                    }

                    return Err(ParseError2::new_error(
                        &ctx.module.source, literal.identifier.position,
                        &format!("Not all fields are specified, [{}] are missing", not_specified)
                    ));
                }

                // Need to traverse fields expressions in struct and evaluate
                // the poly args
                let old_num_exprs = self.expression_buffer.len();
                self.expression_buffer.extend(literal.fields.iter().map(|v| v.value));
                let new_num_exprs = self.expression_buffer.len();

                self.visit_literal_poly_args(ctx, id)?;

                for expr_idx in old_num_exprs..new_num_exprs {
                    let expr_id = self.expression_buffer[expr_idx];
                    self.expr_parent = ExpressionParent::Expression(upcast_id, expr_idx as u32);
                    self.visit_expr(ctx, expr_id)?;
                }

                self.expression_buffer.truncate(old_num_exprs);
            }
        }

        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let call_expr = &mut ctx.heap[id];
        let num_expr_args = call_expr.arguments.len();

        // Resolve the method to the appropriate definition and check the
        // legality of the particular method call.
        // TODO: @cleanup Unify in some kind of signature call, see similar
        //  cleanup comments with this `match` format.
        let num_definition_args;
        match &mut call_expr.method {
            Method::Create => {
                num_definition_args = 1;
            },
            Method::Fires => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'fires' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'fires' may only occur inside synchronous blocks"
                    ));
                }
                num_definition_args = 1;
            },
            Method::Get => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'get' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'get' may only occur inside synchronous blocks"
                    ));
                }
                num_definition_args = 1;
            },
            Method::Put => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'put' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'put' may only occur inside synchronous blocks"
                    ));
                }
                num_definition_args = 2;
            }
            Method::Symbolic(symbolic) => {
                // Find symbolic procedure
                let expected_type = if let ExpressionParent::New(_) = self.expr_parent {
                    // Expect to find a component
                    TypeClass::Component
                } else {
                    // Expect to find a function
                    TypeClass::Function
                };

                let definition = self.find_symbol_of_type(
                    &ctx.module.source, ctx.module.root_id, &ctx.symbols, &ctx.types,
                    &symbolic.identifier, expected_type
                )?;

                symbolic.definition = Some(definition.ast_definition);
                match &definition.definition {
                    DefinedTypeVariant::Function(definition) => {
                        num_definition_args = definition.arguments.len();
                    },
                    DefinedTypeVariant::Component(definition) => {
                        num_definition_args = definition.arguments.len();
                    }
                    _ => unreachable!(),
                }
            }
        }

        // Check the poly args and the number of variables in the call
        // expression
        self.visit_call_poly_args(ctx, id)?;
        let call_expr = &mut ctx.heap[id];
        if num_expr_args != num_definition_args {
            return Err(ParseError2::new_error(
                &ctx.module.source, call_expr.position,
                &format!(
                    "This call expects {} arguments, but {} were provided",
                    num_definition_args, num_expr_args
                )
            ));
        }

        // Recurse into all of the arguments and set the expression's parent
        let upcast_id = id.upcast();

        let old_num_exprs = self.expression_buffer.len();
        self.expression_buffer.extend(&call_expr.arguments);
        let new_num_exprs = self.expression_buffer.len();

        let old_expr_parent = self.expr_parent;
        call_expr.parent = old_expr_parent;

        for arg_expr_idx in old_num_exprs..new_num_exprs {
            let arg_expr_id = self.expression_buffer[arg_expr_idx];
            self.expr_parent = ExpressionParent::Expression(upcast_id, arg_expr_idx as u32);
            self.visit_expr(ctx, arg_expr_id)?;
        }

        self.expression_buffer.truncate(old_num_exprs);
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let var_expr = &ctx.heap[id];
        let variable_id = self.find_variable(ctx, self.relative_pos_in_block, &var_expr.identifier)?;
        let var_expr = &mut ctx.heap[id];
        var_expr.declaration = Some(variable_id);
        var_expr.parent = self.expr_parent;

        Ok(())
    }

    //--------------------------------------------------------------------------
    // ParserType visitors
    //--------------------------------------------------------------------------

    fn visit_parser_type(&mut self, ctx: &mut Ctx, id: ParserTypeId) -> VisitorResult {
        let old_num_types = self.parser_type_buffer.len();
        match self.visit_parser_type_without_buffer_cleanup(ctx, id) {
            Ok(_) => {
                debug_assert_eq!(self.parser_type_buffer.len(), old_num_types);
                Ok(())
            },
            Err(err) => {
                self.parser_type_buffer.truncate(old_num_types);
                Err(err)
            }
        }
    }
}

impl ValidityAndLinkerVisitor {
    //--------------------------------------------------------------------------
    // Special traversal
    //--------------------------------------------------------------------------

    /// Will visit a statement with a hint about its wrapping statement. This is
    /// used to distinguish block statements with a wrapping synchronous
    /// statement from normal block statements.
    fn visit_stmt_with_hint(&mut self, ctx: &mut Ctx, id: StatementId, hint: Option<SynchronousStatementId>) -> VisitorResult {
        if let Statement::Block(block_stmt) = &ctx.heap[id] {
            let block_id = block_stmt.this;
            self.visit_block_stmt_with_hint(ctx, block_id, hint)
        } else {
            self.visit_stmt(ctx, id)
        }
    }

    fn visit_block_stmt_with_hint(&mut self, ctx: &mut Ctx, id: BlockStatementId, hint: Option<SynchronousStatementId>) -> VisitorResult {
        if self.performing_breadth_pass {
            // Performing a breadth pass, so don't traverse into the statements
            // of the block.
            return Ok(())
        }

        // Set parent scope and relative position in the parent scope. Remember
        // these values to set them back to the old values when we're done with
        // the traversal of the block's statements.
        let body = &mut ctx.heap[id];
        body.parent_scope = self.cur_scope.clone();
        body.relative_pos_in_parent = self.relative_pos_in_block;

        let old_scope = self.cur_scope.replace(match hint {
            Some(sync_id) => Scope::Synchronous((sync_id, id)),
            None => Scope::Regular(id),
        });
        let old_relative_pos = self.relative_pos_in_block;

        // Copy statement IDs into buffer
        let old_num_stmts = self.statement_buffer.len();
        {
            let body = &ctx.heap[id];
            self.statement_buffer.extend_from_slice(&body.statements);
        }
        let new_num_stmts = self.statement_buffer.len();

        // Perform the breadth-first pass. Its main purpose is to find labeled
        // statements such that we can find the `goto`-targets immediately when
        // performing the depth pass
        self.performing_breadth_pass = true;
        for stmt_idx in old_num_stmts..new_num_stmts {
            self.relative_pos_in_block = (stmt_idx - old_num_stmts) as u32;
            self.visit_stmt(ctx, self.statement_buffer[stmt_idx])?;
        }

        if !self.insert_buffer.is_empty() {
            let body = &mut ctx.heap[id];
            for (insert_idx, (pos, stmt)) in self.insert_buffer.drain(..).enumerate() {
                body.statements.insert(pos as usize + insert_idx, stmt);
            }
        }

        // And the depth pass. Because we're not actually visiting any inserted
        // nodes because we're using the statement buffer, we may safely use the
        // relative_pos_in_block counter.
        self.performing_breadth_pass = false;
        for stmt_idx in old_num_stmts..new_num_stmts {
            self.relative_pos_in_block = (stmt_idx - old_num_stmts) as u32;
            self.visit_stmt(ctx, self.statement_buffer[stmt_idx])?;
        }

        self.cur_scope = old_scope;
        self.relative_pos_in_block = old_relative_pos;

        // Pop statement buffer
        debug_assert!(self.insert_buffer.is_empty(), "insert buffer not empty after depth pass");
        self.statement_buffer.truncate(old_num_stmts);

        Ok(())
    }

    /// Visits a particular ParserType in the AST and resolves temporary and
    /// implicitly inferred types into the appropriate tree. Note that a
    /// ParserType node is a tree. Only call this function on the root node of
    /// that tree to prevent doing work more than once.
    fn visit_parser_type_without_buffer_cleanup(&mut self, ctx: &mut Ctx, id: ParserTypeId) -> VisitorResult {
        use ParserTypeVariant as PTV;
        debug_assert!(!self.performing_breadth_pass);

        let init_num_types = self.parser_type_buffer.len();
        self.parser_type_buffer.push(id);

        'resolve_loop: while self.parser_type_buffer.len() > init_num_types {
            let parser_type_id = self.parser_type_buffer.pop().unwrap();
            let parser_type = &ctx.heap[parser_type_id];

            let (symbolic_pos, symbolic_variant, num_inferred_to_allocate) = match &parser_type.variant {
                PTV::Message | PTV::Bool |
                PTV::Byte | PTV::Short | PTV::Int | PTV::Long |
                PTV::String |
                PTV::IntegerLiteral | PTV::Inferred => {
                    // Builtin types or types that do not require recursion
                    continue 'resolve_loop;
                },
                PTV::Array(subtype_id) |
                PTV::Input(subtype_id) |
                PTV::Output(subtype_id) => {
                    // Requires recursing
                    self.parser_type_buffer.push(*subtype_id);
                    continue 'resolve_loop;
                },
                PTV::Symbolic(symbolic) => {
                    // Retrieve poly_vars from function/component definition to
                    // match against.
                    let (definition_id, poly_vars) = match self.def_type {
                        DefinitionType::None => unreachable!(),
                        DefinitionType::Primitive(id) => (id.upcast(), &ctx.heap[id].poly_vars),
                        DefinitionType::Composite(id) => (id.upcast(), &ctx.heap[id].poly_vars),
                        DefinitionType::Function(id) => (id.upcast(), &ctx.heap[id].poly_vars),
                    };

                    let mut symbolic_variant = None;
                    for (poly_var_idx, poly_var) in poly_vars.iter().enumerate() {
                        if symbolic.identifier.matches_identifier(poly_var) {
                            // Type refers to a polymorphic variable.
                            // TODO: @hkt Maybe allow higher-kinded types?
                            if symbolic.identifier.get_poly_args().is_some() {
                                return Err(ParseError2::new_error(
                                    &ctx.module.source, symbolic.identifier.position,
                                    "Polymorphic arguments to a polymorphic variable (higher-kinded types) are not allowed (yet)"
                                ));
                            }
                            symbolic_variant = Some(SymbolicParserTypeVariant::PolyArg(definition_id, poly_var_idx));
                        }
                    }

                    if let Some(symbolic_variant) = symbolic_variant {
                        // Identifier points to a polymorphic argument
                        (symbolic.identifier.position, symbolic_variant, 0)
                    } else {
                        // Must be a user-defined type, otherwise an error
                        let (found_type, ident_iter) = find_type_definition(
                            &ctx.symbols, &ctx.types, ctx.module.root_id, &symbolic.identifier
                        ).as_parse_error(&ctx.module.source)?;

                        // TODO: @function_ptrs: Allow function pointers at some
                        //  point in the future
                        if found_type.definition.type_class().is_proc_type() {
                            return Err(ParseError2::new_error(
                                &ctx.module.source, symbolic.identifier.position,
                                &format!(
                                    "This identifier points to a {} type, expected a datatype",
                                    found_type.definition.type_class()
                                )
                            ));
                        }

                        // If the type is polymorphic then we have two cases: if
                        // the programmer did not specify the polyargs then we
                        // assume we're going to infer all of them. Otherwise we
                        // make sure that they match in count.
                        let (_, poly_args) = ident_iter.prev().unwrap();
                        let num_to_infer = match_polymorphic_args_to_vars(
                            found_type, poly_args, symbolic.identifier.position
                        ).as_parse_error(&ctx.heap, &ctx.module.source)?;

                        (
                            symbolic.identifier.position,
                            SymbolicParserTypeVariant::Definition(found_type.ast_definition),
                            num_to_infer
                        )
                    }
                }
            };

            // If here then type is symbolic, perform a mutable borrow (and do
            // some rust shenanigans) to set the required information.
            for _ in 0..num_inferred_to_allocate {
                // TODO: @hack, not very user friendly to manually allocate
                //  `inferred` ParserTypes with the InputPosition of the
                //  symbolic type's identifier.
                // We reuse the `parser_type_buffer` to temporarily store these
                // and we'll take them out later
                self.parser_type_buffer.push(ctx.heap.alloc_parser_type(|this| ParserType{
                    this,
                    pos: symbolic_pos,
                    variant: ParserTypeVariant::Inferred,
                }));
            }

            if let PTV::Symbolic(symbolic) = &mut ctx.heap[parser_type_id].variant {
                if num_inferred_to_allocate != 0 {
                    symbolic.poly_args2.reserve(num_inferred_to_allocate);
                    for _ in 0..num_inferred_to_allocate {
                        symbolic.poly_args2.push(self.parser_type_buffer.pop().unwrap());
                    }
                } else if !symbolic.identifier.poly_args.is_empty() {
                    symbolic.poly_args2.extend(&symbolic.identifier.poly_args)
                }
                symbolic.variant = Some(symbolic_variant);
            } else {
                unreachable!();
            }
        }

        Ok(())
    }

    //--------------------------------------------------------------------------
    // Utilities
    //--------------------------------------------------------------------------

    /// Adds a local variable to the current scope. It will also annotate the
    /// `Local` in the AST with its relative position in the block.
    fn checked_local_add(&mut self, ctx: &mut Ctx, relative_pos: u32, id: LocalId) -> Result<(), ParseError2> {
        debug_assert!(self.cur_scope.is_some());

        // Make sure we do not conflict with any global symbols
        {
            let ident = &ctx.heap[id].identifier;
            if let Some(symbol) = ctx.symbols.resolve_symbol(ctx.module.root_id, &ident.value) {
                return Err(
                    ParseError2::new_error(&ctx.module.source, ident.position, "Local variable declaration conflicts with symbol")
                        .with_postfixed_info(&ctx.module.source, symbol.position, "Conflicting symbol is found here")
                );
            }
        }

        let local = &mut ctx.heap[id];
        local.relative_pos_in_block = relative_pos;

        // Make sure we do not shadow any variables in any of the scopes. Note
        // that variables in parent scopes may be declared later
        let local = &ctx.heap[id];
        let mut scope = self.cur_scope.as_ref().unwrap();
        let mut local_relative_pos = self.relative_pos_in_block;

        loop {
            debug_assert!(scope.is_block(), "scope is not a block");
            let block = &ctx.heap[scope.to_block()];
            for other_local_id in &block.locals {
                let other_local = &ctx.heap[*other_local_id];
                // Position check in case another variable with the same name
                // is defined in a higher-level scope, but later than the scope
                // in which the current variable resides.
                if local.this != *other_local_id &&
                    local_relative_pos >= other_local.relative_pos_in_block &&
                    local.identifier == other_local.identifier {
                    // Collision within this scope
                    return Err(
                        ParseError2::new_error(&ctx.module.source, local.position, "Local variable name conflicts with another variable")
                            .with_postfixed_info(&ctx.module.source, other_local.position, "Previous variable is found here")
                    );
                }
            }

            // Current scope is fine, move to parent scope if any
            debug_assert!(block.parent_scope.is_some(), "block scope does not have a parent");
            scope = block.parent_scope.as_ref().unwrap();
            if let Scope::Definition(definition_id) = scope {
                // At outer scope, check parameters of function/component
                for parameter_id in ctx.heap[*definition_id].parameters() {
                    let parameter = &ctx.heap[*parameter_id];
                    if local.identifier == parameter.identifier {
                        return Err(
                            ParseError2::new_error(&ctx.module.source, local.position, "Local variable name conflicts with parameter")
                                .with_postfixed_info(&ctx.module.source, parameter.position, "Parameter definition is found here")
                        );
                    }
                }

                break;
            }

            // If here, then we are dealing with a block-like parent block
            local_relative_pos = ctx.heap[scope.to_block()].relative_pos_in_parent;
        }

        // No collisions at all
        let block = &mut ctx.heap[self.cur_scope.as_ref().unwrap().to_block()];
        block.locals.push(id);

        Ok(())
    }

    /// Finds a variable in the visitor's scope that must appear before the
    /// specified relative position within that block.
    fn find_variable(&self, ctx: &Ctx, mut relative_pos: u32, identifier: &NamespacedIdentifier) -> Result<VariableId, ParseError2> {
        debug_assert!(self.cur_scope.is_some());
        debug_assert!(identifier.parts.len() == 1, "implement namespaced seeking of target associated with identifier");

        // TODO: May still refer to an alias of a global symbol using a single
        //  identifier in the namespace.
        // No need to use iterator over namespaces if here
        let mut scope = self.cur_scope.as_ref().unwrap();
        
        loop {
            debug_assert!(scope.is_block());
            let block = &ctx.heap[scope.to_block()];
            
            for local_id in &block.locals {
                let local = &ctx.heap[*local_id];
                
                if local.relative_pos_in_block < relative_pos && identifier.matches_identifier(&local.identifier) {
                    return Ok(local_id.upcast());
                }
            }

            debug_assert!(block.parent_scope.is_some());
            scope = block.parent_scope.as_ref().unwrap();
            if !scope.is_block() {
                // Definition scope, need to check arguments to definition
                match scope {
                    Scope::Definition(definition_id) => {
                        let definition = &ctx.heap[*definition_id];
                        for parameter_id in definition.parameters() {
                            let parameter = &ctx.heap[*parameter_id];
                            if identifier.matches_identifier(&parameter.identifier) {
                                return Ok(parameter_id.upcast());
                            }
                        }
                    },
                    _ => unreachable!(),
                }

                // Variable could not be found
                return Err(ParseError2::new_error(
                    &ctx.module.source, identifier.position, "This variable is not declared"
                ));
            } else {
                relative_pos = block.relative_pos_in_parent;
            }
        }
    }

    /// Adds a particular label to the current scope. Will return an error if
    /// there is another label with the same name visible in the current scope.
    fn checked_label_add(&mut self, ctx: &mut Ctx, id: LabeledStatementId) -> Result<(), ParseError2> {
        debug_assert!(self.cur_scope.is_some());

        // Make sure label is not defined within the current scope or any of the
        // parent scope.
        let label = &ctx.heap[id];
        let mut scope = self.cur_scope.as_ref().unwrap();

        loop {
            debug_assert!(scope.is_block(), "scope is not a block");
            let block = &ctx.heap[scope.to_block()];
            for other_label_id in &block.labels {
                let other_label = &ctx.heap[*other_label_id];
                if other_label.label == label.label {
                    // Collision
                    return Err(
                        ParseError2::new_error(&ctx.module.source, label.position, "Label name conflicts with another label")
                            .with_postfixed_info(&ctx.module.source, other_label.position, "Other label is found here")
                    );
                }
            }

            debug_assert!(block.parent_scope.is_some(), "block scope does not have a parent");
            scope = block.parent_scope.as_ref().unwrap();
            if !scope.is_block() {
                break;
            }
        }

        // No collisions
        let block = &mut ctx.heap[self.cur_scope.as_ref().unwrap().to_block()];
        block.labels.push(id);

        Ok(())
    }

    /// Finds a particular labeled statement by its identifier. Once found it
    /// will make sure that the target label does not skip over any variable
    /// declarations within the scope in which the label was found.
    fn find_label(&self, ctx: &Ctx, identifier: &Identifier) -> Result<LabeledStatementId, ParseError2> {
        debug_assert!(self.cur_scope.is_some());

        let mut scope = self.cur_scope.as_ref().unwrap();
        loop {
            debug_assert!(scope.is_block(), "scope is not a block");
            let relative_scope_pos = ctx.heap[scope.to_block()].relative_pos_in_parent;

            let block = &ctx.heap[scope.to_block()];
            for label_id in &block.labels {
                let label = &ctx.heap[*label_id];
                if label.label == *identifier {
                    for local_id in &block.locals {
                        // TODO: Better to do this in control flow analysis, it
                        //  is legal to skip over a variable declaration if it
                        //  is not actually being used. I might be missing
                        //  something here when laying out the bytecode...
                        let local = &ctx.heap[*local_id];
                        if local.relative_pos_in_block > relative_scope_pos && local.relative_pos_in_block < label.relative_pos_in_block {
                            return Err(
                                ParseError2::new_error(&ctx.module.source, identifier.position, "This target label skips over a variable declaration")
                                    .with_postfixed_info(&ctx.module.source, label.position, "Because it jumps to this label")
                                    .with_postfixed_info(&ctx.module.source, local.position, "Which skips over this variable")
                            );
                        }
                    }
                    return Ok(*label_id);
                }
            }

            debug_assert!(block.parent_scope.is_some(), "block scope does not have a parent");
            scope = block.parent_scope.as_ref().unwrap();
            if !scope.is_block() {
                return Err(ParseError2::new_error(&ctx.module.source, identifier.position, "Could not find this label"));
            }

        }
    }

    /// Finds a particular symbol in the symbol table which must correspond to
    /// a definition of a particular type.
    // Note: root_id, symbols and types passed in explicitly to prevent
    //  borrowing errors
    fn find_symbol_of_type<'a>(
        &self, source: &InputSource, root_id: RootId, symbols: &SymbolTable, types: &'a TypeTable,
        identifier: &NamespacedIdentifier, expected_type_class: TypeClass
    ) -> Result<&'a DefinedType, ParseError2> {
        // Find symbol associated with identifier
        let (find_result, _) = find_type_definition(symbols, types, root_id, identifier)
            .as_parse_error(source)?;

        let definition_type_class = find_result.definition.type_class();
        if expected_type_class != definition_type_class {
            return Err(ParseError2::new_error(
                source, identifier.position,
                &format!(
                    "Expected to find a {}, this symbol points to a {}",
                    expected_type_class, definition_type_class
                )
            ))
        }

        Ok(find_result)
    }

    /// This function will check if the provided while statement ID has a block
    /// statement that is one of our current parents.
    fn has_parent_while_scope(&self, ctx: &Ctx, id: WhileStatementId) -> bool {
        debug_assert!(self.cur_scope.is_some());
        let mut scope = self.cur_scope.as_ref().unwrap();
        let while_stmt = &ctx.heap[id];
        loop {
            debug_assert!(scope.is_block());
            let block = scope.to_block();
            if while_stmt.body == block.upcast() {
                return true;
            }

            let block = &ctx.heap[block];
            debug_assert!(block.parent_scope.is_some(), "block scope does not have a parent");
            scope = block.parent_scope.as_ref().unwrap();
            if !scope.is_block() {
                return false;
            }
        }
    }

    /// This function should be called while dealing with break/continue
    /// statements. It will try to find the targeted while statement, using the
    /// target label if provided. If a valid target is found then the loop's
    /// ID will be returned, otherwise a parsing error is constructed.
    /// The provided input position should be the position of the break/continue
    /// statement.
    fn resolve_break_or_continue_target(&self, ctx: &Ctx, position: InputPosition, label: &Option<Identifier>) -> Result<WhileStatementId, ParseError2> {
        let target = match label {
            Some(label) => {
                let target_id = self.find_label(ctx, label)?;

                // Make sure break target is a while statement
                let target = &ctx.heap[target_id];
                if let Statement::While(target_stmt) = &ctx.heap[target.body] {
                    // Even though we have a target while statement, the break might not be
                    // present underneath this particular labeled while statement
                    if !self.has_parent_while_scope(ctx, target_stmt.this) {
                        ParseError2::new_error(&ctx.module.source, label.position, "Break statement is not nested under the target label's while statement")
                            .with_postfixed_info(&ctx.module.source, target.position, "The targeted label is found here");
                    }

                    target_stmt.this
                } else {
                    return Err(
                        ParseError2::new_error(&ctx.module.source, label.position, "Incorrect break target label, it must target a while loop")
                            .with_postfixed_info(&ctx.module.source, target.position, "The targeted label is found here")
                    );
                }
            },
            None => {
                // Use the enclosing while statement, the break must be
                // nested within that while statement
                if self.in_while.is_none() {
                    return Err(
                        ParseError2::new_error(&ctx.module.source, position, "Break statement is not nested under a while loop")
                    );
                }

                self.in_while.unwrap()
            }
        };

        // We have a valid target for the break statement. But we need to
        // make sure we will not break out of a synchronous block
        {
            let target_while = &ctx.heap[target];
            if target_while.in_sync != self.in_sync {
                // Break is nested under while statement, so can only escape a
                // sync block if the sync is nested inside the while statement.
                debug_assert!(self.in_sync.is_some());
                let sync_stmt = &ctx.heap[self.in_sync.unwrap()];
                return Err(
                    ParseError2::new_error(&ctx.module.source, position, "Break may not escape the surrounding synchronous block")
                        .with_postfixed_info(&ctx.module.source, target_while.position, "The break escapes out of this loop")
                        .with_postfixed_info(&ctx.module.source, sync_stmt.position, "And would therefore escape this synchronous block")
                );
            }
        }

        Ok(target)
    }

    // TODO: @cleanup, merge with function below
    fn visit_call_poly_args(&mut self, ctx: &mut Ctx, call_id: CallExpressionId) -> VisitorResult {
        // TODO: @token Revisit when tokenizer is implemented
        let call_expr = &mut ctx.heap[call_id];
        if let Method::Symbolic(symbolic) = &mut call_expr.method {
            if let Some(poly_args) = symbolic.identifier.get_poly_args() {
                call_expr.poly_args.extend(poly_args);
            }
        }

        let call_expr = &ctx.heap[call_id];

        // Determine the polyarg signature
        let num_expected_poly_args = match &call_expr.method {
            Method::Create => {
                0
            },
            Method::Fires => {
                1
            },
            Method::Get => {
                1
            },
            Method::Put => {
                1
            }
            Method::Symbolic(symbolic) => {
                // Retrieve type and make sure number of specified polymorphic 
                // arguments is correct

                let definition = &ctx.heap[symbolic.definition.unwrap()];
                match definition {
                    Definition::Function(definition) => definition.poly_vars.len(),
                    Definition::Component(definition) => definition.poly_vars.len(),
                    _ => {
                        debug_assert!(false, "expected function or component definition while visiting call poly args");
                        unreachable!();
                    }
                }
            }
        };

        // We allow zero polyargs to imply all args are inferred. Otherwise the
        // number of arguments must be equal
        if call_expr.poly_args.is_empty() {
            if num_expected_poly_args != 0 {
                // Infer all polyargs
                // TODO: @cleanup Not nice to use method position as implicitly
                //  inferred parser type pos.
                let pos = call_expr.position();
                for _ in 0..num_expected_poly_args {
                    self.parser_type_buffer.push(ctx.heap.alloc_parser_type(|this| ParserType {
                        this,
                        pos,
                        variant: ParserTypeVariant::Inferred,
                    }));
                }

                let call_expr = &mut ctx.heap[call_id];
                call_expr.poly_args.reserve(num_expected_poly_args);
                for _ in 0..num_expected_poly_args {
                    call_expr.poly_args.push(self.parser_type_buffer.pop().unwrap());
                }
            }
            Ok(())
        } else if call_expr.poly_args.len() == num_expected_poly_args {
            // Number of args is not 0, so parse all the specified ParserTypes
            let old_num_types = self.parser_type_buffer.len();
            self.parser_type_buffer.extend(&call_expr.poly_args);
            while self.parser_type_buffer.len() > old_num_types {
                let parser_type_id = self.parser_type_buffer.pop().unwrap();
                self.visit_parser_type(ctx, parser_type_id)?;
            }
            self.parser_type_buffer.truncate(old_num_types);
            Ok(())
        } else {
            return Err(ParseError2::new_error(
                &ctx.module.source, call_expr.position,
                &format!(
                    "Expected {} polymorphic arguments (or none, to infer them), but {} were specified",
                    num_expected_poly_args, call_expr.poly_args.len()
                )
            ));
        }
    }

    fn visit_literal_poly_args(&mut self, ctx: &mut Ctx, lit_id: LiteralExpressionId) -> VisitorResult {
        // TODO: @token Revisit when tokenizer is implemented
        let literal_expr = &mut ctx.heap[lit_id];
        if let Literal::Struct(literal) = &mut literal_expr.value {
            literal.poly_args2.extend(&literal.identifier.poly_args);
        }

        let literal_expr = &ctx.heap[lit_id];
        let literal_pos = literal_expr.position;
        let num_poly_args_to_infer = match &literal_expr.value {
            Literal::Null | Literal::False | Literal::True |
            Literal::Character(_) | Literal::Integer(_) => {
                // Not really an error, but a programmer error as we're likely
                // doing work twice
                debug_assert!(false, "called visit_literal_poly_args on a non-polymorphic literal");
                unreachable!();
            },
            Literal::Struct(literal) => {
                // Retrieve type and make sure number of specified polymorphic
                // arguments is correct.
                let defined_type = ctx.types.get_base_definition(literal.definition.as_ref().unwrap())
                    .unwrap();
                let maybe_poly_args = literal.identifier.get_poly_args();
                let num_to_infer = match_polymorphic_args_to_vars(
                    defined_type, maybe_poly_args, literal.identifier.position
                ).as_parse_error(&ctx.heap, &ctx.module.source)?;

                // Visit all specified parser types
                let old_num_types = self.parser_type_buffer.len();
                self.parser_type_buffer.extend(&literal.poly_args2);
                while self.parser_type_buffer.len() > old_num_types {
                    let parser_type_id = self.parser_type_buffer.pop().unwrap();
                    self.visit_parser_type(ctx, parser_type_id)?;
                }
                self.parser_type_buffer.truncate(old_num_types);

                num_to_infer
            }
        };

        if num_poly_args_to_infer != 0 {
            for _ in 0..num_poly_args_to_infer {
                self.parser_type_buffer.push(ctx.heap.alloc_parser_type(|this| ParserType{
                    this, pos: literal_pos, variant: ParserTypeVariant::Inferred
                }));
            }

            let literal = match &mut ctx.heap[lit_id].value {
                Literal::Struct(literal) => literal,
                _ => unreachable!(),
            };
            literal.poly_args2.reserve(num_poly_args_to_infer);
            for _ in 0..num_poly_args_to_infer {
                literal.poly_args2.push(self.parser_type_buffer.pop().unwrap());
            }
        }

        Ok(())
    }
}
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
        self.visit_stmt(ctx, body_id)
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
        self.visit_stmt(ctx, body_id)
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
            self.visit_parser_type(ctx, parser_type_id);

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::Memory(id);
            self.visit_expr(ctx, ctx.heap[id].initial)?;
            self.expr_parent = ExpressionParent::None;
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

            // No fancy recursive parsing, must be followed by a call expression
            let definition_id = {
                let call_expr = &ctx.heap[call_expr_id];
                if let Method::Symbolic(symbolic) = &call_expr.method {
                    let found_symbol = self.find_symbol_of_type(
                        ctx.module.root_id, &ctx.symbols, &ctx.types,
                        &symbolic.identifier, TypeClass::Component
                    );

                    match found_symbol {
                        FindOfTypeResult::Found(definition_id) => definition_id,
                        FindOfTypeResult::TypeMismatch(got_type_class) => {
                            return Err(ParseError2::new_error(
                                &ctx.module.source, symbolic.identifier.position,
                                &format!("New must instantiate a component, this identifier points to a {}", got_type_class)
                            ))
                        },
                        FindOfTypeResult::NotFound => {
                            return Err(ParseError2::new_error(
                                &ctx.module.source, symbolic.identifier.position,
                                "Could not find a defined component with this name"
                            ))
                        }
                    }
                } else {
                    return Err(
                        ParseError2::new_error(&ctx.module.source, call_expr.position, "Must instantiate a component")
                    );
                }
            };

            // Modify new statement's symbolic call to point to the appropriate
            // definition.
            let call_expr = &mut ctx.heap[call_expr_id];
            match &mut call_expr.method {
                Method::Symbolic(method) => method.definition = Some(definition_id),
                _ => unreachable!()
            }
        } else {
            // Performing depth pass. The function definition should have been
            // resolved in the breadth pass, now we recurse to evaluate the
            // arguments
            // TODO: @cleanup Maybe just call `visit_call_expr`?
            let call_expr_id = ctx.heap[id].expression;
            let call_expr = &mut ctx.heap[call_expr_id];
            call_expr.parent = ExpressionParent::New(id);

            let old_num_exprs = self.expression_buffer.len();
            self.expression_buffer.extend(&call_expr.arguments);
            let new_num_exprs = self.expression_buffer.len();

            let old_expr_parent = self.expr_parent;

            for arg_expr_idx in old_num_exprs..new_num_exprs {
                let arg_expr_id = self.expression_buffer[arg_expr_idx];
                self.expr_parent = ExpressionParent::Expression(call_expr_id.upcast(), arg_expr_idx as u32);
                self.visit_expr(ctx, arg_expr_id)?;
            }

            self.expression_buffer.truncate(old_num_exprs);
            self.expr_parent = old_expr_parent;
        }

        Ok(())
    }

    fn visit_put_stmt(&mut self, ctx: &mut Ctx, id: PutStatementId) -> VisitorResult {
        // TODO: Make `put` an expression. Perhaps silly, but much easier to
        //  perform typechecking
        if self.performing_breadth_pass {
            let put_stmt = &ctx.heap[id];
            if self.in_sync.is_none() {
                return Err(ParseError2::new_error(
                    &ctx.module.source, put_stmt.position, "Put must be called in a synchronous block"
                ));
            }
        } else {
            let put_stmt = &ctx.heap[id];
            let port = put_stmt.port;
            let message = put_stmt.message;

            debug_assert_eq!(self.expr_parent, ExpressionParent::None);
            self.expr_parent = ExpressionParent::Put(id, 0);
            self.visit_expr(ctx, port)?;
            self.expr_parent = ExpressionParent::Put(id, 1);
            self.visit_expr(ctx, message)?;
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

    fn visit_constant_expr(&mut self, ctx: &mut Ctx, id: ConstantExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let constant_expr = &mut ctx.heap[id];
        constant_expr.parent = self.expr_parent;

        Ok(())
    }

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        debug_assert!(!self.performing_breadth_pass);

        let call_expr = &mut ctx.heap[id];

        // Resolve the method to the appropriate definition and check the
        // legality of the particular method call.
        match &mut call_expr.method {
            Method::Create => {},
            Method::Fires => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'fires' may only occur in primitive component definitions"
                    ));
                }
            },
            Method::Get => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError2::new_error(
                        &ctx.module.source, call_expr.position,
                        "A call to 'get' may only occur in primitive component definitions"
                    ));
                }
            },
            Method::Symbolic(symbolic) => {
                // Find symbolic method
                let found_symbol = self.find_symbol_of_type(
                    ctx.module.root_id, &ctx.symbols, &ctx.types,
                    &symbolic.identifier, TypeClass::Function
                );
                let definition_id = match found_symbol {
                    FindOfTypeResult::Found(definition_id) => definition_id,
                    FindOfTypeResult::TypeMismatch(got_type_class) => {
                        return Err(ParseError2::new_error(
                            &ctx.module.source, symbolic.identifier.position,
                            &format!("Only functions can be called, this identifier points to a {}", got_type_class)
                        ))
                    },
                    FindOfTypeResult::NotFound => {
                        return Err(ParseError2::new_error(
                            &ctx.module.source, symbolic.identifier.position,
                            &format!("Could not find a function with this name")
                        ))
                    }
                };

                symbolic.definition = Some(definition_id);
            }
        }

        // Parse all the arguments in the depth pass as well. Note that we check
        // the number of arguments in the type checker.
        let call_expr = &mut ctx.heap[id];
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
        // We visit a particular type rooted in a non-ParserType node in the
        // AST. Within this function we set up a buffer to visit all nested
        // ParserType nodes.
        // The goal is to link symbolic ParserType instances to the appropriate
        // definition or symbolic type. Alternatively to throw an error if we
        // cannot resolve the ParserType to either of these (polymorphic) types.
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
                        if symbolic.identifier.value == poly_var.value {
                            // Type refers to a polymorphic variable.
                            // TODO: @hkt Maybe allow higher-kinded types?
                            if !symbolic.poly_args.is_empty() {
                                return Err(ParseError2::new_error(
                                    &ctx.module.source, symbolic.identifier.position, 
                                    "Polymorphic arguments to a polymorphic variable (higher-kinded types) are not allowed (yet)"
                                ));
                            }
                            symbolic_variant = Some(SymbolicParserTypeVariant::PolyArg(definition_id, poly_var_idx));
                        }
                    }

                    if let Some(symbolic_variant) = symbolic_variant {
                        // Identifier points to a symbolic type
                        (symbolic.identifier.position, symbolic_variant, 0)
                    } else {
                        // Must be a user-defined type, otherwise an error
                        let found_type = find_type_definition(
                            &ctx.symbols, &ctx.types, ctx.module.root_id, &symbolic.identifier
                        ).as_parse_error(&ctx.module.source)?;
                        symbolic_variant = Some(SymbolicParserTypeVariant::Definition(found_type.ast_definition));

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
                        if !found_type.poly_args.is_empty() && symbolic.poly_args.is_empty() {
                            // All inferred
                            (
                                symbolic.identifier.position,
                                SymbolicParserTypeVariant::Definition(found_type.ast_definition),
                                found_type.poly_args.len()
                            )
                        } else if symbolic.poly_args.len() != found_type.poly_args.len() {
                            return Err(ParseError2::new_error(
                                &ctx.module.source, symbolic.identifier.position,
                                &format!(
                                    "Expected {} polymorpic arguments (or none, to infer them), but {} were specified",
                                    found_type.poly_args.len(), symbolic.poly_args.len()
                                )
                            ))
                        } else {
                            // If here then the type is not polymorphic, or all 
                            // types are properly specified by the user.
                            for specified_poly_arg in &symbolic.poly_args {
                                self.parser_type_buffer.push(*specified_poly_arg);
                            }

                            (
                                symbolic.identifier.position,
                                SymbolicParserTypeVariant::Definition(found_type.ast_definition),
                                0
                            )
                        }
                    }
                }
            };

            // If here then type is symbolic, perform a mutable borrow to set
            // the target of the symbolic type.
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

            if let PTV::Symbolic(symbolic) = &mut ctx.heap[id].variant {
                for _ in 0..num_inferred_to_allocate {
                    symbolic.poly_args.push(self.parser_type_buffer.pop().unwrap());
                }
                symbolic.variant = Some(symbolic_variant);
            } else {
                unreachable!();
            }
        }

        Ok(())
    }
}

enum FindOfTypeResult {
    // Identifier was exactly matched, type matched as well
    Found(DefinitionId),
    // Identifier was matched, but the type differs from the expected one
    TypeMismatch(&'static str),
    // Identifier could not be found
    NotFound,
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
                    local.identifier.value == other_local.identifier.value {
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
                    if local.identifier.value == parameter.identifier.value {
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
        debug_assert!(identifier.num_namespaces > 0);

        // TODO: Update once globals are possible as well
        if identifier.num_namespaces > 1 {
            todo!("Implement namespaced constant seeking")
        }

        // TODO: May still refer to an alias of a global symbol using a single
        //  identifier in the namespace.
        // No need to use iterator over namespaces if here
        let mut scope = self.cur_scope.as_ref().unwrap();
        
        loop {
            debug_assert!(scope.is_block());
            let block = &ctx.heap[scope.to_block()];
            
            for local_id in &block.locals {
                let local = &ctx.heap[*local_id];
                
                if local.relative_pos_in_block < relative_pos && local.identifier.value == identifier.value {
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
                            if parameter.identifier.value == identifier.value {
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
                if other_label.label.value == label.label.value {
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
                if label.label.value == identifier.value {
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
    fn find_symbol_of_type(
        &self, root_id: RootId, symbols: &SymbolTable, types: &TypeTable,
        identifier: &NamespacedIdentifier, expected_type_class: TypeClass
    ) -> FindOfTypeResult {
        // Find symbol associated with identifier
        let symbol = symbols.resolve_namespaced_symbol(root_id, &identifier);
        if symbol.is_none() { return FindOfTypeResult::NotFound; }

        let (symbol, iter) = symbol.unwrap();
        if iter.num_remaining() != 0 { return FindOfTypeResult::NotFound; }

        match &symbol.symbol {
            Symbol::Definition((_, definition_id)) => {
                // Make sure definition is of the expected type
                let definition_type = types.get_base_definition(&definition_id);
                debug_assert!(definition_type.is_some(), "Found symbol '{}' in symbol table, but not in type table", String::from_utf8_lossy(&identifier.value));
                let definition_type_class = definition_type.unwrap().definition.type_class();

                if definition_type_class != expected_type_class {
                    FindOfTypeResult::TypeMismatch(definition_type_class.display_name())
                } else {
                    FindOfTypeResult::Found(*definition_id)
                }
            },
            Symbol::Namespace(_) => FindOfTypeResult::TypeMismatch("namespace"),
        }
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
}
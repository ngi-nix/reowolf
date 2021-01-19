use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use crate::protocol::library;
use crate::protocol::parser::{symbol_table::*, type_table::*, LexedModule};

type Unit = ();
pub(crate) type VisitorResult = Result<Unit, ParseError2>;

pub(crate) struct Ctx<'p> {
    heap: &'p mut Heap,
    module: &'p LexedModule,
    symbols: &'p mut SymbolTable,
    types: &'p mut TypeTable,
}

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

            self.visit_definition(ctx, definition_id)
        }
    }

    // Definitions
    // --- enum matching
    fn visit_definition(&mut self, ctx: &mut Ctx, id: DefinitionId) -> VisitorResult {
        match &ctx.heap[id] {
            Definition::Enum(def) => self.visit_enum_definition(ctx, def.this),
            Definition::Struct(def) => self.visit_struct_definition(ctx, def.this),
            Definition::Component(def) => self.visit_component_definition(ctx, def.this),
            Definition::Function(def) => self.visit_function_definition(ctx, def.this)
        }
    }

    // --- enum variant handling
    fn visit_enum_definition(&mut self, _ctx: &mut Ctx, id: EnumId) -> VisitorResult { Ok(()) }
    fn visit_struct_definition(&mut self, _ctx: &mut Ctx, id: StructId) -> VisitorResult { Ok(()) }
    fn visit_component_definition(&mut self, _ctx: &mut Ctx, id: ComponentId) -> VisitorResult { Ok(()) }
    fn visit_function_definition(&mut self, _ctx: &mut Ctx, id: FunctionId) -> VisitorResult { Ok(()) }

    // Statements
    // --- enum matching
    fn visit_stmt(&mut self, ctx: &mut Ctx, id: StatementId) -> VisitorResult {
        match &ctx.heap[id] {
            Statement::Block(stmt) => self.visit_block_stmt(ctx, stmt.this),
            Statement::Local(stmt) => self.visit_local_stmt(ctx, stmt.this),
            Statement::Skip(stmt) => self.visit_skip_stmt(ctx, stmt.this),
            Statement::Labeled(stmt) => self.visit_labeled_stmt(ctx, stmt.this),
            Statement::If(stmt) => self.visit_if_stmt(ctx, stmt.this),
            Statement::EndIf(_stmt) => Ok(()),
            Statement::While(stmt) => self.visit_while_stmt(ctx, stmt.this),
            Statement::EndWhile(_stmt) => Ok(()),
            Statement::Break(stmt) => self.visit_break_stmt(ctx, stmt.this),
            Statement::Continue(stmt) => self.visit_continue_stmt(ctx, stmt.this),
            Statement::Synchronous(stmt) => self.visit_synchronous_stmt(ctx, stmt.this),
            Statement::EndSynchronous(_stmt) => Ok(()),
            Statement::Return(stmt) => self.visit_return_stmt(ctx, stmt.this),
            Statement::Assert(stmt) => self.visit_assert_stmt(ctx, stmt.this),
            Statement::Goto(stmt) => self.visit_goto_stmt(ctx, stmt.this),
            Statement::New(stmt) => self.visit_new_stmt(ctx, stmt.this),
            Statement::Put(stmt) => self.visit_put_stmt(ctx, stmt.this),
            Statement::Expression(stmt) => self.visit_expr_stmt(ctx, stmt.this),
        }
    }

    fn visit_local_stmt(&mut self, ctx: &mut Ctx, id: LocalStatementId) -> VisitorResult {
        match &ctx.heap[id] {
            LocalStatement::Channel(stmt) => self.visit_local_channel_stmt(ctx, stmt.this),
            LocalStatement::Memory(stmt) => self.visit_local_memory_stmt(ctx, stmt.this),
        }
    }

    // --- enum variant handling
    fn visit_block_stmt(&mut self, _ctx: &mut Ctx, _id: BlockStatementId) -> VisitorResult { Ok(()) }
    fn visit_local_memory_stmt(&mut self, _ctx: &mut Ctx, _id: MemoryStatementId) -> VisitorResult { Ok(()) }
    fn visit_local_channel_stmt(&mut self, _ctx: &mut Ctx, _id: ChannelStatementId) -> VisitorResult { Ok(()) }
    fn visit_skip_stmt(&mut self, _ctx: &mut Ctx, _id: SkipStatementId) -> VisitorResult { Ok(()) }
    fn visit_labeled_stmt(&mut self, _ctx: &mut Ctx, _id: LabeledStatementId) -> VisitorResult { Ok(()) }
    fn visit_if_stmt(&mut self, _ctx: &mut Ctx, _id: IfStatementId) -> VisitorResult { Ok(()) }
    fn visit_while_stmt(&mut self, _ctx: &mut Ctx, _id: WhileStatementId) -> VisitorResult { Ok(()) }
    fn visit_break_stmt(&mut self, _ctx: &mut Ctx, _id: BreakStatementId) -> VisitorResult { Ok(()) }
    fn visit_continue_stmt(&mut self, _ctx: &mut Ctx, _id: ContinueStatementId) -> VisitorResult { Ok(()) }
    fn visit_synchronous_stmt(&mut self, _ctx: &mut Ctx, _id: SynchronousStatementId) -> VisitorResult { Ok(()) }
    fn visit_return_stmt(&mut self, _ctx: &mut Ctx, _id: ReturnStatementId) -> VisitorResult { Ok(()) }
    fn visit_assert_stmt(&mut self, _ctx: &mut Ctx, _id: AssertStatementId) -> VisitorResult { Ok(()) }
    fn visit_goto_stmt(&mut self, _ctx: &mut Ctx, _id: GotoStatementId) -> VisitorResult { Ok(()) }
    fn visit_new_stmt(&mut self, _ctx: &mut Ctx, _id: NewStatementId) -> VisitorResult { Ok(()) }
    fn visit_put_stmt(&mut self, _ctx: &mut Ctx, _id: PutStatementId) -> VisitorResult { Ok(()) }
    fn visit_expr_stmt(&mut self, _ctx: &mut Ctx, _id: ExpressionStatementId) -> VisitorResult { Ok(()) }

    // Expressions
    // --- enum matching
    fn visit_expr(&mut self, ctx: &mut Ctx, id: ExpressionId) -> VisitorResult {
        match &ctx.heap[id] {
            Expression::Assignment(expr) => self.visit_assignment_expr(ctx, expr.this),
            Expression::Conditional(expr) => self.visit_conditional_expr(ctx, expr.this),
            Expression::Binary(expr) => self.visit_binary_expr(ctx, expr.this),
            Expression::Unary(expr) => self.visit_unary_expr(ctx, expr.this),
            Expression::Indexing(expr) => self.visit_indexing_expr(ctx, expr.this),
            Expression::Slicing(expr) => self.visit_slicing_expr(ctx, expr.this),
            Expression::Select(expr) => self.visit_select_expr(ctx, expr.this),
            Expression::Array(expr) => self.visit_array_expr(ctx, expr.this),
            Expression::Constant(expr) => self.visit_constant_expr(ctx, expr.this),
            Expression::Call(expr) => self.visit_call_expr(ctx, expr.this),
            Expression::Variable(expr) => self.visit_variable_expr(ctx, expr.this),
        }
    }

    fn visit_assignment_expr(&mut self, _ctx: &mut Ctx, _id: AssignmentExpressionId) -> VisitorResult { Ok(()) }
    fn visit_conditional_expr(&mut self, _ctx: &mut Ctx, _id: ConditionalExpressionId) -> VisitorResult { Ok(()) }
    fn visit_binary_expr(&mut self, _ctx: &mut Ctx, _id: BinaryExpressionId) -> VisitorResult { Ok(()) }
    fn visit_unary_expr(&mut self, _ctx: &mut Ctx, _id: UnaryExpressionId) -> VisitorResult { Ok(()) }
    fn visit_indexing_expr(&mut self, _ctx: &mut Ctx, _id: IndexingExpressionId) -> VisitorResult { Ok(()) }
    fn visit_slicing_expr(&mut self, _ctx: &mut Ctx, _id: SlicingExpressionId) -> VisitorResult { Ok(()) }
    fn visit_select_expr(&mut self, _ctx: &mut Ctx, _id: SelectExpressionId) -> VisitorResult { Ok(()) }
    fn visit_array_expr(&mut self, _ctx: &mut Ctx, _id: ArrayExpressionId) -> VisitorResult { Ok(()) }
    fn visit_constant_expr(&mut self, _ctx: &mut Ctx, _id: ConstantExpressionId) -> VisitorResult { Ok(()) }
    fn visit_call_expr(&mut self, _ctx: &mut Ctx, _id: CallExpressionId) -> VisitorResult { Ok(()) }
    fn visit_variable_expr(&mut self, _ctx: &mut Ctx, _id: VariableExpressionId) -> VisitorResult { Ok(()) }
}

enum DefinitionType {
    Primitive,
    Composite,
    Function
}

struct NoNameYet {
    in_sync: Option<SynchronousStatementId>,
    in_while: Option<WhileStatementId>,
    cur_scope: Option<Scope>,
    def_type: DefinitionType,
    performing_breadth_pass: bool,
    // Keeping track of relative position in block in the breadth-first pass.
    // May not correspond to block.statement[index] if any statements are
    // inserted after the breadth-pass
    relative_pos_in_block: u32,
    // Single buffer of statement IDs that we want to traverse in a block.
    // Required to work around Rust borrowing rules
    // TODO: Maybe remove this in the future
    statement_buffer: Vec<StatementId>,
    // Statements to insert after the breadth pass in a single block
    insert_buffer: Vec<(u32, StatementId)>,
}

impl NoNameYet {
    fn new() -> Self {
        Self{
            in_sync: None,
            in_while: None,
            cur_scope: None,
            def_type: DefinitionType::Primitive,
            performing_breadth_pass: false,
            relative_pos_in_block: 0,
            statement_buffer: Vec::with_capacity(256),
            insert_buffer: Vec::with_capacity(32),
        }
    }

    fn reset_state(&mut self) {
        self.in_sync = None;
        self.in_while = None;
        self.cur_scope = None;
        self.def_type = DefinitionType::Primitive;
        self.relative_pos_in_block = 0;
        self.performing_breadth_pass = false;
        self.statement_buffer.clear();
        self.insert_buffer.clear();
    }
}

impl Visitor2 for NoNameYet {
    //--------------------------------------------------------------------------
    // Definition visitors
    //--------------------------------------------------------------------------

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        self.reset_state();

        let block_id = {
            let def = &ctx.heap[id];
            match def.variant {
                ComponentVariant::Primitive => self.def_type = DefinitionType::Primitive,
                ComponentVariant::Composite => self.def_type = DefinitionType::Composite,
            }

            let body = ctx.heap[def.body].as_block_mut();

            self.statement_buffer.extend_from_slice(&body.statements);
            self.statement_stack_indices.push(0);
            body.this
        };

        self.cur_scope = Some(Scope {
            variant: ScopeVariant::Definition(id.upcast()),
            parent: None,
        });

        self.performing_breadth_pass = true;
        self.visit_block_stmt(ctx, block_id)?;
        self.performing_breadth_pass = false;
        self.visit_block_stmt(ctx, block_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        self.reset_state();

        // Set internal statement indices
        let block_id = {
            let def = &ctx.heap[id];
            self.def_type = DefinitionType::Function;
            let body = ctx.heap[def.body].as_block_mut();

            self.statement_buffer.extend_from_slice(&body.statements);
            self.statement_stack_indices.push(0);
            body.this
        };

        self.cur_scope = Some(Scope {
            variant: ScopeVariant::Definition(id.upcast()),
            parent: None,
        });

        self.performing_breadth_pass = true;
        self.visit_block_stmt(ctx, block_id)?;
        self.performing_breadth_pass = false;
        self.visit_block_stmt(ctx, block_id)
    }

    //--------------------------------------------------------------------------
    // Statement visitors
    //--------------------------------------------------------------------------

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            // Our parent is performing a breadth-pass. We do this simple stuff
            // here
            let body = &mut ctx.heap[id];
            body.parent_scope = self.cur_scope.clone();
            body.relative_pos_in_parent = self.relative_pos_in_block;
            return Ok(())
        }

        // We may descend into children of this block. However, this is
        // where we first perform a breadth-first pass
        self.performing_breadth_pass = true;
        self.cur_scope = Some(Scope::Block(id));
        let first_statement_index = self.statement_buffer.len();

        {
            let body = &ctx.heap[id];
            self.statement_buffer.extend_from_slice(&body.statements);
        }

        let mut stmt_index = first_statement_index;
        while stmt_index < self.statement_buffer.len() {
            self.relative_pos_in_block = (stmt_index - first_statement_index) as u32;
            self.visit_stmt(ctx, self.statement_buffer[stmt_index])?;
            stmt_index += 1;
        }

        if !self.insert_buffer.is_empty() {
            let body = &mut ctx.heap[id];
            for (pos, stmt) in self.insert_buffer.drain(..) {
                body.statements.insert(pos as usize, stmt);
            }
        }

        // And the depth pass
        self.performing_breadth_pass = false;
        stmt_index = first_statement_index;
        while stmt_index < self.statement_buffer.len() {
            self.visit_stmt(ctx, self.statement_buffer[stmt_index])?;
            stmt_index += 1;
        }

        // Pop statement buffer
        debug_assert!(self.insert_buffer.is_empty(), "insert buffer not empty after depth pass");
        self.statement_buffer.truncate(first_statement_index);

        Ok(())
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let stmt = &ctx.heap[id];
            stmt.relative_pos_in_block = self.relative_pos_in_block;
            self.checked_local_add(ctx, stmt.variable)?;
        }

        self.visit_expr(ctx, ctx.heap[id].initial)?;

        Ok(())
    }

    fn visit_labeled_stmt(&mut self, ctx: &mut Ctx, id: LabeledStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            // Retrieve scope
            let scope = self.cur_scope.as_ref().unwrap();
            debug_assert!(scope.statement.is_some(), "expected scope statement at labeled stmt");
            debug_assert_eq!(
                scope.variant == ScopeVariant::Synchronous,
                self.in_sync.is_some(),
                "in synchronous scope variant, but 'in_sync' not set"
            );

            // Add label to block lookup
            self.checked_label_add(ctx, id)?;

            // Modify labeled statement itself
            let labeled = &mut ctx.heap[id];
            labeled.relative_pos_in_block = self.relative_pos_in_block;
            labeled.in_sync = if scope.variant == ScopeVariant::Synchronous {
                self.in_sync.clone()
            } else {
                None
            };
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
        }

        // Traverse expression and bodies
        let (test_id, true_id, false_id) = {
            let stmt = &ctx.heap[id];
            (stmt.test, stmt.true_body, stmt.false_body)
        };
        self.visit_expr(ctx, test_id)?;
        self.visit_stmt(ctx, true_id)?;
        self.visit_stmt(ctx, false_id)?;

        Ok(())
    }

    fn visit_while_stmt(&mut self, ctx: &mut Ctx, id: WhileStatementId) -> VisitorResult {
        if self.performing_breadth_pass {
            let scope = self.cur_scope.as_ref().unwrap();
            let position = ctx.heap[id].position;
            debug_assert_eq!(
                scope.variant == ScopeVariant::Synchronous,
                self.in_sync.is_some(),
                "in synchronous scope variant, but 'in_sync' not set"
            );
            let end_while_id = ctx.heap.alloc_end_while_statement(|this| {
                EndWhileStatement {
                    this,
                    start_while: Some(id),
                    position,
                    next: None,
                }
            });
            let stmt = &mut ctx.heap[id];
            stmt.end_while = Some(end_while_id);
            stmt.in_sync = self.in_sync.clone();

            self.insert_buffer.push((self.relative_pos_in_block + 1, end_while_id.upcast()));
        }

        let (test_id, body_id) = {
            let stmt = &ctx.heap[id];
            (stmt.test, stmt.body)
        };
        let old_while = self.in_while.replace(id);
        self.visit_expr(ctx, test_id)?;
        self.visit_stmt(ctx, body_id)?;
        self.in_while = old_while;

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
        let stmt = &ctx.heap[id];
        stmt.
    }
}

impl NoNameYet {
    //--------------------------------------------------------------------------
    // Utilities
    //--------------------------------------------------------------------------

    fn checked_local_add(&mut self, ctx: &mut Ctx, id: LocalId) -> Result<(), ParseError2> {
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
                if local_relative_pos >= other_local.relative_pos_in_block && local.identifier.value == other_local.identifier.pos {
                    // Collision within this scope
                    return Err(
                        ParseError2::new_error(&ctx.module.source, local.position, "Local variable name conflicts with another variable")
                            .with_postfixed_info(&ctx.module.source, other_local.position, "Previous variable is found here")
                    );
                }
            }

            // Current scope is fine, move to parent scope if any
            debug_assert!(scope.parent.is_some(), "block scope does not have a parent");
            scope = scope.parent.as_ref().unwrap();
            if let ScopeVariant::Definition(definition_id) = scope.variant {
                // At outer scope, check parameters of function/component
                for parameter_id in ctx.heap[definition_id].parameters() {
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

            debug_assert!(scope.parent.is_some(), "block scope does not have a parent");
            scope = scope.parent.as_ref().unwrap();
            if !scope.is_block() {
                break;
            }
        }

        // No collisions
        let block = &mut ctx.heap[self.cur_scope.as_ref().unwrap().to_block()];
        block.labels.push(id);

        Ok(())
    }

    fn find_label(&self, ctx: &Ctx, identifier: &Identifier) -> Result<LabeledStatementId, ParseError2> {
        debug_assert!(self.cur_scope.is_some());

        let mut scope = self.cur_scope.as_ref().unwrap();
        loop {
            debug_assert!(scope.is_block(), "scope is not a block");
            let block = &ctx.heap[scope.to_block()];
            for label_id in &block.labels {
                let label = &ctx.heap[*label_id];
                if label.label.value == identifier.value {
                    return Ok(*label_id);
                }
            }

            debug_assert!(scope.parent.is_some(), "block scope does not have a parent");
            scope = scope.parent.as_ref().unwrap();
            if !scope.is_block() {
                return Err(ParseError2::new_error(&ctx.module.source, identifier.position, "Could not find this label"));
            }
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

            debug_assert!(scope.parent.is_some(), "block scope does not have a parent");
            scope = scope.parent.as_ref().unwrap();
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
                if let Statement::While(target_stmt) = &target.body {
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
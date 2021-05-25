use crate::protocol::ast::*;
use crate::protocol::input_source::*;

// The following indirection is needed due to a bug in the cbindgen tool.
type Unit = ();
pub(crate) type VisitorError = (InputPosition, String); // TODO: Revise when multi-file compiling is in place
pub(crate) type VisitorResult = Result<Unit, VisitorError>;

pub(crate) trait Visitor: Sized {
    fn visit_protocol_description(&mut self, h: &mut Heap, pd: RootId) -> VisitorResult {
        recursive_protocol_description(self, h, pd)
    }
    fn visit_pragma(&mut self, _h: &mut Heap, _pragma: PragmaId) -> VisitorResult {
        Ok(())
    }
    fn visit_import(&mut self, _h: &mut Heap, _import: ImportId) -> VisitorResult {
        Ok(())
    }

    fn visit_symbol_definition(&mut self, h: &mut Heap, def: DefinitionId) -> VisitorResult {
        recursive_symbol_definition(self, h, def)
    }
    fn visit_struct_definition(&mut self, _h: &mut Heap, _def: StructDefinitionId) -> VisitorResult {
        Ok(())
    }
    fn visit_enum_definition(&mut self, _h: &mut Heap, _def: EnumDefinitionId) -> VisitorResult {
        Ok(())
    }
    fn visit_union_definition(&mut self, _h: &mut Heap, _def: UnionDefinitionId) -> VisitorResult {
        Ok(())
    }
    fn visit_component_definition(&mut self, h: &mut Heap, def: ComponentDefinitionId) -> VisitorResult {
        recursive_component_definition(self, h, def)
    }
    fn visit_composite_definition(&mut self, h: &mut Heap, def: ComponentDefinitionId) -> VisitorResult {
        recursive_composite_definition(self, h, def)
    }
    fn visit_primitive_definition(&mut self, h: &mut Heap, def: ComponentDefinitionId) -> VisitorResult {
        recursive_primitive_definition(self, h, def)
    }
    fn visit_function_definition(&mut self, h: &mut Heap, def: FunctionDefinitionId) -> VisitorResult {
        recursive_function_definition(self, h, def)
    }

    fn visit_variable_declaration(&mut self, h: &mut Heap, decl: VariableId) -> VisitorResult {
        Ok(())
    }
    fn visit_statement(&mut self, h: &mut Heap, stmt: StatementId) -> VisitorResult {
        recursive_statement(self, h, stmt)
    }
    fn visit_local_statement(&mut self, h: &mut Heap, stmt: LocalStatementId) -> VisitorResult {
        recursive_local_statement(self, h, stmt)
    }
    fn visit_memory_statement(&mut self, _h: &mut Heap, _stmt: MemoryStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_channel_statement(
        &mut self,
        _h: &mut Heap,
        _stmt: ChannelStatementId,
    ) -> VisitorResult {
        Ok(())
    }
    fn visit_block_statement(&mut self, h: &mut Heap, stmt: BlockStatementId) -> VisitorResult {
        recursive_block_statement(self, h, stmt)
    }
    fn visit_end_block_statement(&mut self, h: &mut Heap, stmt: EndBlockStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_if_statement(&mut self, h: &mut Heap, stmt: IfStatementId) -> VisitorResult {
        recursive_if_statement(self, h, stmt)
    }
    fn visit_end_if_statement(&mut self, _h: &mut Heap, _stmt: EndIfStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_while_statement(&mut self, h: &mut Heap, stmt: WhileStatementId) -> VisitorResult {
        recursive_while_statement(self, h, stmt)
    }
    fn visit_end_while_statement(&mut self, _h: &mut Heap, _stmt: EndWhileStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_break_statement(&mut self, _h: &mut Heap, _stmt: BreakStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_continue_statement(
        &mut self,
        _h: &mut Heap,
        _stmt: ContinueStatementId,
    ) -> VisitorResult {
        Ok(())
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        recursive_synchronous_statement(self, h, stmt)
    }
    fn visit_end_synchronous_statement(&mut self, _h: &mut Heap, _stmt: EndSynchronousStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_return_statement(&mut self, h: &mut Heap, stmt: ReturnStatementId) -> VisitorResult {
        recursive_return_statement(self, h, stmt)
    }
    fn visit_new_statement(&mut self, h: &mut Heap, stmt: NewStatementId) -> VisitorResult {
        recursive_new_statement(self, h, stmt)
    }
    fn visit_goto_statement(&mut self, _h: &mut Heap, _stmt: GotoStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_expression_statement(
        &mut self,
        h: &mut Heap,
        stmt: ExpressionStatementId,
    ) -> VisitorResult {
        recursive_expression_statement(self, h, stmt)
    }

    fn visit_expression(&mut self, h: &mut Heap, expr: ExpressionId) -> VisitorResult {
        recursive_expression(self, h, expr)
    }
    fn visit_assignment_expression(
        &mut self,
        h: &mut Heap,
        expr: AssignmentExpressionId,
    ) -> VisitorResult {
        recursive_assignment_expression(self, h, expr)
    }
    fn visit_binding_expression(
        &mut self,
        h: &mut Heap,
        expr: BindingExpressionId
    ) -> VisitorResult {
        recursive_binding_expression(self, h, expr)
    }
    fn visit_conditional_expression(
        &mut self,
        h: &mut Heap,
        expr: ConditionalExpressionId,
    ) -> VisitorResult {
        recursive_conditional_expression(self, h, expr)
    }
    fn visit_binary_expression(&mut self, h: &mut Heap, expr: BinaryExpressionId) -> VisitorResult {
        recursive_binary_expression(self, h, expr)
    }
    fn visit_unary_expression(&mut self, h: &mut Heap, expr: UnaryExpressionId) -> VisitorResult {
        recursive_unary_expression(self, h, expr)
    }
    fn visit_indexing_expression(
        &mut self,
        h: &mut Heap,
        expr: IndexingExpressionId,
    ) -> VisitorResult {
        recursive_indexing_expression(self, h, expr)
    }
    fn visit_slicing_expression(
        &mut self,
        h: &mut Heap,
        expr: SlicingExpressionId,
    ) -> VisitorResult {
        recursive_slicing_expression(self, h, expr)
    }
    fn visit_select_expression(&mut self, h: &mut Heap, expr: SelectExpressionId) -> VisitorResult {
        recursive_select_expression(self, h, expr)
    }
    fn visit_cast_expression(&mut self, h: &mut Heap, expr: CastExpressionId) -> VisitorResult {
        let subject = h[expr].subject;
        self.visit_expression(h, subject)
    }
    fn visit_call_expression(&mut self, h: &mut Heap, expr: CallExpressionId) -> VisitorResult {
        recursive_call_expression(self, h, expr)
    }
    fn visit_constant_expression(
        &mut self,
        _h: &mut Heap,
        _expr: LiteralExpressionId,
    ) -> VisitorResult {
        Ok(())
    }
    fn visit_variable_expression(
        &mut self,
        _h: &mut Heap,
        _expr: VariableExpressionId,
    ) -> VisitorResult {
        Ok(())
    }
}

fn recursive_call_expression_as_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    call: CallExpressionId,
) -> VisitorResult {
    this.visit_expression(h, call.upcast())
}

// Recursive procedures
fn recursive_protocol_description<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    pd: RootId,
) -> VisitorResult {
    for &pragma in h[pd].pragmas.clone().iter() {
        this.visit_pragma(h, pragma)?;
    }
    for &import in h[pd].imports.clone().iter() {
        this.visit_import(h, import)?;
    }
    for &def in h[pd].definitions.clone().iter() {
        this.visit_symbol_definition(h, def)?;
    }
    Ok(())
}

fn recursive_symbol_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: DefinitionId,
) -> VisitorResult {
    // We clone the definition in case it is modified
    // TODO: Fix me
    match h[def].clone() {
        Definition::Struct(def) => this.visit_struct_definition(h, def.this),
        Definition::Enum(def) => this.visit_enum_definition(h, def.this),
        Definition::Union(def) => this.visit_union_definition(h, def.this),
        Definition::Component(cdef) => this.visit_component_definition(h, cdef.this),
        Definition::Function(fdef) => this.visit_function_definition(h, fdef.this),
    }
}

fn recursive_component_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: ComponentDefinitionId,
) -> VisitorResult {
    let component_variant = h[def].variant;
    match component_variant {
        ComponentVariant::Primitive => this.visit_primitive_definition(h, def),
        ComponentVariant::Composite => this.visit_composite_definition(h, def),
    }
}

fn recursive_composite_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: ComponentDefinitionId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        this.visit_variable_declaration(h, param)?;
    }
    this.visit_block_statement(h, h[def].body)
}

fn recursive_primitive_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: ComponentDefinitionId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        this.visit_variable_declaration(h, param)?;
    }
    this.visit_block_statement(h, h[def].body)
}

fn recursive_function_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: FunctionDefinitionId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        this.visit_variable_declaration(h, param)?;
    }
    this.visit_block_statement(h, h[def].body)
}

fn recursive_statement<T: Visitor>(this: &mut T, h: &mut Heap, stmt: StatementId) -> VisitorResult {
    match h[stmt].clone() {
        Statement::Block(stmt) => this.visit_block_statement(h, stmt.this),
        Statement::EndBlock(stmt) => this.visit_end_block_statement(h, stmt.this),
        Statement::Local(stmt) => this.visit_local_statement(h, stmt.this()),
        Statement::Labeled(stmt) => this.visit_labeled_statement(h, stmt.this),
        Statement::If(stmt) => this.visit_if_statement(h, stmt.this),
        Statement::While(stmt) => this.visit_while_statement(h, stmt.this),
        Statement::Break(stmt) => this.visit_break_statement(h, stmt.this),
        Statement::Continue(stmt) => this.visit_continue_statement(h, stmt.this),
        Statement::Synchronous(stmt) => this.visit_synchronous_statement(h, stmt.this),
        Statement::Return(stmt) => this.visit_return_statement(h, stmt.this),
        Statement::Goto(stmt) => this.visit_goto_statement(h, stmt.this),
        Statement::New(stmt) => this.visit_new_statement(h, stmt.this),
        Statement::Expression(stmt) => this.visit_expression_statement(h, stmt.this),
        Statement::EndSynchronous(stmt) => this.visit_end_synchronous_statement(h, stmt.this),
        Statement::EndWhile(stmt) => this.visit_end_while_statement(h, stmt.this),
        Statement::EndIf(stmt) => this.visit_end_if_statement(h, stmt.this),
    }
}

fn recursive_block_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    block: BlockStatementId,
) -> VisitorResult {
    for &local in h[block].locals.clone().iter() {
        this.visit_variable_declaration(h, local)?;
    }
    for &stmt in h[block].statements.clone().iter() {
        this.visit_statement(h, stmt)?;
    }
    Ok(())
}

fn recursive_local_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: LocalStatementId,
) -> VisitorResult {
    match h[stmt].clone() {
        LocalStatement::Channel(stmt) => this.visit_channel_statement(h, stmt.this),
        LocalStatement::Memory(stmt) => this.visit_memory_statement(h, stmt.this),
    }
}

fn recursive_labeled_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: LabeledStatementId,
) -> VisitorResult {
    this.visit_statement(h, h[stmt].body)
}

fn recursive_if_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: IfStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].test)?;
    this.visit_block_statement(h, h[stmt].true_body)?;
    if let Some(false_body) = h[stmt].false_body {
        this.visit_block_statement(h, false_body)?;
    }
    Ok(())
}

fn recursive_while_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: WhileStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].test)?;
    this.visit_block_statement(h, h[stmt].body)
}

fn recursive_synchronous_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: SynchronousStatementId,
) -> VisitorResult {
    // TODO: Check where this was used for
    // for &param in h[stmt].parameters.clone().iter() {
    //     recursive_parameter_as_variable(this, h, param)?;
    // }
    this.visit_block_statement(h, h[stmt].body)
}

fn recursive_return_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: ReturnStatementId,
) -> VisitorResult {
    debug_assert_eq!(h[stmt].expressions.len(), 1);
    this.visit_expression(h, h[stmt].expressions[0])
}

fn recursive_new_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: NewStatementId,
) -> VisitorResult {
    recursive_call_expression_as_expression(this, h, h[stmt].expression)
}

fn recursive_expression_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: ExpressionStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].expression)
}

fn recursive_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: ExpressionId,
) -> VisitorResult {
    match h[expr].clone() {
        Expression::Assignment(expr) => this.visit_assignment_expression(h, expr.this),
        Expression::Binding(expr) => this.visit_binding_expression(h, expr.this),
        Expression::Conditional(expr) => this.visit_conditional_expression(h, expr.this),
        Expression::Binary(expr) => this.visit_binary_expression(h, expr.this),
        Expression::Unary(expr) => this.visit_unary_expression(h, expr.this),
        Expression::Indexing(expr) => this.visit_indexing_expression(h, expr.this),
        Expression::Slicing(expr) => this.visit_slicing_expression(h, expr.this),
        Expression::Select(expr) => this.visit_select_expression(h, expr.this),
        Expression::Literal(expr) => this.visit_constant_expression(h, expr.this),
        Expression::Cast(expr) => this.visit_cast_expression(h, expr.this),
        Expression::Call(expr) => this.visit_call_expression(h, expr.this),
        Expression::Variable(expr) => this.visit_variable_expression(h, expr.this),
    }
}

fn recursive_assignment_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: AssignmentExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].left)?;
    this.visit_expression(h, h[expr].right)
}

fn recursive_binding_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: BindingExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].bound_from)?;
    this.visit_expression(h, h[expr].bound_to)
}

fn recursive_conditional_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: ConditionalExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].test)?;
    this.visit_expression(h, h[expr].true_expression)?;
    this.visit_expression(h, h[expr].false_expression)
}

fn recursive_binary_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: BinaryExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].left)?;
    this.visit_expression(h, h[expr].right)
}

fn recursive_unary_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: UnaryExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].expression)
}

fn recursive_indexing_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: IndexingExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].subject)?;
    this.visit_expression(h, h[expr].index)
}

fn recursive_slicing_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: SlicingExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].subject)?;
    this.visit_expression(h, h[expr].from_index)?;
    this.visit_expression(h, h[expr].to_index)
}

fn recursive_select_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: SelectExpressionId,
) -> VisitorResult {
    this.visit_expression(h, h[expr].subject)
}

fn recursive_call_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: CallExpressionId,
) -> VisitorResult {
    for &expr in h[expr].arguments.clone().iter() {
        this.visit_expression(h, expr)?;
    }
    Ok(())
}

// ====================
// Grammar Rules
// ====================

pub(crate) struct UniqueStatementId(StatementId);

pub(crate) struct LinkStatements {
    prev: Option<UniqueStatementId>,
}

impl LinkStatements {
    pub(crate) fn new() -> Self {
        LinkStatements { prev: None }
    }
}

impl Visitor for LinkStatements {
    fn visit_symbol_definition(&mut self, h: &mut Heap, def: DefinitionId) -> VisitorResult {
        assert!(self.prev.is_none());
        recursive_symbol_definition(self, h, def)?;
        // Clear out last statement
        self.prev = None;
        Ok(())
    }
    fn visit_statement(&mut self, h: &mut Heap, stmt: StatementId) -> VisitorResult {
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(stmt);
        }
        recursive_statement(self, h, stmt)
    }
    fn visit_block_statement(&mut self, h: &mut Heap, stmt: BlockStatementId) -> VisitorResult {
        if let Some(prev) = self.prev.take() {
            h[prev.0].link_next(stmt.upcast());
        }
        let end_block = h[stmt].end_block;
        recursive_block_statement(self, h, stmt)?;
        if let Some(prev) = self.prev.take() {
            h[prev.0].link_next(end_block.upcast());
        }
        self.prev = Some(UniqueStatementId(end_block.upcast()));
        Ok(())
    }
    fn visit_local_statement(&mut self, _h: &mut Heap, stmt: LocalStatementId) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_if_statement(&mut self, h: &mut Heap, stmt: IfStatementId) -> VisitorResult {
        // Link the two branches to the corresponding EndIf pseudo-statement
        let end_if_id = h[stmt].end_if;
        assert!(!end_if_id.is_invalid());

        assert!(self.prev.is_none());
        self.visit_block_statement(h, h[stmt].true_body)?;
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(end_if_id.upcast());
        }

        assert!(self.prev.is_none());
        if let Some(false_body) = h[stmt].false_body {
            self.visit_block_statement(h, false_body)?;
        }
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(end_if_id.upcast());
        }

        // Use the pseudo-statement as the statement where to update the next pointer
        // self.prev = Some(UniqueStatementId(end_if_id.upcast()));
        Ok(())
    }
    fn visit_end_if_statement(&mut self, _h: &mut Heap, stmt: EndIfStatementId) -> VisitorResult {
        assert!(self.prev.is_none());
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_while_statement(&mut self, h: &mut Heap, stmt: WhileStatementId) -> VisitorResult {
        // We allocate a pseudo-statement, to which the break statement finds its target
        // Update the while's next statement to point to the pseudo-statement
        let end_while_id = h[stmt].end_while;
        assert!(!end_while_id.is_invalid());

        assert!(self.prev.is_none());
        self.visit_block_statement(h, h[stmt].body)?;
        // The body's next statement loops back to the while statement itself
        // Note: continue statements also loop back to the while statement itself
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(stmt.upcast());
        }
        // Use the while statement as the statement where the next pointer is updated
        // self.prev = Some(UniqueStatementId(end_while_id.upcast()));
        Ok(())
    }
    fn visit_end_while_statement(&mut self, _h: &mut Heap, stmt: EndWhileStatementId) -> VisitorResult {
        assert!(self.prev.is_none());
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_break_statement(&mut self, _h: &mut Heap, _stmt: BreakStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_continue_statement(
        &mut self,
        _h: &mut Heap,
        _stmt: ContinueStatementId,
    ) -> VisitorResult {
        Ok(())
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        // Allocate a pseudo-statement, that is added for helping the evaluator to issue a command
        // that marks the end of the synchronous block. Every evaluation has to pause at this
        // point, only to resume later when the thread is selected as unique thread to continue.
        let end_sync_id = h[stmt].end_sync;
        assert!(!end_sync_id.is_invalid());

        assert!(self.prev.is_none());
        self.visit_block_statement(h, h[stmt].body)?;
        // The body's next statement points to the pseudo element
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(end_sync_id.upcast());
        }
        // Use the pseudo-statement as the statement where the next pointer is updated
        // self.prev = Some(UniqueStatementId(end_sync_id.upcast()));
        Ok(())
    }
    fn visit_end_synchronous_statement(&mut self, _h: &mut Heap, stmt: EndSynchronousStatementId) -> VisitorResult {
        assert!(self.prev.is_none());
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_return_statement(&mut self, _h: &mut Heap, _stmt: ReturnStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_goto_statement(&mut self, _h: &mut Heap, _stmt: GotoStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_new_statement(&mut self, _h: &mut Heap, stmt: NewStatementId) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_expression_statement(
        &mut self,
        _h: &mut Heap,
        stmt: ExpressionStatementId,
    ) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}
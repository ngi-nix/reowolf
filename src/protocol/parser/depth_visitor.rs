use crate::protocol::ast::*;
use crate::protocol::inputsource::*;

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
    fn visit_struct_definition(&mut self, _h: &mut Heap, _def: StructId) -> VisitorResult {
        Ok(())
    }
    fn visit_enum_definition(&mut self, _h: &mut Heap, _def: EnumId) -> VisitorResult {
        Ok(())
    }
    fn visit_union_definition(&mut self, _h: &mut Heap, _def: UnionId) -> VisitorResult {
        Ok(())
    }
    fn visit_component_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        recursive_component_definition(self, h, def)
    }
    fn visit_composite_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        recursive_composite_definition(self, h, def)
    }
    fn visit_primitive_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        recursive_primitive_definition(self, h, def)
    }
    fn visit_function_definition(&mut self, h: &mut Heap, def: FunctionId) -> VisitorResult {
        recursive_function_definition(self, h, def)
    }

    fn visit_variable_declaration(&mut self, h: &mut Heap, decl: VariableId) -> VisitorResult {
        recursive_variable_declaration(self, h, decl)
    }
    fn visit_parameter_declaration(&mut self, _h: &mut Heap, _decl: ParameterId) -> VisitorResult {
        Ok(())
    }
    fn visit_local_declaration(&mut self, _h: &mut Heap, _decl: LocalId) -> VisitorResult {
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
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_skip_statement(&mut self, _h: &mut Heap, _stmt: SkipStatementId) -> VisitorResult {
        Ok(())
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
    fn visit_assert_statement(&mut self, h: &mut Heap, stmt: AssertStatementId) -> VisitorResult {
        recursive_assert_statement(self, h, stmt)
    }
    fn visit_goto_statement(&mut self, _h: &mut Heap, _stmt: GotoStatementId) -> VisitorResult {
        Ok(())
    }
    fn visit_new_statement(&mut self, h: &mut Heap, stmt: NewStatementId) -> VisitorResult {
        recursive_new_statement(self, h, stmt)
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
    fn visit_array_expression(&mut self, h: &mut Heap, expr: ArrayExpressionId) -> VisitorResult {
        recursive_array_expression(self, h, expr)
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

// Bubble-up helpers
fn recursive_parameter_as_variable<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    param: ParameterId,
) -> VisitorResult {
    this.visit_variable_declaration(h, param.upcast())
}

fn recursive_local_as_variable<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    local: LocalId,
) -> VisitorResult {
    this.visit_variable_declaration(h, local.upcast())
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
    def: ComponentId,
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
    def: ComponentId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        recursive_parameter_as_variable(this, h, param)?;
    }
    this.visit_statement(h, h[def].body)
}

fn recursive_primitive_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: ComponentId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        recursive_parameter_as_variable(this, h, param)?;
    }
    this.visit_statement(h, h[def].body)
}

fn recursive_function_definition<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    def: FunctionId,
) -> VisitorResult {
    for &param in h[def].parameters.clone().iter() {
        recursive_parameter_as_variable(this, h, param)?;
    }
    this.visit_statement(h, h[def].body)
}

fn recursive_variable_declaration<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    decl: VariableId,
) -> VisitorResult {
    match h[decl].clone() {
        Variable::Parameter(decl) => this.visit_parameter_declaration(h, decl.this),
        Variable::Local(decl) => this.visit_local_declaration(h, decl.this),
    }
}

fn recursive_statement<T: Visitor>(this: &mut T, h: &mut Heap, stmt: StatementId) -> VisitorResult {
    match h[stmt].clone() {
        Statement::Block(stmt) => this.visit_block_statement(h, stmt.this),
        Statement::Local(stmt) => this.visit_local_statement(h, stmt.this()),
        Statement::Skip(stmt) => this.visit_skip_statement(h, stmt.this),
        Statement::Labeled(stmt) => this.visit_labeled_statement(h, stmt.this),
        Statement::If(stmt) => this.visit_if_statement(h, stmt.this),
        Statement::While(stmt) => this.visit_while_statement(h, stmt.this),
        Statement::Break(stmt) => this.visit_break_statement(h, stmt.this),
        Statement::Continue(stmt) => this.visit_continue_statement(h, stmt.this),
        Statement::Synchronous(stmt) => this.visit_synchronous_statement(h, stmt.this),
        Statement::Return(stmt) => this.visit_return_statement(h, stmt.this),
        Statement::Assert(stmt) => this.visit_assert_statement(h, stmt.this),
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
        recursive_local_as_variable(this, h, local)?;
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
    this.visit_statement(h, h[stmt].true_body)?;
    this.visit_statement(h, h[stmt].false_body)
}

fn recursive_while_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: WhileStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].test)?;
    this.visit_statement(h, h[stmt].body)
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
    this.visit_statement(h, h[stmt].body)
}

fn recursive_return_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: ReturnStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].expression)
}

fn recursive_assert_statement<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    stmt: AssertStatementId,
) -> VisitorResult {
    this.visit_expression(h, h[stmt].expression)
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
        Expression::Array(expr) => this.visit_array_expression(h, expr.this),
        Expression::Literal(expr) => this.visit_constant_expression(h, expr.this),
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
    this.visit_expression(h, h[expr].left.upcast())?;
    this.visit_expression(h, h[expr].right)
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

fn recursive_array_expression<T: Visitor>(
    this: &mut T,
    h: &mut Heap,
    expr: ArrayExpressionId,
) -> VisitorResult {
    for &expr in h[expr].elements.clone().iter() {
        this.visit_expression(h, expr)?;
    }
    Ok(())
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

pub(crate) struct NestedSynchronousStatements {
    illegal: bool,
}

impl NestedSynchronousStatements {
    pub(crate) fn new() -> Self {
        NestedSynchronousStatements { illegal: false }
    }
}

impl Visitor for NestedSynchronousStatements {
    fn visit_composite_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        assert!(!self.illegal);
        self.illegal = true;
        recursive_composite_definition(self, h, def)?;
        self.illegal = false;
        Ok(())
    }
    fn visit_function_definition(&mut self, h: &mut Heap, def: FunctionId) -> VisitorResult {
        assert!(!self.illegal);
        self.illegal = true;
        recursive_function_definition(self, h, def)?;
        self.illegal = false;
        Ok(())
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        if self.illegal {
            return Err((
                h[stmt].position(),
                "Illegal nested synchronous statement".to_string(),
            ));
        }
        self.illegal = true;
        recursive_synchronous_statement(self, h, stmt)?;
        self.illegal = false;
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct ChannelStatementOccurrences {
    illegal: bool,
}

impl ChannelStatementOccurrences {
    pub(crate) fn new() -> Self {
        ChannelStatementOccurrences { illegal: false }
    }
}

impl Visitor for ChannelStatementOccurrences {
    fn visit_primitive_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        assert!(!self.illegal);
        self.illegal = true;
        recursive_primitive_definition(self, h, def)?;
        self.illegal = false;
        Ok(())
    }
    fn visit_function_definition(&mut self, h: &mut Heap, def: FunctionId) -> VisitorResult {
        assert!(!self.illegal);
        self.illegal = true;
        recursive_function_definition(self, h, def)?;
        self.illegal = false;
        Ok(())
    }
    fn visit_channel_statement(&mut self, h: &mut Heap, stmt: ChannelStatementId) -> VisitorResult {
        if self.illegal {
            return Err((h[stmt].position(), "Illegal channel declaration".to_string()));
        }
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct FunctionStatementReturns {}

impl FunctionStatementReturns {
    pub(crate) fn new() -> Self {
        FunctionStatementReturns {}
    }
    fn function_error(&self, position: InputPosition) -> VisitorResult {
        Err((position, "Function definition must return".to_string()))
    }
}

impl Visitor for FunctionStatementReturns {
    fn visit_component_definition(&mut self, _h: &mut Heap, _def: ComponentId) -> VisitorResult {
        Ok(())
    }
    fn visit_variable_declaration(&mut self, _h: &mut Heap, _decl: VariableId) -> VisitorResult {
        Ok(())
    }
    fn visit_block_statement(&mut self, h: &mut Heap, block: BlockStatementId) -> VisitorResult {
        let len = h[block].statements.len();
        assert!(len > 0);
        self.visit_statement(h, h[block].statements[len - 1])
    }
    fn visit_skip_statement(&mut self, h: &mut Heap, stmt: SkipStatementId) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_break_statement(&mut self, h: &mut Heap, stmt: BreakStatementId) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_continue_statement(
        &mut self,
        h: &mut Heap,
        stmt: ContinueStatementId,
    ) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_assert_statement(&mut self, h: &mut Heap, stmt: AssertStatementId) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_new_statement(&mut self, h: &mut Heap, stmt: NewStatementId) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_expression_statement(
        &mut self,
        h: &mut Heap,
        stmt: ExpressionStatementId,
    ) -> VisitorResult {
        self.function_error(h[stmt].position)
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct ComponentStatementReturnNew {
    illegal_new: bool,
    illegal_return: bool,
}

impl ComponentStatementReturnNew {
    pub(crate) fn new() -> Self {
        ComponentStatementReturnNew { illegal_new: false, illegal_return: false }
    }
}

impl Visitor for ComponentStatementReturnNew {
    fn visit_component_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        assert!(!(self.illegal_new || self.illegal_return));
        self.illegal_return = true;
        recursive_component_definition(self, h, def)?;
        self.illegal_return = false;
        Ok(())
    }
    fn visit_primitive_definition(&mut self, h: &mut Heap, def: ComponentId) -> VisitorResult {
        assert!(!self.illegal_new);
        self.illegal_new = true;
        recursive_primitive_definition(self, h, def)?;
        self.illegal_new = false;
        Ok(())
    }
    fn visit_function_definition(&mut self, h: &mut Heap, def: FunctionId) -> VisitorResult {
        assert!(!(self.illegal_new || self.illegal_return));
        self.illegal_new = true;
        recursive_function_definition(self, h, def)?;
        self.illegal_new = false;
        Ok(())
    }
    fn visit_variable_declaration(&mut self, _h: &mut Heap, _decl: VariableId) -> VisitorResult {
        Ok(())
    }
    fn visit_return_statement(&mut self, h: &mut Heap, stmt: ReturnStatementId) -> VisitorResult {
        if self.illegal_return {
            Err((h[stmt].position, "Component definition must not return".to_string()))
        } else {
            recursive_return_statement(self, h, stmt)
        }
    }
    fn visit_new_statement(&mut self, h: &mut Heap, stmt: NewStatementId) -> VisitorResult {
        if self.illegal_new {
            Err((
                h[stmt].position,
                "Symbol definition contains illegal new statement".to_string(),
            ))
        } else {
            recursive_new_statement(self, h, stmt)
        }
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct CheckBuiltinOccurrences {
    legal: bool,
}

impl CheckBuiltinOccurrences {
    pub(crate) fn new() -> Self {
        CheckBuiltinOccurrences { legal: false }
    }
}

impl Visitor for CheckBuiltinOccurrences {
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        assert!(!self.legal);
        self.legal = true;
        recursive_synchronous_statement(self, h, stmt)?;
        self.legal = false;
        Ok(())
    }
    fn visit_call_expression(&mut self, h: &mut Heap, expr: CallExpressionId) -> VisitorResult {
        match h[expr].method {
            Method::Get | Method::Fires => {
                if !self.legal {
                    return Err((h[expr].position, "Illegal built-in occurrence".to_string()));
                }
            }
            _ => {}
        }
        recursive_call_expression(self, h, expr)
    }
}

pub(crate) struct BuildScope {
    scope: Option<Scope>,
}

impl BuildScope {
    pub(crate) fn new() -> Self {
        BuildScope { scope: None }
    }
}

impl Visitor for BuildScope {
    fn visit_symbol_definition(&mut self, h: &mut Heap, def: DefinitionId) -> VisitorResult {
        assert!(self.scope.is_none());
        self.scope = Some(Scope::Definition(def));
        recursive_symbol_definition(self, h, def)?;
        self.scope = None;
        Ok(())
    }
    fn visit_block_statement(&mut self, h: &mut Heap, stmt: BlockStatementId) -> VisitorResult {
        assert!(!self.scope.is_none());
        let old = self.scope;
        // First store the current scope
        h[stmt].parent_scope = self.scope;
        // Then move scope down to current block
        self.scope = Some(Scope::Regular(stmt));
        recursive_block_statement(self, h, stmt)?;
        // Move scope back up
        self.scope = old;
        Ok(())
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        assert!(!self.scope.is_none());
        let old = self.scope;
        // First store the current scope
        h[stmt].parent_scope = self.scope;
        // Then move scope down to current sync
        // TODO: Should be legal-ish, but very wrong
        self.scope = Some(Scope::Synchronous((stmt, BlockStatementId(stmt.upcast()))));
        recursive_synchronous_statement(self, h, stmt)?;
        // Move scope back up
        self.scope = old;
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

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
    fn visit_local_statement(&mut self, _h: &mut Heap, stmt: LocalStatementId) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_skip_statement(&mut self, _h: &mut Heap, stmt: SkipStatementId) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
        Ok(())
    }
    fn visit_if_statement(&mut self, h: &mut Heap, stmt: IfStatementId) -> VisitorResult {
        // Link the two branches to the corresponding EndIf pseudo-statement
        let end_if_id = h[stmt].end_if;
        assert!(end_if_id.is_some());
        let end_if_id = end_if_id.unwrap();

        assert!(self.prev.is_none());
        self.visit_statement(h, h[stmt].true_body)?;
        if let Some(UniqueStatementId(prev)) = self.prev.take() {
            h[prev].link_next(end_if_id.upcast());
        }

        assert!(self.prev.is_none());
        self.visit_statement(h, h[stmt].false_body)?;
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
        assert!(end_while_id.is_some());
        // let end_while_id = end_while_id.unwrap();

        assert!(self.prev.is_none());
        self.visit_statement(h, h[stmt].body)?;
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
        assert!(end_sync_id.is_some());
        let end_sync_id = end_sync_id.unwrap();

        assert!(self.prev.is_none());
        self.visit_statement(h, h[stmt].body)?;
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
    fn visit_assert_statement(&mut self, _h: &mut Heap, stmt: AssertStatementId) -> VisitorResult {
        self.prev = Some(UniqueStatementId(stmt.upcast()));
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

pub(crate) struct BuildLabels {
    block: Option<BlockStatementId>,
    sync_enclosure: Option<SynchronousStatementId>,
}

impl BuildLabels {
    pub(crate) fn new() -> Self {
        BuildLabels { block: None, sync_enclosure: None }
    }
}

impl Visitor for BuildLabels {
    fn visit_block_statement(&mut self, h: &mut Heap, stmt: BlockStatementId) -> VisitorResult {
        assert_eq!(self.block, h[stmt].parent_block(h));
        let old = self.block;
        self.block = Some(stmt);
        recursive_block_statement(self, h, stmt)?;
        self.block = old;
        Ok(())
    }
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        assert!(!self.block.is_none());
        // Store label in current block (on the fly)
        h[self.block.unwrap()].labels.push(stmt);
        // Update synchronous scope of label
        h[stmt].in_sync = self.sync_enclosure;
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_while_statement(&mut self, h: &mut Heap, stmt: WhileStatementId) -> VisitorResult {
        h[stmt].in_sync = self.sync_enclosure;
        recursive_while_statement(self, h, stmt)
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        assert!(self.sync_enclosure.is_none());
        self.sync_enclosure = Some(stmt);
        recursive_synchronous_statement(self, h, stmt)?;
        self.sync_enclosure = None;
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct ResolveLabels {
    block: Option<BlockStatementId>,
    while_enclosure: Option<WhileStatementId>,
    sync_enclosure: Option<SynchronousStatementId>,
}

impl ResolveLabels {
    pub(crate) fn new() -> Self {
        ResolveLabels { block: None, while_enclosure: None, sync_enclosure: None }
    }
    fn check_duplicate_impl(
        h: &Heap,
        block: Option<BlockStatementId>,
        stmt: LabeledStatementId,
    ) -> VisitorResult {
        if let Some(block) = block {
            // Checking the parent first is important. Otherwise, labels
            // overshadow previously defined labels: and this is illegal!
            ResolveLabels::check_duplicate_impl(h, h[block].parent_block(h), stmt)?;
            // For the current block, check for a duplicate.
            for &other_stmt in h[block].labels.iter() {
                if other_stmt == stmt {
                    continue;
                } else {
                    if h[other_stmt].label == h[stmt].label {
                        return Err((h[stmt].position, "Duplicate label".to_string()));
                    }
                }
            }
        }
        Ok(())
    }
    fn check_duplicate(&self, h: &Heap, stmt: LabeledStatementId) -> VisitorResult {
        ResolveLabels::check_duplicate_impl(h, self.block, stmt)
    }
    fn get_target(
        &self,
        h: &Heap,
        id: &Identifier,
    ) -> Result<LabeledStatementId, VisitorError> {
        if let Some(stmt) = ResolveLabels::find_target(h, self.block, id) {
            Ok(stmt)
        } else {
            Err((id.position, "Unresolved label".to_string()))
        }
    }
    fn find_target(
        h: &Heap,
        block: Option<BlockStatementId>,
        id: &Identifier,
    ) -> Option<LabeledStatementId> {
        if let Some(block) = block {
            // It does not matter in what order we find the labels.
            // If there are duplicates: that is checked elsewhere.
            for &stmt in h[block].labels.iter() {
                if h[stmt].label == *id {
                    return Some(stmt);
                }
            }
            if let Some(stmt) = ResolveLabels::find_target(h, h[block].parent_block(h), id) {
                return Some(stmt);
            }
        }
        None
    }
}

impl Visitor for ResolveLabels {
    fn visit_block_statement(&mut self, h: &mut Heap, stmt: BlockStatementId) -> VisitorResult {
        assert_eq!(self.block, h[stmt].parent_block(h));
        let old = self.block;
        self.block = Some(stmt);
        recursive_block_statement(self, h, stmt)?;
        self.block = old;
        Ok(())
    }
    fn visit_labeled_statement(&mut self, h: &mut Heap, stmt: LabeledStatementId) -> VisitorResult {
        assert!(!self.block.is_none());
        self.check_duplicate(h, stmt)?;
        recursive_labeled_statement(self, h, stmt)
    }
    fn visit_while_statement(&mut self, h: &mut Heap, stmt: WhileStatementId) -> VisitorResult {
        let old = self.while_enclosure;
        self.while_enclosure = Some(stmt);
        recursive_while_statement(self, h, stmt)?;
        self.while_enclosure = old;
        Ok(())
    }
    fn visit_break_statement(&mut self, h: &mut Heap, stmt: BreakStatementId) -> VisitorResult {
        let the_while;
        if let Some(label) = &h[stmt].label {
            let target = self.get_target(h, label)?;
            let target = &h[h[target].body];
            if !target.is_while() {
                return Err((
                    h[stmt].position,
                    "Illegal break: target not a while statement".to_string(),
                ));
            }
            the_while = target.as_while();
            // TODO: check if break is nested under while
        } else {
            if self.while_enclosure.is_none() {
                return Err((
                    h[stmt].position,
                    "Illegal break: no surrounding while statement".to_string(),
                ));
            }
            the_while = &h[self.while_enclosure.unwrap()];
            // break is always nested under while, by recursive vistor
        }
        if the_while.in_sync != self.sync_enclosure {
            return Err((
                h[stmt].position,
                "Illegal break: synchronous statement escape".to_string(),
            ));
        }
        h[stmt].target = the_while.end_while;
        Ok(())
    }
    fn visit_continue_statement(
        &mut self,
        h: &mut Heap,
        stmt: ContinueStatementId,
    ) -> VisitorResult {
        let the_while;
        if let Some(label) = &h[stmt].label {
            let target = self.get_target(h, label)?;
            let target = &h[h[target].body];
            if !target.is_while() {
                return Err((
                    h[stmt].position,
                    "Illegal continue: target not a while statement".to_string(),
                ));
            }
            the_while = target.as_while();
            // TODO: check if continue is nested under while
        } else {
            if self.while_enclosure.is_none() {
                return Err((
                    h[stmt].position,
                    "Illegal continue: no surrounding while statement".to_string(),
                ));
            }
            the_while = &h[self.while_enclosure.unwrap()];
            // continue is always nested under while, by recursive vistor
        }
        if the_while.in_sync != self.sync_enclosure {
            return Err((
                h[stmt].position,
                "Illegal continue: synchronous statement escape".to_string(),
            ));
        }
        h[stmt].target = Some(the_while.this);
        Ok(())
    }
    fn visit_synchronous_statement(
        &mut self,
        h: &mut Heap,
        stmt: SynchronousStatementId,
    ) -> VisitorResult {
        assert!(self.sync_enclosure.is_none());
        self.sync_enclosure = Some(stmt);
        recursive_synchronous_statement(self, h, stmt)?;
        self.sync_enclosure = None;
        Ok(())
    }
    fn visit_goto_statement(&mut self, h: &mut Heap, stmt: GotoStatementId) -> VisitorResult {
        let target = self.get_target(h, &h[stmt].label)?;
        if h[target].in_sync != self.sync_enclosure {
            return Err((
                h[stmt].position,
                "Illegal goto: synchronous statement escape".to_string(),
            ));
        }
        h[stmt].target = Some(target);
        Ok(())
    }
    fn visit_expression(&mut self, _h: &mut Heap, _expr: ExpressionId) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct AssignableExpressions {
    assignable: bool,
}

impl AssignableExpressions {
    pub(crate) fn new() -> Self {
        AssignableExpressions { assignable: false }
    }
    fn error(&self, position: InputPosition) -> VisitorResult {
        Err((position, "Unassignable expression".to_string()))
    }
}

impl Visitor for AssignableExpressions {
    fn visit_assignment_expression(
        &mut self,
        h: &mut Heap,
        expr: AssignmentExpressionId,
    ) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            self.assignable = true;
            self.visit_expression(h, h[expr].left)?;
            self.assignable = false;
            self.visit_expression(h, h[expr].right)
        }
    }
    fn visit_conditional_expression(
        &mut self,
        h: &mut Heap,
        expr: ConditionalExpressionId,
    ) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            recursive_conditional_expression(self, h, expr)
        }
    }
    fn visit_binary_expression(&mut self, h: &mut Heap, expr: BinaryExpressionId) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            recursive_binary_expression(self, h, expr)
        }
    }
    fn visit_unary_expression(&mut self, h: &mut Heap, expr: UnaryExpressionId) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            match h[expr].operation {
                UnaryOperation::PostDecrement
                | UnaryOperation::PreDecrement
                | UnaryOperation::PostIncrement
                | UnaryOperation::PreIncrement => {
                    self.assignable = true;
                    recursive_unary_expression(self, h, expr)?;
                    self.assignable = false;
                    Ok(())
                }
                _ => recursive_unary_expression(self, h, expr),
            }
        }
    }
    fn visit_indexing_expression(
        &mut self,
        h: &mut Heap,
        expr: IndexingExpressionId,
    ) -> VisitorResult {
        let old = self.assignable;
        self.assignable = false;
        recursive_indexing_expression(self, h, expr)?;
        self.assignable = old;
        Ok(())
    }
    fn visit_slicing_expression(
        &mut self,
        h: &mut Heap,
        expr: SlicingExpressionId,
    ) -> VisitorResult {
        let old = self.assignable;
        self.assignable = false;
        recursive_slicing_expression(self, h, expr)?;
        self.assignable = old;
        Ok(())
    }
    fn visit_select_expression(&mut self, h: &mut Heap, expr: SelectExpressionId) -> VisitorResult {
        if h[expr].field.is_length() && self.assignable {
            return self.error(h[expr].position);
        }
        let old = self.assignable;
        self.assignable = false;
        recursive_select_expression(self, h, expr)?;
        self.assignable = old;
        Ok(())
    }
    fn visit_array_expression(&mut self, h: &mut Heap, expr: ArrayExpressionId) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            recursive_array_expression(self, h, expr)
        }
    }
    fn visit_call_expression(&mut self, h: &mut Heap, expr: CallExpressionId) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            recursive_call_expression(self, h, expr)
        }
    }
    fn visit_constant_expression(
        &mut self,
        h: &mut Heap,
        expr: LiteralExpressionId,
    ) -> VisitorResult {
        if self.assignable {
            self.error(h[expr].position)
        } else {
            Ok(())
        }
    }
    fn visit_variable_expression(
        &mut self,
        _h: &mut Heap,
        _expr: VariableExpressionId,
    ) -> VisitorResult {
        Ok(())
    }
}

pub(crate) struct IndexableExpressions {
    indexable: bool,
}

impl IndexableExpressions {
    pub(crate) fn new() -> Self {
        IndexableExpressions { indexable: false }
    }
    fn error(&self, position: InputPosition) -> VisitorResult {
        Err((position, "Unindexable expression".to_string()))
    }
}

impl Visitor for IndexableExpressions {
    fn visit_assignment_expression(
        &mut self,
        h: &mut Heap,
        expr: AssignmentExpressionId,
    ) -> VisitorResult {
        if self.indexable {
            self.error(h[expr].position)
        } else {
            recursive_assignment_expression(self, h, expr)
        }
    }
    fn visit_conditional_expression(
        &mut self,
        h: &mut Heap,
        expr: ConditionalExpressionId,
    ) -> VisitorResult {
        let old = self.indexable;
        self.indexable = false;
        self.visit_expression(h, h[expr].test)?;
        self.indexable = old;
        self.visit_expression(h, h[expr].true_expression)?;
        self.visit_expression(h, h[expr].false_expression)
    }
    fn visit_binary_expression(&mut self, h: &mut Heap, expr: BinaryExpressionId) -> VisitorResult {
        if self.indexable && h[expr].operation != BinaryOperator::Concatenate {
            self.error(h[expr].position)
        } else {
            recursive_binary_expression(self, h, expr)
        }
    }
    fn visit_unary_expression(&mut self, h: &mut Heap, expr: UnaryExpressionId) -> VisitorResult {
        if self.indexable {
            self.error(h[expr].position)
        } else {
            recursive_unary_expression(self, h, expr)
        }
    }
    fn visit_indexing_expression(
        &mut self,
        h: &mut Heap,
        expr: IndexingExpressionId,
    ) -> VisitorResult {
        let old = self.indexable;
        self.indexable = true;
        self.visit_expression(h, h[expr].subject)?;
        self.indexable = false;
        self.visit_expression(h, h[expr].index)?;
        self.indexable = old;
        Ok(())
    }
    fn visit_slicing_expression(
        &mut self,
        h: &mut Heap,
        expr: SlicingExpressionId,
    ) -> VisitorResult {
        let old = self.indexable;
        self.indexable = true;
        self.visit_expression(h, h[expr].subject)?;
        self.indexable = false;
        self.visit_expression(h, h[expr].from_index)?;
        self.visit_expression(h, h[expr].to_index)?;
        self.indexable = old;
        Ok(())
    }
    fn visit_select_expression(&mut self, h: &mut Heap, expr: SelectExpressionId) -> VisitorResult {
        let old = self.indexable;
        self.indexable = false;
        recursive_select_expression(self, h, expr)?;
        self.indexable = old;
        Ok(())
    }
    fn visit_array_expression(&mut self, h: &mut Heap, expr: ArrayExpressionId) -> VisitorResult {
        let old = self.indexable;
        self.indexable = false;
        recursive_array_expression(self, h, expr)?;
        self.indexable = old;
        Ok(())
    }
    fn visit_call_expression(&mut self, h: &mut Heap, expr: CallExpressionId) -> VisitorResult {
        let old = self.indexable;
        self.indexable = false;
        recursive_call_expression(self, h, expr)?;
        self.indexable = old;
        Ok(())
    }
    fn visit_constant_expression(
        &mut self,
        h: &mut Heap,
        expr: LiteralExpressionId,
    ) -> VisitorResult {
        if self.indexable {
            self.error(h[expr].position)
        } else {
            Ok(())
        }
    }
}

pub(crate) struct SelectableExpressions {
    selectable: bool,
}

impl SelectableExpressions {
    pub(crate) fn new() -> Self {
        SelectableExpressions { selectable: false }
    }
    fn error(&self, position: InputPosition) -> VisitorResult {
        Err((position, "Unselectable expression".to_string()))
    }
}

impl Visitor for SelectableExpressions {
    fn visit_assignment_expression(
        &mut self,
        h: &mut Heap,
        expr: AssignmentExpressionId,
    ) -> VisitorResult {
        // left-hand side of assignment can be skipped
        let old = self.selectable;
        self.selectable = false;
        self.visit_expression(h, h[expr].right)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_conditional_expression(
        &mut self,
        h: &mut Heap,
        expr: ConditionalExpressionId,
    ) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        self.visit_expression(h, h[expr].test)?;
        self.selectable = old;
        self.visit_expression(h, h[expr].true_expression)?;
        self.visit_expression(h, h[expr].false_expression)
    }
    fn visit_binary_expression(&mut self, h: &mut Heap, expr: BinaryExpressionId) -> VisitorResult {
        if self.selectable && h[expr].operation != BinaryOperator::Concatenate {
            self.error(h[expr].position)
        } else {
            recursive_binary_expression(self, h, expr)
        }
    }
    fn visit_unary_expression(&mut self, h: &mut Heap, expr: UnaryExpressionId) -> VisitorResult {
        if self.selectable {
            self.error(h[expr].position)
        } else {
            recursive_unary_expression(self, h, expr)
        }
    }
    fn visit_indexing_expression(
        &mut self,
        h: &mut Heap,
        expr: IndexingExpressionId,
    ) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        recursive_indexing_expression(self, h, expr)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_slicing_expression(
        &mut self,
        h: &mut Heap,
        expr: SlicingExpressionId,
    ) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        recursive_slicing_expression(self, h, expr)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_select_expression(&mut self, h: &mut Heap, expr: SelectExpressionId) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        recursive_select_expression(self, h, expr)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_array_expression(&mut self, h: &mut Heap, expr: ArrayExpressionId) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        recursive_array_expression(self, h, expr)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_call_expression(&mut self, h: &mut Heap, expr: CallExpressionId) -> VisitorResult {
        let old = self.selectable;
        self.selectable = false;
        recursive_call_expression(self, h, expr)?;
        self.selectable = old;
        Ok(())
    }
    fn visit_constant_expression(
        &mut self,
        h: &mut Heap,
        expr: LiteralExpressionId,
    ) -> VisitorResult {
        if self.selectable {
            self.error(h[expr].position)
        } else {
            Ok(())
        }
    }
}

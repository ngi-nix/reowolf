use crate::collections::{ScopedBuffer};
use crate::protocol::ast::*;
use crate::protocol::input_source::*;
use crate::protocol::parser::symbol_table::*;
use crate::protocol::parser::type_table::*;

use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx, 
    Visitor2, 
    VisitorResult
};
use crate::protocol::parser::ModuleCompilationPhase;

#[derive(PartialEq, Eq)]
enum DefinitionType {
    Primitive(ComponentDefinitionId),
    Composite(ComponentDefinitionId),
    Function(FunctionDefinitionId)
}

impl DefinitionType {
    fn is_primitive(&self) -> bool { if let Self::Primitive(_) = self { true } else { false } }
    fn is_composite(&self) -> bool { if let Self::Composite(_) = self { true } else { false } }
    fn is_function(&self) -> bool { if let Self::Function(_) = self { true } else { false } }
    fn definition_id(&self) -> DefinitionId {
        match self {
            DefinitionType::Primitive(v) => v.upcast(),
            DefinitionType::Composite(v) => v.upcast(),
            DefinitionType::Function(v) => v.upcast(),
        }
    }
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
///
/// Because of this scheme expressions will not be visited in the breadth-first
/// pass.
pub(crate) struct PassValidationLinking {
    // `in_sync` is `Some(id)` if the visitor is visiting the children of a
    // synchronous statement. A single value is sufficient as nested
    // synchronous statements are not allowed
    in_sync: Option<SynchronousStatementId>,
    // `in_while` contains the last encountered `While` statement. This is used
    // to resolve unlabeled `Continue`/`Break` statements.
    in_while: Option<WhileStatementId>,
    // Traversal state: current scope (which can be used to find the parent
    // scope) and the definition variant we are considering.
    cur_scope: Scope,
    def_type: DefinitionType,
    // Parent expression (the previous stmt/expression we visited that could be
    // used as an expression parent)
    expr_parent: ExpressionParent,
    // Set by parent to indicate that child expression must be assignable. The
    // child will throw an error if it is not assignable. The stored span is
    // used for the error's position
    must_be_assignable: Option<InputSpan>,
    // Keeping track of relative position in block in the breadth-first pass.
    relative_pos_in_block: u32,
    // Various temporary buffers for traversal. Essentially working around
    // Rust's borrowing rules since it cannot understand we're modifying AST
    // members but not the AST container.
    variable_buffer: ScopedBuffer<VariableId>,
    definition_buffer: ScopedBuffer<DefinitionId>,
    statement_buffer: ScopedBuffer<StatementId>,
    expression_buffer: ScopedBuffer<ExpressionId>,
}

impl PassValidationLinking {
    pub(crate) fn new() -> Self {
        Self{
            in_sync: None,
            in_while: None,
            cur_scope: Scope::Definition(DefinitionId::new_invalid()),
            expr_parent: ExpressionParent::None,
            def_type: DefinitionType::Function(FunctionDefinitionId::new_invalid()),
            must_be_assignable: None,
            relative_pos_in_block: 0,
            variable_buffer: ScopedBuffer::new_reserved(128),
            definition_buffer: ScopedBuffer::new_reserved(128),
            statement_buffer: ScopedBuffer::new_reserved(STMT_BUFFER_INIT_CAPACITY),
            expression_buffer: ScopedBuffer::new_reserved(EXPR_BUFFER_INIT_CAPACITY),
        }
    }

    fn reset_state(&mut self) {
        self.in_sync = None;
        self.in_while = None;
        self.cur_scope = Scope::Definition(DefinitionId::new_invalid());
        self.expr_parent = ExpressionParent::None;
        self.def_type = DefinitionType::Function(FunctionDefinitionId::new_invalid());
        self.relative_pos_in_block = 0;
    }
}

impl Visitor2 for PassValidationLinking {
    fn visit_module(&mut self, ctx: &mut Ctx) -> VisitorResult {
        debug_assert_eq!(ctx.module.phase, ModuleCompilationPhase::TypesAddedToTable);

        let root = &ctx.heap[ctx.module.root_id];
        let section = self.definition_buffer.start_section_initialized(&root.definitions);
        for definition_idx in 0..section.len() {
            let definition_id = section[definition_idx];
            self.visit_definition(ctx, definition_id)?;
        }
        section.forget();

        ctx.module.phase = ModuleCompilationPhase::ValidatedAndLinked;
        Ok(())
    }
    //--------------------------------------------------------------------------
    // Definition visitors
    //--------------------------------------------------------------------------

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentDefinitionId) -> VisitorResult {
        self.reset_state();

        self.def_type = match &ctx.heap[id].variant {
            ComponentVariant::Primitive => DefinitionType::Primitive(id),
            ComponentVariant::Composite => DefinitionType::Composite(id),
        };
        self.cur_scope = Scope::Definition(id.upcast());
        self.expr_parent = ExpressionParent::None;

        // Visit parameters and assign a unique scope ID
        let definition = &ctx.heap[id];
        let body_id = definition.body;
        let section = self.variable_buffer.start_section_initialized(&definition.parameters);
        for variable_idx in 0..section.len() {
            let variable_id = section[variable_idx];
            let variable = &mut ctx.heap[variable_id];
            variable.unique_id_in_scope = variable_idx as i32;
        }
        section.forget();

        // Visit statements in component body
        self.visit_block_stmt(ctx, body_id)?;
        Ok(())
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionDefinitionId) -> VisitorResult {
        self.reset_state();

        // Set internal statement indices
        self.def_type = DefinitionType::Function(id);
        self.cur_scope = Scope::Definition(id.upcast());
        self.expr_parent = ExpressionParent::None;

        // Visit parameters and assign a unique scope ID
        let definition = &ctx.heap[id];
        let body_id = definition.body;
        let section = self.variable_buffer.start_section_initialized(&definition.parameters);
        for variable_idx in 0..section.len() {
            let variable_id = section[variable_idx];
            let variable = &mut ctx.heap[variable_id];
            variable.unique_id_in_scope = variable_idx as i32;
        }
        section.forget();

        // Visit statements in function body
        self.visit_block_stmt(ctx, body_id)?;
        Ok(())
    }

    //--------------------------------------------------------------------------
    // Statement visitors
    //--------------------------------------------------------------------------

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        self.visit_block_stmt_with_hint(ctx, id, None)
    }

    fn visit_local_memory_stmt(&mut self, _ctx: &mut Ctx, _id: MemoryStatementId) -> VisitorResult {
        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, _ctx: &mut Ctx, _id: ChannelStatementId) -> VisitorResult {
        Ok(())
    }

    fn visit_labeled_stmt(&mut self, ctx: &mut Ctx, id: LabeledStatementId) -> VisitorResult {
        let body_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_id)?;

        Ok(())
    }

    fn visit_if_stmt(&mut self, ctx: &mut Ctx, id: IfStatementId) -> VisitorResult {
        // Traverse expression and bodies
        let (test_id, true_id, false_id) = {
            let stmt = &ctx.heap[id];
            (stmt.test, stmt.true_body, stmt.false_body)
        };

        debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        self.expr_parent = ExpressionParent::If(id);
        self.visit_expr(ctx, test_id)?;
        self.expr_parent = ExpressionParent::None;

        self.visit_block_stmt(ctx, true_id)?;
        if let Some(false_id) = false_id {
            self.visit_block_stmt(ctx, false_id)?;
        }

        Ok(())
    }

    fn visit_while_stmt(&mut self, ctx: &mut Ctx, id: WhileStatementId) -> VisitorResult {
        let (test_id, body_id) = {
            let stmt = &ctx.heap[id];
            (stmt.test, stmt.body)
        };
        let old_while = self.in_while.replace(id);
        debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        self.expr_parent = ExpressionParent::While(id);
        self.visit_expr(ctx, test_id)?;
        self.expr_parent = ExpressionParent::None;

        self.visit_block_stmt(ctx, body_id)?;
        self.in_while = old_while;

        Ok(())
    }

    fn visit_break_stmt(&mut self, ctx: &mut Ctx, id: BreakStatementId) -> VisitorResult {
        // Resolve break target
        let target_end_while = {
            let stmt = &ctx.heap[id];
            let target_while_id = self.resolve_break_or_continue_target(ctx, stmt.span, &stmt.label)?;
            let target_while = &ctx.heap[target_while_id];
            debug_assert!(!target_while.end_while.is_invalid());

            target_while.end_while
        };

        let stmt = &mut ctx.heap[id];
        stmt.target = Some(target_end_while);

        Ok(())
    }

    fn visit_continue_stmt(&mut self, ctx: &mut Ctx, id: ContinueStatementId) -> VisitorResult {
        // Resolve continue target
        let target_while_id = {
            let stmt = &ctx.heap[id];
            self.resolve_break_or_continue_target(ctx, stmt.span, &stmt.label)?
        };

        let stmt = &mut ctx.heap[id];
        stmt.target = Some(target_while_id);

        Ok(())
    }

    fn visit_synchronous_stmt(&mut self, ctx: &mut Ctx, id: SynchronousStatementId) -> VisitorResult {
        // Check for validity of synchronous statement
        let cur_sync_span = ctx.heap[id].span;
        if self.in_sync.is_some() {
            // Nested synchronous statement
            let old_sync_span = ctx.heap[self.in_sync.unwrap()].span;
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, cur_sync_span, "Illegal nested synchronous statement"
            ).with_info_str_at_span(
                &ctx.module.source, old_sync_span, "It is nested in this synchronous statement"
            ));
        }

        if !self.def_type.is_primitive() {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, cur_sync_span,
                "synchronous statements may only be used in primitive components"
            ));
        }

        let sync_body = ctx.heap[id].body;
        let old = self.in_sync.replace(id);
        self.visit_block_stmt_with_hint(ctx, sync_body, Some(id))?;
        self.in_sync = old;

        Ok(())
    }

    fn visit_return_stmt(&mut self, ctx: &mut Ctx, id: ReturnStatementId) -> VisitorResult {
        // Check if "return" occurs within a function
        let stmt = &ctx.heap[id];
        if !self.def_type.is_function() {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, stmt.span,
                "return statements may only appear in function bodies"
            ));
        }

        // If here then we are within a function
        debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        debug_assert_eq!(ctx.heap[id].expressions.len(), 1);
        self.expr_parent = ExpressionParent::Return(id);
        self.visit_expr(ctx, ctx.heap[id].expressions[0])?;
        self.expr_parent = ExpressionParent::None;

        Ok(())
    }

    fn visit_goto_stmt(&mut self, ctx: &mut Ctx, id: GotoStatementId) -> VisitorResult {
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
                ParseError::new_error_str_at_span(&ctx.module.source, goto_stmt.span, "goto may not escape the surrounding synchronous block")
                .with_info_str_at_span(&ctx.module.source, target.label.span, "this is the target of the goto statement")
                .with_info_str_at_span(&ctx.module.source, sync_stmt.span, "which will jump past this statement")
            );
        }

        Ok(())
    }

    fn visit_new_stmt(&mut self, ctx: &mut Ctx, id: NewStatementId) -> VisitorResult {
        // Make sure the new statement occurs inside a composite component
        if !self.def_type.is_composite() {
            let new_stmt = &ctx.heap[id];
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, new_stmt.span,
                "instantiating components may only be done in composite components"
            ));
        }

        // Recurse into call expression (which will check the expression parent
        // to ensure that the "new" statment instantiates a component)
        let call_expr_id = ctx.heap[id].expression;

        debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        self.expr_parent = ExpressionParent::New(id);
        self.visit_call_expr(ctx, call_expr_id)?;
        self.expr_parent = ExpressionParent::None;

        Ok(())
    }

    fn visit_expr_stmt(&mut self, ctx: &mut Ctx, id: ExpressionStatementId) -> VisitorResult {
        let expr_id = ctx.heap[id].expression;

        debug_assert_eq!(self.expr_parent, ExpressionParent::None);
        self.expr_parent = ExpressionParent::ExpressionStmt(id);
        self.visit_expr(ctx, expr_id)?;
        self.expr_parent = ExpressionParent::None;

        Ok(())
    }


    //--------------------------------------------------------------------------
    // Expression visitors
    //--------------------------------------------------------------------------

    fn visit_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        let assignment_expr = &mut ctx.heap[id];

        let left_expr_id = assignment_expr.left;
        let right_expr_id = assignment_expr.right;
        let old_expr_parent = self.expr_parent;
        assignment_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(upcast_id, 0);
        self.must_be_assignable = Some(assignment_expr.span);
        self.visit_expr(ctx, left_expr_id)?;
        self.expr_parent = ExpressionParent::Expression(upcast_id, 1);
        self.must_be_assignable = None;
        self.visit_expr(ctx, right_expr_id)?;
        self.expr_parent = old_expr_parent;
        Ok(())
    }

    fn visit_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        let conditional_expr = &mut ctx.heap[id];

        if let Some(span) = self.must_be_assignable {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, span, "cannot assign to the result from a conditional expression"
            ))
        }

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
        let upcast_id = id.upcast();
        let binary_expr = &mut ctx.heap[id];

        if let Some(span) = self.must_be_assignable {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, span, "cannot assign to the result from a binary expression"
            ))
        }

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
        let unary_expr = &mut ctx.heap[id];
        let expr_id = unary_expr.expression;

        if let Some(span) = self.must_be_assignable {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, span, "cannot assign to the result from a unary expression"
            ))
        }

        let old_expr_parent = self.expr_parent;
        unary_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(id.upcast(), 0);
        self.visit_expr(ctx, expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> VisitorResult {
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
        let select_expr = &mut ctx.heap[id];
        let expr_id = select_expr.subject;

        let old_expr_parent = self.expr_parent;
        select_expr.parent = old_expr_parent;

        self.expr_parent = ExpressionParent::Expression(id.upcast(), 0);
        self.visit_expr(ctx, expr_id)?;
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_literal_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> VisitorResult {
        let literal_expr = &mut ctx.heap[id];
        let old_expr_parent = self.expr_parent;
        literal_expr.parent = old_expr_parent;

        if let Some(span) = self.must_be_assignable {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, span, "cannot assign to a literal expression"
            ))
        }

        match &mut literal_expr.value {
            Literal::Null | Literal::True | Literal::False |
            Literal::Character(_) | Literal::String(_) | Literal::Integer(_) => {
                // Just the parent has to be set, done above
            },
            Literal::Struct(literal) => {
                let upcast_id = id.upcast();
                // Retrieve type definition
                let type_definition = ctx.types.get_base_definition(&literal.definition).unwrap();
                let struct_definition = type_definition.definition.as_struct();

                // Make sure all fields are specified, none are specified twice
                // and all fields exist on the struct definition
                let mut specified = Vec::new(); // TODO: @performance
                specified.resize(struct_definition.fields.len(), false);

                for field in &mut literal.fields {
                    // Find field in the struct definition
                    let field_idx = struct_definition.fields.iter().position(|v| v.identifier == field.identifier);
                    if field_idx.is_none() {
                        let field_span = field.identifier.span;
                        let literal = ctx.heap[id].value.as_struct();
                        let ast_definition = &ctx.heap[literal.definition];
                        return Err(ParseError::new_error_at_span(
                            &ctx.module.source, field_span, format!(
                                "This field does not exist on the struct '{}'",
                                ast_definition.identifier().value.as_str()
                            )
                        ));
                    }
                    field.field_idx = field_idx.unwrap();

                    // Check if specified more than once
                    if specified[field.field_idx] {
                        return Err(ParseError::new_error_str_at_span(
                            &ctx.module.source, field.identifier.span,
                            "This field is specified more than once"
                        ));
                    }

                    specified[field.field_idx] = true;
                }

                if !specified.iter().all(|v| *v) {
                    // Some fields were not specified
                    let mut not_specified = String::new();
                    let mut num_not_specified = 0;
                    for (def_field_idx, is_specified) in specified.iter().enumerate() {
                        if !is_specified {
                            if !not_specified.is_empty() { not_specified.push_str(", ") }
                            let field_ident = &struct_definition.fields[def_field_idx].identifier;
                            not_specified.push_str(field_ident.value.as_str());
                            num_not_specified += 1;
                        }
                    }

                    debug_assert!(num_not_specified > 0);
                    let msg = if num_not_specified == 1 {
                        format!("not all fields are specified, '{}' is missing", not_specified)
                    } else {
                        format!("not all fields are specified, [{}] are missing", not_specified)
                    };
                    return Err(ParseError::new_error_at_span(
                        &ctx.module.source, literal.parser_type.elements[0].full_span, msg
                    ));
                }

                // Need to traverse fields expressions in struct and evaluate
                // the poly args
                let mut expr_section = self.expression_buffer.start_section();
                for field in &literal.fields {
                    expr_section.push(field.value);
                }

                for expr_idx in 0..expr_section.len() {
                    let expr_id = expr_section[expr_idx];
                    self.expr_parent = ExpressionParent::Expression(upcast_id, expr_idx as u32);
                    self.visit_expr(ctx, expr_id)?;
                }

                expr_section.forget();
            },
            Literal::Enum(literal) => {
                // Make sure the variant exists
                let type_definition = ctx.types.get_base_definition(&literal.definition).unwrap();
                let enum_definition = type_definition.definition.as_enum();

                let variant_idx = enum_definition.variants.iter().position(|v| {
                    v.identifier == literal.variant
                });

                if variant_idx.is_none() {
                    let literal = ctx.heap[id].value.as_enum();
                    let ast_definition = ctx.heap[literal.definition].as_enum();
                    return Err(ParseError::new_error_at_span(
                        &ctx.module.source, literal.parser_type.elements[0].full_span, format!(
                            "the variant '{}' does not exist on the enum '{}'",
                            literal.variant.value.as_str(), ast_definition.identifier.value.as_str()
                        )
                    ));
                }

                literal.variant_idx = variant_idx.unwrap();
            },
            Literal::Union(literal) => {
                // Make sure the variant exists
                let type_definition = ctx.types.get_base_definition(&literal.definition).unwrap();
                let union_definition = type_definition.definition.as_union();

                let variant_idx = union_definition.variants.iter().position(|v| {
                    v.identifier == literal.variant
                });
                if variant_idx.is_none() {
                    let literal = ctx.heap[id].value.as_union();
                    let ast_definition = ctx.heap[literal.definition].as_union();
                    return Err(ParseError::new_error_at_span(
                        &ctx.module.source, literal.parser_type.elements[0].full_span, format!(
                            "the variant '{}' does not exist on the union '{}'",
                            literal.variant.value.as_str(), ast_definition.identifier.value.as_str()
                        )
                    ));
                }

                literal.variant_idx = variant_idx.unwrap();

                // Make sure the number of specified values matches the expected
                // number of embedded values in the union variant.
                let union_variant = &union_definition.variants[literal.variant_idx];
                if union_variant.embedded.len() != literal.values.len() {
                    let literal = ctx.heap[id].value.as_union();
                    let ast_definition = ctx.heap[literal.definition].as_union();
                    return Err(ParseError::new_error_at_span(
                        &ctx.module.source, literal.parser_type.elements[0].full_span, format!(
                            "The variant '{}' of union '{}' expects {} embedded values, but {} were specified",
                            literal.variant.value.as_str(), ast_definition.identifier.value.as_str(),
                            union_variant.embedded.len(), literal.values.len()
                        ),
                    ))
                }

                // Traverse embedded values of union (if any) and evaluate the
                // polymorphic arguments
                let upcast_id = id.upcast();
                let mut expr_section = self.expression_buffer.start_section();
                for value in &literal.values {
                    expr_section.push(*value);
                }

                for expr_idx in 0..expr_section.len() {
                    let expr_id = expr_section[expr_idx];
                    self.expr_parent = ExpressionParent::Expression(upcast_id, expr_idx as u32);
                    self.visit_expr(ctx, expr_id)?;
                }

                expr_section.forget();
            },
            Literal::Array(literal) => {
                // Visit all expressions in the array
                let upcast_id = id.upcast();
                let expr_section = self.expression_buffer.start_section_initialized(literal);
                for expr_idx in 0..expr_section.len() {
                    let expr_id = expr_section[expr_idx];
                    self.expr_parent = ExpressionParent::Expression(upcast_id, expr_idx as u32);
                    self.visit_expr(ctx, expr_id)?;
                }

                expr_section.forget();
            }
        }

        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        let call_expr = &mut ctx.heap[id];

        if let Some(span) = self.must_be_assignable {
            return Err(ParseError::new_error_str_at_span(
                &ctx.module.source, span, "cannot assign to the result from a call expression"
            ))
        }

        // Check whether the method is allowed to be called within the code's
        // context (in sync, definition type, etc.)
        let mut expected_wrapping_new_stmt = false;
        match &mut call_expr.method {
            Method::Get => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'get' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'get' may only occur inside synchronous blocks"
                    ));
                }
            },
            Method::Put => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'put' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'put' may only occur inside synchronous blocks"
                    ));
                }
            },
            Method::Fires => {
                if !self.def_type.is_primitive() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'fires' may only occur in primitive component definitions"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "a call to 'fires' may only occur inside synchronous blocks"
                    ));
                }
            },
            Method::Create => {},
            Method::Length => {},
            Method::Assert => {
                if self.def_type.is_function() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "assert statement may only occur in components"
                    ));
                }
                if self.in_sync.is_none() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, call_expr.span,
                        "assert statements may only occur inside synchronous blocks"
                    ));
                }
            },
            Method::UserFunction => {},
            Method::UserComponent => {
                expected_wrapping_new_stmt = true;
            },
        }

        if expected_wrapping_new_stmt {
            if !self.expr_parent.is_new() {
                return Err(ParseError::new_error_str_at_span(
                    &ctx.module.source, call_expr.span,
                    "cannot call a component, it can only be instantiated by using 'new'"
                ));
            }
        } else {
            if self.expr_parent.is_new() {
                return Err(ParseError::new_error_str_at_span(
                    &ctx.module.source, call_expr.span,
                    "only components can be instantiated, this is a function"
                ));
            }
        }

        // Check the number of arguments
        let call_definition = ctx.types.get_base_definition(&call_expr.definition).unwrap();
        let num_expected_args = match &call_definition.definition {
            DefinedTypeVariant::Function(definition) => definition.arguments.len(),
            DefinedTypeVariant::Component(definition) => definition.arguments.len(),
            v => unreachable!("encountered {} type in call expression", v.type_class()),
        };

        let num_provided_args = call_expr.arguments.len();
        if num_provided_args != num_expected_args {
            let argument_text = if num_expected_args == 1 { "argument" } else { "arguments" };
            return Err(ParseError::new_error_at_span(
                &ctx.module.source, call_expr.span, format!(
                    "expected {} {}, but {} were provided",
                    num_expected_args, argument_text, num_provided_args
                )
            ));
        }

        // Recurse into all of the arguments and set the expression's parent
        let upcast_id = id.upcast();

        let section = self.expression_buffer.start_section_initialized(&call_expr.arguments);
        let old_expr_parent = self.expr_parent;
        call_expr.parent = old_expr_parent;

        for arg_expr_idx in 0..section.len() {
            let arg_expr_id = section[arg_expr_idx];
            self.expr_parent = ExpressionParent::Expression(upcast_id, arg_expr_idx as u32);
            self.visit_expr(ctx, arg_expr_id)?;
        }

        section.forget();
        self.expr_parent = old_expr_parent;

        Ok(())
    }

    fn visit_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> VisitorResult {
        let var_expr = &ctx.heap[id];
        let variable_id = self.find_variable(ctx, self.relative_pos_in_block, &var_expr.identifier)?;
        let var_expr = &mut ctx.heap[id];
        var_expr.declaration = Some(variable_id);
        var_expr.parent = self.expr_parent;

        Ok(())
    }
}

impl PassValidationLinking {
    //--------------------------------------------------------------------------
    // Special traversal
    //--------------------------------------------------------------------------

    fn visit_block_stmt_with_hint(&mut self, ctx: &mut Ctx, id: BlockStatementId, hint: Option<SynchronousStatementId>) -> VisitorResult {
        // Set parent scope and relative position in the parent scope. Remember
        // these values to set them back to the old values when we're done with
        // the traversal of the block's statements.
        let scope_next_unique_id = get_scope_next_unique_id(ctx, &self.cur_scope);

        let body = &mut ctx.heap[id];
        body.parent_scope = self.cur_scope.clone();
        body.relative_pos_in_parent = self.relative_pos_in_block;
        body.first_unique_id_in_scope = scope_next_unique_id;
        body.next_unique_id_in_scope = scope_next_unique_id;

        let old_scope = self.cur_scope.clone();
        self.cur_scope = match hint {
            Some(sync_id) => Scope::Synchronous((sync_id, id)),
            None => Scope::Regular(id),
        };
        let old_relative_pos = self.relative_pos_in_block;

        // Copy statement IDs into buffer
        let statement_section = self.statement_buffer.start_section_initialized(&body.statements);

        // Perform the breadth-first pass. Its main purpose is to find labeled
        // statements such that we can find the `goto`-targets immediately when
        // performing the depth pass
        for stmt_idx in 0..statement_section.len() {
            self.relative_pos_in_block = stmt_idx as u32;
            self.visit_statement_for_locals_labels_and_in_sync(ctx, self.relative_pos_in_block, statement_section[stmt_idx])?;
        }

        // Perform the depth-first traversal
        for stmt_idx in 0..statement_section.len() {
            self.relative_pos_in_block = stmt_idx as u32;
            self.visit_stmt(ctx, statement_section[stmt_idx])?;
        }

        self.cur_scope = old_scope;
        self.relative_pos_in_block = old_relative_pos;
        statement_section.forget();

        Ok(())
    }

    fn visit_statement_for_locals_labels_and_in_sync(&mut self, ctx: &mut Ctx, relative_pos: u32, id: StatementId) -> VisitorResult {
        let statement = &mut ctx.heap[id];
        match statement {
            Statement::Local(stmt) => {
                match stmt {
                    LocalStatement::Memory(local) => {
                        let variable_id = local.variable;
                        self.checked_local_add(ctx, relative_pos, variable_id)?;
                    },
                    LocalStatement::Channel(local) => {
                        let from_id = local.from;
                        let to_id = local.to;
                        self.checked_local_add(ctx, relative_pos, from_id)?;
                        self.checked_local_add(ctx, relative_pos, to_id)?;
                    }
                }
            }
            Statement::Labeled(stmt) => {
                let stmt_id = stmt.this;
                let body_id = stmt.body;
                self.checked_label_add(ctx, relative_pos, self.in_sync, stmt_id)?;
                self.visit_statement_for_locals_labels_and_in_sync(ctx, relative_pos, body_id)?;
            },
            Statement::While(stmt) => {
                stmt.in_sync = self.in_sync;
            },
            _ => {},
        }

        return Ok(())
    }

    //--------------------------------------------------------------------------
    // Utilities
    //--------------------------------------------------------------------------

    /// Adds a local variable to the current scope. It will also annotate the
    /// `Local` in the AST with its relative position in the block.
    fn checked_local_add(&mut self, ctx: &mut Ctx, relative_pos: u32, id: VariableId) -> Result<(), ParseError> {
        debug_assert!(self.cur_scope.is_block());

        // Make sure we do not conflict with any global symbols
        let cur_scope = SymbolScope::Definition(self.def_type.definition_id());
        {
            let ident = &ctx.heap[id].identifier;
            if let Some(symbol) = ctx.symbols.get_symbol_by_name(cur_scope, &ident.value.as_bytes()) {
                return Err(ParseError::new_error_str_at_span(
                    &ctx.module.source, ident.span,
                    "local variable declaration conflicts with symbol"
                ).with_info_str_at_span(
                    &ctx.module.source, symbol.variant.span_of_introduction(&ctx.heap), "the conflicting symbol is introduced here"
                ));
            }
        }

        let local = &mut ctx.heap[id];
        local.relative_pos_in_block = relative_pos;

        // Make sure we do not shadow any variables in any of the scopes. Note
        // that variables in parent scopes may be declared later
        let local = &ctx.heap[id];
        let mut scope = &self.cur_scope;
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
                        ParseError::new_error_str_at_span(
                            &ctx.module.source, local.identifier.span, "Local variable name conflicts with another variable"
                        ).with_info_str_at_span(
                            &ctx.module.source, other_local.identifier.span, "Previous variable is found here"
                        )
                    );
                }
            }

            // Current scope is fine, move to parent scope if any
            scope = &block.parent_scope;
            if let Scope::Definition(definition_id) = scope {
                // At outer scope, check parameters of function/component
                for parameter_id in ctx.heap[*definition_id].parameters() {
                    let parameter = &ctx.heap[*parameter_id];
                    if local.identifier == parameter.identifier {
                        return Err(
                            ParseError::new_error_str_at_span(
                                &ctx.module.source, local.identifier.span, "Local variable name conflicts with parameter"
                            ).with_info_str_at_span(
                                &ctx.module.source, parameter.identifier.span, "Parameter definition is found here"
                            )
                        );
                    }
                }

                break;
            }

            // If here, then we are dealing with a block-like parent block
            local_relative_pos = ctx.heap[scope.to_block()].relative_pos_in_parent;
        }

        // No collisions at all
        let block = &mut ctx.heap[self.cur_scope.to_block()];
        block.locals.push(id);
        let unique_id_in_scope = block.next_unique_id_in_scope;
        block.next_unique_id_in_scope += 1;

        let variable = &mut ctx.heap[id];
        variable.unique_id_in_scope = unique_id_in_scope;

        Ok(())
    }

    /// Finds a variable in the visitor's scope that must appear before the
    /// specified relative position within that block.
    fn find_variable(&self, ctx: &Ctx, mut relative_pos: u32, identifier: &Identifier) -> Result<VariableId, ParseError> {
        debug_assert!(self.cur_scope.is_block());

        // TODO: May still refer to an alias of a global symbol using a single
        //  identifier in the namespace.
        // No need to use iterator over namespaces if here
        let mut scope = &self.cur_scope;
        
        loop {
            debug_assert!(scope.is_block());
            let block = &ctx.heap[scope.to_block()];
            
            for local_id in &block.locals {
                let local = &ctx.heap[*local_id];
                
                if local.relative_pos_in_block < relative_pos && identifier == &local.identifier {
                    return Ok(*local_id);
                }
            }

            scope = &block.parent_scope;
            if !scope.is_block() {
                // Definition scope, need to check arguments to definition
                match scope {
                    Scope::Definition(definition_id) => {
                        let definition = &ctx.heap[*definition_id];
                        for parameter_id in definition.parameters() {
                            let parameter = &ctx.heap[*parameter_id];
                            if identifier == &parameter.identifier {
                                return Ok(*parameter_id);
                            }
                        }
                    },
                    _ => unreachable!(),
                }

                // Variable could not be found
                return Err(ParseError::new_error_str_at_span(
                    &ctx.module.source, identifier.span, "unresolved variable"
                ));
            } else {
                relative_pos = block.relative_pos_in_parent;
            }
        }
    }

    /// Adds a particular label to the current scope. Will return an error if
    /// there is another label with the same name visible in the current scope.
    fn checked_label_add(&mut self, ctx: &mut Ctx, relative_pos: u32, in_sync: Option<SynchronousStatementId>, id: LabeledStatementId) -> Result<(), ParseError> {
        debug_assert!(self.cur_scope.is_block());

        // Make sure label is not defined within the current scope or any of the
        // parent scope.
        let label = &mut ctx.heap[id];
        label.relative_pos_in_block = relative_pos;
        label.in_sync = in_sync;

        let label = &ctx.heap[id];
        let mut scope = &self.cur_scope;

        loop {
            debug_assert!(scope.is_block(), "scope is not a block");
            let block = &ctx.heap[scope.to_block()];
            for other_label_id in &block.labels {
                let other_label = &ctx.heap[*other_label_id];
                if other_label.label == label.label {
                    // Collision
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, label.label.span, "label name is used more than once"
                    ).with_info_str_at_span(
                        &ctx.module.source, other_label.label.span, "the other label is found here"
                    ));
                }
            }

            scope = &block.parent_scope;
            if !scope.is_block() {
                break;
            }
        }

        // No collisions
        let block = &mut ctx.heap[self.cur_scope.to_block()];
        block.labels.push(id);

        Ok(())
    }

    /// Finds a particular labeled statement by its identifier. Once found it
    /// will make sure that the target label does not skip over any variable
    /// declarations within the scope in which the label was found.
    fn find_label(&self, ctx: &Ctx, identifier: &Identifier) -> Result<LabeledStatementId, ParseError> {
        debug_assert!(self.cur_scope.is_block());

        let mut scope = &self.cur_scope;
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
                                ParseError::new_error_str_at_span(&ctx.module.source, identifier.span, "this target label skips over a variable declaration")
                                .with_info_str_at_span(&ctx.module.source, label.label.span, "because it jumps to this label")
                                .with_info_str_at_span(&ctx.module.source, local.identifier.span, "which skips over this variable")
                            );
                        }
                    }
                    return Ok(*label_id);
                }
            }

            scope = &block.parent_scope;
            if !scope.is_block() {
                return Err(ParseError::new_error_str_at_span(
                    &ctx.module.source, identifier.span, "could not find this label"
                ));
            }

        }
    }

    /// This function will check if the provided while statement ID has a block
    /// statement that is one of our current parents.
    fn has_parent_while_scope(&self, ctx: &Ctx, id: WhileStatementId) -> bool {
        let mut scope = &self.cur_scope;
        let while_stmt = &ctx.heap[id];
        loop {
            debug_assert!(scope.is_block());
            let block = scope.to_block();
            if while_stmt.body == block {
                return true;
            }

            let block = &ctx.heap[block];
            scope = &block.parent_scope;
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
    fn resolve_break_or_continue_target(&self, ctx: &Ctx, span: InputSpan, label: &Option<Identifier>) -> Result<WhileStatementId, ParseError> {
        let target = match label {
            Some(label) => {
                let target_id = self.find_label(ctx, label)?;

                // Make sure break target is a while statement
                let target = &ctx.heap[target_id];
                if let Statement::While(target_stmt) = &ctx.heap[target.body] {
                    // Even though we have a target while statement, the break might not be
                    // present underneath this particular labeled while statement
                    if !self.has_parent_while_scope(ctx, target_stmt.this) {
                        return Err(ParseError::new_error_str_at_span(
                            &ctx.module.source, label.span, "break statement is not nested under the target label's while statement"
                        ).with_info_str_at_span(
                            &ctx.module.source, target.label.span, "the targeted label is found here"
                        ));
                    }

                    target_stmt.this
                } else {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, label.span, "incorrect break target label, it must target a while loop"
                    ).with_info_str_at_span(
                        &ctx.module.source, target.label.span, "The targeted label is found here"
                    ));
                }
            },
            None => {
                // Use the enclosing while statement, the break must be
                // nested within that while statement
                if self.in_while.is_none() {
                    return Err(ParseError::new_error_str_at_span(
                        &ctx.module.source, span, "Break statement is not nested under a while loop"
                    ));
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
                    ParseError::new_error_str_at_span(&ctx.module.source, span, "break may not escape the surrounding synchronous block")
                        .with_info_str_at_span(&ctx.module.source, target_while.span, "the break escapes out of this loop")
                        .with_info_str_at_span(&ctx.module.source, sync_stmt.span, "And would therefore escape this synchronous block")
                );
            }
        }

        Ok(target)
    }
}

fn get_scope_next_unique_id(ctx: &Ctx, scope: &Scope) -> i32 {
    match scope {
        Scope::Definition(definition_id) => {
            let definition = &ctx.heap[*definition_id];
            match definition {
                Definition::Component(definition) => definition.parameters.len() as i32,
                Definition::Function(definition) => definition.parameters.len() as i32,
                _ => unreachable!("Scope::Definition points to non-procedure type")
            }
        },
        Scope::Synchronous((_, block_id)) | Scope::Regular(block_id) => {
            let block = &ctx.heap[*block_id];
            block.next_unique_id_in_scope
        }
    }
}
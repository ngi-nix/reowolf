use crate::protocol::ast::*;
use super::symbol_table::*;
use super::{Module, ModuleCompilationPhase, PassCtx};
use super::tokens::*;
use super::token_parsing::*;
use crate::protocol::input_source::{InputSource as InputSource, InputPosition as InputPosition, InputSpan, ParseError};
use crate::collections::*;

/// Parses all the tokenized definitions into actual AST nodes.
pub(crate) struct PassDefinitions {
    // State
    cur_definition: DefinitionId,
    // Temporary buffers of various kinds
    buffer: String,
    struct_fields: ScopedBuffer<StructFieldDefinition>,
    enum_variants: ScopedBuffer<EnumVariantDefinition>,
    union_variants: ScopedBuffer<UnionVariantDefinition>,
    parameters: ScopedBuffer<ParameterId>,
    expressions: ScopedBuffer<ExpressionId>,
    statements: ScopedBuffer<StatementId>,
    parser_types: ScopedBuffer<ParserType>,
}

impl PassDefinitions {
    pub(crate) fn new() -> Self {
        Self{
            cur_definition: DefinitionId::new_invalid(),
            buffer: String::with_capacity(128),
            struct_fields: ScopedBuffer::new_reserved(128),
            enum_variants: ScopedBuffer::new_reserved(128),
            union_variants: ScopedBuffer::new_reserved(128),
            parameters: ScopedBuffer::new_reserved(128),
            expressions: ScopedBuffer::new_reserved(128),
            statements: ScopedBuffer::new_reserved(128),
            parser_types: ScopedBuffer::new_reserved(128),
        }
    }

    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut PassCtx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert_eq!(module.phase, ModuleCompilationPhase::ImportsResolved);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);

        // Although we only need to parse the definitions, we want to go through
        // code ranges as well such that we can throw errors if we get
        // unexpected tokens at the module level of the source.
        let mut range_idx = module_range.first_child_idx;
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];

            match cur_range.range_kind {
                TokenRangeKind::Module => unreachable!(), // should not be reachable
                TokenRangeKind::Pragma | TokenRangeKind::Import => {
                    // Already fully parsed, fall through and go to next range
                },
                TokenRangeKind::Definition | TokenRangeKind::Code => {
                    // Visit range even if it is a "code" range to provide
                    // proper error messages.
                    self.visit_range(modules, module_idx, ctx, range_idx_usize)?;
                },
            }

            if cur_range.next_sibling_idx == NO_SIBLING {
                break;
            } else {
                range_idx = cur_range.next_sibling_idx;
            }
        }

        modules[module_idx].phase = ModuleCompilationPhase::DefinitionsParsed;

        Ok(())
    }

    fn visit_range(
        &mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize
    ) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let cur_range = &module.tokens.ranges[range_idx];
        debug_assert!(cur_range.range_kind == TokenRangeKind::Definition || cur_range.range_kind == TokenRangeKind::Code);

        // Detect which definition we're parsing
        let mut iter = module.tokens.iter_range(cur_range);
        loop {
            let next = iter.next();
            if next.is_none() {
                return Ok(())
            }

            // Token was not None, so peek_ident returns None if not an ident
            let ident = peek_ident(&module.source, &mut iter);
            match ident {
                Some(KW_STRUCT) => self.visit_struct_definition(module, &mut iter, ctx)?,
                Some(KW_ENUM) => self.visit_enum_definition(module, &mut iter, ctx)?,
                Some(KW_UNION) => self.visit_union_definition(module, &mut iter, ctx)?,
                Some(KW_FUNCTION) => self.visit_function_definition(module, &mut iter, ctx)?,
                Some(KW_PRIMITIVE) | Some(KW_COMPOSITE) => self.visit_component_definition(module, &mut iter, ctx)?,
                _ => return Err(ParseError::new_error_str_at_pos(
                    &module.source, iter.last_valid_pos(),
                    "unexpected symbol, expected a keyword marking the start of a definition"
                )),
            }
        }
    }

    fn visit_struct_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        consume_exact_ident(&module.source, iter, KW_STRUCT)?;
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated DefinitionId
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        self.cur_definition = definition_id;

        // Parse struct definition
        consume_polymorphic_vars_spilled(&module.source, iter, ctx)?;

        let mut fields_section = self.struct_fields.start_section();
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter, ctx,
            |source, iter, ctx| {
                let poly_vars = ctx.heap[definition_id].poly_vars(); // TODO: @Cleanup, this is really ugly. But rust...

                let start_pos = iter.last_valid_pos();
                let parser_type = consume_parser_type(
                    source, iter, &ctx.symbols, &ctx.heap, poly_vars, module_scope,
                    definition_id, false, 0
                )?;
                let field = consume_ident_interned(source, iter, ctx)?;
                Ok(StructFieldDefinition{
                    span: InputSpan::from_positions(start_pos, field.span.end),
                    field, parser_type
                })
            },
            &mut fields_section, "a struct field", "a list of struct fields", None
        )?;

        // Transfer to preallocated definition
        let struct_def = ctx.heap[definition_id].as_struct_mut();
        struct_def.fields = fields_section.into_vec();

        Ok(())
    }

    fn visit_enum_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        consume_exact_ident(&module.source, iter, KW_ENUM)?;
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated DefinitionId
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        self.cur_definition = definition_id;

        // Parse enum definition
        consume_polymorphic_vars_spilled(&module.source, iter, ctx)?;

        let mut enum_section = self.enum_variants.start_section();
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter, ctx,
            |source, iter, ctx| {
                let identifier = consume_ident_interned(source, iter, ctx)?;
                let value = if iter.next() == Some(TokenKind::Equal) {
                    iter.consume();
                    let (variant_number, _) = consume_integer_literal(source, iter, &mut self.buffer)?;
                    EnumVariantValue::Integer(variant_number as i64) // TODO: @int
                } else {
                    EnumVariantValue::None
                };
                Ok(EnumVariantDefinition{ identifier, value })
            },
            &mut enum_section, "an enum variant", "a list of enum variants", None
        )?;

        // Transfer to definition
        let enum_def = ctx.heap[definition_id].as_enum_mut();
        enum_def.variants = enum_section.into_vec();

        Ok(())
    }

    fn visit_union_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        consume_exact_ident(&module.source, iter, KW_UNION)?;
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated DefinitionId
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        self.cur_definition = definition_id;

        // Parse union definition
        consume_polymorphic_vars_spilled(&module.source, iter, ctx)?;

        let mut variants_section = self.union_variants.start_section();
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter, ctx,
            |source, iter, ctx| {
                let identifier = consume_ident_interned(source, iter, ctx)?;
                let mut close_pos = identifier.span.end;

                let mut types_section = self.parser_types.start_section();

                let has_embedded = maybe_consume_comma_separated(
                    TokenKind::OpenParen, TokenKind::CloseParen, source, iter, ctx,
                    |source, iter, ctx| {
                        let poly_vars = ctx.heap[definition_id].poly_vars(); // TODO: @Cleanup, this is really ugly. But rust...
                        consume_parser_type(
                            source, iter, &ctx.symbols, &ctx.heap, poly_vars,
                            module_scope, definition_id, false, 0
                        )
                    },
                    &mut types_section, "an embedded type", Some(&mut close_pos)
                )?;
                let value = if has_embedded {
                    UnionVariantValue::Embedded(types_section.into_vec())
                } else {
                    types_section.forget();
                    UnionVariantValue::None
                };

                Ok(UnionVariantDefinition{
                    span: InputSpan::from_positions(identifier.span.begin, close_pos),
                    identifier,
                    value
                })
            },
            &mut variants_section, "a union variant", "a list of union variants", None
        )?;

        // Transfer to AST
        let union_def = ctx.heap[definition_id].as_union_mut();
        union_def.variants = variants_section.into_vec();

        Ok(())
    }

    fn visit_function_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        consume_exact_ident(&module.source, iter, KW_FUNCTION)?;
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated DefinitionId
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        self.cur_definition = definition_id;

        consume_polymorphic_vars_spilled(&module.source, iter, ctx)?;

        // Parse function's argument list
        let mut parameter_section = self.parameters.start_section();
        consume_parameter_list(
            &module.source, iter, ctx, &mut parameter_section, module_scope, definition_id
        )?;
        let parameters = parameter_section.into_vec();

        // Consume return types
        consume_token(&module.source, iter, TokenKind::ArrowRight)?;
        let mut return_types = self.parser_types.start_section();
        let mut open_curly_pos = iter.last_valid_pos(); // bogus value
        consume_comma_separated_until(
            TokenKind::OpenCurly, &module.source, iter, ctx,
            |source, iter, ctx| {
                let poly_vars = ctx.heap[definition_id].poly_vars(); // TODO: @Cleanup, this is really ugly. But rust...
                consume_parser_type(source, iter, &ctx.symbols, &ctx.heap, poly_vars, module_scope, definition_id, false, 0)
            },
            &mut return_types, "a return type", Some(&mut open_curly_pos)
        )?;
        let return_types = return_types.into_vec();

        // TODO: @ReturnValues
        match return_types.len() {
            0 => return Err(ParseError::new_error_str_at_pos(&module.source, open_curly_pos, "expected a return type")),
            1 => {},
            _ => return Err(ParseError::new_error_str_at_pos(&module.source, open_curly_pos, "multiple return types are not (yet) allowed")),
        }

        // Consume block
        let body = self.consume_block_statement_without_leading_curly(module, iter, ctx, open_curly_pos)?;

        // Assign everything in the preallocated AST node
        let function = ctx.heap[definition_id].as_function_mut();
        function.return_types = return_types;
        function.parameters = parameters;
        function.body = body;

        Ok(())
    }

    fn visit_component_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        let (_variant_text, _) = consume_any_ident(&module.source, iter)?;
        debug_assert!(_variant_text == KW_PRIMITIVE || _variant_text == KW_COMPOSITE);
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated definition
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        self.cur_definition = definition_id;

        consume_polymorphic_vars_spilled(&module.source, iter, ctx)?;

        // Parse component's argument list
        let mut parameter_section = self.parameters.start_section();
        consume_parameter_list(
            &module.source, iter, ctx, &mut parameter_section, module_scope, definition_id
        )?;
        let parameters = parameter_section.into_vec();

        // Consume block
        let body = self.consume_block_statement(module, iter, ctx)?;

        // Assign everything in the AST node
        let component = ctx.heap[definition_id].as_component_mut();
        component.parameters = parameters;
        component.body = body;

        Ok(())
    }

    /// Consumes a block statement. If the resulting statement is not a block
    /// (e.g. for a shorthand "if (expr) single_statement") then it will be
    /// wrapped in one
    fn consume_block_or_wrapped_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<BlockStatementId, ParseError> {
        if Some(TokenKind::OpenCurly) == iter.next() {
            // This is a block statement
            self.consume_block_statement(module, iter, ctx)
        } else {
            // Not a block statement, so wrap it in one
            let mut statements = self.statements.start_section();
            let wrap_begin_pos = iter.last_valid_pos();
            self.consume_statement(module, iter, ctx, &mut statements)?;
            let wrap_end_pos = iter.last_valid_pos();

            debug_assert_eq!(statements.len(), 1);
            let statements = statements.into_vec();

            Ok(ctx.heap.alloc_block_statement(|this| BlockStatement{
                this,
                is_implicit: true,
                span: InputSpan::from_positions(wrap_begin_pos, wrap_end_pos), // TODO: @Span
                statements,
                parent_scope: Scope::Definition(DefinitionId::new_invalid()),
                first_unique_id_in_scope: -1,
                next_unique_id_in_scope: -1,
                relative_pos_in_parent: 0,
                locals: Vec::new(),
                labels: Vec::new()
            }))
        }
    }

    /// Consumes a statement and returns a boolean indicating whether it was a
    /// block or not.
    fn consume_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, section: &mut ScopedSection<StatementId>
    ) -> Result<(), ParseError> {
        let next = iter.next().expect("consume_statement has a next token");

        if next == TokenKind::OpenCurly {
            let id = self.consume_block_statement(module, iter, ctx)?;
            section.push(id.upcast());
        } else if next == TokenKind::Ident {
            let ident = peek_ident(&module.source, iter).unwrap();
            if ident == KW_STMT_IF {
                // Consume if statement and place end-if statement directly
                // after it.
                let id = self.consume_if_statement(module, iter, ctx)?;
                section.push(id.upcast());

                let end_if = ctx.heap.alloc_end_if_statement(|this| EndIfStatement{
                    this, start_if: id, next: StatementId::new_invalid()
                });
                section.push(id.upcast());

                let if_stmt = &mut ctx.heap[id];
                if_stmt.end_if = Some(end_if);
            } else if ident == KW_STMT_WHILE {
                let id = self.consume_while_statement(module, iter, ctx)?;
                section.push(id.upcast());

                let end_while = ctx.heap.alloc_end_while_statement(|this| EndWhileStatement{
                    this, start_while: id, next: StatementId::new_invalid()
                });
                section.push(id.upcast());

                let while_stmt = &mut ctx.heap[id];
                while_stmt.end_while = Some(end_while);
            } else if ident == KW_STMT_BREAK {
                let id = self.consume_break_statement(module, iter, ctx)?;
                section.push(id.upcast());
            } else if ident == KW_STMT_CONTINUE {
                let id = self.consume_continue_statement(module, iter, ctx)?;
                section.push(id.upcast());
            } else if ident == KW_STMT_SYNC {
                let id = self.consume_synchronous_statement(module, iter, ctx)?;
                section.push(id.upcast());

                let end_sync = ctx.heap.alloc_end_synchronous_statement(|this| EndSynchronousStatement{
                    this, start_sync: id, next: StatementId::new_invalid()
                });

                let sync_stmt = &mut ctx.heap[id];
                sync_stmt.end_sync = Some(end_sync);
            } else if ident == KW_STMT_RETURN {
                let id = self.consume_return_statement(module, iter, ctx)?;
                section.push(id.upcast());
            } else if ident == KW_STMT_GOTO {
                let id = self.consume_goto_statement(module, iter, ctx)?;
                section.push(id.upcast());
            } else if ident == KW_STMT_NEW {
                let id = self.consume_new_statement(module, iter, ctx)?;
                section.push(id.upcast());
            } else if ident == KW_STMT_CHANNEL {
                let id = self.consume_channel_statement(module, iter, ctx)?;
                section.push(id.upcast().upcast());
            } else if iter.peek() == Some(TokenKind::Colon) {
                self.consume_labeled_statement(module, iter, ctx, section)?;
            } else {
                // Two fallback possibilities: the first one is a memory
                // declaration, the other one is to parse it as a regular
                // expression. This is a bit ugly
                if let Some((memory_stmt_id, assignment_stmt_id)) = self.maybe_consume_memory_statement(module, iter, ctx)? {
                    section.push(memory_stmt_id.upcast().upcast());
                    section.push(assignment_stmt_id.upcast());
                } else {
                    let id = self.consume_expression_statement(module, iter, ctx)?;
                    section.push(id.upcast());
                }
            }
        };

        return Ok(());
    }

    fn consume_block_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<BlockStatementId, ParseError> {
        let open_span = consume_token(&module.source, iter, TokenKind::OpenCurly)?;
        self.consume_block_statement_without_leading_curly(module, iter, ctx, open_span.begin)
    }

    fn consume_block_statement_without_leading_curly(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, open_curly_pos: InputPosition
    ) -> Result<BlockStatementId, ParseError> {
        let mut stmt_section = self.statements.start_section();
        let mut next = iter.next();
        while next != Some(TokenKind::CloseCurly) {
            if next.is_none() {
                return Err(ParseError::new_error_str_at_pos(
                    &module.source, iter.last_valid_pos(), "expected a statement or '}'"
                ));
            }
            self.consume_statement(module, iter, ctx, &mut stmt_section)?;
            next = iter.next();
        }

        let statements = stmt_section.into_vec();
        let mut block_span = consume_token(&module.source, iter, TokenKind::CloseCurly)?;
        block_span.begin = open_curly_pos;

        Ok(ctx.heap.alloc_block_statement(|this| BlockStatement{
            this,
            is_implicit: false,
            span: block_span,
            statements,
            parent_scope: Scope::Definition(DefinitionId::new_invalid()),
            first_unique_id_in_scope: -1,
            next_unique_id_in_scope: -1,
            relative_pos_in_parent: 0,
            locals: Vec::new(),
            labels: Vec::new(),
        }))
    }

    fn consume_if_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<IfStatementId, ParseError> {
        let if_span = consume_exact_ident(&module.source, iter, KW_STMT_IF)?;
        consume_token(&module.source, iter, TokenKind::OpenParen)?;
        let test = self.consume_expression(module, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::CloseParen)?;
        let true_body = self.consume_block_or_wrapped_statement(module, iter, ctx)?;

        let false_body = if has_ident(&module.source, iter, KW_STMT_ELSE) {
            iter.consume();
            let false_body = self.consume_block_or_wrapped_statement(module, iter, ctx)?;
            Some(false_body)
        } else {
            None
        };

        Ok(ctx.heap.alloc_if_statement(|this| IfStatement{
            this,
            span: if_span,
            test,
            true_body,
            false_body,
            end_if: None,
        }))
    }

    fn consume_while_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<WhileStatementId, ParseError> {
        let while_span = consume_exact_ident(&module.source, iter, KW_STMT_WHILE)?;
        consume_token(&module.source, iter, TokenKind::OpenParen)?;
        let test = self.consume_expression(module, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::CloseParen)?;
        let body = self.consume_block_or_wrapped_statement(module, iter, ctx)?;

        Ok(ctx.heap.alloc_while_statement(|this| WhileStatement{
            this,
            span: while_span,
            test,
            body,
            end_while: None,
            in_sync: None,
        }))
    }

    fn consume_break_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<BreakStatementId, ParseError> {
        let break_span = consume_exact_ident(&module.source, iter, KW_STMT_BREAK)?;
        let label = if Some(TokenKind::Ident) == iter.next() {
            let label = consume_ident_interned(&module.source, iter, ctx)?;
            Some(label)
        } else {
            None
        };
        consume_token(&module.source, iter, TokenKind::SemiColon)?;
        Ok(ctx.heap.alloc_break_statement(|this| BreakStatement{
            this,
            span: break_span,
            label,
            target: None,
        }))
    }

    fn consume_continue_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ContinueStatementId, ParseError> {
        let continue_span = consume_exact_ident(&module.source, iter, KW_STMT_CONTINUE)?;
        let label=  if Some(TokenKind::Ident) == iter.next() {
            let label = consume_ident_interned(&module.source, iter, ctx)?;
            Some(label)
        } else {
            None
        };
        consume_token(&module.source, iter, TokenKind::SemiColon)?;
        Ok(ctx.heap.alloc_continue_statement(|this| ContinueStatement{
            this,
            span: continue_span,
            label,
            target: None
        }))
    }

    fn consume_synchronous_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<SynchronousStatementId, ParseError> {
        let synchronous_span = consume_exact_ident(&module.source, iter, KW_STMT_SYNC)?;
        let body = self.consume_block_or_wrapped_statement(module, iter, ctx)?;

        Ok(ctx.heap.alloc_synchronous_statement(|this| SynchronousStatement{
            this,
            span: synchronous_span,
            body,
            end_sync: None,
            parent_scope: None,
        }))
    }

    fn consume_return_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ReturnStatementId, ParseError> {
        let return_span = consume_exact_ident(&module.source, iter, KW_STMT_RETURN)?;
        let mut scoped_section = self.expressions.start_section();

        consume_comma_separated_until(
            TokenKind::SemiColon, &module.source, iter, ctx,
            |_source, iter, ctx| self.consume_expression(module, iter, ctx),
            &mut scoped_section, "a return expression", None
        )?;
        let expressions = scoped_section.into_vec();

        if expressions.is_empty() {
            return Err(ParseError::new_error_str_at_span(&module.source, return_span, "expected at least one return value"));
        } else if expressions.len() > 1 {
            return Err(ParseError::new_error_str_at_span(&module.source, return_span, "multiple return values are not (yet) supported"))
        }

        Ok(ctx.heap.alloc_return_statement(|this| ReturnStatement{
            this,
            span: return_span,
            expressions
        }))
    }

    fn consume_goto_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<GotoStatementId, ParseError> {
        let goto_span = consume_exact_ident(&module.source, iter, KW_STMT_GOTO)?;
        let label = consume_ident_interned(&module.source, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::SemiColon)?;
        Ok(ctx.heap.alloc_goto_statement(|this| GotoStatement{
            this,
            span: goto_span,
            label,
            target: None
        }))
    }

    fn consume_new_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<NewStatementId, ParseError> {
        let new_span = consume_exact_ident(&module.source, iter, KW_STMT_NEW)?;

        // TODO: @Cleanup, should just call something like consume_component_expression-ish
        let start_pos = iter.last_valid_pos();
        let expression_id = self.consume_primary_expression(module, iter, ctx)?;
        let expression = &ctx.heap[expression_id];
        let mut valid = false;

        let mut call_id = CallExpressionId::new_invalid();
        if let Expression::Call(expression) = expression {
            // Allow both components and functions, as it makes more sense to
            // check their correct use in the validation and linking pass
            if expression.method == Method::UserComponent || expression.method == Method::UserFunction {
                call_id = expression.this;
                valid = true;
            }
        }

        if !valid {
            return Err(ParseError::new_error_str_at_span(
                &module.source, InputSpan::from_positions(start_pos, iter.last_valid_pos()), "expected a call expression"
            ));
        }
        consume_token(&module.source, iter, TokenKind::SemiColon)?;

        debug_assert!(!call_id.is_invalid());
        Ok(ctx.heap.alloc_new_statement(|this| NewStatement{
            this,
            span: new_span,
            expression: call_id,
            next: StatementId::new_invalid(),
        }))
    }

    fn consume_channel_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ChannelStatementId, ParseError> {
        // Consume channel specification
        let channel_span = consume_exact_ident(&module.source, iter, KW_STMT_CHANNEL)?;
        let channel_type = if Some(TokenKind::OpenAngle) == iter.next() {
            // Retrieve the type of the channel, we're cheating a bit here by
            // consuming the first '<' and setting the initial angle depth to 1
            // such that our final '>' will be consumed as well.
            iter.consume();
            let definition_id = self.cur_definition;
            let poly_vars = ctx.heap[definition_id].poly_vars();
            consume_parser_type(
                &module.source, iter, &ctx.symbols, &ctx.heap,
                &poly_vars, SymbolScope::Module(module.root_id), definition_id,
                true, 1
            )?
        } else {
            // Assume inferred
            ParserType{ elements: vec![ParserTypeElement{
                full_span: channel_span, // TODO: @Span fix
                variant: ParserTypeVariant::Inferred
            }]}
        };

        let from_identifier = consume_ident_interned(&module.source, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::ArrowRight)?;
        let to_identifier = consume_ident_interned(&module.source, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::SemiColon)?;

        // Construct ports
        let from = ctx.heap.alloc_variable(|this| Variable{
            this,
            kind: VariableKind::Local,
            identifier: from_identifier,
            parser_type: channel_type.clone(),
            relative_pos_in_block: 0,
            unique_id_in_scope: -1,
        });
        let to = ctx.heap.alloc_variable(|this|Variable{
            this,
            kind: VariableKind::Local,
            identifier: to_identifier,
            parser_type: channel_type,
            relative_pos_in_block: 0,
            unique_id_in_scope: -1,
        });

        // Construct the channel
        Ok(ctx.heap.alloc_channel_statement(|this| ChannelStatement{
            this,
            span: channel_span,
            from, to,
            relative_pos_in_block: 0,
            next: StatementId::new_invalid(),
        }))
    }

    fn consume_labeled_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, section: &mut ScopedSection<StatementId>
    ) -> Result<(), ParseError> {
        let label = consume_ident_interned(&module.source, iter, ctx)?;
        consume_token(&module.source, iter, TokenKind::Colon)?;

        // Not pretty: consume_statement may produce more than one statement.
        // The values in the section need to be in the correct order if some
        // kind of outer block is consumed, so we take another section, push
        // the expressions in that one, and then allocate the labeled statement.
        let mut inner_section = self.statements.start_section();
        self.consume_statement(module, iter, ctx, &mut inner_section)?;
        debug_assert!(inner_section.len() >= 1);

        let stmt_id = ctx.heap.alloc_labeled_statement(|this| LabeledStatement {
            this,
            label,
            body: inner_section[0],
            relative_pos_in_block: 0,
            in_sync: None,
        });

        if inner_section.len() == 1 {
            // Produce the labeled statement pointing to the first statement.
            // This is by far the most common case.
            inner_section.forget();
            section.push(stmt_id.upcast());
        } else {
            // Produce the labeled statement using the first statement, and push
            // the remaining ones at the end.
            let inner_statements = inner_section.into_vec();
            section.push(stmt_id.upcast());
            for idx in 1..inner_statements.len() {
                section.push(inner_statements[idx])
            }
        }

        Ok(())
    }

    fn maybe_consume_memory_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<Option<(MemoryStatementId, ExpressionStatementId)>, ParseError> {
        // This is a bit ugly. It would be nicer if we could somehow
        // consume the expression with a type hint if we do get a valid
        // type, but we don't get an identifier following it
        let iter_state = iter.save();
        let definition_id = self.cur_definition;
        let poly_vars = ctx.heap[definition_id].poly_vars();

        let parser_type = consume_parser_type(
            &module.source, iter, &ctx.symbols, &ctx.heap, poly_vars,
            SymbolScope::Definition(definition_id), definition_id, true, 0
        );

        if let Ok(parser_type) = parser_type {
            if Some(TokenKind::Ident) == iter.next() {
                // Assume this is a proper memory statement
                let identifier = consume_ident_interned(&module.source, iter, ctx)?;
                let memory_span = InputSpan::from_positions(parser_type.elements[0].full_span.begin, identifier.span.end);
                let assign_span = consume_token(&module.source, iter, TokenKind::Equal)?;

                let initial_expr_begin_pos = iter.last_valid_pos();
                let initial_expr_id = self.consume_expression(module, iter, ctx)?;
                let initial_expr_end_pos = iter.last_valid_pos();
                consume_token(&module.source, iter, TokenKind::SemiColon)?;

                // Allocate the memory statement with the variable
                let local_id = ctx.heap.alloc_variable(|this| Variable{
                    this,
                    kind: VariableKind::Local,
                    identifier: identifier.clone(),
                    parser_type,
                    relative_pos_in_block: 0,
                    unique_id_in_scope: -1,
                });
                let memory_stmt_id = ctx.heap.alloc_memory_statement(|this| MemoryStatement{
                    this,
                    span: memory_span,
                    variable: local_id,
                    next: StatementId::new_invalid()
                });

                // Allocate the initial assignment
                let variable_expr_id = ctx.heap.alloc_variable_expression(|this| VariableExpression{
                    this,
                    identifier,
                    declaration: None,
                    parent: ExpressionParent::None,
                    concrete_type: Default::default()
                });
                let assignment_expr_id = ctx.heap.alloc_assignment_expression(|this| AssignmentExpression{
                    this,
                    span: assign_span,
                    left: variable_expr_id.upcast(),
                    operation: AssignmentOperator::Set,
                    right: initial_expr_id,
                    parent: ExpressionParent::None,
                    concrete_type: Default::default(),
                });
                let assignment_stmt_id = ctx.heap.alloc_expression_statement(|this| ExpressionStatement{
                    this,
                    span: InputSpan::from_positions(initial_expr_begin_pos, initial_expr_end_pos),
                    expression: assignment_expr_id.upcast(),
                    next: StatementId::new_invalid(),
                });

                return Ok(Some((memory_stmt_id, assignment_stmt_id)))
            }
        }

        // If here then one of the preconditions for a memory statement was not
        // met. So recover the iterator and return
        iter.load(iter_state);
        Ok(None)
    }

    fn consume_expression_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionStatementId, ParseError> {
        let start_pos = iter.last_valid_pos();
        let expression = self.consume_expression(module, iter, ctx)?;
        let end_pos = iter.last_valid_pos();
        consume_token(&module.source, iter, TokenKind::SemiColon)?;

        Ok(ctx.heap.alloc_expression_statement(|this| ExpressionStatement{
            this,
            span: InputSpan::from_positions(start_pos, end_pos),
            expression,
            next: StatementId::new_invalid(),
        }))
    }

    //--------------------------------------------------------------------------
    // Expression Parsing
    //--------------------------------------------------------------------------

    // TODO: @Cleanup This is fine for now. But I prefer my stacktraces not to
    //  look like enterprise Java code...
    fn consume_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_assignment_expression(module, iter, ctx)
    }

    fn consume_assignment_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        // Utility to convert token into assignment operator
        fn parse_assignment_operator(token: Option<TokenKind>) -> Option<AssignmentOperator> {
            use TokenKind as TK;
            use AssignmentOperator as AO;

            if token.is_none() {
                return None
            }

            match token.unwrap() {
                TK::Equal               => Some(AO::Set),
                TK::StarEquals          => Some(AO::Multiplied),
                TK::SlashEquals         => Some(AO::Divided),
                TK::PercentEquals       => Some(AO::Remained),
                TK::PlusEquals          => Some(AO::Added),
                TK::MinusEquals         => Some(AO::Subtracted),
                TK::ShiftLeftEquals     => Some(AO::ShiftedLeft),
                TK::ShiftRightEquals    => Some(AO::ShiftedRight),
                TK::AndEquals           => Some(AO::BitwiseAnded),
                TK::CaretEquals         => Some(AO::BitwiseXored),
                TK::OrEquals            => Some(AO::BitwiseOred),
                _                       => None
            }
        }

        let expr = self.consume_conditional_expression(module, iter, ctx)?;
        if let Some(operation) = parse_assignment_operator(iter.next()) {
            let span = iter.next_span();
            iter.consume();

            let left = expr;
            let right = self.consume_expression(module, iter, ctx)?;

            Ok(ctx.heap.alloc_assignment_expression(|this| AssignmentExpression{
                this, span, left, operation, right,
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast())
        } else {
            Ok(expr)
        }
    }

    fn consume_conditional_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        let result = self.consume_concat_expression(module, iter, ctx)?;
        if let Some(TokenKind::Question) = iter.next() {
            let span = iter.next_span();
            iter.consume();

            let test = result;
            let true_expression = self.consume_expression(module, iter, ctx)?;
            consume_token(&module.source, iter, TokenKind::Colon)?;
            let false_expression = self.consume_expression(module, iter, ctx)?;
            Ok(ctx.heap.alloc_conditional_expression(|this| ConditionalExpression{
                this, span, test, true_expression, false_expression,
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast())
        } else {
            Ok(result)
        }
    }

    fn consume_concat_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::At) => Some(BinaryOperator::Concatenate),
                _ => None
            },
            Self::consume_logical_or_expression
        )
    }

    fn consume_logical_or_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::OrOr) => Some(BinaryOperator::LogicalOr),
                _ => None
            },
            Self::consume_logical_and_expression
        )
    }

    fn consume_logical_and_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::AndAnd) => Some(BinaryOperator::LogicalAnd),
                _ => None
            },
            Self::consume_bitwise_or_expression
        )
    }

    fn consume_bitwise_or_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::Or) => Some(BinaryOperator::BitwiseOr),
                _ => None
            },
            Self::consume_bitwise_xor_expression
        )
    }

    fn consume_bitwise_xor_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::Caret) => Some(BinaryOperator::BitwiseXor),
                _ => None
            },
            Self::consume_bitwise_and_expression
        )
    }

    fn consume_bitwise_and_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::And) => Some(BinaryOperator::BitwiseAnd),
                _ => None
            },
            Self::consume_equality_expression
        )
    }

    fn consume_equality_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::EqualEqual) => Some(BinaryOperator::Equality),
                Some(TokenKind::NotEqual) => Some(BinaryOperator::Inequality),
                _ => None
            },
            Self::consume_relational_expression
        )
    }

    fn consume_relational_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::OpenAngle) => Some(BinaryOperator::LessThan),
                Some(TokenKind::CloseAngle) => Some(BinaryOperator::GreaterThan),
                Some(TokenKind::LessEquals) => Some(BinaryOperator::LessThanEqual),
                Some(TokenKind::GreaterEquals) => Some(BinaryOperator::GreaterThanEqual),
                _ => None
            },
            Self::consume_shift_expression
        )
    }

    fn consume_shift_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::ShiftLeft) => Some(BinaryOperator::ShiftLeft),
                Some(TokenKind::ShiftRight) => Some(BinaryOperator::ShiftRight),
                _ => None
            },
            Self::consume_add_or_subtract_expression
        )
    }

    fn consume_add_or_subtract_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::Plus) => Some(BinaryOperator::Add),
                Some(TokenKind::Minus) => Some(BinaryOperator::Subtract),
                _ => None,
            },
            Self::consume_multiply_divide_or_modulus_expression
        )
    }

    fn consume_multiply_divide_or_modulus_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        self.consume_generic_binary_expression(
            module, iter, ctx,
            |token| match token {
                Some(TokenKind::Star) => Some(BinaryOperator::Multiply),
                Some(TokenKind::Slash) => Some(BinaryOperator::Divide),
                Some(TokenKind::Percent) => Some(BinaryOperator::Remainder),
                _ => None
            },
            Self::consume_prefix_expression
        )
    }

    fn consume_prefix_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        fn parse_prefix_token(token: Option<TokenKind>) -> Option<UnaryOperator> {
            use TokenKind as TK;
            use UnaryOperator as UO;
            match token {
                Some(TK::Plus) => Some(UO::Positive),
                Some(TK::Minus) => Some(UO::Negative),
                Some(TK::PlusPlus) => Some(UO::PreIncrement),
                Some(TK::MinusMinus) => Some(UO::PreDecrement),
                Some(TK::Tilde) => Some(UO::BitwiseNot),
                Some(TK::Exclamation) => Some(UO::LogicalNot),
                _ => None
            }
        }

        if let Some(operation) = parse_prefix_token(iter.next()) {
            let span = iter.next_span();
            iter.consume();

            let expression = self.consume_prefix_expression(module, iter, ctx)?;
            Ok(ctx.heap.alloc_unary_expression(|this| UnaryExpression {
                this, span, operation, expression,
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default()
            }).upcast())
        } else {
            self.consume_postfix_expression(module, iter, ctx)
        }
    }

    fn consume_postfix_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        fn has_matching_postfix_token(token: Option<TokenKind>) -> bool {
            use TokenKind as TK;

            if token.is_none() { return false; }
            match token.unwrap() {
                TK::PlusPlus | TK::MinusMinus | TK::OpenSquare | TK::Dot => true,
                _ => false
            }
        }

        let mut result = self.consume_primary_expression(module, iter, ctx)?;
        let mut next = iter.next();
        while has_matching_postfix_token(next) {
            let token = next.unwrap();
            let mut span = iter.next_span();
            iter.consume();

            if token == TokenKind::PlusPlus {
                result = ctx.heap.alloc_unary_expression(|this| UnaryExpression{
                    this, span,
                    operation: UnaryOperator::PostIncrement,
                    expression: result,
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default()
                }).upcast();
            } else if token == TokenKind::MinusMinus {
                result = ctx.heap.alloc_unary_expression(|this| UnaryExpression{
                    this, span,
                    operation: UnaryOperator::PostDecrement,
                    expression: result,
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default()
                }).upcast();
            } else if token == TokenKind::OpenSquare {
                let subject = result;
                let from_index = self.consume_expression(module, iter, ctx)?;

                // Check if we have an indexing or slicing operation
                next = iter.next();
                if Some(TokenKind::DotDot) == next {
                    iter.consume();

                    let to_index = self.consume_expression(module, iter, ctx)?;
                    let end_span = consume_token(&module.source, iter, TokenKind::CloseSquare)?;
                    span.end = end_span.end;

                    result = ctx.heap.alloc_slicing_expression(|this| SlicingExpression{
                        this, span, subject, from_index, to_index,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default()
                    }).upcast();
                } else if Some(TokenKind::CloseSquare) == next {
                    let end_span = consume_token(&module.source, iter, TokenKind::CloseSquare)?;
                    span.end = end_span.end;

                    result = ctx.heap.alloc_indexing_expression(|this| IndexingExpression{
                        this, span, subject,
                        index: from_index,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default()
                    }).upcast();
                } else {
                    return Err(ParseError::new_error_str_at_pos(
                        &module.source, iter.last_valid_pos(), "unexpected token: expected ']' or '..'"
                    ));
                }
            } else {
                debug_assert_eq!(token, TokenKind::Dot);
                let subject = result;
                let (field_text, field_span) = consume_ident(&module.source, iter)?;
                let field = if field_text == b"length" {
                    Field::Length
                } else {
                    let value = ctx.pool.intern(field_text);
                    let identifier = Identifier{ value, span: field_span };
                    Field::Symbolic(FieldSymbolic{ identifier, definition: None, field_idx: 0 })
                };

                result = ctx.heap.alloc_select_expression(|this| SelectExpression{
                    this, span, subject, field,
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default()
                }).upcast();
            }

            next = iter.next();
        }

        Ok(result)
    }

    fn consume_primary_expression(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<ExpressionId, ParseError> {
        let next = iter.next();

        let result = if next == Some(TokenKind::OpenParen) {
            // Expression between parentheses
            iter.consume();
            let result = self.consume_expression(module, iter, ctx)?;
            consume_token(&module.source, iter, TokenKind::CloseParen)?;

            result
        } else if next == Some(TokenKind::OpenCurly) {
            // Array literal
            let (start_pos, mut end_pos) = iter.next_positions();
            let mut scoped_section = self.expressions.start_section();
            consume_comma_separated(
                TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter, ctx,
                |_source, iter, ctx| self.consume_expression(module, iter, ctx),
                &mut scoped_section, "an expression", "a list of expressions", Some(&mut end_pos)
            )?;

            ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this,
                span: InputSpan::from_positions(start_pos, end_pos),
                value: Literal::Array(scoped_section.into_vec()),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast()
        } else if next == Some(TokenKind::Integer) {
            let (literal, span) = consume_integer_literal(&module.source, iter, &mut self.buffer)?;

            ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::Integer(LiteralInteger{ unsigned_value: literal, negated: false }),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast()
        } else if next == Some(TokenKind::String) {
            let span = consume_string_literal(&module.source, iter, &mut self.buffer)?;
            let interned = ctx.pool.intern(self.buffer.as_bytes());

            ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::String(interned),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast()
        } else if next == Some(TokenKind::Character) {
            let (character, span) = consume_character_literal(&module.source, iter)?;

            ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::Character(character),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast()
        } else if next == Some(TokenKind::Ident) {
            // May be a variable, a type instantiation or a function call. If we
            // have a single identifier that we cannot find in the type table
            // then we're going to assume that we're dealing with a variable.
            let ident_span = iter.next_span();
            let ident_text = module.source.section_at_span(ident_span);
            let symbol = ctx.symbols.get_symbol_by_name(SymbolScope::Module(module.root_id), ident_text);

            if symbol.is_some() {
                // The first bit looked like a symbol, so we're going to follow
                // that all the way through, assume we arrive at some kind of
                // function call or type instantiation
                use ParserTypeVariant as PTV;

                let symbol_scope = SymbolScope::Definition(self.cur_definition);
                let poly_vars = ctx.heap[self.cur_definition].poly_vars();
                let parser_type = consume_parser_type(
                    &module.source, iter, &ctx.symbols, &ctx.heap, poly_vars, symbol_scope,
                    self.cur_definition, true, 0
                )?;
                debug_assert!(!parser_type.elements.is_empty());
                match parser_type.elements[0].variant {
                    PTV::Definition(target_definition_id, _) => {
                        let definition = &ctx.heap[target_definition_id];
                        match definition {
                            Definition::Struct(_) => {
                                // Struct literal
                                let mut last_token = iter.last_valid_pos();
                                let mut struct_fields = Vec::new();
                                consume_comma_separated(
                                    TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter, ctx,
                                    |source, iter, ctx| {
                                        let identifier = consume_ident_interned(source, iter, ctx)?;
                                        consume_token(source, iter, TokenKind::Colon)?;
                                        let value = self.consume_expression(module, iter, ctx)?;
                                        Ok(LiteralStructField{ identifier, value, field_idx: 0 })
                                    },
                                    &mut struct_fields, "a struct field", "a list of struct fields", Some(&mut last_token)
                                )?;

                                ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                                    this,
                                    span: InputSpan::from_positions(ident_span.begin, last_token),
                                    value: Literal::Struct(LiteralStruct{
                                        parser_type,
                                        fields: struct_fields,
                                        definition: target_definition_id,
                                    }),
                                    parent: ExpressionParent::None,
                                    concrete_type: ConcreteType::default(),
                                }).upcast()
                            },
                            Definition::Enum(_) => {
                                // Enum literal: consume the variant
                                consume_token(&module.source, iter, TokenKind::ColonColon)?;
                                let variant = consume_ident_interned(&module.source, iter, ctx)?;

                                ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                                    this,
                                    span: InputSpan::from_positions(ident_span.begin, variant.span.end),
                                    value: Literal::Enum(LiteralEnum{
                                        parser_type,
                                        variant,
                                        definition: target_definition_id,
                                        variant_idx: 0
                                    }),
                                    parent: ExpressionParent::None,
                                    concrete_type: ConcreteType::default()
                                }).upcast()
                            },
                            Definition::Union(_) => {
                                // Union literal: consume the variant
                                consume_token(&module.source, iter, TokenKind::ColonColon)?;
                                let variant = consume_ident_interned(&module.source, iter, ctx)?;

                                // Consume any possible embedded values
                                let mut end_pos = variant.span.end;
                                let values = if Some(TokenKind::OpenParen) == iter.next() {
                                    self.consume_expression_list(module, iter, ctx, Some(&mut end_pos))?
                                } else {
                                    Vec::new()
                                };

                                ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                                    this,
                                    span: InputSpan::from_positions(ident_span.begin, end_pos),
                                    value: Literal::Union(LiteralUnion{
                                        parser_type, variant, values,
                                        definition: target_definition_id,
                                        variant_idx: 0,
                                    }),
                                    parent: ExpressionParent::None,
                                    concrete_type: ConcreteType::default()
                                }).upcast()
                            },
                            Definition::Component(_) => {
                                // Component instantiation
                                let arguments = self.consume_expression_list(module, iter, ctx, None)?;

                                ctx.heap.alloc_call_expression(|this| CallExpression{
                                    this,
                                    span: parser_type.elements[0].full_span, // TODO: @Span fix
                                    parser_type,
                                    method: Method::UserComponent,
                                    arguments,
                                    definition: target_definition_id,
                                    parent: ExpressionParent::None,
                                    concrete_type: ConcreteType::default(),
                                }).upcast()
                            },
                            Definition::Function(function_definition) => {
                                // Check whether it is a builtin function
                                let method = if function_definition.builtin {
                                    match function_definition.identifier.value.as_str() {
                                        "get" => Method::Get,
                                        "put" => Method::Put,
                                        "fires" => Method::Fires,
                                        "create" => Method::Create,
                                        "length" => Method::Length,
                                        "assert" => Method::Assert,
                                        _ => unreachable!(),
                                    }
                                } else {
                                    Method::UserFunction
                                };

                                // Function call: consume the arguments
                                let arguments = self.consume_expression_list(module, iter, ctx, None)?;

                                ctx.heap.alloc_call_expression(|this| CallExpression{
                                    this,
                                    span: parser_type.elements[0].full_span, // TODO: @Span fix
                                    parser_type,
                                    method,
                                    arguments,
                                    definition: target_definition_id,
                                    parent: ExpressionParent::None,
                                    concrete_type: ConcreteType::default(),
                                }).upcast()
                            }
                        }
                    },
                    _ => {
                        // TODO: Casting expressions
                        return Err(ParseError::new_error_str_at_span(
                            &module.source, parser_type.elements[0].full_span,
                            "unexpected type in expression, note that casting expressions are not yet implemented"
                        ))
                    }
                }
            } else {
                // Check for builtin keywords or builtin functions
                if ident_text == KW_LIT_NULL || ident_text == KW_LIT_TRUE || ident_text == KW_LIT_FALSE {
                    iter.consume();

                    // Parse builtin literal
                    let value = match ident_text {
                        KW_LIT_NULL => Literal::Null,
                        KW_LIT_TRUE => Literal::True,
                        KW_LIT_FALSE => Literal::False,
                        _ => unreachable!(),
                    };

                    ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                        this,
                        span: ident_span,
                        value,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default(),
                    }).upcast()
                } else {
                    // Not a builtin literal, but also not a known type. So we
                    // assume it is a variable expression. Although if we do,
                    // then if a programmer mistyped a struct/function name the
                    // error messages will be rather cryptic. For polymorphic
                    // arguments we can't really do anything at all (because it
                    // uses the '<' token). In the other cases we try to provide
                    // a better error message.
                    iter.consume();
                    let next = iter.next();
                    if Some(TokenKind::ColonColon) == next {
                        return Err(ParseError::new_error_str_at_span(&module.source, ident_span, "unknown identifier"));
                    } else if Some(TokenKind::OpenParen) == next {
                        return Err(ParseError::new_error_str_at_span(
                            &module.source, ident_span,
                            "unknown identifier, did you mistype a union variant's or a function's name?"
                        ));
                    } else if Some(TokenKind::OpenCurly) == next {
                        return Err(ParseError::new_error_str_at_span(
                            &module.source, ident_span,
                            "unknown identifier, did you mistype a struct type's name?"
                        ))
                    }

                    let ident_text = ctx.pool.intern(ident_text);
                    let identifier = Identifier { span: ident_span, value: ident_text };

                    ctx.heap.alloc_variable_expression(|this| VariableExpression {
                        this,
                        identifier,
                        declaration: None,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default()
                    }).upcast()
                }
            }
        } else {
            return Err(ParseError::new_error_str_at_pos(
                &module.source, iter.last_valid_pos(), "expected an expression"
            ));
        };

        Ok(result)
    }

    //--------------------------------------------------------------------------
    // Expression Utilities
    //--------------------------------------------------------------------------

    #[inline]
    fn consume_generic_binary_expression<
        M: Fn(Option<TokenKind>) -> Option<BinaryOperator>,
        F: Fn(&mut PassDefinitions, &Module, &mut TokenIter, &mut PassCtx) -> Result<ExpressionId, ParseError>
    >(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, match_fn: M, higher_precedence_fn: F
    ) -> Result<ExpressionId, ParseError> {
        let mut result = higher_precedence_fn(self, module, iter, ctx)?;
        while let Some(operation) = match_fn(iter.next()) {
            let span = iter.next_span();
            iter.consume();

            let left = result;
            let right = higher_precedence_fn(self, module, iter, ctx)?;

            result = ctx.heap.alloc_binary_expression(|this| BinaryExpression{
                this, span, left, operation, right,
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default()
            }).upcast();
        }

        Ok(result)
    }

    #[inline]
    fn consume_expression_list(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, end_pos: Option<&mut InputPosition>
    ) -> Result<Vec<ExpressionId>, ParseError> {
        let mut section = self.expressions.start_section();
        consume_comma_separated(
            TokenKind::OpenParen, TokenKind::CloseParen, &module.source, iter, ctx,
            |_source, iter, ctx| self.consume_expression(module, iter, ctx),
            &mut section, "an expression", "a list of expressions", end_pos
        )?;
        Ok(section.into_vec())
    }
}

/// Consumes a type. A type always starts with an identifier which may indicate
/// a builtin type or a user-defined type. The fact that it may contain
/// polymorphic arguments makes it a tree-like structure. Because we cannot rely
/// on knowing the exact number of polymorphic arguments we do not check for
/// these.
///
/// Note that the first depth index is used as a hack.
// TODO: @Optimize, @Span fix, @Cleanup
fn consume_parser_type(
    source: &InputSource, iter: &mut TokenIter, symbols: &SymbolTable, heap: &Heap, poly_vars: &[Identifier],
    cur_scope: SymbolScope, wrapping_definition: DefinitionId, allow_inference: bool, first_angle_depth: i32,
) -> Result<ParserType, ParseError> {
    struct Entry{
        element: ParserTypeElement,
        depth: i32,
    }

    // After parsing the array modified "[]", we need to insert an array type
    // before the most recently parsed type.
    fn insert_array_before(elements: &mut Vec<Entry>, depth: i32, span: InputSpan) {
        let index = elements.iter().rposition(|e| e.depth == depth).unwrap();
        elements.insert(index, Entry{
            element: ParserTypeElement{ full_span: span, variant: ParserTypeVariant::Array },
            depth,
        });
    }

    // Most common case we just have one type, perhaps with some array
    // annotations. This is both the hot-path, and simplifies the state machine
    // that follows and is responsible for parsing more complicated types.
    let element = consume_parser_type_ident(source, iter, symbols, heap, poly_vars, cur_scope, wrapping_definition, allow_inference)?;
    if iter.next() != Some(TokenKind::OpenAngle) {
        let num_embedded = element.variant.num_embedded();
        let mut num_array = 0;
        while iter.next() == Some(TokenKind::OpenSquare) {
            iter.consume();
            consume_token(source, iter, TokenKind::CloseSquare)?;
            num_array += 1;
        }

        let array_span = element.full_span;
        let mut elements = Vec::with_capacity(num_array + num_embedded + 1);
        for _ in 0..num_array {
            elements.push(ParserTypeElement{ full_span: array_span, variant: ParserTypeVariant::Array });
        }
        elements.push(element);

        if num_embedded != 0 {
            if !allow_inference {
                return Err(ParseError::new_error_str_at_span(source, array_span, "type inference is not allowed here"));
            }

            for _ in 0..num_embedded {
                elements.push(ParserTypeElement { full_span: array_span, variant: ParserTypeVariant::Inferred });
            }
        }

        return Ok(ParserType{ elements });
    };

    // We have a polymorphic specification. So we start by pushing the item onto
    // our stack, then start adding entries together with the angle-brace depth
    // at which they're found.
    let mut elements = Vec::new();
    elements.push(Entry{ element, depth: 0 });

    // Start out with the first '<' consumed.
    iter.consume();
    enum State { Ident, Open, Close, Comma }
    let mut state = State::Open;
    let mut angle_depth = first_angle_depth + 1;

    loop {
        let next = iter.next();

        match state {
            State::Ident => {
                // Just parsed an identifier, may expect comma, angled braces,
                // or the tokens indicating an array
                if Some(TokenKind::OpenAngle) == next {
                    angle_depth += 1;
                    state = State::Open;
                } else if Some(TokenKind::CloseAngle) == next {
                    angle_depth -= 1;
                    state = State::Close;
                } else if Some(TokenKind::ShiftRight) == next {
                    angle_depth -= 2;
                    state = State::Close;
                } else if Some(TokenKind::Comma) == next {
                    state = State::Comma;
                } else if Some(TokenKind::OpenSquare) == next {
                    let (start_pos, _) = iter.next_positions();
                    iter.consume(); // consume opening square
                    if iter.next() != Some(TokenKind::CloseSquare) {
                        return Err(ParseError::new_error_str_at_pos(
                            source, iter.last_valid_pos(),
                            "unexpected token: expected ']'"
                        ));
                    }
                    let (_, end_pos) = iter.next_positions();
                    let array_span = InputSpan::from_positions(start_pos, end_pos);
                    insert_array_before(&mut elements, angle_depth, array_span);
                } else {
                    return Err(ParseError::new_error_str_at_pos(
                        source, iter.last_valid_pos(),
                        "unexpected token: expected '<', '>', ',' or '['")
                    );
                }

                iter.consume();
            },
            State::Open => {
                // Just parsed an opening angle bracket, expecting an identifier
                let element = consume_parser_type_ident(source, iter, symbols, heap, poly_vars, cur_scope, wrapping_definition, allow_inference)?;
                elements.push(Entry{ element, depth: angle_depth });
                state = State::Ident;
            },
            State::Close => {
                // Just parsed 1 or 2 closing angle brackets, expecting comma,
                // more closing brackets or the tokens indicating an array
                if Some(TokenKind::Comma) == next {
                    state = State::Comma;
                } else if Some(TokenKind::CloseAngle) == next {
                    angle_depth -= 1;
                    state = State::Close;
                } else if Some(TokenKind::ShiftRight) == next {
                    angle_depth -= 2;
                    state = State::Close;
                } else if Some(TokenKind::OpenSquare) == next {
                    let (start_pos, _) = iter.next_positions();
                    iter.consume();
                    if iter.next() != Some(TokenKind::CloseSquare) {
                        return Err(ParseError::new_error_str_at_pos(
                            source, iter.last_valid_pos(),
                            "unexpected token: expected ']'"
                        ));
                    }
                    let (_, end_pos) = iter.next_positions();
                    let array_span = InputSpan::from_positions(start_pos, end_pos);
                    insert_array_before(&mut elements, angle_depth, array_span);
                } else {
                    return Err(ParseError::new_error_str_at_pos(
                        source, iter.last_valid_pos(),
                        "unexpected token: expected ',', '>', or '['")
                    );
                }

                iter.consume();
            },
            State::Comma => {
                // Just parsed a comma, expecting an identifier or more closing
                // braces
                if Some(TokenKind::Ident) == next {
                    let element = consume_parser_type_ident(source, iter, symbols, heap, poly_vars, cur_scope, wrapping_definition, allow_inference)?;
                    elements.push(Entry{ element, depth: angle_depth });
                    state = State::Ident;
                } else if Some(TokenKind::CloseAngle) == next {
                    iter.consume();
                    angle_depth -= 1;
                    state = State::Close;
                } else if Some(TokenKind::ShiftRight) == next {
                    iter.consume();
                    angle_depth -= 2;
                    state = State::Close;
                } else {
                    return Err(ParseError::new_error_str_at_pos(
                        source, iter.last_valid_pos(),
                        "unexpected token: expected '>' or a type name"
                    ));
                }
            }
        }

        if angle_depth < 0 {
            return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "unmatched '>'"));
        } else if angle_depth == 0 {
            break;
        }
    }

    // If here then we found the correct number of angle braces. But we still
    // need to make sure that each encountered type has the correct number of
    // embedded types.
    let mut idx = 0;
    while idx < elements.len() {
        let cur_element = &elements[idx];
        let expected_subtypes = cur_element.element.variant.num_embedded();
        let mut encountered_subtypes = 0;
        for peek_idx in idx + 1..elements.len() {
            let peek_element = &elements[peek_idx];
            if peek_element.depth == cur_element.depth + 1 {
                encountered_subtypes += 1;
            } else if peek_element.depth <= cur_element.depth {
                break;
            }
        }

        if expected_subtypes != encountered_subtypes {
            if encountered_subtypes == 0 {
                // Case where we have elided the embedded types, all of them
                // should be inferred.
                if !allow_inference {
                    return Err(ParseError::new_error_str_at_span(
                        source, cur_element.element.full_span,
                        "type inference is not allowed here"
                    ));
                }

                // Insert the missing types
                let inserted_span = cur_element.element.full_span;
                let inserted_depth = cur_element.depth + 1;
                elements.reserve(expected_subtypes);
                for _ in 0..expected_subtypes {
                    elements.insert(idx + 1, Entry{
                        element: ParserTypeElement{ full_span: inserted_span, variant: ParserTypeVariant::Inferred },
                        depth: inserted_depth,
                    });
                }
            } else {
                // Mismatch in number of embedded types, produce a neat error
                // message.
                let type_name = String::from_utf8_lossy(source.section_at_span(cur_element.element.full_span));
                fn polymorphic_name_text(num: usize) -> &'static str {
                    if num == 1 { "polymorphic argument" } else { "polymorphic arguments" }
                }
                fn were_or_was(num: usize) -> &'static str {
                    if num == 1 { "was" } else { "were" }
                }

                if expected_subtypes == 0 {
                    return Err(ParseError::new_error_at_span(
                        source, cur_element.element.full_span,
                        format!(
                            "the type '{}' is not polymorphic, yet {} {} {} provided",
                            type_name, encountered_subtypes, polymorphic_name_text(encountered_subtypes),
                            were_or_was(encountered_subtypes)
                        )
                    ));
                }

                let maybe_infer_text = if allow_inference {
                    " (or none, to perform implicit type inference)"
                } else {
                    ""
                };

                return Err(ParseError::new_error_at_span(
                    source, cur_element.element.full_span,
                    format!(
                        "expected {} {}{} for the type '{}', but {} {} provided",
                        expected_subtypes, polymorphic_name_text(expected_subtypes),
                        maybe_infer_text, type_name, encountered_subtypes,
                        were_or_was(encountered_subtypes)
                    )
                ));
            }
        }

        idx += 1;
    }

    let mut constructed_elements = Vec::with_capacity(elements.len());
    for element in elements.into_iter() {
        constructed_elements.push(element.element);
    }

    Ok(ParserType{ elements: constructed_elements })
}

/// Consumes an identifier for which we assume that it resolves to some kind of
/// type. Once we actually arrive at a type we will stop parsing. Hence there
/// may be trailing '::' tokens in the iterator.
fn consume_parser_type_ident(
    source: &InputSource, iter: &mut TokenIter, symbols: &SymbolTable, heap: &Heap, poly_vars: &[Identifier],
    mut scope: SymbolScope, wrapping_definition: DefinitionId, allow_inference: bool,
) -> Result<ParserTypeElement, ParseError> {
    use ParserTypeVariant as PTV;
    let (mut type_text, mut type_span) = consume_any_ident(source, iter)?;

    let variant = match type_text {
        KW_TYPE_MESSAGE => PTV::Message,
        KW_TYPE_BOOL => PTV::Bool,
        KW_TYPE_UINT8 => PTV::UInt8,
        KW_TYPE_UINT16 => PTV::UInt16,
        KW_TYPE_UINT32 => PTV::UInt32,
        KW_TYPE_UINT64 => PTV::UInt64,
        KW_TYPE_SINT8 => PTV::SInt8,
        KW_TYPE_SINT16 => PTV::SInt16,
        KW_TYPE_SINT32 => PTV::SInt32,
        KW_TYPE_SINT64 => PTV::SInt64,
        KW_TYPE_IN_PORT => PTV::Input,
        KW_TYPE_OUT_PORT => PTV::Output,
        KW_TYPE_CHAR => PTV::Character,
        KW_TYPE_STRING => PTV::String,
        KW_TYPE_INFERRED => {
            if !allow_inference {
                return Err(ParseError::new_error_str_at_span(source, type_span, "type inference is not allowed here"));
            }

            PTV::Inferred
        },
        _ => {
            // Must be some kind of symbolic type
            let mut type_kind = None;
            for (poly_idx, poly_var) in poly_vars.iter().enumerate() {
                if poly_var.value.as_bytes() == type_text {
                    type_kind = Some(PTV::PolymorphicArgument(wrapping_definition, poly_idx));
                }
            }

            if type_kind.is_none() {
                // Check symbol table for definition. To be fair, the language
                // only allows a single namespace for now. That said:
                let last_symbol = symbols.get_symbol_by_name(scope, type_text);
                if last_symbol.is_none() {
                    return Err(ParseError::new_error_str_at_span(source, type_span, "unknown type"));
                }
                let mut last_symbol = last_symbol.unwrap();

                loop {
                    match &last_symbol.variant {
                        SymbolVariant::Module(symbol_module) => {
                            // Expecting more identifiers
                            if Some(TokenKind::ColonColon) != iter.next() {
                                return Err(ParseError::new_error_str_at_span(source, type_span, "expected a type but got a module"));
                            }

                            consume_token(source, iter, TokenKind::ColonColon)?;

                            // Consume next part of type and prepare for next
                            // lookup loop
                            let (next_text, next_span) = consume_any_ident(source, iter)?;
                            let old_text = type_text;
                            type_text = next_text;
                            type_span.end = next_span.end;
                            scope = SymbolScope::Module(symbol_module.root_id);

                            let new_symbol = symbols.get_symbol_by_name_defined_in_scope(scope, type_text);
                            if new_symbol.is_none() {
                                // If the type is imported in the module then notify the programmer
                                // that imports do not leak outside of a module
                                let type_name = String::from_utf8_lossy(type_text);
                                let module_name = String::from_utf8_lossy(old_text);
                                let suffix = if symbols.get_symbol_by_name(scope, type_text).is_some() {
                                    format!(
                                        ". The module '{}' does import '{}', but these imports are not visible to other modules",
                                        &module_name, &type_name
                                    )
                                } else {
                                    String::new()
                                };

                                return Err(ParseError::new_error_at_span(
                                    source, next_span,
                                    format!("unknown type '{}' in module '{}'{}", type_name, module_name, suffix)
                                ));
                            }

                            last_symbol = new_symbol.unwrap();
                        },
                        SymbolVariant::Definition(symbol_definition) => {
                            let num_poly_vars = heap[symbol_definition.definition_id].poly_vars().len();
                            type_kind = Some(PTV::Definition(symbol_definition.definition_id, num_poly_vars));
                            break;
                        }
                    }
                }
            }

            debug_assert!(type_kind.is_some());
            type_kind.unwrap()
        },
    };

    Ok(ParserTypeElement{ full_span: type_span, variant })
}

/// Consumes polymorphic variables and throws them on the floor.
fn consume_polymorphic_vars_spilled(source: &InputSource, iter: &mut TokenIter, _ctx: &mut PassCtx) -> Result<(), ParseError> {
    maybe_consume_comma_separated_spilled(
        TokenKind::OpenAngle, TokenKind::CloseAngle, source, iter, _ctx,
        |source, iter, _ctx| {
            consume_ident(source, iter)?;
            Ok(())
        }, "a polymorphic variable"
    )?;
    Ok(())
}

/// Consumes the parameter list to functions/components
fn consume_parameter_list(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx,
    target: &mut ScopedSection<ParameterId>, scope: SymbolScope, definition_id: DefinitionId
) -> Result<(), ParseError> {
    consume_comma_separated(
        TokenKind::OpenParen, TokenKind::CloseParen, source, iter, ctx,
        |source, iter, ctx| {
            let poly_vars = ctx.heap[definition_id].poly_vars(); // TODO: @Cleanup, this is really ugly. But rust...
            let parser_type = consume_parser_type(
                source, iter, &ctx.symbols, &ctx.heap, poly_vars, scope,
                definition_id, false, 0
            )?;
            let identifier = consume_ident_interned(source, iter, ctx)?;
            let parameter_id = ctx.heap.alloc_variable(|this| Variable{
                this,
                kind: VariableKind::Parameter,
                parser_type,
                identifier,
                relative_pos_in_block: 0,
                unique_id_in_scope: -1,
            });
            Ok(parameter_id)
        },
        target, "a parameter", "a parameter list", None
    )
}
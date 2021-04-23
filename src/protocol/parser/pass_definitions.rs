use crate::protocol::ast::*;
use super::symbol_table2::*;
use super::{Module, ModuleCompilationPhase, PassCtx};
use super::tokens::*;
use super::token_parsing::*;
use crate::protocol::input_source2::{InputSource2 as InputSource, InputPosition2 as InputPosition, InputSpan, ParseError};
use crate::collections::*;

/// Parses all the tokenized definitions into actual AST nodes.
pub(crate) struct PassDefinitions {
    buffer: String,
    identifiers: Vec<Identifier>,
    struct_fields: Vec<StructFieldDefinition>,
    enum_variants: Vec<EnumVariantDefinition>,
    union_variants: Vec<UnionVariantDefinition>,
    parameters: Vec<ParameterId>,
    expressions: ScopedBuffer<ExpressionId>,
    parser_types: Vec<ParserType>,
}

impl PassDefinitions {
    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut PassCtx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert_eq!(module.phase, ModuleCompilationPhase::ImportsResolved);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);

        // TODO: Very important to go through ALL ranges of the module so that we parse the entire
        //  input source. Only skip the ones we're certain we've handled before.
        let mut range_idx = module_range.first_child_idx;
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];

            if cur_range.range_kind == TokenRangeKind::Definition {
                self.visit_definition_range(modules, module_idx, ctx, range_idx_usize)?;
            }

            match cur_range.next_sibling_idx {
                Some(idx) => { range_idx = idx; },
                None => { break; },
            }
        }



        Ok(())
    }

    fn visit_definition_range(
        &mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize
    ) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let cur_range = &module.tokens.ranges[range_idx];
        debug_assert_eq!(cur_range.range_kind, TokenRangeKind::Definition);

        // Detect which definition we're parsing
        let mut iter = module.tokens.iter_range(cur_range);
        let keyword = peek_ident(&module.source, &mut iter).unwrap();
        match keyword {
            KW_STRUCT => {

            },
            KW_ENUM => {

            },
            KW_UNION => {

            },
            KW_FUNCTION => {

            },
            KW_PRIMITIVE => {

            },
            KW_COMPOSITE => {

            },
            _ => unreachable!("encountered keyword '{}' in definition range", String::from_utf8_lossy(keyword)),
        };

        Ok(())
    }

    // TODO: @Cleanup, still not sure about polymorphic variable parsing. Pre-parsing the variables
    //  allows us to directly construct proper ParserType trees. But this does require two lookups
    //  of the corresponding definition.
    fn visit_struct_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        consume_exact_ident(&module.source, iter, KW_STRUCT)?;
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // Retrieve preallocated DefinitionId
        let module_scope = SymbolScope::Module(module.root_id);
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(module_scope, ident_text)
            .unwrap().variant.as_definition().definition_id;
        let poly_vars = ctx.heap[definition_id].poly_vars();

        // Parse struct definition
        consume_polymorphic_vars_spilled(source, iter)?;
        debug_assert!(self.struct_fields.is_empty());
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, source, iter,
            |source, iter| {
                let parser_type = consume_parser_type(
                    source, iter, &ctx.symbols, &ctx.heap, poly_vars, module_scope, definition_id, false
                )?;
                let field = consume_ident_interned(source, iter, ctx)?;
                Ok(StructFieldDefinition{ field, parser_type })
            },
            &mut self.struct_fields, "a struct field", "a list of struct fields"
        )?;

        // Transfer to preallocated definition
        let struct_def = ctx.heap[definition_id].as_struct_mut();
        struct_def.fields.clone_from(&self.struct_fields);
        self.struct_fields.clear();

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
        let poly_vars = ctx.heap[definition_id].poly_vars();

        // Parse enum definition
        consume_polymorphic_vars_spilled(source, iter)?;
        debug_assert!(self.enum_variants.is_empty());
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, source, iter,
            |source, iter| {
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
            &mut self.enum_variants, "an enum variant", "a list of enum variants"
        )?;

        // Transfer to definition
        let enum_def = ctx.heap[definition_id].as_enum_mut();
        enum_def.variants.clone_from(&self.enum_variants);
        self.enum_variants.clear();

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
        let poly_vars = ctx.heap[definition_id].poly_vars();

        // Parse union definition
        consume_polymorphic_vars_spilled(source, iter)?;
        debug_assert!(self.union_variants.is_empty());
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, source, iter,
            |source, iter| {
                let identifier = consume_ident_interned(source, iter, ctx)?;
                let close_pos = identifier.span.end;
                let has_embedded = maybe_consume_comma_separated(
                    TokenKind::OpenParen, TokenKind::CloseParen, source, iter,
                    |source, iter| {
                        consume_parser_type(
                            source, iter, &ctx.symbols, &ctx.heap, poly_vars,
                            module_scope, definition_id, false
                        )
                    },
                    &mut self.parser_types, "an embedded type", Some(&mut close_pos)
                )?;
                let value = if has_embedded {
                    UnionVariantValue::Embedded(self.parser_types.clone())
                } else {
                    UnionVariantValue::None
                };
                self.parser_types.clear();

                Ok(UnionVariantDefinition{
                    span: InputSpan::from_positions(identifier.span.begin, close_pos),
                    identifier,
                    value
                })
            },
            &mut self.union_variants, "a union variant", "a list of union variants", None
        )?;

        // Transfer to AST
        let union_def = ctx.heap[definition_id].as_union_mut();
        union_def.variants.clone_from(&self.union_variants);
        self.union_variants.clear();

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
        let poly_vars = ctx.heap[definition_id].poly_vars();

        // Parse function's argument list
        consume_parameter_list(
            source, iter, ctx, &mut self.parameters, poly_vars, module_scope, definition_id
        )?;
        let parameters = self.parameters.clone();
        self.parameters.clear();

        // Consume return types
        consume_comma_separated(
            TokenKind::ArrowRight, TokenKind::OpenCurly, &module.source, iter,
            |source, iter| {
                consume_parser_type(source, iter, &ctx.symbols, &ctx.heap, poly_vars, module_scope, definition_id, false)
            },
            &mut self.parser_types, "a return type", "the return types", None
        )?;
        let return_types = self.parser_types.clone();
        self.parser_types.clear();

        // Consume block
    }

    fn consume_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<StatementId, ParseError> {
        let next = iter.next().expect("consume_statement has a next token");

        if next == TokenKind::OpenCurly {
            return self.consume_block_statement(module, iter, ctx)?.upcast();
        } else if next == TokenKind::Ident {
            let (ident, _) = consume_any_ident(source, iter)?;
            if ident == KW_STMT_IF {
                return self.consume_if_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_WHILE {
                return self.consume_while_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_BREAK {
                return self.consume_break_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_CONTINUE {
                return self.consume_continue_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_SYNC {
                return self.consume_synchronous_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_RETURN {
                return self.consume_return_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_ASSERT {
                // TODO: Unify all builtin function calls as expressions
                return self.consume_assert_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_GOTO {
                return self.consume_goto_statement(module, iter, ctx)?;
            } else if ident == KW_STMT_NEW {
                return self.consume_new_statement(module, iter, ctx)?;
            } else if iter.peek() == Some(TokenKind::Colon) {
                return self.consume_labeled_statement(module, iter, ctx)?;
            }
        }

        // If here then attempt to parse as expression
        return self.consume_expr_statement(module, iter, ctx)?;
    }

    fn consume_block_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<BlockStatementId, ParseError> {
        let open_span = consume_token(source, iter, TokenKind::OpenCurly)?;
        self.consume_block_statement_without_leading_curly(module, iter, ctx, open_span.begin)
    }

    fn consume_block_statement_without_leading_curly(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx, open_curly_pos: InputPosition
    ) -> Result<BlockStatementId, ParseError> {
        let mut statements = Vec::new();
        let mut next = iter.next();
        while next.is_some() && next != Some(TokenKind::CloseCurly) {

        }

        let mut block_span = consume_token(&module.source, iter, TokenKind::CloseCurly)?;
        block_span.begin = open_curly_pos;

        Ok(ctx.heap.alloc_block_statement(|this| BlockStatement{
            this,
            span: block_span,
            statements,
            parent_scope: None,
            relative_pos_in_parent: 0,
            locals: Vec::new(),
            labels: Vec::new(),
        }))
    }

    fn consume_if_statement(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<IfStatementId, ParseError> {
        consume_exact_ident(&module.source, iter, KW_STMT_IF)?;
        let test = consume_parenthesized_expression()
    }

    //--------------------------------------------------------------------------
    // Expression Parsing
    //--------------------------------------------------------------------------

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

            let matched = match token.unwrap() {
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
            };
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
            consume_token(source, iter, TokenKind::Colon)?;
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
        &mut self, module: &Module, iter: &mut Tokeniter, ctx: &mut PassCtx
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
        fn parse_prefix_token(token: Option<TokenKind>) -> Some(UnaryOperation) {
            use TokenKind as TK;
            use UnaryOperation as UO;
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
                    operation: UnaryOperation::PostIncrement,
                    expression: result,
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default()
                }).upcast();
            } else if token == TokenKind::MinusMinus {
                result = ctx.heap.alloc_unary_expression(|this| UnaryExpression{
                    this, span,
                    operation: UnaryOperation::PostDecrement,
                    expression: result,
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default()
                }).upcast();
            } else if token == TokenKind::OpenSquare {
                let subject = result;
                let from_index = self.consume_expression(module, iter, ctx)?;

                // Check if we have an indexing or slicing operation
                next = iter.next();
                if Some(TokenKind::DotDot) = next {
                    iter.consume();

                    let to_index = self.consume_expression(module, iter, ctx)?;
                    let end_span = consume_token(&module.source, iter, TokenKind::CloseSquare)?;
                    span.end = end_span.end;

                    result = ctx.heap.alloc_slicing_expression(|this| SlicingExpression{
                        this, span, subject, from_index, to_index,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default()
                    }).upcast();
                } else if Some(TokenKind::CloseSquare) {
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
                    Field::Symbolic(FieldSymbolic{ identifier, definition: None, field_idx: 0 });
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

        let result;
        if next == Some(TokenKind::OpenParen) {
            // Expression between parentheses
            iter.consume();
            result = self.consume_expression(module, iter, ctx)?;
            consume_token(&module.source, iter, TokenKind::CloseParen)?;
        } else if next == Some(TokenKind::OpenCurly) {
            // Array literal
            let (start_pos, mut end_pos) = iter.next_positions();
            let mut expressions = Vec::new();
            consume_comma_separated(
                TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, iter,
                |source, iter| self.consume_expression(module, iter, ctx),
                &mut expressions, "an expression", "a list of expressions", Some(&mut end_pos)
            )?;

            // TODO: Turn into literal
            result = ctx.heap.alloc_array_expression(|this| ArrayExpression{
                this,
                span: InputSpan::from_positions(start_pos, end_pos),
                elements: expressions,
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast();
        } else if next == Some(TokenKind::Integer) {
            let (literal, span) = consume_integer_literal(&module.source, iter, &mut self.buffer)?;
            result = ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::Integer(LiteralInteger{ unsigned_value: literal, negated: false }),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast();
        } else if next == Some(TokenKind::String) {
            let span = consume_string_literal(&module.source, iter, &mut self.buffer)?;
            let interned = ctx.pool.intern(self.buffer.as_bytes());
            result = ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::String(interned),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast();
        } else if next == Some(TokenKind::Character) {
            let (character, span) = consume_character_literal(&module.source, iter)?;
            result = ctx.heap.alloc_literal_expression(|this| LiteralExpression{
                this, span,
                value: Literal::Character(character),
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            }).upcast();
        } else if next == Some(TokenKind::Ident) {
            // May be a variable, a type instantiation or a function call. If we
            // have a single identifier that we cannot find in the type table
            // then we're going to assume that we're dealign with a variable.
        }

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
}

/// Consumes a type. A type always starts with an identifier which may indicate
/// a builtin type or a user-defined type. The fact that it may contain
/// polymorphic arguments makes it a tree-like structure. Because we cannot rely
/// on knowing the exact number of polymorphic arguments we do not check for
/// these.
// TODO: @Optimize, and fix spans if needed
fn consume_parser_type(
    source: &InputSource, iter: &mut TokenIter, symbols: &SymbolTable, heap: &Heap, poly_vars: &[Identifier],
    cur_scope: SymbolScope, wrapping_definition: DefinitionId, allow_inference: bool
) -> Result<ParserType, ParseError> {
    struct Entry{
        element: ParserTypeElement,
        depth: i32,
    }

    fn insert_array_before(elements: &mut Vec<Entry>, depth: i32, span: InputSpan) {
        let index = elements.iter().rposition(|e| e.depth == depth).unwrap();
        elements.insert(index, Entry{
            element: ParserTypeElement{ full_span: span, variant: ParserTypeVariant::Array },
            depth,
        });
    }

    // Most common case we just have one type, perhaps with some array
    // annotations.
    let element = consume_parser_type_ident(source, iter, symbols, heap, poly_vars, cur_scope, wrapping_definition, allow_inference)?;
    if iter.next() != Some(TokenKind::OpenAngle) {
        let mut num_array = 0;
        while iter.next() == Some(TokenKind::OpenSquare) {
            iter.consume();
            consume_token(source, iter, TokenKind::CloseSquare)?;
            num_array += 1;
        }

        let array_span = element.full_span;
        let mut elements = Vec::with_capacity(num_array + 1);
        for _ in 0..num_array {
            elements.push(ParserTypeElement{ full_span: array_span, variant: ParserTypeVariant::Array });
        }
        elements.push(element);

        return Ok(ParserType{ elements });
    };

    // We have a polymorphic specification. So we start by pushing the item onto
    // our stack, then start adding entries together with the angle-brace depth
    // at which they're found.
    let mut elements = Vec::new();
    elements.push(Entry{ element, depth: 0 });

    // Start out with the first '<' consumed.
    iter.consume();
    enum State { Ident, Open, Close, Comma };
    let mut state = State::Open;
    let mut angle_depth = 1;

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
                // Mismatch in number of embedded types
                let expected_args_text = if expected_subtypes == 1 {
                    "polymorphic argument"
                } else {
                    "polymorphic arguments"
                };

                let maybe_infer_text = if allow_inference {
                    " (or none, to perform implicit type inference)"
                } else {
                    ""
                };

                return Err(ParseError::new_error_at_span(
                    source, cur_element.element.full_span,
                    format!(
                        "expected {} {}{}, but {} were provided",
                        expected_subtypes, expected_args_text, maybe_infer_text, encountered_subtypes
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
                                return Err(ParseError::new_error_str_at_span(source, type_span, "expected type but got module"));
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
                                return Err(ParseError::new_error_at_span(
                                    source, next_span,
                                    format!(
                                        "unknown type '{}' in module '{}'",
                                        String::from_utf8_lossy(type_text),
                                        String::from_utf8_lossy(old_text)
                                    )
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
fn consume_polymorphic_vars_spilled(source: &InputSource, iter: &mut TokenIter) -> Result<(), ParseError> {
    maybe_consume_comma_separated_spilled(
        TokenKind::OpenAngle, TokenKind::CloseAngle, source, iter,
        |source, iter| {
            consume_ident(source, iter)?;
            Ok(())
        }, "a polymorphic variable"
    )?;
    Ok(())
}

/// Consumes the parameter list to functions/components
fn consume_parameter_list(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx, target: &mut Vec<ParameterId>,
    poly_vars: &[Identifier], scope: SymbolScope, definition_id: DefinitionId
) -> Result<(), ParseError> {
    consume_comma_separated(
        TokenKind::OpenParen, TokenKind::CloseParen, source, iter,
        |source, iter| {
            let (start_pos, _) = iter.next_positions();
            let parser_type = consume_parser_type(
                source, iter, &ctx.symbols, &ctx.heap, poly_vars, scope, definition_id, false
            )?;
            let identifier = consume_ident_interned(source, iter, ctx)?;
            let parameter_id = ctx.heap.alloc_parameter(|this| Parameter{
                this,
                span: InputSpan::from_positions(start_pos, identifier.span.end),
                parser_type,
                identifier
            });
            Ok(parameter_id)
        },
        target, "a parameter", "a parameter list", None
    )
}
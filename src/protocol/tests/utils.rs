use crate::collections::StringPool;
use crate::protocol::{
    Module,
    ast::*,
    input_source::*,
    parser::{
        Parser,
        type_table::{TypeTable, DefinedTypeVariant},
        symbol_table::SymbolTable,
        token_parsing::*,
    },
    eval::*,
};

// Carries information about the test into utility structures for builder-like
// assertions
#[derive(Clone, Copy)]
struct TestCtx<'a> {
    test_name: &'a str,
    heap: &'a Heap,
    modules: &'a Vec<Module>,
    types: &'a TypeTable,
    symbols: &'a SymbolTable,
}

//------------------------------------------------------------------------------
// Interface for parsing and compiling
//------------------------------------------------------------------------------

pub(crate) struct Tester {
    test_name: String,
    sources: Vec<String>
}

impl Tester {
    /// Constructs a new tester, allows adding multiple sources before compiling
    pub(crate) fn new<S: ToString>(test_name: S) -> Self {
        Self{
            test_name: test_name.to_string(),
            sources: Vec::new()
        }
    }

    /// Utility for quick tests that use a single source file and expect the
    /// compilation to succeed.
    pub(crate) fn new_single_source_expect_ok<T: ToString, S: ToString>(test_name: T, source: S) -> AstOkTester {
        Self::new(test_name)
            .with_source(source)
            .compile()
            .expect_ok()
    }

    /// Utility for quick tests that use a single source file and expect the
    /// compilation to fail.
    pub(crate) fn new_single_source_expect_err<T: ToString, S: ToString>(test_name: T, source: S) -> AstErrTester {
        Self::new(test_name)
            .with_source(source)
            .compile()
            .expect_err()
    }

    pub(crate) fn with_source<S: ToString>(mut self, source: S) -> Self {
        self.sources.push(source.to_string());
        self
    }

    pub(crate) fn compile(self) -> AstTesterResult {
        let mut parser = Parser::new();
        for source in self.sources.into_iter() {
            let source = source.into_bytes();
            let input_source = InputSource::new(String::from(""), source);

            if let Err(err) = parser.feed(input_source) {
                return AstTesterResult::Err(AstErrTester::new(self.test_name, err))
            }
        }

        if let Err(err) = parser.parse() {
            return AstTesterResult::Err(AstErrTester::new(self.test_name, err))
        }

        AstTesterResult::Ok(AstOkTester::new(self.test_name, parser))
    }
}

pub(crate) enum AstTesterResult {
    Ok(AstOkTester),
    Err(AstErrTester)
}

impl AstTesterResult {
    pub(crate) fn expect_ok(self) -> AstOkTester {
        match self {
            AstTesterResult::Ok(v) => v,
            AstTesterResult::Err(err) => {
                let wrapped = ErrorTester{ test_name: &err.test_name, error: &err.error };
                println!("DEBUG: Full error:\n{}", &err.error);
                assert!(
                    false,
                    "[{}] Expected compilation to succeed, but it failed with {}",
                    err.test_name, wrapped.assert_postfix()
                );
                unreachable!();
            }
        }
    }

    pub(crate) fn expect_err(self) -> AstErrTester {
        match self {
            AstTesterResult::Ok(ok) => {
                assert!(false, "[{}] Expected compilation to fail, but it succeeded", ok.test_name);
                unreachable!();
            },
            AstTesterResult::Err(err) => err,
        }
    }
}

//------------------------------------------------------------------------------
// Interface for successful compilation
//------------------------------------------------------------------------------

pub(crate) struct AstOkTester {
    test_name: String,
    modules: Vec<Module>,
    heap: Heap,
    symbols: SymbolTable,
    types: TypeTable,
    pool: StringPool, // This is stored because if we drop it on the floor, we lose all our `StringRef<'static>`s
}

impl AstOkTester {
    fn new(test_name: String, parser: Parser) -> Self {
        Self {
            test_name,
            modules: parser.modules.into_iter().map(|module| Module{
                source: module.source,
                root_id: module.root_id,
                name: module.name.map(|(_, name)| name)
            }).collect(),
            heap: parser.heap,
            symbols: parser.symbol_table,
            types: parser.type_table,
            pool: parser.string_pool,
        }
    }

    pub(crate) fn for_struct<F: Fn(StructTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Struct(definition) = definition {
                if definition.identifier.value.as_str() != name {
                    continue;
                }

                // Found struct with the same name
                let tester = StructTester::new(self.ctx(), definition);
                f(tester);
                found = true;
                break
            }
        }

        assert!(
            found, "[{}] Failed to find definition for struct '{}'",
            self.test_name, name
        );
        self
    }

    pub(crate) fn for_enum<F: Fn(EnumTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Enum(definition) = definition {
                if definition.identifier.value.as_str() != name {
                    continue;
                }

                // Found enum with the same name
                let tester = EnumTester::new(self.ctx(), definition);
                f(tester);
                found = true;
                break;
            }
        }

        assert!(
            found, "[{}] Failed to find definition for enum '{}'",
            self.test_name, name
        );
        self
    }

    pub(crate) fn for_union<F: Fn(UnionTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Union(definition) = definition {
                if definition.identifier.value.as_str() != name {
                    continue;
                }

                // Found union with the same name
                let tester = UnionTester::new(self.ctx(), definition);
                f(tester);
                found = true;
                break;
            }
        }

        assert!(
            found, "[{}] Failed to find definition for union '{}'",
            self.test_name, name
        );
        self
    }

    pub(crate) fn for_function<F: FnOnce(FunctionTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Function(definition) = definition {
                if definition.identifier.value.as_str() != name {
                    continue;
                }

                // Found function
                let tester = FunctionTester::new(self.ctx(), definition);
                f(tester);
                found = true;
                break;
            }
        }

        if found { return self }

        assert!(
            false, "[{}] failed to find definition for function '{}'",
            self.test_name, name
        );
        unreachable!();
    }

    fn ctx(&self) -> TestCtx {
        TestCtx{
            test_name: &self.test_name,
            modules: &self.modules,
            heap: &self.heap,
            types: &self.types,
            symbols: &self.symbols,
        }
    }
}

//------------------------------------------------------------------------------
// Utilities for successful compilation
//------------------------------------------------------------------------------

pub(crate) struct StructTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a StructDefinition,
}

impl<'a> StructTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a StructDefinition) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn assert_num_fields(self, num: usize) -> Self {
        assert_eq!(
            num, self.def.fields.len(),
            "[{}] Expected {} struct fields, but found {} for {}",
            self.ctx.test_name, num, self.def.fields.len(), self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_num_monomorphs(self, num: usize) -> Self {
        let (is_equal, num_encountered) = has_equal_num_monomorphs(self.ctx, num, self.def.this.upcast());
        assert!(
            is_equal, "[{}] Expected {} monomorphs, but got {} for {}",
            self.ctx.test_name, num, num_encountered, self.assert_postfix()
        );
        self
    }

    /// Asserts that a monomorph exist, separate polymorphic variable types by
    /// a semicolon.
    pub(crate) fn assert_has_monomorph(self, serialized_monomorph: &str) -> Self {
        let (has_monomorph, serialized) = has_monomorph(self.ctx, self.def.this.upcast(), serialized_monomorph);
        assert!(
            has_monomorph, "[{}] Expected to find monomorph {}, but got {} for {}",
            self.ctx.test_name, serialized_monomorph, &serialized, self.assert_postfix()
        );
        self
    }

    pub(crate) fn for_field<F: Fn(StructFieldTester)>(self, name: &str, f: F) -> Self {
        // Find field with specified name
        for field in &self.def.fields {
            if field.field.value.as_str() == name {
                let tester = StructFieldTester::new(self.ctx, field);
                f(tester);
                return self;
            }
        }

        assert!(
            false, "[{}] Could not find struct field '{}' for {}",
            self.ctx.test_name, name, self.assert_postfix()
        );
        unreachable!();
    }

    fn assert_postfix(&self) -> String {
        let mut v = String::new();
        v.push_str("Struct{ name: ");
        v.push_str(self.def.identifier.value.as_str());
        v.push_str(", fields: [");
        for (field_idx, field) in self.def.fields.iter().enumerate() {
            if field_idx != 0 { v.push_str(", "); }
            v.push_str(field.field.value.as_str());
        }
        v.push_str("] }");
        v
    }
}

pub(crate) struct StructFieldTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a StructFieldDefinition,
}

impl<'a> StructFieldTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a StructFieldDefinition) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn assert_parser_type(self, expected: &str) -> Self {
        let mut serialized_type = String::new();
        serialize_parser_type(&mut serialized_type, &self.ctx.heap, &self.def.parser_type);
        assert_eq!(
            expected, &serialized_type,
            "[{}] Expected type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized_type, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        let mut serialized_type = String::new();
        serialize_parser_type(&mut serialized_type, &self.ctx.heap, &self.def.parser_type);
        format!("StructField{{ name: {}, parser_type: {} }}", self.def.field.value.as_str(), serialized_type)
    }
}

pub(crate) struct EnumTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a EnumDefinition,
}

impl<'a> EnumTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a EnumDefinition) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn assert_num_variants(self, num: usize) -> Self {
        assert_eq!(
            num, self.def.variants.len(),
            "[{}] Expected {} enum variants, but found {} for {}",
            self.ctx.test_name, num, self.def.variants.len(), self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_num_monomorphs(self, num: usize) -> Self {
        let (is_equal, num_encountered) = has_equal_num_monomorphs(self.ctx, num, self.def.this.upcast());
        assert!(
            is_equal, "[{}] Expected {} monomorphs, but got {} for {}",
            self.ctx.test_name, num, num_encountered, self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_has_monomorph(self, serialized_monomorph: &str) -> Self {
        let (has_monomorph, serialized) = has_monomorph(self.ctx, self.def.this.upcast(), serialized_monomorph);
        assert!(
            has_monomorph, "[{}] Expected to find monomorph {}, but got {} for {}",
            self.ctx.test_name, serialized_monomorph, serialized, self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_postfix(&self) -> String {
        let mut v = String::new();
        v.push_str("Enum{ name: ");
        v.push_str(self.def.identifier.value.as_str());
        v.push_str(", variants: [");
        for (variant_idx, variant) in self.def.variants.iter().enumerate() {
            if variant_idx != 0 { v.push_str(", "); }
            v.push_str(variant.identifier.value.as_str());
        }
        v.push_str("] }");
        v
    }
}

pub(crate) struct UnionTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a UnionDefinition,
}

impl<'a> UnionTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a UnionDefinition) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn assert_num_variants(self, num: usize) -> Self {
        assert_eq!(
            num, self.def.variants.len(),
            "[{}] Expected {} union variants, but found {} for {}",
            self.ctx.test_name, num, self.def.variants.len(), self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_num_monomorphs(self, num: usize) -> Self {
        let (is_equal, num_encountered) = has_equal_num_monomorphs(self.ctx, num, self.def.this.upcast());
        assert!(
            is_equal, "[{}] Expected {} monomorphs, but got {} for {}",
            self.ctx.test_name, num, num_encountered, self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_has_monomorph(self, serialized_monomorph: &str) -> Self {
        let (has_monomorph, serialized) = has_monomorph(self.ctx, self.def.this.upcast(), serialized_monomorph);
        assert!(
            has_monomorph, "[{}] Expected to find monomorph {}, but got {} for {}",
            self.ctx.test_name, serialized_monomorph, serialized, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        let mut v = String::new();
        v.push_str("Union{ name: ");
        v.push_str(self.def.identifier.value.as_str());
        v.push_str(", variants: [");
        for (variant_idx, variant) in self.def.variants.iter().enumerate() {
            if variant_idx != 0 { v.push_str(", "); }
            v.push_str(variant.identifier.value.as_str());
        }
        v.push_str("] }");
        v
    }
}

pub(crate) struct FunctionTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a FunctionDefinition,
}

impl<'a> FunctionTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a FunctionDefinition) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn for_variable<F: Fn(VariableTester)>(self, name: &str, f: F) -> Self {
        // Seek through the blocks in order to find the variable
        let wrapping_block_id = seek_stmt(
            self.ctx.heap, self.def.body.upcast(),
            &|stmt| {
                if let Statement::Block(block) = stmt {
                    for local_id in &block.locals {
                        let var = &self.ctx.heap[*local_id];
                        if var.identifier.value.as_str() == name {
                            return true;
                        }
                    }
                }

                false
            }
        );

        let mut found_local_id = None;
        if let Some(block_id) = wrapping_block_id {
            let block_stmt = self.ctx.heap[block_id].as_block();
            for local_id in &block_stmt.locals {
                let var = &self.ctx.heap[*local_id];
                if var.identifier.value.as_str() == name {
                    found_local_id = Some(*local_id);
                }
            }
        }

        assert!(
            found_local_id.is_some(), "[{}] Failed to find variable '{}' in {}",
            self.ctx.test_name, name, self.assert_postfix()
        );

        let local = &self.ctx.heap[found_local_id.unwrap()];

        // Find an instance of the variable expression so we can determine its
        // type.
        let var_expr = seek_expr_in_stmt(
            self.ctx.heap, self.def.body.upcast(),
            &|expr| {
                if let Expression::Variable(variable_expr) = expr {
                    if variable_expr.identifier.value.as_str() == name {
                        return true;
                    }
                }

                false
            }
        );

        assert!(
            var_expr.is_some(), "[{}] Failed to find variable expression of '{}' in {}",
            self.ctx.test_name, name, self.assert_postfix()
        );

        let var_expr = &self.ctx.heap[var_expr.unwrap()];

        // Construct tester and pass to tester function
        let tester = VariableTester::new(
            self.ctx, self.def.this.upcast(), local,
            var_expr.as_variable()
        );

        f(tester);

        self
    }

    /// Finds a specific expression within a function. There are two matchers:
    /// one outer matcher (to find a rough indication of the expression) and an
    /// inner matcher to find the exact expression. 
    ///
    /// The reason being that, for example, a function's body might be littered
    /// with addition symbols, so we first match on "some_var + some_other_var",
    /// and then match exactly on "+".
    pub(crate) fn for_expression_by_source<F: Fn(ExpressionTester)>(self, outer_match: &str, inner_match: &str, f: F) -> Self {
        // Seek the expression in the source code
        assert!(outer_match.contains(inner_match), "improper testing code");

        let module = seek_def_in_modules(
            &self.ctx.heap, &self.ctx.modules, self.def.this.upcast()
        ).unwrap();

        // Find the first occurrence of the expression after the definition of
        // the function, we'll check that it is included in the body later.
        let mut outer_match_idx = self.def.span.begin.offset as usize;
        while outer_match_idx < module.source.input.len() {
            if module.source.input[outer_match_idx..].starts_with(outer_match.as_bytes()) {
                break;
            }
            outer_match_idx += 1
        }

        assert!(
            outer_match_idx < module.source.input.len(),
            "[{}] Failed to find '{}' within the source that contains {}",
            self.ctx.test_name, outer_match, self.assert_postfix()
        );
        let inner_match_idx = outer_match_idx + outer_match.find(inner_match).unwrap();

        // Use the inner match index to find the expression
        let expr_id = seek_expr_in_stmt(
            &self.ctx.heap, self.def.body.upcast(),
            &|expr| expr.span().begin.offset as usize == inner_match_idx
        );
        assert!(
            expr_id.is_some(),
            "[{}] Failed to find '{}' within the source that contains {} \
            (note: expression was found, but not within the specified function",
            self.ctx.test_name, outer_match, self.assert_postfix()
        );
        let expr_id = expr_id.unwrap();

        // We have the expression, call the testing function
        let tester = ExpressionTester::new(
            self.ctx, self.def.this.upcast(), &self.ctx.heap[expr_id]
        );
        f(tester);

        self
    }

    pub(crate) fn call_ok(self, expected_result: Option<Value>) -> Self {
        use crate::protocol::*;

        let (prompt, result) = self.eval_until_end();
        match result {
            Ok(_) => {
                assert!(
                    prompt.store.stack.len() > 0, // note: stack never shrinks
                    "[{}] No value on stack after calling function for {}",
                    self.ctx.test_name, self.assert_postfix()
                );
            },
            Err(err) => {
                println!("DEBUG: Formatted evaluation error:\n{}", err);
                assert!(
                    false,
                    "[{}] Expected call to succeed, but got {:?} for {}",
                    self.ctx.test_name, err, self.assert_postfix()
                )
            }
        }

        if let Some(expected_result) = expected_result {
            debug_assert!(expected_result.get_heap_pos().is_none(), "comparing against heap thingamajigs is not yet implemented");
            assert!(
                value::apply_equality_operator(&prompt.store, &prompt.store.stack[0], &expected_result),
                "[{}] Result from call was {:?}, but expected {:?} for {}",
                self.ctx.test_name, &prompt.store.stack[0], &expected_result, self.assert_postfix()
            )
        }

        self
    }

    // Keeping this simple for now, will likely change
    pub(crate) fn call_err(self, expected_result: &str) -> Self {
        let (_, result) = self.eval_until_end();
        match result {
            Ok(_) => {
                assert!(
                    false,
                    "[{}] Expected an error, but evaluation finished successfully for {}",
                    self.ctx.test_name, self.assert_postfix()
                );
            },
            Err(err) => {
                println!("DEBUG: Formatted evaluation error:\n{}", err);
                debug_assert_eq!(err.statements.len(), 1);
                assert!(
                    err.statements[0].message.contains(&expected_result),
                    "[{}] Expected error message to contain '{}', but it was '{}' for {}",
                    self.ctx.test_name, expected_result, err.statements[0].message, self.assert_postfix()
                );
            }
        }

        self
    }

    fn eval_until_end(&self) -> (Prompt, Result<EvalContinuation, EvalError>) {
        use crate::protocol::*;

        let mut prompt = Prompt::new(&self.ctx.types, &self.ctx.heap, self.def.this.upcast(), 0, ValueGroup::new_stack(Vec::new()));
        let mut call_context = EvalContext::None;
        loop {
            let result = prompt.step(&self.ctx.types, &self.ctx.heap, &self.ctx.modules, &mut call_context);
            match result {
                Ok(EvalContinuation::Stepping) => {},
                _ => return (prompt, result),
            }
        }
    }

    fn assert_postfix(&self) -> String {
        format!("Function{{ name: {} }}", self.def.identifier.value.as_str())
    }
}

pub(crate) struct VariableTester<'a> {
    ctx: TestCtx<'a>,
    definition_id: DefinitionId,
    variable: &'a Variable,
    var_expr: &'a VariableExpression,
}

impl<'a> VariableTester<'a> {
    fn new(
        ctx: TestCtx<'a>, definition_id: DefinitionId, variable: &'a Variable, var_expr: &'a VariableExpression
    ) -> Self {
        Self{ ctx, definition_id, variable, var_expr }
    }

    pub(crate) fn assert_parser_type(self, expected: &str) -> Self {
        let mut serialized = String::new();
        serialize_parser_type(&mut serialized, self.ctx.heap, &self.variable.parser_type);

        assert_eq!(
            expected, &serialized,
            "[{}] Expected parser type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized, self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_concrete_type(self, expected: &str) -> Self {
        // Lookup concrete type in type table
        let mono_data = self.ctx.types.get_procedure_expression_data(&self.definition_id, 0);
        let concrete_type = &mono_data.expr_data[self.var_expr.unique_id_in_definition as usize].expr_type;

        // Serialize and check
        let mut serialized = String::new();
        serialize_concrete_type(&mut serialized, self.ctx.heap, self.definition_id, concrete_type);

        assert_eq!(
            expected, &serialized,
            "[{}] Expected concrete type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        format!("Variable{{ name: {} }}", self.variable.identifier.value.as_str())
    }
}

pub(crate) struct ExpressionTester<'a> {
    ctx: TestCtx<'a>,
    definition_id: DefinitionId, // of the enclosing function/component
    expr: &'a Expression
}

impl<'a> ExpressionTester<'a> {
    fn new(
        ctx: TestCtx<'a>, definition_id: DefinitionId, expr: &'a Expression
    ) -> Self {
        Self{ ctx, definition_id, expr }
    }

    pub(crate) fn assert_concrete_type(self, expected: &str) -> Self {
        // Lookup concrete type
        let mono_data = self.ctx.types.get_procedure_expression_data(&self.definition_id, 0);
        let expr_index = self.expr.get_unique_id_in_definition();
        let concrete_type = &mono_data.expr_data[expr_index as usize].expr_type;

        // Serialize and check type
        let mut serialized = String::new();
        serialize_concrete_type(&mut serialized, self.ctx.heap, self.definition_id, concrete_type);

        assert_eq!(
            expected, &serialized,
            "[{}] Expected concrete type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        format!(
            "Expression{{ debug: {:?} }}",
            self.expr
        )
    }
}

//------------------------------------------------------------------------------
// Interface for failed compilation
//------------------------------------------------------------------------------

pub(crate) struct AstErrTester {
    test_name: String,
    error: ParseError,
}

impl AstErrTester {
    fn new(test_name: String, error: ParseError) -> Self {
        Self{ test_name, error }
    }

    pub(crate) fn error<F: Fn(ErrorTester)>(&self, f: F) {
        // Maybe multiple errors will be supported in the future
        let tester = ErrorTester{ test_name: &self.test_name, error: &self.error };
        f(tester)
    }
}

//------------------------------------------------------------------------------
// Utilities for failed compilation
//------------------------------------------------------------------------------

pub(crate) struct ErrorTester<'a> {
    test_name: &'a str,
    error: &'a ParseError,
}

impl<'a> ErrorTester<'a> {
    pub(crate) fn assert_num(self, num: usize) -> Self {
        assert_eq!(
            num, self.error.statements.len(),
            "[{}] expected error to consist of '{}' parts, but encountered '{}' for {}",
            self.test_name, num, self.error.statements.len(), self.assert_postfix()
        );

        self
    }

    pub(crate) fn assert_ctx_has(self, idx: usize, msg: &str) -> Self {
        assert!(
            self.error.statements[idx].context.contains(msg),
            "[{}] expected error statement {}'s context to contain '{}' for {}",
            self.test_name, idx, msg, self.assert_postfix()
        );

        self
    }

    pub(crate) fn assert_msg_has(self, idx: usize, msg: &str) -> Self {
        assert!(
            self.error.statements[idx].message.contains(msg),
            "[{}] expected error statement {}'s message to contain '{}' for {}",
            self.test_name, idx, msg, self.assert_postfix()
        );

        self
    }

    // TODO: @tokenizer This should really be removed, as compilation should be
    //  deterministic, but we're currently using rather inefficient hashsets for
    //  the type inference, so remove once compiler architecture has changed.
    pub(crate) fn assert_any_msg_has(self, msg: &str) -> Self {
        let mut is_present = false;
        for statement in &self.error.statements {
            if statement.message.contains(msg) {
                is_present = true;
                break;
            }
        }

        assert!(
            is_present, "[{}] Expected an error statement to contain '{}' for {}",
            self.test_name, msg, self.assert_postfix()
        );
        
        self
    }

    /// Seeks the index of the pattern in the context message, then checks if
    /// the input position corresponds to that index.
    pub (crate) fn assert_occurs_at(self, idx: usize, pattern: &str) -> Self {
        let pos = self.error.statements[idx].context.find(pattern);
        assert!(
            pos.is_some(),
            "[{}] incorrect occurs_at: '{}' could not be found in the context for {}",
            self.test_name, pattern, self.assert_postfix()
        );
        let pos = pos.unwrap();
        let col = self.error.statements[idx].start_column as usize;
        assert_eq!(
            pos + 1, col,
            "[{}] Expected error to occur at column {}, but found it at {} for {}",
            self.test_name, pos + 1, col, self.assert_postfix()
        );

        self
    }

    fn assert_postfix(&self) -> String {
        let mut v = String::new();
        v.push_str("error: [");
        for (idx, stmt) in self.error.statements.iter().enumerate() {
            if idx != 0 {
                v.push_str(", ");
            }

            v.push_str(&format!("{{ context: {}, message: {} }}", &stmt.context, stmt.message));
        }
        v.push(']');
        v
    }
}

//------------------------------------------------------------------------------
// Generic utilities
//------------------------------------------------------------------------------

fn has_equal_num_monomorphs(ctx: TestCtx, num: usize, definition_id: DefinitionId) -> (bool, usize) {
    use DefinedTypeVariant::*;

    let type_def = ctx.types.get_base_definition(&definition_id).unwrap();
    let num_on_type = match &type_def.definition {
        Struct(v) => v.monomorphs.len(),
        Enum(v) => v.monomorphs.len(),
        Union(v) => v.monomorphs.len(),
        Function(v) => v.monomorphs.len(),
        Component(v) => v.monomorphs.len(),
    };

    (num_on_type == num, num_on_type)
}

fn has_monomorph(ctx: TestCtx, definition_id: DefinitionId, serialized_monomorph: &str) -> (bool, String) {
    use DefinedTypeVariant::*;

    let type_def = ctx.types.get_base_definition(&definition_id).unwrap();

    // Note: full_buffer is just for error reporting
    let mut full_buffer = String::new();
    let mut has_match = false;

    let serialize_monomorph = |monomorph: &Vec<ConcreteType>| -> String {
        let mut buffer = String::new();
        for (element_idx, element) in monomorph.iter().enumerate() {
            if element_idx != 0 {
                buffer.push(';');
            }
            serialize_concrete_type(&mut buffer, ctx.heap, definition_id, element);
        }

        buffer
    };

    full_buffer.push('[');
    let mut append_to_full_buffer = |buffer: String| {
        if full_buffer.len() != 1 {
            full_buffer.push_str(", ");
        }
        full_buffer.push('"');
        full_buffer.push_str(&buffer);
        full_buffer.push('"');
    };

    match &type_def.definition {
        Enum(_) | Union(_) | Struct(_) => {
            let monomorphs = type_def.definition.data_monomorphs();
            for monomorph in monomorphs.iter() {
                let buffer = serialize_monomorph(&monomorph.poly_args);
                if buffer == serialized_monomorph {
                    has_match = true;
                }
                append_to_full_buffer(buffer);
            }
        },
        Function(_) | Component(_) => {
            let monomorphs = type_def.definition.procedure_monomorphs();
            for monomorph in monomorphs.iter() {
                let buffer = serialize_monomorph(&monomorph.poly_args);
                if buffer == serialized_monomorph {
                    has_match = true;
                }
                append_to_full_buffer(buffer);
            }
        }
    }

    full_buffer.push(']');

    (has_match, full_buffer)
}

fn serialize_parser_type(buffer: &mut String, heap: &Heap, parser_type: &ParserType) {
    use ParserTypeVariant as PTV;

    fn write_bytes(buffer: &mut String, bytes: &[u8]) {
        let utf8 = String::from_utf8_lossy(bytes);
        buffer.push_str(&utf8);
    }

    fn serialize_variant(buffer: &mut String, heap: &Heap, parser_type: &ParserType, mut idx: usize) -> usize {
        match &parser_type.elements[idx].variant {
            PTV::Void => buffer.push_str("void"),
            PTV::InputOrOutput => {
                buffer.push_str("portlike<");
                idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                buffer.push('>');
            },
            PTV::ArrayLike => {
                idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                buffer.push_str("[???]");
            },
            PTV::IntegerLike => buffer.push_str("integerlike"),
            PTV::Message => buffer.push_str(KW_TYPE_MESSAGE_STR),
            PTV::Bool => buffer.push_str(KW_TYPE_BOOL_STR),
            PTV::UInt8 => buffer.push_str(KW_TYPE_UINT8_STR),
            PTV::UInt16 => buffer.push_str(KW_TYPE_UINT16_STR),
            PTV::UInt32 => buffer.push_str(KW_TYPE_UINT32_STR),
            PTV::UInt64 => buffer.push_str(KW_TYPE_UINT64_STR),
            PTV::SInt8 => buffer.push_str(KW_TYPE_SINT8_STR),
            PTV::SInt16 => buffer.push_str(KW_TYPE_SINT16_STR),
            PTV::SInt32 => buffer.push_str(KW_TYPE_SINT32_STR),
            PTV::SInt64 => buffer.push_str(KW_TYPE_SINT64_STR),
            PTV::Character => buffer.push_str(KW_TYPE_CHAR_STR),
            PTV::String => buffer.push_str(KW_TYPE_STRING_STR),
            PTV::IntegerLiteral => buffer.push_str("int_literal"),
            PTV::Inferred => buffer.push_str(KW_TYPE_INFERRED_STR),
            PTV::Array => {
                idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                buffer.push_str("[]");
            },
            PTV::Input => {
                buffer.push_str(KW_TYPE_IN_PORT_STR);
                buffer.push('<');
                idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                buffer.push('>');
            },
            PTV::Output => {
                buffer.push_str(KW_TYPE_OUT_PORT_STR);
                buffer.push('<');
                idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                buffer.push('>');
            },
            PTV::PolymorphicArgument(definition_id, poly_idx) => {
                let definition = &heap[*definition_id];
                let poly_arg = &definition.poly_vars()[*poly_idx as usize];
                buffer.push_str(poly_arg.value.as_str());
            },
            PTV::Definition(definition_id, num_embedded) => {
                let definition = &heap[*definition_id];
                buffer.push_str(definition.identifier().value.as_str());

                let num_embedded = *num_embedded;
                if num_embedded != 0 {
                    buffer.push('<');
                    for embedded_idx in 0..num_embedded {
                        if embedded_idx != 0 {
                            buffer.push(',');
                        }
                        idx = serialize_variant(buffer, heap, parser_type, idx + 1);
                    }
                    buffer.push('>');
                }
            }
        }

        idx
    }

    serialize_variant(buffer, heap, parser_type, 0);
}

fn serialize_concrete_type(buffer: &mut String, heap: &Heap, def: DefinitionId, concrete: &ConcreteType) {
    // Retrieve polymorphic variables
    let poly_vars = heap[def].poly_vars();

    fn write_bytes(buffer: &mut String, bytes: &[u8]) {
        let utf8 = String::from_utf8_lossy(bytes);
        buffer.push_str(&utf8);
    }

    fn serialize_recursive(
        buffer: &mut String, heap: &Heap, poly_vars: &Vec<Identifier>, concrete: &ConcreteType, mut idx: usize
    ) -> usize {
        use ConcreteTypePart as CTP;

        let part = &concrete.parts[idx];
        match part {
            CTP::Void => buffer.push_str("void"),
            CTP::Message => write_bytes(buffer, KW_TYPE_MESSAGE),
            CTP::Bool => write_bytes(buffer, KW_TYPE_BOOL),
            CTP::UInt8 => write_bytes(buffer, KW_TYPE_UINT8),
            CTP::UInt16 => write_bytes(buffer, KW_TYPE_UINT16),
            CTP::UInt32 => write_bytes(buffer, KW_TYPE_UINT32),
            CTP::UInt64 => write_bytes(buffer, KW_TYPE_UINT64),
            CTP::SInt8 => write_bytes(buffer, KW_TYPE_SINT8),
            CTP::SInt16 => write_bytes(buffer, KW_TYPE_SINT16),
            CTP::SInt32 => write_bytes(buffer, KW_TYPE_SINT32),
            CTP::SInt64 => write_bytes(buffer, KW_TYPE_SINT64),
            CTP::Character => write_bytes(buffer, KW_TYPE_CHAR),
            CTP::String => write_bytes(buffer, KW_TYPE_STRING),
            CTP::Array => {
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push_str("[]");
            },
            CTP::Slice => {
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push_str("[..]");
            },
            CTP::Input => {
                write_bytes(buffer, KW_TYPE_IN_PORT);
                buffer.push('<');
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push('>');
            },
            CTP::Output => {
                write_bytes(buffer, KW_TYPE_OUT_PORT);
                buffer.push('<');
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push('>');
            },
            CTP::Instance(definition_id, num_sub) => {
                let definition_name = heap[*definition_id].identifier();
                buffer.push_str(definition_name.value.as_str());
                if *num_sub != 0 {
                    buffer.push('<');
                    for sub_idx in 0..*num_sub {
                        if sub_idx != 0 { buffer.push(','); }
                        idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                    }
                    buffer.push('>');
                }
            }
        }

        idx
    }

    serialize_recursive(buffer, heap, poly_vars, concrete, 0);
}

fn seek_def_in_modules<'a>(heap: &Heap, modules: &'a [Module], def_id: DefinitionId) -> Option<&'a Module> {
    for module in modules {
        let root = &heap.protocol_descriptions[module.root_id];
        for definition in &root.definitions {
            if *definition == def_id {
                return Some(module)
            }
        }
    }

    None
}

fn seek_stmt<F: Fn(&Statement) -> bool>(heap: &Heap, start: StatementId, f: &F) -> Option<StatementId> {
    let stmt = &heap[start];
    if f(stmt) { return Some(start); }

    // This statement wasn't it, try to recurse
    let matched = match stmt {
        Statement::Block(block) => {
            for sub_id in &block.statements {
                if let Some(id) = seek_stmt(heap, *sub_id, f) {
                    return Some(id);
                }
            }

            None
        },
        Statement::Labeled(stmt) => seek_stmt(heap, stmt.body, f),
        Statement::If(stmt) => {
            if let Some(id) = seek_stmt(heap, stmt.true_body.upcast(), f) {
                return Some(id);
            } else if let Some(false_body) = stmt.false_body {
                if let Some(id) = seek_stmt(heap, false_body.upcast(), f) {
                    return Some(id);
                }
            }
            None
        },
        Statement::While(stmt) => seek_stmt(heap, stmt.body.upcast(), f),
        Statement::Synchronous(stmt) => seek_stmt(heap, stmt.body.upcast(), f),
        _ => None
    };

    matched
}

fn seek_expr_in_expr<F: Fn(&Expression) -> bool>(heap: &Heap, start: ExpressionId, f: &F) -> Option<ExpressionId> {
    let expr = &heap[start];
    if f(expr) { return Some(start); }

    match expr {
        Expression::Assignment(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.left, f))
            .or_else(|| seek_expr_in_expr(heap, expr.right, f))
        },
        Expression::Binding(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.bound_to, f))
            .or_else(|| seek_expr_in_expr(heap, expr.bound_from, f))
        }
        Expression::Conditional(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.test, f))
            .or_else(|| seek_expr_in_expr(heap, expr.true_expression, f))
            .or_else(|| seek_expr_in_expr(heap, expr.false_expression, f))
        },
        Expression::Binary(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.left, f))
            .or_else(|| seek_expr_in_expr(heap, expr.right, f))
        },
        Expression::Unary(expr) => {
            seek_expr_in_expr(heap, expr.expression, f)
        },
        Expression::Indexing(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.subject, f))
            .or_else(|| seek_expr_in_expr(heap, expr.index, f))
        },
        Expression::Slicing(expr) => {
            None
            .or_else(|| seek_expr_in_expr(heap, expr.subject, f))
            .or_else(|| seek_expr_in_expr(heap, expr.from_index, f))
            .or_else(|| seek_expr_in_expr(heap, expr.to_index, f))
        },
        Expression::Select(expr) => {
            seek_expr_in_expr(heap, expr.subject, f)
        },
        Expression::Literal(expr) => {
            if let Literal::Struct(lit) = &expr.value {
                for field in &lit.fields {
                    if let Some(id) = seek_expr_in_expr(heap, field.value, f) {
                        return Some(id)
                    }
                }
            } else if let Literal::Array(elements) = &expr.value {
                for element in elements {
                    if let Some(id) = seek_expr_in_expr(heap, *element, f) {
                        return Some(id)
                    }
                }
            }
            None
        },
        Expression::Cast(expr) => {
            seek_expr_in_expr(heap, expr.subject, f)
        }
        Expression::Call(expr) => {
            for arg in &expr.arguments {
                if let Some(id) = seek_expr_in_expr(heap, *arg, f) {
                    return Some(id)
                }
            }
            None
        },
        Expression::Variable(_expr) => {
            None
        }
    }
}

fn seek_expr_in_stmt<F: Fn(&Expression) -> bool>(heap: &Heap, start: StatementId, f: &F) -> Option<ExpressionId> {
    let stmt = &heap[start];

    match stmt {
        Statement::Block(stmt) => {
            for stmt_id in &stmt.statements {
                if let Some(id) = seek_expr_in_stmt(heap, *stmt_id, f) {
                    return Some(id)
                }
            }
            None
        },
        Statement::Labeled(stmt) => {
            seek_expr_in_stmt(heap, stmt.body, f)
        },
        Statement::If(stmt) => {
            None
            .or_else(|| seek_expr_in_expr(heap, stmt.test, f))
            .or_else(|| seek_expr_in_stmt(heap, stmt.true_body.upcast(), f))
            .or_else(|| if let Some(false_body) = stmt.false_body {
                seek_expr_in_stmt(heap, false_body.upcast(), f)
            } else {
                None
            })
        },
        Statement::While(stmt) => {
            None
            .or_else(|| seek_expr_in_expr(heap, stmt.test, f))
            .or_else(|| seek_expr_in_stmt(heap, stmt.body.upcast(), f))
        },
        Statement::Synchronous(stmt) => {
            seek_expr_in_stmt(heap, stmt.body.upcast(), f)
        },
        Statement::Return(stmt) => {
            for expr_id in &stmt.expressions {
                if let Some(id) = seek_expr_in_expr(heap, *expr_id, f) {
                    return Some(id);
                }
            }
            None
        },
        Statement::New(stmt) => {
            seek_expr_in_expr(heap, stmt.expression.upcast(), f)
        },
        Statement::Expression(stmt) => {
            seek_expr_in_expr(heap, stmt.expression, f)
        },
        _ => None
    }
}
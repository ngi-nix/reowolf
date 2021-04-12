use crate::protocol::{
    ast::*,
    inputsource::*,
    parser::{
        *,
        type_table::TypeTable,
        symbol_table::SymbolTable,
    },
};

// Carries information about the test into utility structures for builder-like
// assertions
#[derive(Clone, Copy)]
struct TestCtx<'a> {
    test_name: &'a str,
    heap: &'a Heap,
    modules: &'a Vec<LexedModule>,
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
        for (source_idx, source) in self.sources.into_iter().enumerate() {
            let mut cursor = std::io::Cursor::new(source);
            let input_source = InputSource::new("", &mut cursor)
                .expect(&format!("parsing source {}", source_idx + 1));

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
    modules: Vec<LexedModule>,
    heap: Heap,
    symbols: SymbolTable,
    types: TypeTable,
}

impl AstOkTester {
    fn new(test_name: String, parser: Parser) -> Self {
        Self {
            test_name,
            modules: parser.modules,
            heap: parser.heap,
            symbols: parser.symbol_table,
            types: parser.type_table,
        }
    }

    pub(crate) fn for_struct<F: Fn(StructTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Struct(definition) = definition {
                if String::from_utf8_lossy(&definition.identifier.value) != name {
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
                if String::from_utf8_lossy(&definition.identifier.value) != name {
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
                if String::from_utf8_lossy(&definition.identifier.value) != name {
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

    pub(crate) fn for_function<F: Fn(FunctionTester)>(self, name: &str, f: F) -> Self {
        let mut found = false;
        for definition in self.heap.definitions.iter() {
            if let Definition::Function(definition) = definition {
                if String::from_utf8_lossy(&definition.identifier.value) != name {
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
            if String::from_utf8_lossy(&field.field.value) == name {
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
        v.push_str(&String::from_utf8_lossy(&self.def.identifier.value));
        v.push_str(", fields: [");
        for (field_idx, field) in self.def.fields.iter().enumerate() {
            if field_idx != 0 { v.push_str(", "); }
            v.push_str(&String::from_utf8_lossy(&field.field.value));
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
        serialize_parser_type(&mut serialized_type, &self.ctx.heap, self.def.parser_type);
        assert_eq!(
            expected, &serialized_type,
            "[{}] Expected type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized_type, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        let mut serialized_type = String::new();
        serialize_parser_type(&mut serialized_type, &self.ctx.heap, self.def.parser_type);
        format!(
            "StructField{{ name: {}, parser_type: {} }}",
            String::from_utf8_lossy(&self.def.field.value), serialized_type
        )
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
        v.push_str(&String::from_utf8_lossy(&self.def.identifier.value));
        v.push_str(", variants: [");
        for (variant_idx, variant) in self.def.variants.iter().enumerate() {
            if variant_idx != 0 { v.push_str(", "); }
            v.push_str(&String::from_utf8_lossy(&variant.identifier.value));
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
        v.push_str(&String::from_utf8_lossy(&self.def.identifier.value));
        v.push_str(", variants: [");
        for (variant_idx, variant) in self.def.variants.iter().enumerate() {
            if variant_idx != 0 { v.push_str(", "); }
            v.push_str(&String::from_utf8_lossy(&variant.identifier.value));
        }
        v.push_str("] }");
        v
    }
}

pub(crate) struct FunctionTester<'a> {
    ctx: TestCtx<'a>,
    def: &'a Function,
}

impl<'a> FunctionTester<'a> {
    fn new(ctx: TestCtx<'a>, def: &'a Function) -> Self {
        Self{ ctx, def }
    }

    pub(crate) fn for_variable<F: Fn(VariableTester)>(self, name: &str, f: F) -> Self {
        // Find the memory statement in order to find the local
        let mem_stmt_id = seek_stmt(
            self.ctx.heap, self.def.body,
            &|stmt| {
                if let Statement::Local(local) = stmt {
                    if let LocalStatement::Memory(memory) = local {
                        let local = &self.ctx.heap[memory.variable];
                        if local.identifier.value == name.as_bytes() {
                            return true;
                        }
                    }
                }

                false
            }
        );

        assert!(
            mem_stmt_id.is_some(), "[{}] Failed to find variable '{}' in {}",
            self.ctx.test_name, name, self.assert_postfix()
        );

        let mem_stmt_id = mem_stmt_id.unwrap();
        let local_id = self.ctx.heap[mem_stmt_id].as_memory().variable;
        let local = &self.ctx.heap[local_id];

        // Find the assignment expression that follows it
        let assignment_id = seek_expr_in_stmt(
            self.ctx.heap, self.def.body,
            &|expr| {
                if let Expression::Assignment(assign_expr) = expr {
                    if let Expression::Variable(variable_expr) = &self.ctx.heap[assign_expr.left] {
                        if variable_expr.position.offset == local.identifier.position.offset {
                            return true;
                        }
                    }
                }

                false
            }
        );

        assert!(
            assignment_id.is_some(), "[{}] Failed to find assignment to variable '{}' in {}",
            self.ctx.test_name, name, self.assert_postfix()
        );

        let assignment = &self.ctx.heap[assignment_id.unwrap()];

        // Construct tester and pass to tester function
        let tester = VariableTester::new(
            self.ctx, self.def.this.upcast(), local, 
            assignment.as_assignment()
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
        let mut outer_match_idx = self.def.position.offset;
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
            &self.ctx.heap, self.def.body,
            &|expr| expr.position().offset == inner_match_idx
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

    fn assert_postfix(&self) -> String {
        format!(
            "Function{{ name: {} }}",
            &String::from_utf8_lossy(&self.def.identifier.value)
        )
    }
}

pub(crate) struct VariableTester<'a> {
    ctx: TestCtx<'a>,
    definition_id: DefinitionId,
    local: &'a Local,
    assignment: &'a AssignmentExpression,
}

impl<'a> VariableTester<'a> {
    fn new(
        ctx: TestCtx<'a>, definition_id: DefinitionId, local: &'a Local, assignment: &'a AssignmentExpression
    ) -> Self {
        Self{ ctx, definition_id, local, assignment }
    }

    pub(crate) fn assert_parser_type(self, expected: &str) -> Self {
        let mut serialized = String::new();
        serialize_parser_type(&mut serialized, self.ctx.heap, self.local.parser_type);

        assert_eq!(
            expected, &serialized,
            "[{}] Expected parser type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized, self.assert_postfix()
        );
        self
    }

    pub(crate) fn assert_concrete_type(self, expected: &str) -> Self {
        let mut serialized = String::new();
        serialize_concrete_type(
            &mut serialized, self.ctx.heap, self.definition_id, 
            &self.assignment.concrete_type
        );

        assert_eq!(
            expected, &serialized,
            "[{}] Expected concrete type '{}', but got '{}' for {}",
            self.ctx.test_name, expected, &serialized, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        println!("DEBUG: {:?}", self.assignment.concrete_type);
        format!(
            "Variable{{ name: {} }}",
            &String::from_utf8_lossy(&self.local.identifier.value)
        )
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
        let mut serialized = String::new();
        serialize_concrete_type(
            &mut serialized, self.ctx.heap, self.definition_id,
            self.expr.get_type()
        );

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
        let col = self.error.statements[idx].position.column;
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

fn has_equal_num_monomorphs<'a>(ctx: TestCtx<'a>, num: usize, definition_id: DefinitionId) -> (bool, usize) {
    let type_def = ctx.types.get_base_definition(&definition_id).unwrap();
    let num_on_type = type_def.monomorphs.len();
    
    (num_on_type == num, num_on_type)
}

fn has_monomorph<'a>(ctx: TestCtx<'a>, definition_id: DefinitionId, serialized_monomorph: &str) -> (bool, String) {
    let type_def = ctx.types.get_base_definition(&definition_id).unwrap();

    let mut full_buffer = String::new();
    let mut has_match = false;
    full_buffer.push('[');
    for (monomorph_idx, monomorph) in type_def.monomorphs.iter().enumerate() {
        let mut buffer = String::new();
        for (element_idx, monomorph_element) in monomorph.iter().enumerate() {
            if element_idx != 0 { buffer.push(';'); }
            serialize_concrete_type(&mut buffer, ctx.heap, definition_id, monomorph_element);
        }

        if buffer == serialized_monomorph {
            // Found an exact match
            has_match = true;
        }

        if monomorph_idx != 0 {
            full_buffer.push_str(", ");
        }
        full_buffer.push('"');
        full_buffer.push_str(&buffer);
        full_buffer.push('"');
    }
    full_buffer.push(']');

    (has_match, full_buffer)
}

fn serialize_parser_type(buffer: &mut String, heap: &Heap, id: ParserTypeId) {
    use ParserTypeVariant as PTV;

    let p = &heap[id];
    match &p.variant {
        PTV::Message => buffer.push_str("msg"),
        PTV::Bool => buffer.push_str("bool"),
        PTV::Byte => buffer.push_str("byte"),
        PTV::Short => buffer.push_str("short"),
        PTV::Int => buffer.push_str("int"),
        PTV::Long => buffer.push_str("long"),
        PTV::String => buffer.push_str("string"),
        PTV::IntegerLiteral => buffer.push_str("intlit"),
        PTV::Inferred => buffer.push_str("auto"),
        PTV::Array(sub_id) => {
            serialize_parser_type(buffer, heap, *sub_id);
            buffer.push_str("[]");
        },
        PTV::Input(sub_id) => {
            buffer.push_str("in<");
            serialize_parser_type(buffer, heap, *sub_id);
            buffer.push('>');
        },
        PTV::Output(sub_id) => {
            buffer.push_str("out<");
            serialize_parser_type(buffer, heap, *sub_id);
            buffer.push('>');
        },
        PTV::Symbolic(symbolic) => {
            buffer.push_str(&String::from_utf8_lossy(&symbolic.identifier.value));
            if symbolic.poly_args2.len() > 0 {
                buffer.push('<');
                for (poly_idx, poly_arg) in symbolic.poly_args2.iter().enumerate() {
                    if poly_idx != 0 { buffer.push(','); }
                    serialize_parser_type(buffer, heap, *poly_arg);
                }
                buffer.push('>');
            }
        }
    }
}

fn serialize_concrete_type(buffer: &mut String, heap: &Heap, def: DefinitionId, concrete: &ConcreteType) {
    // Retrieve polymorphic variables
    let poly_vars = match &heap[def] {
        Definition::Function(definition) => &definition.poly_vars,
        Definition::Component(definition) => &definition.poly_vars,
        Definition::Struct(definition) => &definition.poly_vars,
        Definition::Enum(definition) => &definition.poly_vars,
        Definition::Union(definition) => &definition.poly_vars,
    };

    fn serialize_recursive(
        buffer: &mut String, heap: &Heap, poly_vars: &Vec<Identifier>, concrete: &ConcreteType, mut idx: usize
    ) -> usize {
        use ConcreteTypePart as CTP;

        let part = &concrete.parts[idx];
        match part {
            CTP::Marker(poly_idx) => {
                buffer.push_str(&String::from_utf8_lossy(&poly_vars[*poly_idx].value));
            },
            CTP::Void => buffer.push_str("void"),
            CTP::Message => buffer.push_str("msg"),
            CTP::Bool => buffer.push_str("bool"),
            CTP::Byte => buffer.push_str("byte"),
            CTP::Short => buffer.push_str("short"),
            CTP::Int => buffer.push_str("int"),
            CTP::Long => buffer.push_str("long"),
            CTP::String => buffer.push_str("string"),
            CTP::Array => {
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push_str("[]");
            },
            CTP::Slice => {
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push_str("[..]");
            },
            CTP::Input => {
                buffer.push_str("in<");
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push('>');
            },
            CTP::Output => {
                buffer.push_str("out<");
                idx = serialize_recursive(buffer, heap, poly_vars, concrete, idx + 1);
                buffer.push('>');
            },
            CTP::Instance(definition_id, num_sub) => {
                let definition_name = heap[*definition_id].identifier();
                buffer.push_str(&String::from_utf8_lossy(&definition_name.value));
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

fn seek_def_in_modules<'a>(heap: &Heap, modules: &'a [LexedModule], def_id: DefinitionId) -> Option<&'a LexedModule> {
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
            if let Some(id) = seek_stmt(heap,stmt.true_body, f) {
                return Some(id);
            } else if let Some(id) = seek_stmt(heap, stmt.false_body, f) {
                return Some(id);
            }
            None
        },
        Statement::While(stmt) => seek_stmt(heap, stmt.body, f),
        Statement::Synchronous(stmt) => seek_stmt(heap, stmt.body, f),
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
            .or_else(|| seek_expr_in_expr(heap, expr.left.upcast(), f))
            .or_else(|| seek_expr_in_expr(heap, expr.right, f))
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
        Expression::Array(expr) => {
            for element in &expr.elements {
                if let Some(id) = seek_expr_in_expr(heap, *element, f) {
                    return Some(id)
                }
            }
            None
        },
        Expression::Literal(expr) => {
            if let Literal::Struct(lit) = &expr.value {
                for field in &lit.fields {
                    if let Some(id) = seek_expr_in_expr(heap, field.value, f) {
                        return Some(id)
                    }
                }
            }
            None
        },
        Expression::Call(expr) => {
            for arg in &expr.arguments {
                if let Some(id) = seek_expr_in_expr(heap, *arg, f) {
                    return Some(id)
                }
            }
            None
        },
        Expression::Variable(expr) => {
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
            .or_else(|| seek_expr_in_stmt(heap, stmt.true_body, f))
            .or_else(|| seek_expr_in_stmt(heap, stmt.false_body, f))
        },
        Statement::While(stmt) => {
            None
            .or_else(|| seek_expr_in_expr(heap, stmt.test, f))
            .or_else(|| seek_expr_in_stmt(heap, stmt.body, f))
        },
        Statement::Synchronous(stmt) => {
            seek_expr_in_stmt(heap, stmt.body, f)
        },
        Statement::Return(stmt) => {
            seek_expr_in_expr(heap, stmt.expression, f)
        },
        Statement::Assert(stmt) => {
            seek_expr_in_expr(heap, stmt.expression, f)
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
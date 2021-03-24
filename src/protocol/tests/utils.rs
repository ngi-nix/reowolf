use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use crate::protocol::parser::*;

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

        parser.compile();
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
}

impl AstOkTester {
    fn new(test_name: String, parser: Parser) -> Self {
        Self {
            test_name,
            modules: parser.modules,
            heap: parser.heap
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
                let tester = StructTester::new(&self.test_name, definition, &self.heap);
                f(tester);
                found = true;
                break
            }
        }

        if found { return self }

        assert!(
            false, "[{}] Failed to find definition for struct '{}'",
            self.test_name, name
        );
        unreachable!()
    }
}

//------------------------------------------------------------------------------
// Utilities for successful compilation
//------------------------------------------------------------------------------

pub(crate) struct StructTester<'a> {
    test_name: &'a str,
    def: &'a StructDefinition,
    heap: &'a Heap,
}

impl<'a> StructTester<'a> {
    fn new(test_name: &'a str, def: &'a StructDefinition, heap: &'a Heap) -> Self {
        Self{ test_name, def, heap }
    }

    pub(crate) fn assert_num_fields(self, num: usize) -> Self {
        debug_assert_eq!(
            num, self.def.fields.len(),
            "[{}] Expected {} struct fields, but found {} for {}",
            self.test_name, num, self.def.fields.len(), self.assert_postfix()
        );
        self
    }

    pub(crate) fn for_field<F: Fn(StructFieldTester)>(self, name: &str, f: F) -> Self {
        // Find field with specified name
        for field in &self.def.fields {
            if String::from_utf8_lossy(&field.field.value) == name {
                let tester = StructFieldTester::new(self.test_name, field, self.heap);
                f(tester);
                return self;
            }
        }

        assert!(
            false, "[{}] Could not find struct field '{}' for {}",
            self.test_name, name, self.assert_postfix()
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
    test_name: &'a str,
    def: &'a StructFieldDefinition,
    heap: &'a Heap,
}

impl<'a> StructFieldTester<'a> {
    fn new(test_name: &'a str, def: &'a StructFieldDefinition, heap: &'a Heap) -> Self {
        Self{ test_name, def, heap }
    }

    pub(crate) fn assert_parser_type(self, expected: &str) -> Self {
        let mut serialized_type = String::new();
        serialize_parser_type(&mut serialized_type, &self.heap, self.def.parser_type);
        debug_assert_eq!(
            expected, &serialized_type,
            "[{}] Expected type '{}', but got '{}' for {}",
            self.test_name, expected, &serialized_type, self.assert_postfix()
        );
        self
    }

    fn assert_postfix(&self) -> String {
        let mut serialized_type = String::new();
        serialize_parser_type(&mut serialized_type, &self.heap, self.def.parser_type);
        format!(
            "StructField{{ name: {}, parser_type: {} }}",
            String::from_utf8_lossy(&self.def.field.value), serialized_type
        )
    }
}

//------------------------------------------------------------------------------
// Interface for failed compilation
//------------------------------------------------------------------------------

pub(crate) struct AstErrTester {
    test_name: String,
    error: ParseError2,
}

impl AstErrTester {
    fn new(test_name: String, error: ParseError2) -> Self {
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
    error: &'a ParseError2,
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
        let col = self.error.statements[idx].position.col();
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
            if symbolic.poly_args.len() > 0 {
                buffer.push('<');
                for (poly_idx, poly_arg) in symbolic.poly_args.iter().enumerate() {
                    if poly_idx != 0 { buffer.push(','); }
                    serialize_parser_type(buffer, heap, *poly_arg);
                }
                buffer.push('>');
            }
        }
    }
}
use crate::protocol::ast::*;
use crate::protocol::inputsource::*;

const MAX_LEVEL: usize = 128;
const MAX_NAMESPACES: u8 = 8; // only three levels are supported at the moment

fn is_vchar(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= 0x21 && c <= 0x7E
    } else {
        false
    }
}

fn is_wsp(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c == b' ' || c == b'\t'
    } else {
        false
    }
}

fn is_ident_start(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= b'A' && c <= b'Z' || c >= b'a' && c <= b'z'
    } else {
        false
    }
}

fn is_ident_rest(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= b'A' && c <= b'Z' || c >= b'a' && c <= b'z' || c >= b'0' && c <= b'9' || c == b'_'
    } else {
        false
    }
}

fn is_constant(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= b'0' && c <= b'9' || c == b'\''
    } else {
        false
    }
}

fn is_integer_start(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= b'0' && c <= b'9'
    } else {
        false
    }
}

fn is_integer_rest(x: Option<u8>) -> bool {
    if let Some(c) = x {
        c >= b'0' && c <= b'9'
            || c >= b'a' && c <= b'f'
            || c >= b'A' && c <= b'F'
            || c == b'x'
            || c == b'o'
    } else {
        false
    }
}

fn lowercase(x: u8) -> u8 {
    if x >= b'A' && x <= b'Z' {
        x - b'A' + b'a'
    } else {
        x
    }
}

pub struct Lexer<'a> {
    source: &'a mut InputSource,
    level: usize,
}

impl Lexer<'_> {
    pub fn new(source: &mut InputSource) -> Lexer {
        Lexer { source, level: 0 }
    }
    fn error_at_pos(&self, msg: &str) -> ParseError2 {
        ParseError2::new_error(self.source, self.source.pos(), msg)
    }
    fn consume_line(&mut self) -> Result<Vec<u8>, ParseError2> {
        let mut result: Vec<u8> = Vec::new();
        let mut next = self.source.next();
        while next.is_some() && next != Some(b'\n') && next != Some(b'\r') {
            if !(is_vchar(next) || is_wsp(next)) {
                return Err(self.error_at_pos("Expected visible character or whitespace"));
            }
            result.push(next.unwrap());
            self.source.consume();
            next = self.source.next();
        }
        if next.is_some() {
            self.source.consume();
        }
        if next == Some(b'\r') && self.source.next() == Some(b'\n') {
            self.source.consume();
        }
        Ok(result)
    }
    fn consume_whitespace(&mut self, expected: bool) -> Result<(), ParseError2> {
        let mut found = false;
        let mut next = self.source.next();
        while next.is_some() {
            if next == Some(b' ')
                || next == Some(b'\t')
                || next == Some(b'\r')
                || next == Some(b'\n')
            {
                self.source.consume();
                next = self.source.next();
                found = true;
                continue;
            }
            if next == Some(b'/') {
                next = self.source.lookahead(1);
                if next == Some(b'/') {
                    self.source.consume(); // slash
                    self.source.consume(); // slash
                    self.consume_line()?;
                    next = self.source.next();
                    found = true;
                    continue;
                }
                if next == Some(b'*') {
                    self.source.consume(); // slash
                    self.source.consume(); // star
                    next = self.source.next();
                    while next.is_some() {
                        if next == Some(b'*') {
                            next = self.source.lookahead(1);
                            if next == Some(b'/') {
                                self.source.consume(); // star
                                self.source.consume(); // slash
                                break;
                            }
                        }
                        self.source.consume();
                        next = self.source.next();
                    }
                    next = self.source.next();
                    found = true;
                    continue;
                }
            }
            break;
        }
        if expected && !found {
            Err(self.error_at_pos("Expected whitespace"))
        } else {
            Ok(())
        }
    }
    fn consume_any_chars(&mut self) {
        if !is_ident_start(self.source.next()) { return }
        self.source.consume();
        while is_ident_rest(self.source.next()) {
            self.source.consume()
        }
    }
    fn has_keyword(&self, keyword: &[u8]) -> bool {
        if !self.source.has(keyword) {
            return false;
        }

        // Word boundary
        if let Some(next) = self.source.lookahead(keyword.len()) {
            !(next >= b'A' && next <= b'Z' || next >= b'a' && next <= b'z')
        } else {
            true
        }
    }
    fn consume_keyword(&mut self, keyword: &[u8]) -> Result<(), ParseError2> {
        let len = keyword.len();
        for i in 0..len {
            let expected = Some(lowercase(keyword[i]));
            let next = self.source.next();
            if next != expected {
                return Err(self.error_at_pos(&format!("Expected keyword '{}'", String::from_utf8_lossy(keyword))));
            }
            self.source.consume();
        }
        if let Some(next) = self.source.next() {
            if next >= b'A' && next <= b'Z' || next >= b'a' && next <= b'z' || next >= b'0' && next <= b'9' {
                return Err(self.error_at_pos(&format!("Expected word boundary after '{}'", String::from_utf8_lossy(keyword))));
            }
        }
        Ok(())
    }
    fn has_string(&self, string: &[u8]) -> bool {
        self.source.has(string)
    }
    fn consume_string(&mut self, string: &[u8]) -> Result<(), ParseError2> {
        let len = string.len();
        for i in 0..len {
            let expected = Some(string[i]);
            let next = self.source.next();
            if next != expected {
                return Err(self.error_at_pos(&format!("Expected {}", String::from_utf8_lossy(string))));
            }
            self.source.consume();
        }
        Ok(())
    }
    fn consume_ident(&mut self) -> Result<Vec<u8>, ParseError2> {
        if !self.has_identifier() {
            return Err(self.error_at_pos("Expected identifier"));
        }
        let mut result = Vec::new();
        let mut next = self.source.next();
        result.push(next.unwrap());
        self.source.consume();
        next = self.source.next();
        while is_ident_rest(next) {
            result.push(next.unwrap());
            self.source.consume();
            next = self.source.next();
        }
        Ok(result)
    }
    fn has_integer(&mut self) -> bool {
        is_integer_start(self.source.next())
    }
    fn consume_integer(&mut self) -> Result<i64, ParseError2> {
        let position = self.source.pos();
        let mut data = Vec::new();
        let mut next = self.source.next();
        while is_integer_rest(next) {
            data.push(next.unwrap());
            self.source.consume();
            next = self.source.next();
        }

        let data_len = data.len();
        debug_assert_ne!(data_len, 0);
        if data_len == 1 {
            debug_assert!(data[0] >= b'0' && data[0] <= b'9');
            return Ok((data[0] - b'0') as i64);
        } else {
            // TODO: Fix, u64 should be supported as well
            let parsed = if data[1] == b'b' {
                let data = String::from_utf8_lossy(&data[2..]);
                i64::from_str_radix(&data, 2)
            } else if data[1] == b'o' {
                let data = String::from_utf8_lossy(&data[2..]);
                i64::from_str_radix(&data, 8)
            } else if data[1] == b'x' {
                let data = String::from_utf8_lossy(&data[2..]);
                i64::from_str_radix(&data, 16)
            } else {
                // Assume decimal
                let data = String::from_utf8_lossy(&data);
                i64::from_str_radix(&data, 10)
            };

            if let Err(_err) = parsed {
                return Err(ParseError2::new_error(&self.source, position, "Invalid integer constant"));
            }

            Ok(parsed.unwrap())
        }
    }

    // Statement keywords
    // TODO: Clean up these functions
    fn has_statement_keyword(&self) -> bool {
        self.has_keyword(b"channel")
            || self.has_keyword(b"skip")
            || self.has_keyword(b"if")
            || self.has_keyword(b"while")
            || self.has_keyword(b"break")
            || self.has_keyword(b"continue")
            || self.has_keyword(b"synchronous")
            || self.has_keyword(b"return")
            || self.has_keyword(b"assert")
            || self.has_keyword(b"goto")
            || self.has_keyword(b"new")
            || self.has_keyword(b"put") // TODO: @fix, should be a function, even though it has sideeffects
    }
    fn has_type_keyword(&self) -> bool {
        self.has_keyword(b"in")
            || self.has_keyword(b"out")
            || self.has_keyword(b"msg")
            || self.has_keyword(b"boolean")
            || self.has_keyword(b"byte")
            || self.has_keyword(b"short")
            || self.has_keyword(b"int")
            || self.has_keyword(b"long")
            || self.has_keyword(b"auto")
    }
    fn has_builtin_keyword(&self) -> bool {
        self.has_keyword(b"get")
            || self.has_keyword(b"fires")
            || self.has_keyword(b"create")
            || self.has_keyword(b"length")
    }
    fn has_reserved(&self) -> bool {
        self.has_statement_keyword()
            || self.has_type_keyword()
            || self.has_builtin_keyword()
            || self.has_keyword(b"let")
            || self.has_keyword(b"struct")
            || self.has_keyword(b"enum")
            || self.has_keyword(b"true")
            || self.has_keyword(b"false")
            || self.has_keyword(b"null")
    }

    // Identifiers

    fn has_identifier(&self) -> bool {
        if self.has_statement_keyword() || self.has_type_keyword() || self.has_builtin_keyword() {
            return false;
        }
        let next = self.source.next();
        is_ident_start(next)
    }
    fn consume_identifier(&mut self) -> Result<Identifier, ParseError2> {
        if self.has_statement_keyword() || self.has_type_keyword() || self.has_builtin_keyword() {
            return Err(self.error_at_pos("Expected identifier"));
        }
        let position = self.source.pos();
        let value = self.consume_ident()?;
        Ok(Identifier{ position, value })
    }
    fn consume_identifier_spilled(&mut self) -> Result<(), ParseError2> {
        if self.has_statement_keyword() || self.has_type_keyword() || self.has_builtin_keyword() {
            return Err(self.error_at_pos("Expected identifier"));
        }
        self.consume_ident()?;
        Ok(())
    }
    fn has_namespaced_identifier(&self) -> bool { 
        self.has_identifier() 
    }
    fn consume_namespaced_identifier(&mut self) -> Result<NamespacedIdentifier, ParseError2> {
        if self.has_reserved() {
            return Err(self.error_at_pos("Encountered reserved keyword"));
        }

        let position = self.source.pos();
        let mut ns_ident = self.consume_ident()?;
        let mut num_namespaces = 1;
        while self.has_string(b"::") {
            if num_namespaces >= MAX_NAMESPACES {
                return Err(self.error_at_pos("Too many namespaces in identifier"));
            }
            let new_ident = self.consume_ident()?;
            ns_ident.extend(b"::");
            ns_ident.extend(new_ident);
            num_namespaces += 1;
        }

        Ok(NamespacedIdentifier{
            position,
            value: ns_ident,
            num_namespaces,
        })
    }

    // Types and type annotations

    /// Consumes a type definition. When called the input position should be at
    /// the type specification. When done the input position will be at the end
    /// of the type specifications (hence may be at whitespace).
    fn consume_type2(&mut self, h: &mut Heap, allow_inference: bool) -> Result<ParserTypeId, ParseError2> {
        // Small helper function to convert in/out polymorphic arguments
        let reduce_port_poly_args = |
            heap: &mut Heap,
            port_pos: &InputPosition,
            args: Vec<ParserTypeId>,
        | -> Result<ParserTypeId, ()> {
            match args.len() {
                0 => Ok(heap.alloc_parser_type(|this| ParserType{
                        this,
                        pos: port_pos.clone(),
                        variant: ParserTypeVariant::Inferred
                })),
                1 => Ok(args[0]),
                _ => Err(())
            }
        };

        // Consume the type
        let pos = self.source.pos();
        let parser_type_variant = if self.has_keyword(b"msg") {
            self.consume_keyword(b"msg");
            ParserTypeVariant::Message
        } else if self.has_keyword(b"boolean") {
            self.consume_keyword(b"boolean");
            ParserTypeVariant::Bool
        } else if self.has_keyword(b"byte") {
            self.consume_keyword(b"byte");
            ParserTypeVariant::Byte
        } else if self.has_keyword(b"short") {
            self.consume_keyword(b"short");
            ParserTypeVariant::Short
        } else if self.has_keyword(b"int") {
            self.consume_keyword(b"int");
            ParserTypeVariant::Int
        } else if self.has_keyword(b"long") {
            self.consume_keyword(b"long");
            ParserTypeVariant::Long
        } else if self.has_keyword(b"str") {
            self.consume_keyword(b"str");
            ParserTypeVariant::String
        } else if self.has_keyword(b"auto") {
            if !allow_inference {
                return Err(ParseError2::new_error(
                        &self.source, pos,
                        "Type inference is not allowed here"
                ));
            }

            self.consume_keyword(b"auto");
            ParserTypeVariant::Inferred
        } else if self.has_keyword(b"in") {
            // TODO: @cleanup: not particularly neat to have this special case
            //  where we enforce polyargs in the parser-phase
            self.consume_keyword(b"in");
            let poly_args = self.consume_polymorphic_args(h, allow_inference)?;
            let poly_arg = reduce_port_poly_args(h, &pos, poly_args)
                .map_err(|_| ParseError2::new_error(
                    &self.source, pos, "'in' type only accepts up to 1 polymorphic argument"
                ))?;
            ParserTypeVariant::Input(poly_arg)
        } else if self.has_keyword(b"out") {
            self.consume_keyword(b"out");
            let poly_args = self.consume_polymorphic_args(h, allow_inference)?;
            let poly_arg = reduce_port_poly_args(h, &pos, poly_args)
                .map_err(|_| ParseError2::new_error(
                    &self.source, pos, "'out' type only accepts up to 1 polymorphic argument"
                ))?;
            ParserTypeVariant::Output(poly_arg)
        } else {
            // Must be a symbolic type
            let identifier = self.consume_namespaced_identifier()?;
            let poly_args = self.consume_polymorphic_args(h, allow_inference)?;
            ParserTypeVariant::Symbolic(SymbolicParserType{identifier, poly_args, variant: None})
        };

        // If the type was a basic type (not supporting polymorphic type
        // arguments), then we make sure the user did not specify any of them.
        let mut backup_pos = self.source.pos();
        if !parser_type_variant.supports_polymorphic_args() {
            self.consume_whitespace(false)?;
            if let Some(b'<') = self.source.next() {
                return Err(ParseError2::new_error(
                    &self.source, self.source.pos(),
                    "This type does not allow polymorphic arguments"
                ));
            }

            self.source.seek(backup_pos);
        }

        let mut parser_type_id = h.alloc_parser_type(|this| ParserType{
            this, pos, variant: parser_type_variant
        });

        // If we're dealing with arrays, then we need to wrap the currently
        // parsed type in array types
        self.consume_whitespace(false)?;
        while let Some(b'[') = self.source.next() {
            let pos = self.source.pos();
            self.source.consume();
            self.consume_whitespace(false)?;
            if let Some(b']') = self.source.next() {
                // Type is wrapped in an array
                self.source.consume();
                parser_type_id = h.alloc_parser_type(|this| ParserType{
                    this, pos, variant: ParserTypeVariant::Array(parser_type_id)
                });
                backup_pos = self.source.pos();

                // In case we're dealing with another array
                self.consume_whitespace(false)?;
            } else {
                return Err(ParseError2::new_error(
                    &self.source, pos,
                    "Expected a closing ']'"
                ));
            }
        }

        self.source.seek(backup_pos);
        Ok(parser_type_id)
    }

    /// Consumes things that look like types. If everything seems to look like
    /// a type then `true` will be returned and the input position will be
    /// placed after the type. If it doesn't appear to be a type then `false`
    /// will be returned.
    /// TODO: @cleanup, this is not particularly pretty or robust, methinks
    fn maybe_consume_type_spilled(&mut self) -> bool {
        // Spilling polymorphic args. Don't care about the input position
        fn maybe_consume_polymorphic_args(v: &mut Lexer) -> bool {
            if v.consume_whitespace(false).is_err() { return false; }
            if let Some(b'<') = v.source.next() {
                v.source.consume();
                if v.consume_whitespace(false).is_err() { return false; }
                loop {
                    if !maybe_consume_type_inner(v) { return false; }
                    if v.consume_whitespace(false).is_err() { return false; }
                    let has_comma = v.source.next() == Some(b',');
                    if has_comma {
                        v.source.consume();
                        if v.consume_whitespace(false).is_err() { return false; }
                    }
                    if let Some(b'>') = v.source.next() {
                        v.source.consume();
                        break;
                    } else if !has_comma {
                        return false;
                    }
                }
            }
            return true;
        }

        // Inner recursive type parser. This method simply advances the lexer
        // and does not store the backup position in case parsing fails
        fn maybe_consume_type_inner(v: &mut Lexer) -> bool {
            // Consume type identifier and optional polymorphic args
            if v.has_type_keyword() {
                v.consume_any_chars()
            } else {
                let ident = v.consume_namespaced_identifier();
                if ident.is_err() { return false }
            }

            if !maybe_consume_polymorphic_args(v) { return false; }

            // Check if wrapped in array
            if v.consume_whitespace(false).is_err() { return false }
            while let Some(b'[') = v.source.next() {
                v.source.consume();
                if v.consume_whitespace(false).is_err() { return false; }
                if Some(b']') != v.source.next() { return false; }
                v.source.consume();
            }

            return true;
        }

        let backup_pos = self.source.pos();
        if !maybe_consume_type_inner(self) {
            // Not a type
            self.source.seek(backup_pos);
            return false;
        }

        return true;
    }

    /// Consumes polymorphic arguments and its delimiters if specified. The
    /// input position may be at whitespace. If polyargs are present then the
    /// whitespace and the args are consumed and the input position will be
    /// placed after the polyarg list. If polyargs are not present then the
    /// input position will remain unmodified and an empty vector will be
    /// returned.
    ///
    /// Polymorphic arguments represent the specification of the parametric
    /// types of a polymorphic type: they specify the value of the polymorphic
    /// type's polymorphic variables.
    fn consume_polymorphic_args(&mut self, h: &mut Heap, allow_inference: bool) -> Result<Vec<ParserTypeId>, ParseError2> {
        let backup_pos = self.source.pos();
        self.consume_whitespace(false)?;
        if let Some(b'<') = self.source.next() {
            // Has polymorphic args, at least one type must be specified
            self.source.consume();
            self.consume_whitespace(false)?;
            let mut poly_args = Vec::new();

            loop {
                // TODO: @cleanup, remove the no_more_types var
                poly_args.push(self.consume_type2(h, allow_inference)?);
                self.consume_whitespace(false)?;

                let has_comma = self.source.next() == Some(b',');
                if has_comma {
                    // We might not actually be getting more types when the
                    // comma is at the end of the line, and we get a closing
                    // angular bracket on the next line.
                    self.source.consume();
                    self.consume_whitespace(false)?;
                }

                if let Some(b'>') = self.source.next() {
                    self.source.consume();
                    break;
                } else if !has_comma {
                    return Err(ParseError2::new_error(
                        &self.source, self.source.pos(),
                        "Expected the end of the polymorphic argument list"
                    ))
                }
            }

            Ok(poly_args)
        } else {
            // No polymorphic args
            self.source.seek(backup_pos);
            Ok(vec!())
        }
    }

    /// Consumes polymorphic variables. These are identifiers that are used
    /// within polymorphic types. The input position may be at whitespace. If
    /// polymorphic variables are present then the whitespace, wrapping
    /// delimiters and the polymorphic variables are consumed. Otherwise the
    /// input position will stay where it is. If no polymorphic variables are
    /// present then an empty vector will be returned.
    fn consume_polymorphic_vars(&mut self) -> Result<Vec<Identifier>, ParseError2> {
        let backup_pos = self.source.pos();
        self.consume_whitespace(false)?;
        if let Some(b'<') = self.source.next() {
            // Found the opening delimiter, we want at least one polyvar
            self.source.consume();
            self.consume_whitespace(false)?;
            let mut poly_vars = Vec::new();

            loop {
                poly_vars.push(self.consume_identifier()?);
                self.consume_whitespace(false)?;

                let has_comma = self.source.next() == Some(b',');
                if has_comma {
                    // We may get another variable
                    self.source.consume();
                    self.consume_whitespace(false)?;
                }

                if let Some(b'>') = self.source.next() {
                    self.source.consume();
                    break;
                } else if !has_comma {
                    return Err(ParseError2::new_error(
                        &self.source, self.source.pos(),
                        "Expected the end of the polymorphic variable list"
                    ))
                }
            }

            Ok(poly_vars)
        } else {
            // No polymorphic args
            self.source.seek(backup_pos);
            Ok(vec!())
        }
    }

    // fn consume_primitive_type(&mut self) -> Result<PrimitiveType, ParseError2> {
    //     if self.has_keyword(b"in") {
    //         self.consume_keyword(b"in")?;
    //         Ok(PrimitiveType::Input)
    //     } else if self.has_keyword(b"out") {
    //         self.consume_keyword(b"out")?;
    //         Ok(PrimitiveType::Output)
    //     } else if self.has_keyword(b"msg") {
    //         self.consume_keyword(b"msg")?;
    //         Ok(PrimitiveType::Message)
    //     } else if self.has_keyword(b"boolean") {
    //         self.consume_keyword(b"boolean")?;
    //         Ok(PrimitiveType::Boolean)
    //     } else if self.has_keyword(b"byte") {
    //         self.consume_keyword(b"byte")?;
    //         Ok(PrimitiveType::Byte)
    //     } else if self.has_keyword(b"short") {
    //         self.consume_keyword(b"short")?;
    //         Ok(PrimitiveType::Short)
    //     } else if self.has_keyword(b"int") {
    //         self.consume_keyword(b"int")?;
    //         Ok(PrimitiveType::Int)
    //     } else if self.has_keyword(b"long") {
    //         self.consume_keyword(b"long")?;
    //         Ok(PrimitiveType::Long)
    //     } else if self.has_keyword(b"auto") {
    //         // TODO: @types
    //         return Err(self.error_at_pos("inferred types using 'auto' are reserved, but not yet implemented"));
    //     } else {
    //         let identifier = self.consume_namespaced_identifier()?;
    //         Ok(PrimitiveType::Symbolic(PrimitiveSymbolic{
    //             identifier,
    //             definition: None
    //         }))
    //     }
    // }
    // fn has_array(&mut self) -> bool {
    //     let backup_pos = self.source.pos();
    //     let mut result = false;
    //     match self.consume_whitespace(false) {
    //         Ok(_) => result = self.has_string(b"["),
    //         Err(_) => {}
    //     }
    //     self.source.seek(backup_pos);
    //     return result;
    // }
    // fn consume_type(&mut self) -> Result<Type, ParseError2> {
    //     let primitive = self.consume_primitive_type()?;
    //     let array;
    //     if self.has_array() {
    //         self.consume_string(b"[]")?;
    //         array = true;
    //     } else {
    //         array = false;
    //     }
    //     Ok(Type { primitive, array })
    // }
    // fn create_type_annotation_input(&self, h: &mut Heap) -> Result<TypeAnnotationId, ParseError2> {
    //     let position = self.source.pos();
    //     let the_type = Type::INPUT;
    //     let id = h.alloc_type_annotation(|this| TypeAnnotation { this, position, the_type });
    //     Ok(id)
    // }
    // fn create_type_annotation_output(&self, h: &mut Heap) -> Result<TypeAnnotationId, ParseError2> {
    //     let position = self.source.pos();
    //     let the_type = Type::OUTPUT;
    //     let id = h.alloc_type_annotation(|this| TypeAnnotation { this, position, the_type });
    //     Ok(id)
    // }
    // fn consume_type_annotation(&mut self, h: &mut Heap) -> Result<TypeAnnotationId, ParseError2> {
    //     let position = self.source.pos();
    //     let the_type = self.consume_type()?;
    //     let id = h.alloc_type_annotation(|this| TypeAnnotation { this, position, the_type });
    //     Ok(id)
    // }
    // fn consume_type_annotation_spilled(&mut self) -> Result<(), ParseError2> {
    //     self.consume_type()?;
    //     Ok(())
    // }

    // Parameters

    fn consume_parameter(&mut self, h: &mut Heap) -> Result<ParameterId, ParseError2> {
        let parser_type = self.consume_type2(h, false)?;
        self.consume_whitespace(true)?;
        let position = self.source.pos();
        let identifier = self.consume_identifier()?;
        let id =
            h.alloc_parameter(|this| Parameter { this, position, parser_type, identifier });
        Ok(id)
    }
    fn consume_parameters(
        &mut self,
        h: &mut Heap,
        params: &mut Vec<ParameterId>,
    ) -> Result<(), ParseError2> {
        self.consume_string(b"(")?;
        self.consume_whitespace(false)?;
        if !self.has_string(b")") {
            while self.source.next().is_some() {
                params.push(self.consume_parameter(h)?);
                self.consume_whitespace(false)?;
                if self.has_string(b")") {
                    break;
                }
                self.consume_string(b",")?;
                self.consume_whitespace(false)?;
            }
        }
        self.consume_string(b")")?;

        Ok(())
    }

    // ====================
    // Expressions
    // ====================

    fn consume_paren_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        self.consume_string(b"(")?;
        self.consume_whitespace(false)?;
        let result = self.consume_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b")")?;
        Ok(result)
    }
    fn consume_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        if self.level >= MAX_LEVEL {
            return Err(self.error_at_pos("Too deeply nested expression"));
        }
        self.level += 1;
        let result = self.consume_assignment_expression(h);
        self.level -= 1;
        result
    }
    fn consume_assignment_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let result = self.consume_conditional_expression(h)?;
        self.consume_whitespace(false)?;
        if self.has_assignment_operator() {
            let position = self.source.pos();
            let left = result;
            let operation = self.consume_assignment_operator()?;
            self.consume_whitespace(false)?;
            let right = self.consume_expression(h)?;
            Ok(h.alloc_assignment_expression(|this| AssignmentExpression {
                this,
                position,
                left,
                operation,
                right,
            })
            .upcast())
        } else {
            Ok(result)
        }
    }
    fn has_assignment_operator(&self) -> bool {
        self.has_string(b"=")
            || self.has_string(b"*=")
            || self.has_string(b"/=")
            || self.has_string(b"%=")
            || self.has_string(b"+=")
            || self.has_string(b"-=")
            || self.has_string(b"<<=")
            || self.has_string(b">>=")
            || self.has_string(b"&=")
            || self.has_string(b"^=")
            || self.has_string(b"|=")
    }
    fn consume_assignment_operator(&mut self) -> Result<AssignmentOperator, ParseError2> {
        if self.has_string(b"=") {
            self.consume_string(b"=")?;
            Ok(AssignmentOperator::Set)
        } else if self.has_string(b"*=") {
            self.consume_string(b"*=")?;
            Ok(AssignmentOperator::Multiplied)
        } else if self.has_string(b"/=") {
            self.consume_string(b"/=")?;
            Ok(AssignmentOperator::Divided)
        } else if self.has_string(b"%=") {
            self.consume_string(b"%=")?;
            Ok(AssignmentOperator::Remained)
        } else if self.has_string(b"+=") {
            self.consume_string(b"+=")?;
            Ok(AssignmentOperator::Added)
        } else if self.has_string(b"-=") {
            self.consume_string(b"-=")?;
            Ok(AssignmentOperator::Subtracted)
        } else if self.has_string(b"<<=") {
            self.consume_string(b"<<=")?;
            Ok(AssignmentOperator::ShiftedLeft)
        } else if self.has_string(b">>=") {
            self.consume_string(b">>=")?;
            Ok(AssignmentOperator::ShiftedRight)
        } else if self.has_string(b"&=") {
            self.consume_string(b"&=")?;
            Ok(AssignmentOperator::BitwiseAnded)
        } else if self.has_string(b"^=") {
            self.consume_string(b"^=")?;
            Ok(AssignmentOperator::BitwiseXored)
        } else if self.has_string(b"|=") {
            self.consume_string(b"|=")?;
            Ok(AssignmentOperator::BitwiseOred)
        } else {
            Err(self.error_at_pos("Expected assignment operator"))
        }
    }
    fn consume_conditional_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let result = self.consume_concat_expression(h)?;
        self.consume_whitespace(false)?;
        if self.has_string(b"?") {
            let position = self.source.pos();
            let test = result;
            self.consume_string(b"?")?;
            self.consume_whitespace(false)?;
            let true_expression = self.consume_expression(h)?;
            self.consume_whitespace(false)?;
            self.consume_string(b":")?;
            self.consume_whitespace(false)?;
            let false_expression = self.consume_expression(h)?;
            Ok(h.alloc_conditional_expression(|this| ConditionalExpression {
                this,
                position,
                test,
                true_expression,
                false_expression,
            })
            .upcast())
        } else {
            Ok(result)
        }
    }
    fn consume_concat_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_lor_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"@") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"@")?;
            let operation = BinaryOperator::Concatenate;
            self.consume_whitespace(false)?;
            let right = self.consume_lor_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_lor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_land_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"||") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"||")?;
            let operation = BinaryOperator::LogicalOr;
            self.consume_whitespace(false)?;
            let right = self.consume_land_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_land_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_bor_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"&&") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"&&")?;
            let operation = BinaryOperator::LogicalAnd;
            self.consume_whitespace(false)?;
            let right = self.consume_bor_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_bor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_xor_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"|") && !self.has_string(b"||") && !self.has_string(b"|=") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"|")?;
            let operation = BinaryOperator::BitwiseOr;
            self.consume_whitespace(false)?;
            let right = self.consume_xor_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_xor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_band_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"^") && !self.has_string(b"^=") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"^")?;
            let operation = BinaryOperator::BitwiseXor;
            self.consume_whitespace(false)?;
            let right = self.consume_band_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_band_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_eq_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"&") && !self.has_string(b"&&") && !self.has_string(b"&=") {
            let position = self.source.pos();
            let left = result;
            self.consume_string(b"&")?;
            let operation = BinaryOperator::BitwiseAnd;
            self.consume_whitespace(false)?;
            let right = self.consume_eq_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_eq_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_rel_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"==") || self.has_string(b"!=") {
            let position = self.source.pos();
            let left = result;
            let operation;
            if self.has_string(b"==") {
                self.consume_string(b"==")?;
                operation = BinaryOperator::Equality;
            } else {
                self.consume_string(b"!=")?;
                operation = BinaryOperator::Inequality;
            }
            self.consume_whitespace(false)?;
            let right = self.consume_rel_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_rel_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_shift_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"<=")
            || self.has_string(b">=")
            || self.has_string(b"<") && !self.has_string(b"<<=")
            || self.has_string(b">") && !self.has_string(b">>=")
        {
            let position = self.source.pos();
            let left = result;
            let operation;
            if self.has_string(b"<=") {
                self.consume_string(b"<=")?;
                operation = BinaryOperator::LessThanEqual;
            } else if self.has_string(b">=") {
                self.consume_string(b">=")?;
                operation = BinaryOperator::GreaterThanEqual;
            } else if self.has_string(b"<") {
                self.consume_string(b"<")?;
                operation = BinaryOperator::LessThan;
            } else {
                self.consume_string(b">")?;
                operation = BinaryOperator::GreaterThan;
            }
            self.consume_whitespace(false)?;
            let right = self.consume_shift_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_shift_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_add_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"<<") && !self.has_string(b"<<=")
            || self.has_string(b">>") && !self.has_string(b">>=")
        {
            let position = self.source.pos();
            let left = result;
            let operation;
            if self.has_string(b"<<") {
                self.consume_string(b"<<")?;
                operation = BinaryOperator::ShiftLeft;
            } else {
                self.consume_string(b">>")?;
                operation = BinaryOperator::ShiftRight;
            }
            self.consume_whitespace(false)?;
            let right = self.consume_add_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_add_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_mul_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"+") && !self.has_string(b"+=")
            || self.has_string(b"-") && !self.has_string(b"-=")
        {
            let position = self.source.pos();
            let left = result;
            let operation;
            if self.has_string(b"+") {
                self.consume_string(b"+")?;
                operation = BinaryOperator::Add;
            } else {
                self.consume_string(b"-")?;
                operation = BinaryOperator::Subtract;
            }
            self.consume_whitespace(false)?;
            let right = self.consume_mul_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_mul_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_prefix_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"*") && !self.has_string(b"*=")
            || self.has_string(b"/") && !self.has_string(b"/=")
            || self.has_string(b"%") && !self.has_string(b"%=")
        {
            let position = self.source.pos();
            let left = result;
            let operation;
            if self.has_string(b"*") {
                self.consume_string(b"*")?;
                operation = BinaryOperator::Multiply;
            } else if self.has_string(b"/") {
                self.consume_string(b"/")?;
                operation = BinaryOperator::Divide;
            } else {
                self.consume_string(b"%")?;
                operation = BinaryOperator::Remainder;
            }
            self.consume_whitespace(false)?;
            let right = self.consume_prefix_expression(h)?;
            self.consume_whitespace(false)?;
            result = h
                .alloc_binary_expression(|this| BinaryExpression {
                    this,
                    position,
                    left,
                    operation,
                    right,
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_prefix_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        if self.has_string(b"+")
            || self.has_string(b"-")
            || self.has_string(b"~")
            || self.has_string(b"!")
        {
            let position = self.source.pos();
            let operation;
            if self.has_string(b"+") {
                self.consume_string(b"+")?;
                if self.has_string(b"+") {
                    self.consume_string(b"+")?;
                    operation = UnaryOperation::PreIncrement;
                } else {
                    operation = UnaryOperation::Positive;
                }
            } else if self.has_string(b"-") {
                self.consume_string(b"-")?;
                if self.has_string(b"-") {
                    self.consume_string(b"-")?;
                    operation = UnaryOperation::PreDecrement;
                } else {
                    operation = UnaryOperation::Negative;
                }
            } else if self.has_string(b"~") {
                self.consume_string(b"~")?;
                operation = UnaryOperation::BitwiseNot;
            } else {
                self.consume_string(b"!")?;
                operation = UnaryOperation::LogicalNot;
            }
            self.consume_whitespace(false)?;
            if self.level >= MAX_LEVEL {
                return Err(self.error_at_pos("Too deeply nested expression"));
            }
            self.level += 1;
            let result = self.consume_prefix_expression(h);
            self.level -= 1;
            let expression = result?;
            return Ok(h
                .alloc_unary_expression(|this| UnaryExpression {
                    this,
                    position,
                    operation,
                    expression,
                })
                .upcast());
        }
        self.consume_postfix_expression(h)
    }
    fn consume_postfix_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        let mut result = self.consume_primary_expression(h)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"++")
            || self.has_string(b"--")
            || self.has_string(b"[")
            || (self.has_string(b".") && !self.has_string(b".."))
        {
            let mut position = self.source.pos();
            if self.has_string(b"++") {
                self.consume_string(b"++")?;
                let operation = UnaryOperation::PostIncrement;
                let expression = result;
                self.consume_whitespace(false)?;
                result = h
                    .alloc_unary_expression(|this| UnaryExpression {
                        this,
                        position,
                        operation,
                        expression,
                    })
                    .upcast();
            } else if self.has_string(b"--") {
                self.consume_string(b"--")?;
                let operation = UnaryOperation::PostDecrement;
                let expression = result;
                self.consume_whitespace(false)?;
                result = h
                    .alloc_unary_expression(|this| UnaryExpression {
                        this,
                        position,
                        operation,
                        expression,
                    })
                    .upcast();
            } else if self.has_string(b"[") {
                self.consume_string(b"[")?;
                self.consume_whitespace(false)?;
                let subject = result;
                let index = self.consume_expression(h)?;
                self.consume_whitespace(false)?;
                if self.has_string(b"..") || self.has_string(b":") {
                    position = self.source.pos();
                    if self.has_string(b"..") {
                        self.consume_string(b"..")?;
                    } else {
                        self.consume_string(b":")?;
                    }
                    self.consume_whitespace(false)?;
                    let to_index = self.consume_expression(h)?;
                    self.consume_whitespace(false)?;
                    result = h
                        .alloc_slicing_expression(|this| SlicingExpression {
                            this,
                            position,
                            subject,
                            from_index: index,
                            to_index,
                        })
                        .upcast();
                } else {
                    result = h
                        .alloc_indexing_expression(|this| IndexingExpression {
                            this,
                            position,
                            subject,
                            index,
                        })
                        .upcast();
                }
                self.consume_string(b"]")?;
                self.consume_whitespace(false)?;
            } else {
                assert!(self.has_string(b"."));
                self.consume_string(b".")?;
                self.consume_whitespace(false)?;
                let subject = result;
                let field;
                if self.has_keyword(b"length") {
                    self.consume_keyword(b"length")?;
                    field = Field::Length;
                } else {
                    field = Field::Symbolic(self.consume_identifier()?);
                }
                result = h
                    .alloc_select_expression(|this| SelectExpression {
                        this,
                        position,
                        subject,
                        field,
                    })
                    .upcast();
            }
        }
        Ok(result)
    }
    fn consume_primary_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError2> {
        if self.has_string(b"(") {
            return self.consume_paren_expression(h);
        }
        if self.has_string(b"{") {
            return Ok(self.consume_array_expression(h)?.upcast());
        }
        if self.has_constant()
            || self.has_keyword(b"null")
            || self.has_keyword(b"true")
            || self.has_keyword(b"false")
        {
            return Ok(self.consume_constant_expression(h)?.upcast());
        }
        if self.has_call_expression() {
            return Ok(self.consume_call_expression(h)?.upcast());
        }
        Ok(self.consume_variable_expression(h)?.upcast())
    }
    fn consume_array_expression(&mut self, h: &mut Heap) -> Result<ArrayExpressionId, ParseError2> {
        let position = self.source.pos();
        let mut elements = Vec::new();
        self.consume_string(b"{")?;
        self.consume_whitespace(false)?;
        if !self.has_string(b"}") {
            while self.source.next().is_some() {
                elements.push(self.consume_expression(h)?);
                self.consume_whitespace(false)?;
                if self.has_string(b"}") {
                    break;
                }
                self.consume_string(b",")?;
                self.consume_whitespace(false)?;
            }
        }
        self.consume_string(b"}")?;
        Ok(h.alloc_array_expression(|this| ArrayExpression { this, position, elements }))
    }
    fn has_constant(&self) -> bool {
        is_constant(self.source.next())
    }
    fn consume_constant_expression(
        &mut self,
        h: &mut Heap,
    ) -> Result<ConstantExpressionId, ParseError2> {
        let position = self.source.pos();
        let value;
        if self.has_keyword(b"null") {
            self.consume_keyword(b"null")?;
            value = Constant::Null;
        } else if self.has_keyword(b"true") {
            self.consume_keyword(b"true")?;
            value = Constant::True;
        } else if self.has_keyword(b"false") {
            self.consume_keyword(b"false")?;
            value = Constant::False;
        } else if self.source.next() == Some(b'\'') {
            self.source.consume();
            let mut data = Vec::new();
            let mut next = self.source.next();
            while next != Some(b'\'') && (is_vchar(next) || next == Some(b' ')) {
                data.push(next.unwrap());
                self.source.consume();
                next = self.source.next();
            }
            if next != Some(b'\'') || data.is_empty() {
                return Err(self.error_at_pos("Expected character constant"));
            }
            self.source.consume();
            value = Constant::Character(data);
        } else {
            if !self.has_integer() {
                return Err(self.error_at_pos("Expected integer constant"));
            }

            value = Constant::Integer(self.consume_integer()?);
        }
        Ok(h.alloc_constant_expression(|this| ConstantExpression { this, position, value }))
    }
    fn has_call_expression(&mut self) -> bool {
        /* We prevent ambiguity with variables, by looking ahead
        the identifier to see if we can find an opening
        parenthesis: this signals a call expression. */
        if self.has_builtin_keyword() {
            return true;
        }
        let backup_pos = self.source.pos();
        let mut result = false;
        match self.consume_identifier_spilled() {
            Ok(_) => match self.consume_whitespace(false) {
                Ok(_) => {
                    result = self.has_string(b"(");
                }
                Err(_) => {}
            },
            Err(_) => {}
        }
        self.source.seek(backup_pos);
        return result;
    }
    fn consume_call_expression(&mut self, h: &mut Heap) -> Result<CallExpressionId, ParseError2> {
        let position = self.source.pos();
        let method;
        if self.has_keyword(b"get") {
            self.consume_keyword(b"get")?;
            method = Method::Get;
        } else if self.has_keyword(b"fires") {
            self.consume_keyword(b"fires")?;
            method = Method::Fires;
        } else if self.has_keyword(b"create") {
            self.consume_keyword(b"create")?;
            method = Method::Create;
        } else {
            let identifier = self.consume_namespaced_identifier()?;
            method = Method::Symbolic(MethodSymbolic{
                identifier,
                definition: None
            })
        }
        self.consume_whitespace(false)?;
        let mut arguments = Vec::new();
        self.consume_string(b"(")?;
        self.consume_whitespace(false)?;
        if !self.has_string(b")") {
            while self.source.next().is_some() {
                arguments.push(self.consume_expression(h)?);
                self.consume_whitespace(false)?;
                if self.has_string(b")") {
                    break;
                }
                self.consume_string(b",")?;
                self.consume_whitespace(false)?
            }
        }
        self.consume_string(b")")?;
        Ok(h.alloc_call_expression(|this| CallExpression {
            this,
            position,
            method,
            arguments
        }))
    }
    fn consume_variable_expression(
        &mut self,
        h: &mut Heap,
    ) -> Result<VariableExpressionId, ParseError2> {
        let position = self.source.pos();
        let identifier = self.consume_namespaced_identifier()?;
        Ok(h.alloc_variable_expression(|this| VariableExpression {
            this,
            position,
            identifier,
            declaration: None,
        }))
    }

    // ====================
    // Statements
    // ====================

    /// Consumes any kind of statement from the source and will error if it
    /// did not encounter a statement. Will also return an error if the
    /// statement is nested too deeply.
    ///
    /// `wrap_in_block` may be set to true to ensure that the parsed statement
    /// will be wrapped in a block statement if it is not already a block
    /// statement. This is used to ensure that all `if`, `while` and `sync`
    /// statements have a block statement as body.
    fn consume_statement(&mut self, h: &mut Heap, wrap_in_block: bool) -> Result<StatementId, ParseError2> {
        if self.level >= MAX_LEVEL {
            return Err(self.error_at_pos("Too deeply nested statement"));
        }
        self.level += 1;
        let result = self.consume_statement_impl(h, wrap_in_block);
        self.level -= 1;
        result
    }
    fn has_label(&mut self) -> bool {
        /* To prevent ambiguity with expression statements consisting
        only of an identifier, we look ahead and match the colon
        that signals a labeled statement. */
        let backup_pos = self.source.pos();
        let mut result = false;
        match self.consume_identifier_spilled() {
            Ok(_) => match self.consume_whitespace(false) {
                Ok(_) => {
                    result = self.has_string(b":");
                }
                Err(_) => {}
            },
            Err(_) => {}
        }
        self.source.seek(backup_pos);
        return result;
    }
    fn consume_statement_impl(&mut self, h: &mut Heap, wrap_in_block: bool) -> Result<StatementId, ParseError2> {
        // Parse and allocate statement
        let mut must_wrap = true;
        let mut stmt_id = if self.has_string(b"{") {
            must_wrap = false;
            self.consume_block_statement(h)?
        } else if self.has_keyword(b"skip") {
            must_wrap = false;
            self.consume_skip_statement(h)?.upcast()
        } else if self.has_keyword(b"if") {
            self.consume_if_statement(h)?.upcast()
        } else if self.has_keyword(b"while") {
            self.consume_while_statement(h)?.upcast()
        } else if self.has_keyword(b"break") {
            self.consume_break_statement(h)?.upcast()
        } else if self.has_keyword(b"continue") {
            self.consume_continue_statement(h)?.upcast()
        } else if self.has_keyword(b"synchronous") {
            self.consume_synchronous_statement(h)?.upcast()
        } else if self.has_keyword(b"return") {
            self.consume_return_statement(h)?.upcast()
        } else if self.has_keyword(b"assert") {
            self.consume_assert_statement(h)?.upcast()
        } else if self.has_keyword(b"goto") {
            self.consume_goto_statement(h)?.upcast()
        } else if self.has_keyword(b"new") {
            self.consume_new_statement(h)?.upcast()
        } else if self.has_keyword(b"put") {
            self.consume_put_statement(h)?.upcast()
        } else if self.has_label() {
            self.consume_labeled_statement(h)?.upcast()
        } else {
            self.consume_expression_statement(h)?.upcast()
        };

        // Wrap if desired and if needed
        if must_wrap && wrap_in_block {
            let position = h[stmt_id].position();
            let block_wrapper = h.alloc_block_statement(|this| BlockStatement{
                this,
                position,
                statements: vec![stmt_id],
                parent_scope: None,
                relative_pos_in_parent: 0,
                locals: Vec::new(),
                labels: Vec::new()
            });

            stmt_id = block_wrapper.upcast();
        }

        Ok(stmt_id)
    }
    fn has_local_statement(&mut self) -> bool {
        /* To avoid ambiguity, we look ahead to find either the
        channel keyword that signals a variable declaration, or
        a type annotation followed by another identifier.
        Example:
          my_type[] x = {5}; // memory statement
          my_var[5] = x; // assignment expression, expression statement
        Note how both the local and the assignment
        start with arbitrary identifier followed by [. */
        if self.has_keyword(b"channel") {
            return true;
        }
        if self.has_statement_keyword() {
            return false;
        }
        let backup_pos = self.source.pos();
        let mut result = false;
        if self.maybe_consume_type_spilled() {
            // We seem to have a valid type, do we now have an identifier?
            if self.consume_whitespace(false).is_ok() {
                result = self.has_identifier();
            }
        }

        self.source.seek(backup_pos);
        return result;
    }
    fn consume_block_statement(&mut self, h: &mut Heap) -> Result<StatementId, ParseError2> {
        let position = self.source.pos();
        let mut statements = Vec::new();
        self.consume_string(b"{")?;
        self.consume_whitespace(false)?;
        while self.has_local_statement() {
            statements.push(self.consume_local_statement(h)?.upcast());
            self.consume_whitespace(false)?;
        }
        while !self.has_string(b"}") {
            statements.push(self.consume_statement(h, false)?);
            self.consume_whitespace(false)?;
        }
        self.consume_string(b"}")?;
        if statements.is_empty() {
            Ok(h.alloc_skip_statement(|this| SkipStatement { this, position, next: None }).upcast())
        } else {
            Ok(h.alloc_block_statement(|this| BlockStatement {
                this,
                position,
                statements,
                parent_scope: None,
                relative_pos_in_parent: 0,
                locals: Vec::new(),
                labels: Vec::new(),
            })
            .upcast())
        }
    }
    fn consume_local_statement(&mut self, h: &mut Heap) -> Result<LocalStatementId, ParseError2> {
        if self.has_keyword(b"channel") {
            Ok(self.consume_channel_statement(h)?.upcast())
        } else {
            Ok(self.consume_memory_statement(h)?.upcast())
        }
    }
    fn consume_channel_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<ChannelStatementId, ParseError2> {
        // Consume channel statement and polymorphic argument if specified
        let position = self.source.pos();
        self.consume_keyword(b"channel")?;

        let poly_args = self.consume_polymorphic_args(h, true)?;
        let poly_arg_id = match poly_args.len() {
            0 => h.alloc_parser_type(|this| ParserType{
                this, pos: position.clone(), variant: ParserTypeVariant::Inferred,
            }),
            1 => poly_args[0],
            _ => return Err(ParseError2::new_error(
                &self.source, self.source.pos(),
                "port construction using 'channel' accepts up to 1 polymorphic argument"
            ))
        };
        self.consume_whitespace(true)?;

        // Consume the output port
        let out_parser_type = h.alloc_parser_type(|this| ParserType{
            this, pos: position.clone(), variant: ParserTypeVariant::Output(poly_arg_id)
        });
        let out_identifier = self.consume_identifier()?;

        // Consume the "->" syntax
        self.consume_whitespace(false)?;
        self.consume_string(b"->")?;
        self.consume_whitespace(false)?;

        // Consume the input port
        // TODO: Unsure about this, both ports refer to the same ParserType, is this ok?
        let in_parser_type = h.alloc_parser_type(|this| ParserType{
            this, pos: position.clone(), variant: ParserTypeVariant::Input(poly_arg_id)
        });
        let in_identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        let out_port = h.alloc_local(|this| Local {
            this,
            position,
            parser_type: out_parser_type,
            identifier: out_identifier,
            relative_pos_in_block: 0
        });
        let in_port = h.alloc_local(|this| Local {
            this,
            position,
            parser_type: in_parser_type,
            identifier: in_identifier,
            relative_pos_in_block: 0
        });
        Ok(h.alloc_channel_statement(|this| ChannelStatement {
            this,
            position,
            from: out_port,
            to: in_port,
            relative_pos_in_block: 0,
            next: None,
        }))
    }
    fn consume_memory_statement(&mut self, h: &mut Heap) -> Result<MemoryStatementId, ParseError2> {
        let position = self.source.pos();
        let parser_type = self.consume_type2(h, true)?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        self.consume_string(b"=")?;
        self.consume_whitespace(false)?;
        let initial = self.consume_expression(h)?;
        let variable = h.alloc_local(|this| Local {
            this,
            position,
            parser_type,
            identifier,
            relative_pos_in_block: 0
        });
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_memory_statement(|this| MemoryStatement {
            this,
            position,
            variable,
            initial,
            next: None,
        }))
    }
    fn consume_labeled_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<LabeledStatementId, ParseError2> {
        let position = self.source.pos();
        let label = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        self.consume_string(b":")?;
        self.consume_whitespace(false)?;
        let body = self.consume_statement(h, false)?;
        Ok(h.alloc_labeled_statement(|this| LabeledStatement {
            this,
            position,
            label,
            body,
            relative_pos_in_block: 0,
            in_sync: None,
        }))
    }
    fn consume_skip_statement(&mut self, h: &mut Heap) -> Result<SkipStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"skip")?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_skip_statement(|this| SkipStatement { this, position, next: None }))
    }
    fn consume_if_statement(&mut self, h: &mut Heap) -> Result<IfStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"if")?;
        self.consume_whitespace(false)?;
        let test = self.consume_paren_expression(h)?;
        self.consume_whitespace(false)?;
        let true_body = self.consume_statement(h, true)?;
        self.consume_whitespace(false)?;
        let false_body = if self.has_keyword(b"else") {
            self.consume_keyword(b"else")?;
            self.consume_whitespace(false)?;
            self.consume_statement(h, true)?
        } else {
            h.alloc_skip_statement(|this| SkipStatement { this, position, next: None }).upcast()
        };
        Ok(h.alloc_if_statement(|this| IfStatement { this, position, test, true_body, false_body, end_if: None }))
    }
    fn consume_while_statement(&mut self, h: &mut Heap) -> Result<WhileStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"while")?;
        self.consume_whitespace(false)?;
        let test = self.consume_paren_expression(h)?;
        self.consume_whitespace(false)?;
        let body = self.consume_statement(h, true)?;
        Ok(h.alloc_while_statement(|this| WhileStatement {
            this,
            position,
            test,
            body,
            end_while: None,
            in_sync: None,
        }))
    }
    fn consume_break_statement(&mut self, h: &mut Heap) -> Result<BreakStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"break")?;
        self.consume_whitespace(false)?;
        let label;
        if self.has_identifier() {
            label = Some(self.consume_identifier()?);
            self.consume_whitespace(false)?;
        } else {
            label = None;
        }
        self.consume_string(b";")?;
        Ok(h.alloc_break_statement(|this| BreakStatement { this, position, label, target: None }))
    }
    fn consume_continue_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<ContinueStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"continue")?;
        self.consume_whitespace(false)?;
        let label;
        if self.has_identifier() {
            label = Some(self.consume_identifier()?);
            self.consume_whitespace(false)?;
        } else {
            label = None;
        }
        self.consume_string(b";")?;
        Ok(h.alloc_continue_statement(|this| ContinueStatement {
            this,
            position,
            label,
            target: None,
        }))
    }
    fn consume_synchronous_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<SynchronousStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"synchronous")?;
        self.consume_whitespace(false)?;
        // TODO: What was the purpose of this? Seems superfluous and confusing?
        // let mut parameters = Vec::new();
        // if self.has_string(b"(") {
        //     self.consume_parameters(h, &mut parameters)?;
        //     self.consume_whitespace(false)?;
        // } else if !self.has_keyword(b"skip") && !self.has_string(b"{") {
        //     return Err(self.error_at_pos("Expected block statement"));
        // }
        let body = self.consume_statement(h, true)?;
        Ok(h.alloc_synchronous_statement(|this| SynchronousStatement {
            this,
            position,
            body,
            end_sync: None,
            parent_scope: None,
        }))
    }
    fn consume_return_statement(&mut self, h: &mut Heap) -> Result<ReturnStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"return")?;
        self.consume_whitespace(false)?;
        let expression = if self.has_string(b"(") {
            self.consume_paren_expression(h)
        } else {
            self.consume_expression(h)
        }?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_return_statement(|this| ReturnStatement { this, position, expression }))
    }
    fn consume_assert_statement(&mut self, h: &mut Heap) -> Result<AssertStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"assert")?;
        self.consume_whitespace(false)?;
        let expression = if self.has_string(b"(") {
            self.consume_paren_expression(h)
        } else {
            self.consume_expression(h)
        }?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_assert_statement(|this| AssertStatement {
            this,
            position,
            expression,
            next: None,
        }))
    }
    fn consume_goto_statement(&mut self, h: &mut Heap) -> Result<GotoStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"goto")?;
        self.consume_whitespace(false)?;
        let label = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_goto_statement(|this| GotoStatement { this, position, label, target: None }))
    }
    fn consume_new_statement(&mut self, h: &mut Heap) -> Result<NewStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"new")?;
        self.consume_whitespace(false)?;
        let expression = self.consume_call_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_new_statement(|this| NewStatement { this, position, expression, next: None }))
    }
    fn consume_put_statement(&mut self, h: &mut Heap) -> Result<PutStatementId, ParseError2> {
        let position = self.source.pos();
        self.consume_keyword(b"put")?;
        self.consume_whitespace(false)?;
        self.consume_string(b"(")?;
        let port = self.consume_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b",")?;
        self.consume_whitespace(false)?;
        let message = self.consume_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b")")?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_put_statement(|this| PutStatement { this, position, port, message, next: None }))
    }
    fn consume_expression_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<ExpressionStatementId, ParseError2> {
        let position = self.source.pos();
        let expression = self.consume_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_expression_statement(|this| ExpressionStatement {
            this,
            position,
            expression,
            next: None,
        }))
    }

    // ====================
    // Symbol definitions
    // ====================

    fn has_symbol_definition(&self) -> bool {
        self.has_keyword(b"composite")
            || self.has_keyword(b"primitive")
            || self.has_type_keyword()
            || self.has_identifier()
    }
    fn consume_symbol_definition(&mut self, h: &mut Heap) -> Result<DefinitionId, ParseError2> {
        if self.has_keyword(b"struct") {
            Ok(self.consume_struct_definition(h)?.upcast())
        } else if self.has_keyword(b"enum") {
            Ok(self.consume_enum_definition(h)?.upcast())
        } else if self.has_keyword(b"composite") || self.has_keyword(b"primitive") {
            Ok(self.consume_component_definition(h)?.upcast())
        } else {
            Ok(self.consume_function_definition(h)?.upcast())
        }
    }
    fn consume_struct_definition(&mut self, h: &mut Heap) -> Result<StructId, ParseError2> {
        // Parse "struct" keyword, optional polyvars and its identifier
        let struct_pos = self.source.pos();
        self.consume_keyword(b"struct")?;
        self.consume_whitespace(true)?;
        let struct_ident = self.consume_identifier()?;
        let poly_vars = self.consume_polymorphic_vars()?;
        self.consume_whitespace(false)?;

        // Parse struct fields
        self.consume_string(b"{")?;
        let mut next = self.source.next();
        let mut fields = Vec::new();
        while next.is_some() {
            let char = next.unwrap();
            if char == b'}' {
                break;
            }

            // Consume field definition
            self.consume_whitespace(false)?;
            let field_position = self.source.pos();
            let field_parser_type = self.consume_type2(h, false)?;
            self.consume_whitespace(true)?;
            let field_ident = self.consume_identifier()?;
            self.consume_whitespace(false)?;

            fields.push(StructFieldDefinition{
                position: field_position,
                field: field_ident,
                parser_type: field_parser_type,
            });

            // If we have a comma, then we may or may not have another field
            // definition. Otherwise we expect the struct to be fully defined
            // and expect a closing brace
            next = self.source.next();
            if let Some(b',') = next {
                self.source.consume();
                self.consume_whitespace(false)?;
                next = self.source.next();
            } else {
                break;
            }
        }

        // End of struct definition, so we expect a closing brace
        self.consume_string(b"}")?;

        // Valid struct definition
        Ok(h.alloc_struct_definition(|this| StructDefinition{
            this,
            position: struct_pos,
            identifier: struct_ident,
            poly_vars,
            fields,
        }))
    }
    fn consume_enum_definition(&mut self, h: &mut Heap) -> Result<EnumId, ParseError2> {
        // Parse "enum" keyword, optional polyvars and its identifier
        let enum_pos = self.source.pos();
        self.consume_keyword(b"enum")?;
        self.consume_whitespace(true)?;
        let enum_ident = self.consume_identifier()?;
        let poly_vars = self.consume_polymorphic_vars()?;
        self.consume_whitespace(false)?;

        // Parse enum variants
        self.consume_string(b"{")?;
        let mut next = self.source.next();
        let mut variants = Vec::new();
        while next.is_some() {
            let char = next.unwrap();
            if char == b'}' {
                break;
            }

            // Consume variant identifier
            self.consume_whitespace(false)?;
            let variant_position = self.source.pos();
            let variant_ident = self.consume_identifier()?;
            self.consume_whitespace(false)?;

            // Consume variant (tag) value: may be nothing, in which case it is
            // assigned automatically, may be a constant integer, or an embedded
            // type as value, resulting in a tagged union
            next = self.source.next();
            let variant_value = if let Some(b',') = next {
                EnumVariantValue::None
            } else if let Some(b'=') = next {
                self.source.consume();
                self.consume_whitespace(false)?;
                if !self.has_integer() {
                    return Err(self.error_at_pos("expected integer"));
                }
                let variant_int = self.consume_integer()?;
                self.consume_whitespace(false)?;
                EnumVariantValue::Integer(variant_int)
            } else if let Some(b'(') = next {
                self.source.consume();
                self.consume_whitespace(false)?;
                let variant_type = self.consume_type2(h, false)?;
                self.consume_whitespace(false)?;
                self.consume_string(b")")?;
                self.consume_whitespace(false)?;
                EnumVariantValue::Type(variant_type)
            } else {
                return Err(self.error_at_pos("expected ',', '=', or '('"));
            };

            variants.push(EnumVariantDefinition{
                position: variant_position,
                identifier: variant_ident,
                value: variant_value
            });

            // If we have a comma, then we may or may not have another variant,
            // otherwise we expect the enum is fully defined
            next = self.source.next();
            if let Some(b',') = next {
                self.source.consume();
                self.consume_whitespace(false)?;
                next = self.source.next();
            } else {
                break;
            }
        }

        self.consume_string(b"}")?;

        // An enum without variants is somewhat valid, but completely useless
        // within the language
        if variants.is_empty() {
            return Err(ParseError2::new_error(self.source, enum_pos, "enum definition without variants"));
        }

        Ok(h.alloc_enum_definition(|this| EnumDefinition{
            this,
            position: enum_pos,
            identifier: enum_ident,
            poly_vars,
            variants,
        }))
    }
    fn consume_component_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError2> {
        // TODO: Cleanup
        if self.has_keyword(b"composite") {
            Ok(self.consume_composite_definition(h)?)
        } else {
            Ok(self.consume_primitive_definition(h)?)
        }
    }
    fn consume_composite_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError2> {
        // Parse keyword, optional polyvars and the identifier
        let position = self.source.pos();
        self.consume_keyword(b"composite")?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        let poly_vars = self.consume_polymorphic_vars()?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let mut parameters = Vec::new();
        self.consume_parameters(h, &mut parameters)?;
        self.consume_whitespace(false)?;

        // Parse body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_component(|this| Component { 
            this,
            variant: ComponentVariant::Composite,
            position,
            identifier,
            poly_vars,
            parameters,
            body
        }))
    }
    fn consume_primitive_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError2> {
        // Consume keyword, optional polyvars and identifier
        let position = self.source.pos();
        self.consume_keyword(b"primitive")?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        let poly_vars = self.consume_polymorphic_vars()?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let mut parameters = Vec::new();
        self.consume_parameters(h, &mut parameters)?;
        self.consume_whitespace(false)?;

        // Consume body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_component(|this| Component { 
            this,
            variant: ComponentVariant::Primitive,
            position,
            identifier,
            poly_vars,
            parameters,
            body
        }))
    }
    fn consume_function_definition(&mut self, h: &mut Heap) -> Result<FunctionId, ParseError2> {
        // Consume return type, optional polyvars and identifier
        let position = self.source.pos();
        let return_type = self.consume_type2(h, false)?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        let poly_vars = self.consume_polymorphic_vars()?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let mut parameters = Vec::new();
        self.consume_parameters(h, &mut parameters)?;
        self.consume_whitespace(false)?;

        // Consume body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_function(|this| Function {
            this,
            position,
            return_type,
            identifier,
            poly_vars,
            parameters,
            body,
        }))
    }
    fn has_pragma(&self) -> bool {
        if let Some(c) = self.source.next() {
            c == b'#'
        } else {
            false
        }
    }
    fn consume_pragma(&mut self, h: &mut Heap) -> Result<PragmaId, ParseError2> {
        let position = self.source.pos();
        let next = self.source.next();
        if next != Some(b'#') {
            return Err(self.error_at_pos("Expected pragma"));
        }
        self.source.consume();
        if !is_vchar(self.source.next()) {
            return Err(self.error_at_pos("Expected pragma"));
        }
        if self.has_string(b"version") {
            self.consume_string(b"version")?;
            self.consume_whitespace(true)?;
            if !self.has_integer() {
                return Err(self.error_at_pos("Expected integer constant"));
            }
            let version = self.consume_integer()?;
            debug_assert!(version >= 0);
            return Ok(h.alloc_pragma(|this| Pragma::Version(PragmaVersion{
                this, position, version: version as u64
            })))
        } else if self.has_string(b"module") {
            self.consume_string(b"module")?;
            self.consume_whitespace(true)?;
            if !self.has_identifier() {
                return Err(self.error_at_pos("Expected identifier"));
            }
            let mut value = Vec::new();
            let mut ident = self.consume_ident()?;
            value.append(&mut ident);
            while self.has_string(b".") {
                self.consume_string(b".")?;
                value.push(b'.');
                ident = self.consume_ident()?;
                value.append(&mut ident);
            }
            return Ok(h.alloc_pragma(|this| Pragma::Module(PragmaModule{
                this, position, value
            })));
        } else {
            return Err(self.error_at_pos("Unknown pragma"));
        }
    }

    fn has_import(&self) -> bool {
        self.has_keyword(b"import")
    }
    fn consume_import(&mut self, h: &mut Heap) -> Result<ImportId, ParseError2> {
        // Parse the word "import" and the name of the module
        let position = self.source.pos();
        self.consume_keyword(b"import")?;
        self.consume_whitespace(true)?;
        let mut value = Vec::new();
        let mut ident = self.consume_ident()?;
        value.append(&mut ident);
        let mut last_ident_start = 0;

        while self.has_string(b".") {
            self.consume_string(b".")?;
            value.push(b'.');
            ident = self.consume_ident()?;
            last_ident_start = value.len();
            value.append(&mut ident);
        }


        self.consume_whitespace(false)?;

        // Check for the potential aliasing or specific module imports
        let import = if self.has_string(b"as") {
            self.consume_string(b"as")?;
            self.consume_whitespace(true)?;
            let alias = self.consume_ident()?;

            h.alloc_import(|this| Import::Module(ImportModule{
                this,
                position,
                module_name: value,
                alias,
                module_id: None,
            }))
        } else if self.has_string(b"::") {
            self.consume_string(b"::")?;
            self.consume_whitespace(false)?;

            if let Some(b'{') = self.source.next() {
                // Import specific symbols, optionally with an alias
                self.source.consume();
                self.consume_whitespace(false)?;

                let mut symbols = Vec::new();
                let mut next = self.source.next();

                while next.is_some() {
                    let char = next.unwrap();
                    if char == b'}' {
                        break;
                    }

                    let symbol_position = self.source.pos();
                    let symbol_name = self.consume_ident()?;
                    self.consume_whitespace(false)?;
                    if self.has_string(b"as") {
                        // Symbol has an alias
                        self.consume_string(b"as")?;
                        self.consume_whitespace(true)?;
                        let symbol_alias = self.consume_ident()?;

                        symbols.push(AliasedSymbol{
                            position: symbol_position,
                            name: symbol_name,
                            alias: symbol_alias,
                            definition_id: None,
                        });
                    } else {
                        // Symbol does not have an alias
                        symbols.push(AliasedSymbol{
                            position: symbol_position,
                            name: symbol_name.clone(),
                            alias: symbol_name,
                            definition_id: None,
                        });
                    }

                    // A comma indicates that we may have another symbol coming
                    // up (not necessary), but if not present then we expect the
                    // end of the symbol list
                    self.consume_whitespace(false)?;

                    next = self.source.next();
                    if let Some(b',') = next {
                        self.source.consume();
                        self.consume_whitespace(false)?;
                        next = self.source.next();
                    } else {
                        break;
                    }
                }

                if let Some(b'}') = next {
                    // We are fine, push the imported symbols
                    self.source.consume();
                    if symbols.is_empty() {
                        return Err(ParseError2::new_error(self.source, position, "empty symbol import list"));
                    }

                    h.alloc_import(|this| Import::Symbols(ImportSymbols{
                        this,
                        position,
                        module_name: value,
                        module_id: None,
                        symbols,
                    }))
                } else {
                    return Err(self.error_at_pos("Expected '}'"));
                }
            } else if let Some(b'*') = self.source.next() {
                // Import all symbols without alias
                self.source.consume();
                h.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    position,
                    module_name: value,
                    module_id: None,
                    symbols: Vec::new()
                }))
            } else {
                return Err(self.error_at_pos("Expected '*' or '{'"));
            }
        } else {
            // No explicit alias or subimports, so implicit alias
            let alias = Vec::from(&value[last_ident_start..]);
            h.alloc_import(|this| Import::Module(ImportModule{
                this,
                position,
                module_name: value,
                alias,
                module_id: None,
            }))
        };

        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(import)
    }
    pub fn consume_protocol_description(&mut self, h: &mut Heap) -> Result<RootId, ParseError2> {
        let position = self.source.pos();
        let mut pragmas = Vec::new();
        let mut imports = Vec::new();
        let mut definitions = Vec::new();
        self.consume_whitespace(false)?;
        while self.has_pragma() {
            let pragma = self.consume_pragma(h)?;
            pragmas.push(pragma);
            self.consume_whitespace(false)?;
        }
        while self.has_import() {
            let import = self.consume_import(h)?;
            imports.push(import);
            self.consume_whitespace(false)?;
        }
        while self.has_symbol_definition() {
            let def = self.consume_symbol_definition(h)?;
            definitions.push(def);
            self.consume_whitespace(false)?;
        }
        // end of file
        if !self.source.is_eof() {
            return Err(self.error_at_pos("Expected end of file"));
        }
        Ok(h.alloc_protocol_description(|this| Root {
            this,
            position,
            pragmas,
            imports,
            definitions,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::ast::*;
    use crate::protocol::lexer::*;
    use crate::protocol::inputsource::*;

    #[derive(Debug, Eq, PartialEq)]
    enum ParserTypeClass {
        Message, Bool, Byte, Short, Int, Long, String, Array, Nope
    }
    impl ParserTypeClass {
        fn from(v: &ParserType) -> ParserTypeClass {
            use ParserTypeVariant as PTV;
            use ParserTypeClass as PTC;
            match &v.variant {
                PTV::Message => PTC::Message,
                PTV::Bool => PTC::Bool,
                PTV::Byte => PTC::Byte,
                PTV::Short => PTC::Short,
                PTV::Int => PTC::Int,
                PTV::Long => PTC::Long,
                PTV::String => PTC::String,
                PTV::Array(_) => PTC::Array,
                _ => PTC::Nope,
            }
        }
    }

    #[test]
    fn test_pragmas() {
        let mut h = Heap::new();
        let mut input = InputSource::from_string("
        #version 0o7777
        #module something.dot.separated
        ").expect("new InputSource");
        let mut lex = Lexer::new(&mut input);
        let lexed = lex.consume_protocol_description(&mut h)
            .expect("lex input source");
        let root = &h[lexed];
        assert_eq!(root.pragmas.len(), 2);
        let pv = &h[root.pragmas[0]];
        let pm = &h[root.pragmas[1]];

        if let Pragma::Version(v) = pv {
            assert_eq!(v.version, 0o7777)
        } else {
            assert!(false, "first pragma not version");
        }
        if let Pragma::Module(m) = pm {
            assert_eq!(m.value, b"something.dot.separated");
        } else {
            assert!(false, "second pragma not module");
        }
    }

    #[test]
    fn test_import() {
        let mut h = Heap::new();
        let mut input = InputSource::from_string("
        // Module imports, with optional and explicit aliasing
        import single_module;
        import std.reo;
        import something.other as alias;
        // Symbol imports
        import some_module::*;
        import some_module::{Foo as Bar, Qux, Dix as Flu};
        import std.reo::{
            Foo as Bar, // because thing
            Qux as Mox, // more explanations
            Dix, /* yesh, import me */
        };
        ").unwrap();
        let mut lex = Lexer::new(&mut input);
        let lexed = lex.consume_protocol_description(&mut h).unwrap();
        let root = &h[lexed];
        assert_eq!(root.imports.len(), 6);
        let no_alias_single = h[root.imports[0]].as_module();
        let no_alias_multi = h[root.imports[1]].as_module();
        let with_alias = h[root.imports[2]].as_module();

        assert_eq!(no_alias_single.module_name, b"single_module");
        assert_eq!(no_alias_single.alias, b"single_module");
        assert_eq!(no_alias_multi.module_name, b"std.reo");
        assert_eq!(no_alias_multi.alias, b"reo");
        assert_eq!(with_alias.module_name, b"something.other");
        assert_eq!(with_alias.alias, b"alias");

        let all_symbols = h[root.imports[3]].as_symbols();
        let single_line_symbols = h[root.imports[4]].as_symbols();
        let multi_line_symbols = h[root.imports[5]].as_symbols();

        assert_eq!(all_symbols.module_name, b"some_module");
        assert!(all_symbols.symbols.is_empty());
        assert_eq!(single_line_symbols.module_name, b"some_module");
        assert_eq!(single_line_symbols.symbols.len(), 3);
        assert_eq!(single_line_symbols.symbols[0].name, b"Foo");
        assert_eq!(single_line_symbols.symbols[0].alias, b"Bar");
        assert_eq!(single_line_symbols.symbols[1].name, b"Qux");
        assert_eq!(single_line_symbols.symbols[1].alias, b"Qux");
        assert_eq!(single_line_symbols.symbols[2].name, b"Dix");
        assert_eq!(single_line_symbols.symbols[2].alias, b"Flu");
        assert_eq!(multi_line_symbols.module_name, b"std.reo");
        assert_eq!(multi_line_symbols.symbols.len(), 3);
        assert_eq!(multi_line_symbols.symbols[0].name, b"Foo");
        assert_eq!(multi_line_symbols.symbols[0].alias, b"Bar");
        assert_eq!(multi_line_symbols.symbols[1].name, b"Qux");
        assert_eq!(multi_line_symbols.symbols[1].alias, b"Mox");
        assert_eq!(multi_line_symbols.symbols[2].name, b"Dix");
        assert_eq!(multi_line_symbols.symbols[2].alias, b"Dix");
    }

    #[test]
    fn test_struct_definition() {
        let mut h = Heap::new();
        let mut input = InputSource::from_string("
        struct Foo {
            byte one,
            short two,
            Bar three,
        }
        struct Bar{int[] one, int[] two, Qux[] three}
        ").unwrap();
        let mut lex = Lexer::new(&mut input);
        let lexed = lex.consume_protocol_description(&mut h);
        if let Err(err) = &lexed {
            println!("{}", err);
        }
        let lexed = lexed.unwrap();
        let root = &h[lexed];

        assert_eq!(root.definitions.len(), 2);

        // let symbolic_type = |v: &PrimitiveType| -> Vec<u8> {
        //     if let PrimitiveType::Symbolic(v) = v {
        //         v.identifier.value.clone()
        //     } else {
        //         assert!(false);
        //         unreachable!();
        //     }
        // };

        let foo_def = h[root.definitions[0]].as_struct();
        assert_eq!(foo_def.identifier.value, b"Foo");
        assert_eq!(foo_def.fields.len(), 3);
        assert_eq!(foo_def.fields[0].field.value, b"one");
        assert_eq!(ParserTypeClass::from(&h[foo_def.fields[0].parser_type]), ParserTypeClass::Byte);
        assert_eq!(foo_def.fields[1].field.value, b"two");
        assert_eq!(ParserTypeClass::from(&h[foo_def.fields[1].parser_type]), ParserTypeClass::Short);
        assert_eq!(foo_def.fields[2].field.value, b"three");
        // assert_eq!(
        //     symbolic_type(&h[foo_def.fields[2].the_type].the_type.primitive),
        //     Vec::from("Bar".as_bytes())
        // );

        let bar_def = h[root.definitions[1]].as_struct();
        assert_eq!(bar_def.identifier.value, b"Bar");
        assert_eq!(bar_def.fields.len(), 3);
        assert_eq!(bar_def.fields[0].field.value, b"one");
        assert_eq!(ParserTypeClass::from(&h[bar_def.fields[0].parser_type]), ParserTypeClass::Array);
        assert_eq!(bar_def.fields[1].field.value, b"two");
        assert_eq!(ParserTypeClass::from(&h[bar_def.fields[1].parser_type]), ParserTypeClass::Array);
        assert_eq!(bar_def.fields[2].field.value, b"three");
        assert_eq!(ParserTypeClass::from(&h[bar_def.fields[2].parser_type]), ParserTypeClass::Array);
        // assert_eq!(
        //     symbolic_type(&h[bar_def.fields[2].parser_type].the_type.primitive),
        //     Vec::from("Qux".as_bytes())
        // );
    }

    #[test]
    fn test_enum_definition() {
        let mut h = Heap::new();
        let mut input = InputSource::from_string("
        enum Foo {
            A = 0,
            B = 5,
            C,
            D = 0xFF,
        }
        enum Bar { Ayoo, Byoo, Cyoo,}
        enum Qux { A(byte[]), B(Bar[]), C(byte)
        }
        ").unwrap();
        let mut lex = Lexer::new(&mut input);
        let lexed = lex.consume_protocol_description(&mut h).unwrap();
        let root = &h[lexed];

        assert_eq!(root.definitions.len(), 3);

        let foo_def = h[root.definitions[0]].as_enum();
        assert_eq!(foo_def.identifier.value, b"Foo");
        assert_eq!(foo_def.variants.len(), 4);
        assert_eq!(foo_def.variants[0].identifier.value, b"A");
        assert_eq!(foo_def.variants[0].value, EnumVariantValue::Integer(0));
        assert_eq!(foo_def.variants[1].identifier.value, b"B");
        assert_eq!(foo_def.variants[1].value, EnumVariantValue::Integer(5));
        assert_eq!(foo_def.variants[2].identifier.value, b"C");
        assert_eq!(foo_def.variants[2].value, EnumVariantValue::None);
        assert_eq!(foo_def.variants[3].identifier.value, b"D");
        assert_eq!(foo_def.variants[3].value, EnumVariantValue::Integer(0xFF));

        let bar_def = h[root.definitions[1]].as_enum();
        assert_eq!(bar_def.identifier.value, b"Bar");
        assert_eq!(bar_def.variants.len(), 3);
        assert_eq!(bar_def.variants[0].identifier.value, b"Ayoo");
        assert_eq!(bar_def.variants[0].value, EnumVariantValue::None);
        assert_eq!(bar_def.variants[1].identifier.value, b"Byoo");
        assert_eq!(bar_def.variants[1].value, EnumVariantValue::None);
        assert_eq!(bar_def.variants[2].identifier.value, b"Cyoo");
        assert_eq!(bar_def.variants[2].value, EnumVariantValue::None);

        let qux_def = h[root.definitions[2]].as_enum();
        let enum_type = |value: &EnumVariantValue| -> &ParserType {
            if let EnumVariantValue::Type(t) = value {
                &h[*t]
            } else {
                assert!(false);
                unreachable!();
            }
        };
        assert_eq!(qux_def.identifier.value, b"Qux");
        assert_eq!(qux_def.variants.len(), 3);
        assert_eq!(qux_def.variants[0].identifier.value, b"A");
        assert_eq!(ParserTypeClass::from(enum_type(&qux_def.variants[0].value)), ParserTypeClass::Array);
        assert_eq!(qux_def.variants[1].identifier.value, b"B");
        assert_eq!(ParserTypeClass::from(enum_type(&qux_def.variants[1].value)), ParserTypeClass::Array);
        // if let PrimitiveType::Symbolic(t) = &enum_type(&qux_def.variants[1].value).the_type.primitive {
        //     assert_eq!(t.identifier.value, Vec::from("Bar".as_bytes()));
        // } else { assert!(false) }

        assert_eq!(qux_def.variants[2].identifier.value, b"C");
        assert_eq!(ParserTypeClass::from(enum_type(&qux_def.variants[2].value)), ParserTypeClass::Byte);
    }

//     #[test]
//     fn test_lowercase() {
//         assert_eq!(lowercase(b'a'), b'a');
//         assert_eq!(lowercase(b'A'), b'a');
//         assert_eq!(lowercase(b'z'), b'z');
//         assert_eq!(lowercase(b'Z'), b'z');
//     }

//     #[test]
//     fn test_basic_expression() {
//         let mut h = Heap::new();
//         let mut is = InputSource::from_string("a+b;").unwrap();
//         let mut lex = Lexer::new(&mut is);
//         match lex.consume_expression(&mut h) {
//             Ok(expr) => {
//                 println!("{:?}", expr);
//                 if let Binary(bin) = &h[expr] {
//                     if let Variable(left) = &h[bin.left] {
//                         if let Variable(right) = &h[bin.right] {
//                             assert_eq!("a", format!("{}", h[left.identifier]));
//                             assert_eq!("b", format!("{}", h[right.identifier]));
//                             assert_eq!(Some(b';'), is.next());
//                             return;
//                         }
//                     }
//                 }
//                 assert!(false);
//             }
//             Err(err) => {
//                 err.print(&is);
//                 assert!(false);
//             }
//         }
//     }

//     #[test]
//     fn test_paren_expression() {
//         let mut h = Heap::new();
//         let mut is = InputSource::from_string("(true)").unwrap();
//         let mut lex = Lexer::new(&mut is);
//         match lex.consume_paren_expression(&mut h) {
//             Ok(expr) => {
//                 println!("{:#?}", expr);
//                 if let Constant(con) = &h[expr] {
//                     if let ast::Constant::True = con.value {
//                         return;
//                     }
//                 }
//                 assert!(false);
//             }
//             Err(err) => {
//                 err.print(&is);
//                 assert!(false);
//             }
//         }
//     }

//     #[test]
//     fn test_expression() {
//         let mut h = Heap::new();
//         let mut is = InputSource::from_string("(x(1+5,get(y))-w[5])+z++\n").unwrap();
//         let mut lex = Lexer::new(&mut is);
//         match lex.consume_expression(&mut h) {
//             Ok(expr) => {
//                 println!("{:#?}", expr);
//             }
//             Err(err) => {
//                 err.print(&is);
//                 assert!(false);
//             }
//         }
//     }

//     #[test]
//     fn test_basic_statement() {
//         let mut h = Heap::new();
//         let mut is = InputSource::from_string("while (true) { skip; }").unwrap();
//         let mut lex = Lexer::new(&mut is);
//         match lex.consume_statement(&mut h) {
//             Ok(stmt) => {
//                 println!("{:#?}", stmt);
//                 if let Statement::While(w) = &h[stmt] {
//                     if let Expression::Constant(_) = h[w.test] {
//                         if let Statement::Block(_) = h[w.body] {
//                             return;
//                         }
//                     }
//                 }
//                 assert!(false);
//             }
//             Err(err) => {
//                 err.print(&is);
//                 assert!(false);
//             }
//         }
//     }

//     #[test]
//     fn test_statement() {
//         let mut h = Heap::new();
//         let mut is = InputSource::from_string(
//             "label: while (true) { if (x++ > y[0]) break label; else continue; }\n",
//         )
//         .unwrap();
//         let mut lex = Lexer::new(&mut is);
//         match lex.consume_statement(&mut h) {
//             Ok(stmt) => {
//                 println!("{:#?}", stmt);
//             }
//             Err(err) => {
//                 err.print(&is);
//                 assert!(false);
//             }
//         }
//     }
}

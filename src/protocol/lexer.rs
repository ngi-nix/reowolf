use crate::protocol::ast::*;
use crate::protocol::inputsource::*;

const MAX_LEVEL: usize = 128;
const MAX_NAMESPACES: u8 = 8; // only three levels are supported at the moment

macro_rules! debug_log {
    ($format:literal) => {
        enabled_debug_print!(true, "lexer", $format);
    };
    ($format:literal, $($args:expr),*) => {
        enabled_debug_print!(true, "lexer", $format, $($args),*);
    };
}

macro_rules! debug_line {
    ($source:expr) => {
        {
            let mut buffer = String::with_capacity(128);
            for idx in 0..buffer.capacity() {
                let next = $source.lookahead(idx);
                if next.is_none() || Some(b'\n') == next { break; }
                buffer.push(next.unwrap() as char);
            }
            buffer
        }
    };
}
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

fn identifier_as_namespaced(identifier: Identifier) -> NamespacedIdentifier {
    let identifier_len = identifier.value.len();
    debug_assert!(identifier_len < u16::max_value() as usize);
    NamespacedIdentifier{
        position: identifier.position,
        value: identifier.value,
        poly_args: Vec::new(),
        parts: vec![
            NamespacedIdentifierPart::Identifier{start: 0, end: identifier_len as u16}
        ],
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
    fn error_at_pos(&self, msg: &str) -> ParseError {
        ParseError::new_error(self.source, self.source.pos(), msg)
    }
    fn consume_line(&mut self) -> Result<Vec<u8>, ParseError> {
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
    fn consume_whitespace(&mut self, expected: bool) -> Result<(), ParseError> {
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
        let next = self.source.lookahead(keyword.len());
        if next.is_none() { return true; }
        return !is_ident_rest(next);
    }
    fn consume_keyword(&mut self, keyword: &[u8]) -> Result<(), ParseError> {
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
    fn consume_string(&mut self, string: &[u8]) -> Result<(), ParseError> {
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
    /// Generic comma-separated consumer. If opening delimiter is not found then
    /// `Ok(None)` will be returned. Otherwise will consume the comma separated
    /// values, allowing a trailing comma. If no comma is found and the closing
    /// delimiter is not found, then a parse error with `expected_end_msg` is
    /// returned.
    fn consume_comma_separated<T, F>(
        &mut self, h: &mut Heap, open: u8, close: u8, expected_end_msg: &str, func: F
    ) -> Result<Option<Vec<T>>, ParseError>
        where F: Fn(&mut Lexer, &mut Heap) -> Result<T, ParseError>
    {
        if Some(open) != self.source.next() {
            return Ok(None)
        }

        self.source.consume();
        self.consume_whitespace(false)?;
        let mut elements = Vec::new();
        let mut had_comma = true;

        loop {
            if Some(close) == self.source.next() {
                self.source.consume();
                break;
            } else if !had_comma {
                return Err(ParseError::new_error(
                    &self.source, self.source.pos(), expected_end_msg
                ));
            }

            elements.push(func(self, h)?);
            self.consume_whitespace(false)?;

            had_comma = self.source.next() == Some(b',');
            if had_comma {
                self.source.consume();
                self.consume_whitespace(false)?;
            }
        }

        Ok(Some(elements))
    }
    /// Essentially the same as `consume_comma_separated`, but will not allocate
    /// memory. Will return `Ok(true)` and leave the input position at the end
    /// the comma-separated list if well formed and `Ok(false)` if the list is
    /// not present. Otherwise returns `Err(())` and leaves the input position 
    /// at a "random" position.
    fn consume_comma_separated_spilled_without_pos_recovery<F: Fn(&mut Lexer) -> bool>(
        &mut self, open: u8, close: u8, func: F
    ) -> Result<bool, ()> {
        if Some(open) != self.source.next() {
            return Ok(false);
        }

        self.source.consume();
        if self.consume_whitespace(false).is_err() { return Err(()) };
        let mut had_comma = true;
        loop {
            if Some(close) == self.source.next() {
                self.source.consume();
                return Ok(true);
            } else if !had_comma {
                return Err(());
            }

            if !func(self) { return Err(()); }
            if self.consume_whitespace(false).is_err() { return Err(()) };

            had_comma = self.source.next() == Some(b',');
            if had_comma {
                self.source.consume();
                if self.consume_whitespace(false).is_err() { return Err(()); }
            }
        }
    }
    fn consume_ident(&mut self) -> Result<Vec<u8>, ParseError> {
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
    fn consume_integer(&mut self) -> Result<i64, ParseError> {
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
                return Err(ParseError::new_error(&self.source, position, "Invalid integer constant"));
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
    fn consume_identifier(&mut self) -> Result<Identifier, ParseError> {
        if self.has_statement_keyword() || self.has_type_keyword() || self.has_builtin_keyword() {
            return Err(self.error_at_pos("Expected identifier"));
        }
        let position = self.source.pos();
        let value = self.consume_ident()?;
        Ok(Identifier{ position, value })
    }
    fn consume_identifier_spilled(&mut self) -> Result<(), ParseError> {
        if self.has_statement_keyword() || self.has_type_keyword() || self.has_builtin_keyword() {
            return Err(self.error_at_pos("Expected identifier"));
        }
        self.consume_ident()?;
        Ok(())
    }

    fn consume_namespaced_identifier(&mut self, h: &mut Heap) -> Result<NamespacedIdentifier, ParseError> {
        if self.has_reserved() {
            return Err(self.error_at_pos("Encountered reserved keyword"));
        }

        // Consumes a part of the namespaced identifier, returns a boolean
        // indicating whether polymorphic arguments were specified.
        // TODO: Continue here: if we fail to properly parse the polymorphic
        //  arguments, assume we have reached the end of the namespaced 
        //  identifier and are instead dealing with a less-than operator. Ugly?
        //  Yes. Needs tokenizer? Yes. 
        fn consume_part(
            l: &mut Lexer, h: &mut Heap, ident: &mut NamespacedIdentifier,
            backup_pos: &mut InputPosition
        ) -> Result<(), ParseError> {
            // Consume identifier
            if !ident.value.is_empty() {
                ident.value.extend(b"::");
            }
            let ident_start = ident.value.len();
            ident.value.extend(l.consume_ident()?);
            ident.parts.push(NamespacedIdentifierPart::Identifier{
                start: ident_start as u16,
                end: ident.value.len() as u16
            });

            // Maybe consume polymorphic args.
            *backup_pos = l.source.pos();
            l.consume_whitespace(false)?;
            match l.consume_polymorphic_args(h, true)? {
                Some(args) => {
                    let poly_start = ident.poly_args.len();
                    ident.poly_args.extend(args);

                    ident.parts.push(NamespacedIdentifierPart::PolyArgs{
                        start: poly_start as u16,
                        end: ident.poly_args.len() as u16,
                    });

                    *backup_pos = l.source.pos();
                },
                None => {}
            };

            Ok(())
        }

        let mut ident = NamespacedIdentifier{
            position: self.source.pos(),
            value: Vec::new(),
            poly_args: Vec::new(),
            parts: Vec::new(),
        };

        // Keep consume parts separted by "::". We don't consume the trailing
        // whitespace, hence we keep a backup position at the end of the last
        // valid part of the namespaced identifier (i.e. the last ident, or the
        // last encountered polymorphic arguments).
        let mut backup_pos = self.source.pos();
        consume_part(self, h, &mut ident, &mut backup_pos)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"::") {
            self.consume_string(b"::")?;
            self.consume_whitespace(false)?;
            consume_part(self, h, &mut ident, &mut backup_pos)?;
            self.consume_whitespace(false)?;
        }

        self.source.seek(backup_pos);
        Ok(ident)
    }

    // Consumes a spilled namespaced identifier and returns the number of
    // namespaces that we encountered.
    fn consume_namespaced_identifier_spilled(&mut self) -> Result<usize, ParseError> {
        if self.has_reserved() {
            return Err(self.error_at_pos("Encountered reserved keyword"));
        }

        debug_log!("consume_nsident2_spilled: {}", debug_line!(self.source));

        fn consume_part_spilled(l: &mut Lexer, backup_pos: &mut InputPosition) -> Result<(), ParseError> {
            l.consume_ident()?;
            *backup_pos = l.source.pos();
            l.consume_whitespace(false)?;
            match l.maybe_consume_poly_args_spilled_without_pos_recovery() {
                Ok(true) => { *backup_pos = l.source.pos(); },
                Ok(false) => {},
                Err(_) => { return Err(l.error_at_pos("Failed to parse poly args (spilled)")) },
            }
            Ok(())
        }

        let mut backup_pos = self.source.pos();
        let mut num_namespaces = 1;
        consume_part_spilled(self, &mut backup_pos)?;
        self.consume_whitespace(false)?;
        while self.has_string(b"::") {
            self.consume_string(b"::")?;
            self.consume_whitespace(false)?;
            consume_part_spilled(self, &mut backup_pos)?;
            self.consume_whitespace(false)?;
            num_namespaces += 1;
        }

        self.source.seek(backup_pos);
        Ok(num_namespaces)
    }

    // Types and type annotations

    /// Consumes a type definition. When called the input position should be at
    /// the type specification. When done the input position will be at the end
    /// of the type specifications (hence may be at whitespace).
    fn consume_type(&mut self, h: &mut Heap, allow_inference: bool) -> Result<ParserTypeId, ParseError> {
        // Small helper function to convert in/out polymorphic arguments. Not
        // pretty, but return boolean is true if the error is due to inference
        // not being allowed
        let reduce_port_poly_args = |
            heap: &mut Heap,
            port_pos: &InputPosition,
            args: Vec<ParserTypeId>,
        | -> Result<ParserTypeId, bool> {
            match args.len() {
                0 => if allow_inference {  
                    Ok(heap.alloc_parser_type(|this| ParserType{
                        this,
                        pos: port_pos.clone(),
                        variant: ParserTypeVariant::Inferred
                    }))
                } else {
                    Err(true)
                },
                1 => Ok(args[0]),
                _ => Err(false)
            }
        };

        // Consume the type
        debug_log!("consume_type: {}", debug_line!(self.source));
        let pos = self.source.pos();
        let parser_type_variant = if self.has_keyword(b"msg") {
            self.consume_keyword(b"msg")?;
            ParserTypeVariant::Message
        } else if self.has_keyword(b"boolean") {
            self.consume_keyword(b"boolean")?;
            ParserTypeVariant::Bool
        } else if self.has_keyword(b"byte") {
            self.consume_keyword(b"byte")?;
            ParserTypeVariant::Byte
        } else if self.has_keyword(b"short") {
            self.consume_keyword(b"short")?;
            ParserTypeVariant::Short
        } else if self.has_keyword(b"int") {
            self.consume_keyword(b"int")?;
            ParserTypeVariant::Int
        } else if self.has_keyword(b"long") {
            self.consume_keyword(b"long")?;
            ParserTypeVariant::Long
        } else if self.has_keyword(b"str") {
            self.consume_keyword(b"str")?;
            ParserTypeVariant::String
        } else if self.has_keyword(b"auto") {
            if !allow_inference {
                return Err(ParseError::new_error(
                        &self.source, pos,
                        "Type inference is not allowed here"
                ));
            }

            self.consume_keyword(b"auto")?;
            ParserTypeVariant::Inferred
        } else if self.has_keyword(b"in") {
            // TODO: @cleanup: not particularly neat to have this special case
            //  where we enforce polyargs in the parser-phase
            self.consume_keyword(b"in")?;
            let poly_args = self.consume_polymorphic_args(h, allow_inference)?.unwrap_or_default();
            let poly_arg = reduce_port_poly_args(h, &pos, poly_args)
                .map_err(|infer_error|  {
                    let msg = if infer_error {
                        "Type inference is not allowed here"
                    } else {
                        "Type 'in' only allows for 1 polymorphic argument"
                    };
                    ParseError::new_error(&self.source, pos, msg)
                })?;
            ParserTypeVariant::Input(poly_arg)
        } else if self.has_keyword(b"out") {
            self.consume_keyword(b"out")?;
            let poly_args = self.consume_polymorphic_args(h, allow_inference)?.unwrap_or_default();
            let poly_arg = reduce_port_poly_args(h, &pos, poly_args)
                .map_err(|infer_error| {
                    let msg = if infer_error {
                        "Type inference is not allowed here"
                    } else {
                        "Type 'out' only allows for 1 polymorphic argument, but {} were specified"
                    };
                    ParseError::new_error(&self.source, pos, msg)
                })?;
            ParserTypeVariant::Output(poly_arg)
        } else {
            // Must be a symbolic type
            let identifier = self.consume_namespaced_identifier(h)?;
            ParserTypeVariant::Symbolic(SymbolicParserType{identifier, variant: None, poly_args2: Vec::new()})
        };

        // If the type was a basic type (not supporting polymorphic type
        // arguments), then we make sure the user did not specify any of them.
        let mut backup_pos = self.source.pos();
        if !parser_type_variant.supports_polymorphic_args() {
            self.consume_whitespace(false)?;
            if let Some(b'<') = self.source.next() {
                return Err(ParseError::new_error(
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
                return Err(ParseError::new_error(
                    &self.source, pos,
                    "Expected a closing ']'"
                ));
            }
        }

        self.source.seek(backup_pos);
        Ok(parser_type_id)
    }

    /// Attempts to consume a type without returning it. If it doesn't encounter
    /// a well-formed type, then the input position is left at a "random"
    /// position.
    fn maybe_consume_type_spilled_without_pos_recovery(&mut self) -> bool {
        // Consume type identifier
        debug_log!("maybe_consume_type_spilled_...: {}", debug_line!(self.source));
        if self.has_type_keyword() {
            self.consume_any_chars();
        } else {
            let ident = self.consume_namespaced_identifier_spilled();
            if ident.is_err() { return false; }
        }

        // Consume any polymorphic arguments that follow the type identifier
        let mut backup_pos = self.source.pos();
        if self.consume_whitespace(false).is_err() { return false; }
        
        // Consume any array specifiers. Make sure we always leave the input
        // position at the end of the last array specifier if we do find a
        // valid type
        if self.consume_whitespace(false).is_err() { return false; }
        while let Some(b'[') = self.source.next() {
            self.source.consume();
            if self.consume_whitespace(false).is_err() { return false; }
            if self.source.next() != Some(b']') { return false; }
            self.source.consume();
            backup_pos = self.source.pos();
            if self.consume_whitespace(false).is_err() { return false; }
        }

        self.source.seek(backup_pos);
        return true;
    }

    fn maybe_consume_type_spilled(&mut self) -> bool {
        let backup_pos = self.source.pos();
        if !self.maybe_consume_type_spilled_without_pos_recovery() {
            self.source.seek(backup_pos);
            return false;
        }

        return true;
    }

    /// Attempts to consume polymorphic arguments without returning them. If it
    /// doesn't encounter well-formed polymorphic arguments, then the input
    /// position is left at a "random" position. Returns a boolean indicating if
    /// the poly_args list was present.
    fn maybe_consume_poly_args_spilled_without_pos_recovery(&mut self) -> Result<bool, ()> {
        debug_log!("maybe_consume_poly_args_spilled_...: {}", debug_line!(self.source));
        self.consume_comma_separated_spilled_without_pos_recovery(
            b'<', b'>', |lexer| {
                lexer.maybe_consume_type_spilled_without_pos_recovery()
            })
    }

    /// Consumes polymorphic arguments and its delimiters if specified. If
    /// polyargs are present then the args are consumed and the input position
    /// will be placed after the polyarg list. If polyargs are not present then
    /// the input position will remain unmodified and an empty vector will be
    /// returned.
    ///
    /// Polymorphic arguments represent the specification of the parametric
    /// types of a polymorphic type: they specify the value of the polymorphic
    /// type's polymorphic variables.
    fn consume_polymorphic_args(&mut self, h: &mut Heap, allow_inference: bool) -> Result<Option<Vec<ParserTypeId>>, ParseError> {
        self.consume_comma_separated(
            h, b'<', b'>', "Expected the end of the polymorphic argument list",
            |lexer, heap| lexer.consume_type(heap, allow_inference)
        )
    }

    /// Consumes polymorphic variables. These are identifiers that are used
    /// within polymorphic types. The input position may be at whitespace. If
    /// polymorphic variables are present then the whitespace, wrapping
    /// delimiters and the polymorphic variables are consumed. Otherwise the
    /// input position will stay where it is. If no polymorphic variables are
    /// present then an empty vector will be returned.
    fn consume_polymorphic_vars(&mut self, h: &mut Heap) -> Result<Vec<Identifier>, ParseError> {
        let backup_pos = self.source.pos();
        match self.consume_comma_separated(
            h, b'<', b'>', "Expected the end of the polymorphic variable list",
            |lexer, _heap| lexer.consume_identifier()
        )? {
            Some(poly_vars) => Ok(poly_vars),
            None => {
                self.source.seek(backup_pos);
                Ok(vec!())
            }
        }
    }

    // Parameters

    fn consume_parameter(&mut self, h: &mut Heap) -> Result<ParameterId, ParseError> {
        let parser_type = self.consume_type(h, false)?;
        self.consume_whitespace(true)?;
        let position = self.source.pos();
        let identifier = self.consume_identifier()?;
        let id =
            h.alloc_parameter(|this| Parameter { this, position, parser_type, identifier });
        Ok(id)
    }
    fn consume_parameters(&mut self, h: &mut Heap) -> Result<Vec<ParameterId>, ParseError> {
        match self.consume_comma_separated(
            h, b'(', b')', "Expected the end of the parameter list",
            |lexer, heap| lexer.consume_parameter(heap)
        )? {
            Some(params) => Ok(params),
            None => {
                Err(ParseError::new_error(
                    &self.source, self.source.pos(),
                    "Expected a parameter list"
                ))
            }
        }
    }

    // ====================
    // Expressions
    // ====================

    fn consume_paren_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
        self.consume_string(b"(")?;
        self.consume_whitespace(false)?;
        let result = self.consume_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b")")?;
        Ok(result)
    }
    fn consume_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
        if self.level >= MAX_LEVEL {
            return Err(self.error_at_pos("Too deeply nested expression"));
        }
        self.level += 1;
        let result = self.consume_assignment_expression(h);
        self.level -= 1;
        result
    }
    fn consume_assignment_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
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
    fn consume_assignment_operator(&mut self) -> Result<AssignmentOperator, ParseError> {
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
    fn consume_conditional_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                parent: ExpressionParent::None,
                concrete_type: ConcreteType::default(),
            })
            .upcast())
        } else {
            Ok(result)
        }
    }
    fn consume_concat_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_lor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_land_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_bor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_xor_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_band_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_eq_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_rel_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_shift_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_add_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_mul_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast();
        }
        Ok(result)
    }
    fn consume_prefix_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                    parent: ExpressionParent::None,
                    concrete_type: ConcreteType::default(),
                })
                .upcast());
        }
        self.consume_postfix_expression(h)
    }
    fn consume_postfix_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
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
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default(),
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
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default(),
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
                            parent: ExpressionParent::None,
                            concrete_type: ConcreteType::default(),
                        })
                        .upcast();
                } else {
                    result = h
                        .alloc_indexing_expression(|this| IndexingExpression {
                            this,
                            position,
                            subject,
                            index,
                            parent: ExpressionParent::None,
                            concrete_type: ConcreteType::default(),
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
                    let identifier = self.consume_identifier()?;
                    field = Field::Symbolic(FieldSymbolic{
                        identifier,
                        definition: None,
                        field_idx: 0,
                    });
                }
                result = h
                    .alloc_select_expression(|this| SelectExpression {
                        this,
                        position,
                        subject,
                        field,
                        parent: ExpressionParent::None,
                        concrete_type: ConcreteType::default(),
                    })
                    .upcast();
            }
        }
        Ok(result)
    }
    fn consume_primary_expression(&mut self, h: &mut Heap) -> Result<ExpressionId, ParseError> {
        if self.has_string(b"(") {
            return self.consume_paren_expression(h);
        }
        if self.has_string(b"{") {
            return Ok(self.consume_array_expression(h)?.upcast());
        }
        if self.has_builtin_literal() {
            return Ok(self.consume_builtin_literal_expression(h)?.upcast());
        }
        if self.has_struct_literal() {
            return Ok(self.consume_struct_literal_expression(h)?.upcast());
        }
        if self.has_call_expression() {
            return Ok(self.consume_call_expression(h)?.upcast());
        }
        if self.has_enum_literal() {
            return Ok(self.consume_enum_literal(h)?.upcast());
        }
        Ok(self.consume_variable_expression(h)?.upcast())
    }
    fn consume_array_expression(&mut self, h: &mut Heap) -> Result<ArrayExpressionId, ParseError> {
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
        Ok(h.alloc_array_expression(|this| ArrayExpression {
            this,
            position,
            elements,
            parent: ExpressionParent::None,
            concrete_type: ConcreteType::default(),
        }))
    }
    fn has_builtin_literal(&self) -> bool {
        is_constant(self.source.next())
            || self.has_keyword(b"null")
            || self.has_keyword(b"true")
            || self.has_keyword(b"false")
    }
    fn consume_builtin_literal_expression(
        &mut self,
        h: &mut Heap,
    ) -> Result<LiteralExpressionId, ParseError> {
        let position = self.source.pos();
        let value;
        if self.has_keyword(b"null") {
            self.consume_keyword(b"null")?;
            value = Literal::Null;
        } else if self.has_keyword(b"true") {
            self.consume_keyword(b"true")?;
            value = Literal::True;
        } else if self.has_keyword(b"false") {
            self.consume_keyword(b"false")?;
            value = Literal::False;
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
            value = Literal::Character(data);
        } else {
            if !self.has_integer() {
                return Err(self.error_at_pos("Expected integer constant"));
            }

            value = Literal::Integer(self.consume_integer()?);
        }
        Ok(h.alloc_literal_expression(|this| LiteralExpression {
            this,
            position,
            value,
            parent: ExpressionParent::None,
            concrete_type: ConcreteType::default(),
        }))
    }
    fn has_enum_literal(&mut self) -> bool {
        // An enum literal is always:
        //      maybe_a_namespace::EnumName<maybe_one_of_these>::Variant
        // So may for now be distinguished from other literals/variables by 
        // first checking for struct literals and call expressions, then for
        // enum literals, finally for variable expressions. It is different
        // from a variable expression in that it _always_ contains multiple
        // elements to the enum.
        let backup_pos = self.source.pos();
        let result = match self.consume_namespaced_identifier_spilled() {
            Ok(num_namespaces) => num_namespaces > 1,
            Err(_) => false,
        };
        self.source.seek(backup_pos);
        result
    }
    fn consume_enum_literal(&mut self, h: &mut Heap) -> Result<LiteralExpressionId, ParseError> {
        let identifier = self.consume_namespaced_identifier(h)?;
        Ok(h.alloc_literal_expression(|this| LiteralExpression{
            this,
            position: identifier.position,
            value: Literal::Enum(LiteralEnum{
                identifier,
                poly_args2: Vec::new(),
                definition: None,
                variant_idx: 0,
            }),
            parent: ExpressionParent::None,
            concrete_type: ConcreteType::default(),
        }))
    }
    fn has_struct_literal(&mut self) -> bool {
        // A struct literal is written as:
        //      namespace::StructName<maybe_one_of_these, auto>{ field: expr }
        // We will parse up until the opening brace to see if we're dealing with
        // a struct literal.
        let backup_pos = self.source.pos();
        let result = self.consume_namespaced_identifier_spilled().is_ok() &&
            self.consume_whitespace(false).is_ok() &&
            self.source.next() == Some(b'{');

        self.source.seek(backup_pos);
        return result;
    }

    fn consume_struct_literal_expression(&mut self, h: &mut Heap) -> Result<LiteralExpressionId, ParseError> {
        // Consume identifier and polymorphic arguments
        debug_log!("consume_struct_literal_expression: {}", debug_line!(self.source));
        let position = self.source.pos();
        let identifier = self.consume_namespaced_identifier(h)?;
        self.consume_whitespace(false)?;

        // Consume fields
        let fields = match self.consume_comma_separated(
            h, b'{', b'}', "Expected the end of the list of struct fields",
            |lexer, heap| {
                let identifier = lexer.consume_identifier()?;
                lexer.consume_whitespace(false)?;
                lexer.consume_string(b":")?;
                lexer.consume_whitespace(false)?;
                let value = lexer.consume_expression(heap)?;

                Ok(LiteralStructField{ identifier, value, field_idx: 0 })
            }
        )? {
            Some(fields) => fields,
            None => return Err(ParseError::new_error(
                self.source, self.source.pos(),
                "A struct literal must be followed by its field values"
            ))
        };

        Ok(h.alloc_literal_expression(|this| LiteralExpression{
            this,
            position,
            value: Literal::Struct(LiteralStruct{
                identifier,
                fields,
                poly_args2: Vec::new(),
                definition: None,
            }),
            parent: ExpressionParent::None,
            concrete_type: Default::default()
        }))
    }

    fn has_call_expression(&mut self) -> bool {
        // We need to prevent ambiguity with various operators (because we may
        // be specifying polymorphic variables) and variables.
        if self.has_builtin_keyword() {
            return true;
        }

        let backup_pos = self.source.pos();
        let mut result = false;

        if self.consume_namespaced_identifier_spilled().is_ok() &&
            self.consume_whitespace(false).is_ok() &&
            self.source.next() == Some(b'(') {
            // Seems like we have a function call or an enum literal
            result = true;
        }

        self.source.seek(backup_pos);
        return result;
    }
    fn consume_call_expression(&mut self, h: &mut Heap) -> Result<CallExpressionId, ParseError> {
        let position = self.source.pos();

        // Consume method identifier
        // TODO: @token Replace this conditional polymorphic arg parsing once we have a tokenizer.
        debug_log!("consume_call_expression: {}", debug_line!(self.source));
        let method;
        let mut consume_poly_args_explicitly = true;
        if self.has_keyword(b"get") {
            self.consume_keyword(b"get")?;
            method = Method::Get;
        } else if self.has_keyword(b"put") {
            self.consume_keyword(b"put")?;
            method = Method::Put;
        } else if self.has_keyword(b"fires") {
            self.consume_keyword(b"fires")?;
            method = Method::Fires;
        } else if self.has_keyword(b"create") {
            self.consume_keyword(b"create")?;
            method = Method::Create;
        } else {
            let identifier = self.consume_namespaced_identifier(h)?;
            method = Method::Symbolic(MethodSymbolic{
                identifier,
                definition: None
            });
            consume_poly_args_explicitly = false;
        };

        // Consume polymorphic arguments
        let poly_args = if consume_poly_args_explicitly {
            self.consume_whitespace(false)?;
            self.consume_polymorphic_args(h, true)?.unwrap_or_default()
        } else {
            Vec::new()
        };

        // Consume arguments to call
        self.consume_whitespace(false)?;
        let mut arguments = Vec::new();
        self.consume_string(b"(")?;
        self.consume_whitespace(false)?;
        if !self.has_string(b")") {
            // TODO: allow trailing comma
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
            arguments,
            poly_args,
            parent: ExpressionParent::None,
            concrete_type: ConcreteType::default(),
        }))
    }
    fn consume_variable_expression(
        &mut self,
        h: &mut Heap,
    ) -> Result<VariableExpressionId, ParseError> {
        let position = self.source.pos();
        debug_log!("consume_variable_expression: {}", debug_line!(self.source));

        // TODO: @token Reimplement when tokenizer is implemented, prevent ambiguities
        let identifier = identifier_as_namespaced(self.consume_identifier()?);

        Ok(h.alloc_variable_expression(|this| VariableExpression {
            this,
            position,
            identifier,
            declaration: None,
            parent: ExpressionParent::None,
            concrete_type: ConcreteType::default(),
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
    fn consume_statement(&mut self, h: &mut Heap, wrap_in_block: bool) -> Result<StatementId, ParseError> {
        if self.level >= MAX_LEVEL {
            return Err(self.error_at_pos("Too deeply nested statement"));
        }
        self.level += 1;
        let result = self.consume_statement_impl(h, wrap_in_block);
        self.level -= 1;
        result
    }
    fn has_label(&mut self) -> bool {
        // To prevent ambiguity with expression statements consisting only of an
        // identifier or a namespaced identifier, we look ahead and match on the
        // *single* colon that signals a labeled statement.
        let backup_pos = self.source.pos();
        let mut result = false;
        if self.consume_identifier_spilled().is_ok() {
            // next character is ':', second character is NOT ':'
            result = Some(b':') == self.source.next() && Some(b':') != self.source.lookahead(1)
        }
        self.source.seek(backup_pos);
        return result;
    }
    fn consume_statement_impl(&mut self, h: &mut Heap, wrap_in_block: bool) -> Result<StatementId, ParseError> {
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
        if self.maybe_consume_type_spilled_without_pos_recovery() {
            // We seem to have a valid type, do we now have an identifier?
            if self.consume_whitespace(true).is_ok() {
                result = self.has_identifier();
            }
        }

        self.source.seek(backup_pos);
        return result;
    }
    fn consume_block_statement(&mut self, h: &mut Heap) -> Result<StatementId, ParseError> {
        let position = self.source.pos();
        let mut statements = Vec::new();
        self.consume_string(b"{")?;
        self.consume_whitespace(false)?;
        while self.has_local_statement() {
            let (local_id, stmt_id) = self.consume_local_statement(h)?;
            statements.push(local_id.upcast());
            if let Some(stmt_id) = stmt_id {
                statements.push(stmt_id.upcast());
            }
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
    fn consume_local_statement(&mut self, h: &mut Heap) -> Result<(LocalStatementId, Option<ExpressionStatementId>), ParseError> {
        if self.has_keyword(b"channel") {
            let local_id = self.consume_channel_statement(h)?.upcast();
            Ok((local_id, None))
        } else {
            let (memory_id, stmt_id) = self.consume_memory_statement(h)?;
            Ok((memory_id.upcast(), Some(stmt_id)))
        }
    }
    fn consume_channel_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<ChannelStatementId, ParseError> {
        // Consume channel statement and polymorphic argument if specified.
        // Needs a tiny bit of special parsing to ensure the right amount of
        // whitespace is present.
        let position = self.source.pos();
        self.consume_keyword(b"channel")?;

        let expect_whitespace = self.source.next() != Some(b'<');
        self.consume_whitespace(expect_whitespace)?;
        let poly_args = self.consume_polymorphic_args(h, true)?.unwrap_or_default();
        let poly_arg_id = match poly_args.len() {
            0 => h.alloc_parser_type(|this| ParserType{
                this, pos: position.clone(), variant: ParserTypeVariant::Inferred,
            }),
            1 => poly_args[0],
            _ => return Err(ParseError::new_error(
                &self.source, self.source.pos(),
                "port construction using 'channel' accepts up to 1 polymorphic argument"
            ))
        };
        self.consume_whitespace(false)?;

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
    fn consume_memory_statement(&mut self, h: &mut Heap) -> Result<(MemoryStatementId, ExpressionStatementId), ParseError> {
        let position = self.source.pos();
        let parser_type = self.consume_type(h, true)?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let assignment_position = self.source.pos();
        self.consume_string(b"=")?;
        self.consume_whitespace(false)?;
        let initial = self.consume_expression(h)?;
        let variable = h.alloc_local(|this| Local {
            this,
            position,
            parser_type,
            identifier: identifier.clone(),
            relative_pos_in_block: 0
        });
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;

        // Transform into the variable declaration, followed by an assignment
        let memory_stmt_id = h.alloc_memory_statement(|this| MemoryStatement {
            this,
            position,
            variable,
            next: None,
        });
        let variable_expr_id = h.alloc_variable_expression(|this| VariableExpression{
            this,
            position: identifier.position.clone(),
            identifier: identifier_as_namespaced(identifier),
            declaration: None,
            parent: ExpressionParent::None,
            concrete_type: Default::default()
        });
        let assignment_expr_id = h.alloc_assignment_expression(|this| AssignmentExpression{
            this,
            position: assignment_position,
            left: variable_expr_id.upcast(),
            operation: AssignmentOperator::Set,
            right: initial,
            parent: ExpressionParent::None,
            concrete_type: Default::default()
        });
        let assignment_stmt_id = h.alloc_expression_statement(|this| ExpressionStatement{
            this,
            position,
            expression: assignment_expr_id.upcast(),
            next: None
        });
        Ok((memory_stmt_id, assignment_stmt_id))
    }
    fn consume_labeled_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<LabeledStatementId, ParseError> {
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
    fn consume_skip_statement(&mut self, h: &mut Heap) -> Result<SkipStatementId, ParseError> {
        let position = self.source.pos();
        self.consume_keyword(b"skip")?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_skip_statement(|this| SkipStatement { this, position, next: None }))
    }
    fn consume_if_statement(&mut self, h: &mut Heap) -> Result<IfStatementId, ParseError> {
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
    fn consume_while_statement(&mut self, h: &mut Heap) -> Result<WhileStatementId, ParseError> {
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
    fn consume_break_statement(&mut self, h: &mut Heap) -> Result<BreakStatementId, ParseError> {
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
    ) -> Result<ContinueStatementId, ParseError> {
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
    ) -> Result<SynchronousStatementId, ParseError> {
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
    fn consume_return_statement(&mut self, h: &mut Heap) -> Result<ReturnStatementId, ParseError> {
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
    fn consume_assert_statement(&mut self, h: &mut Heap) -> Result<AssertStatementId, ParseError> {
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
    fn consume_goto_statement(&mut self, h: &mut Heap) -> Result<GotoStatementId, ParseError> {
        let position = self.source.pos();
        self.consume_keyword(b"goto")?;
        self.consume_whitespace(false)?;
        let label = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_goto_statement(|this| GotoStatement { this, position, label, target: None }))
    }
    fn consume_new_statement(&mut self, h: &mut Heap) -> Result<NewStatementId, ParseError> {
        let position = self.source.pos();
        self.consume_keyword(b"new")?;
        self.consume_whitespace(false)?;
        let expression = self.consume_call_expression(h)?;
        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(h.alloc_new_statement(|this| NewStatement { this, position, expression, next: None }))
    }
    fn consume_expression_statement(
        &mut self,
        h: &mut Heap,
    ) -> Result<ExpressionStatementId, ParseError> {
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
    fn consume_symbol_definition(&mut self, h: &mut Heap) -> Result<DefinitionId, ParseError> {
        if self.has_keyword(b"struct") {
            Ok(self.consume_struct_definition(h)?.upcast())
        } else if self.has_keyword(b"enum") {
            Ok(self.consume_enum_definition(h)?.upcast())
        } else if self.has_keyword(b"union") {
            Ok(self.consume_union_definition(h)?.upcast())
        } else if self.has_keyword(b"composite") || self.has_keyword(b"primitive") {
            Ok(self.consume_component_definition(h)?.upcast())
        } else {
            Ok(self.consume_function_definition(h)?.upcast())
        }
    }
    fn consume_struct_definition(&mut self, h: &mut Heap) -> Result<StructId, ParseError> {
        // Parse "struct" keyword, optional polyvars and its identifier
        let struct_pos = self.source.pos();
        self.consume_keyword(b"struct")?;
        self.consume_whitespace(true)?;
        let struct_ident = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        // Parse struct fields
        let fields = match self.consume_comma_separated(
            h, b'{', b'}', "Expected the end of the list of struct fields",
            |lexer, heap| {
                let position = lexer.source.pos();
                let parser_type = lexer.consume_type(heap, false)?;
                lexer.consume_whitespace(true)?;
                let field = lexer.consume_identifier()?;

                Ok(StructFieldDefinition{ position, field, parser_type })
            }
        )? {
            Some(fields) => fields,
            None => return Err(ParseError::new_error(
                self.source, struct_pos,
                "An struct definition must be followed by its fields"
            )),
        };

        // Valid struct definition
        Ok(h.alloc_struct_definition(|this| StructDefinition{
            this,
            position: struct_pos,
            identifier: struct_ident,
            poly_vars,
            fields,
        }))
    }
    fn consume_enum_definition(&mut self, h: &mut Heap) -> Result<EnumId, ParseError> {
        // Parse "enum" keyword, optional polyvars and its identifier
        let enum_pos = self.source.pos();
        self.consume_keyword(b"enum")?;
        self.consume_whitespace(true)?;
        let enum_ident = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        let variants = match self.consume_comma_separated(
            h, b'{', b'}', "Expected end of enum variant list",
            |lexer, heap| {
                // Variant identifier
                let position = lexer.source.pos();
                let identifier = lexer.consume_identifier()?;
                lexer.consume_whitespace(false)?;

                // Optional variant value/type
                let next = lexer.source.next();
                let value = match next {
                    Some(b',') => {
                        // Do not consume, let `consume_comma_separated` handle
                        // the next item
                        EnumVariantValue::None
                    },
                    Some(b'=') => {
                        // Integer value
                        lexer.source.consume();
                        lexer.consume_whitespace(false)?;
                        if !lexer.has_integer() {
                            return Err(lexer.error_at_pos("expected integer"))
                        }
                        let value = lexer.consume_integer()?;
                        EnumVariantValue::Integer(value)
                    },
                    Some(b'}') => {
                        // End of enum
                        EnumVariantValue::None
                    }
                    _ => {
                        return Err(lexer.error_at_pos("Expected ',', '}' or '='"));
                    }
                };

                Ok(EnumVariantDefinition{ position, identifier, value })
            }
        )? {
            Some(variants) => variants,
            None => return Err(ParseError::new_error(
                self.source, enum_pos,
                "An enum definition must be followed by its variants"
            )),
        };

        Ok(h.alloc_enum_definition(|this| EnumDefinition{
            this,
            position: enum_pos,
            identifier: enum_ident,
            poly_vars,
            variants,
        }))
    }
    fn consume_union_definition(&mut self, h: &mut Heap) -> Result<UnionId, ParseError> {
        // Parse "union" keyword, optional polyvars and the identifier
        let union_pos = self.source.pos();
        self.consume_keyword(b"union")?;
        self.consume_whitespace(true)?;
        let union_ident = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        let variants = match self.consume_comma_separated(
            h, b'{', b'}', "Expected end of union variant list",
            |lexer, heap| {
                // Variant identifier
                let position = lexer.source.pos();
                let identifier = lexer.consume_identifier()?;
                lexer.consume_whitespace(false)?;

                // Optional variant value
                let next = lexer.source.next();
                let value = match next {
                    Some(b',') | Some(b'}') => {
                        // Continue parsing using `consume_comma_separated`
                        UnionVariantValue::None
                    },
                    Some(b'(') => {
                        // Embedded type(s)
                        let embedded = lexer.consume_comma_separated(
                            heap, b'(', b')', "Expected end of embedded type list of union variant",
                            |lexer, heap| {
                                lexer.consume_type(heap, false)
                            }
                        )?.unwrap();

                        if embedded.is_empty() {
                            return Err(lexer.error_at_pos("Expected at least one embedded type"));
                        }

                        UnionVariantValue::Embedded(embedded)
                    },
                    _ => {
                        return Err(lexer.error_at_pos("Expected ',', '}' or '('"));
                    },
                };

                Ok(UnionVariantDefinition{ position, identifier, value })
            }
        )? {
            Some(variants) => variants,
            None => return Err(ParseError::new_error(
                self.source, union_pos,
                "A union definition must be followed by its variants"
            )),
        };

        Ok(h.alloc_union_definition(|this| UnionDefinition{
            this,
            position: union_pos,
            identifier: union_ident,
            poly_vars,
            variants,
        }))
    }
    fn consume_component_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError> {
        // TODO: Cleanup
        if self.has_keyword(b"composite") {
            Ok(self.consume_composite_definition(h)?)
        } else {
            Ok(self.consume_primitive_definition(h)?)
        }
    }
    fn consume_composite_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError> {
        // Parse keyword, optional polyvars and the identifier
        let position = self.source.pos();
        self.consume_keyword(b"composite")?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let parameters = self.consume_parameters(h)?;
        self.consume_whitespace(false)?;

        // Parse body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_component(|this| ComponentDefinition {
            this,
            variant: ComponentVariant::Composite,
            position,
            identifier,
            poly_vars,
            parameters,
            body
        }))
    }
    fn consume_primitive_definition(&mut self, h: &mut Heap) -> Result<ComponentId, ParseError> {
        // Consume keyword, optional polyvars and identifier
        let position = self.source.pos();
        self.consume_keyword(b"primitive")?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let parameters = self.consume_parameters(h)?;
        self.consume_whitespace(false)?;

        // Consume body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_component(|this| ComponentDefinition {
            this,
            variant: ComponentVariant::Primitive,
            position,
            identifier,
            poly_vars,
            parameters,
            body
        }))
    }
    fn consume_function_definition(&mut self, h: &mut Heap) -> Result<FunctionId, ParseError> {
        // Consume return type, optional polyvars and identifier
        let position = self.source.pos();
        let return_type = self.consume_type(h, false)?;
        self.consume_whitespace(true)?;
        let identifier = self.consume_identifier()?;
        self.consume_whitespace(false)?;
        let poly_vars = self.consume_polymorphic_vars(h)?;
        self.consume_whitespace(false)?;

        // Consume parameters
        let parameters = self.consume_parameters(h)?;
        self.consume_whitespace(false)?;

        // Consume body
        let body = self.consume_block_statement(h)?;
        Ok(h.alloc_function(|this| FunctionDefinition {
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
    fn consume_pragma(&mut self, h: &mut Heap) -> Result<PragmaId, ParseError> {
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
    fn consume_import(&mut self, h: &mut Heap) -> Result<ImportId, ParseError> {
        // Parse the word "import" and the name of the module
        let position = self.source.pos();
        self.consume_keyword(b"import")?;
        self.consume_whitespace(true)?;
        let mut value = Vec::new();
        let mut last_ident_pos = self.source.pos();
        let mut ident = self.consume_ident()?;
        value.append(&mut ident);
        let mut last_ident_start = 0;

        while self.has_string(b".") {
            self.consume_string(b".")?;
            value.push(b'.');
            last_ident_pos = self.source.pos();
            ident = self.consume_ident()?;
            last_ident_start = value.len();
            value.append(&mut ident);
        }


        self.consume_whitespace(false)?;

        // Check for the potential aliasing or specific module imports
        let import = if self.has_string(b"as") {
            self.consume_string(b"as")?;
            self.consume_whitespace(true)?;
            let alias = self.consume_identifier()?;

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

            let next = self.source.next();
            if Some(b'{') == next {
                let symbols = match self.consume_comma_separated(
                    h, b'{', b'}', "Expected end of import list",
                    |lexer, _heap| {
                        // Symbol name
                        let position = lexer.source.pos();
                        let name = lexer.consume_identifier()?;
                        lexer.consume_whitespace(false)?;

                        // Symbol alias
                        if lexer.has_string(b"as") {
                            // With alias
                            lexer.consume_string(b"as")?;
                            lexer.consume_whitespace(true)?;
                            let alias = lexer.consume_identifier()?;

                            Ok(AliasedSymbol{
                                position,
                                name,
                                alias,
                                definition_id: None
                            })
                        } else {
                            // Without alias
                            Ok(AliasedSymbol{
                                position,
                                name: name.clone(),
                                alias: name,
                                definition_id: None
                            })
                        }
                    }
                )? {
                    Some(symbols) => symbols,
                    None => unreachable!(), // because we checked for opening '{'
                };

                h.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    position,
                    module_name: value,
                    module_id: None,
                    symbols,
                }))
            } else if Some(b'*') == next {
                self.source.consume();
                h.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    position,
                    module_name: value,
                    module_id: None,
                    symbols: Vec::new()
                }))
            } else if self.has_identifier() {
                let position = self.source.pos();
                let name = self.consume_identifier()?;
                self.consume_whitespace(false)?;
                let alias = if self.has_string(b"as") {
                    self.consume_string(b"as")?;
                    self.consume_whitespace(true)?;
                    self.consume_identifier()?
                } else {
                    name.clone()
                };

                h.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    position,
                    module_name: value,
                    module_id: None,
                    symbols: vec![AliasedSymbol{
                        position,
                        name,
                        alias,
                        definition_id: None
                    }]
                }))
            } else {
                return Err(self.error_at_pos("Expected '*', '{' or a symbol name"));
            }
        } else {
            // No explicit alias or subimports, so implicit alias
            let alias_value = Vec::from(&value[last_ident_start..]);
            h.alloc_import(|this| Import::Module(ImportModule{
                this,
                position,
                module_name: value,
                alias: Identifier{
                    position: last_ident_pos,
                    value: Vec::from(alias_value),
                },
                module_id: None,
            }))
        };

        self.consume_whitespace(false)?;
        self.consume_string(b";")?;
        Ok(import)
    }
    pub fn consume_protocol_description(&mut self, h: &mut Heap) -> Result<RootId, ParseError> {
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

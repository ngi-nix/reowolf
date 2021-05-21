use crate::collections::ScopedSection;
use crate::protocol::ast::*;
use crate::protocol::input_source::{
    InputSource as InputSource,
    InputPosition as InputPosition,
    InputSpan,
    ParseError,
};
use super::tokens::*;
use super::symbol_table::*;
use super::{Module, PassCtx};

// Keywords
pub(crate) const KW_LET:       &'static [u8] = b"let";
pub(crate) const KW_AS:        &'static [u8] = b"as";
pub(crate) const KW_STRUCT:    &'static [u8] = b"struct";
pub(crate) const KW_ENUM:      &'static [u8] = b"enum";
pub(crate) const KW_UNION:     &'static [u8] = b"union";
pub(crate) const KW_FUNCTION:  &'static [u8] = b"func";
pub(crate) const KW_PRIMITIVE: &'static [u8] = b"primitive";
pub(crate) const KW_COMPOSITE: &'static [u8] = b"composite";
pub(crate) const KW_IMPORT:    &'static [u8] = b"import";

// Keywords - literals
pub(crate) const KW_LIT_TRUE:  &'static [u8] = b"true";
pub(crate) const KW_LIT_FALSE: &'static [u8] = b"false";
pub(crate) const KW_LIT_NULL:  &'static [u8] = b"null";

// Keywords - function(like)s
pub(crate) const KW_CAST:        &'static [u8] = b"cast";
pub(crate) const KW_FUNC_GET:    &'static [u8] = b"get";
pub(crate) const KW_FUNC_PUT:    &'static [u8] = b"put";
pub(crate) const KW_FUNC_FIRES:  &'static [u8] = b"fires";
pub(crate) const KW_FUNC_CREATE: &'static [u8] = b"create";
pub(crate) const KW_FUNC_LENGTH: &'static [u8] = b"length";
pub(crate) const KW_FUNC_ASSERT: &'static [u8] = b"assert";

// Keywords - statements
pub(crate) const KW_STMT_CHANNEL:  &'static [u8] = b"channel";
pub(crate) const KW_STMT_IF:       &'static [u8] = b"if";
pub(crate) const KW_STMT_ELSE:     &'static [u8] = b"else";
pub(crate) const KW_STMT_WHILE:    &'static [u8] = b"while";
pub(crate) const KW_STMT_BREAK:    &'static [u8] = b"break";
pub(crate) const KW_STMT_CONTINUE: &'static [u8] = b"continue";
pub(crate) const KW_STMT_GOTO:     &'static [u8] = b"goto";
pub(crate) const KW_STMT_RETURN:   &'static [u8] = b"return";
pub(crate) const KW_STMT_SYNC:     &'static [u8] = b"synchronous";
pub(crate) const KW_STMT_NEW:      &'static [u8] = b"new";

// Keywords - types
// Since types are needed for returning diagnostic information to the user, the
// string variants are put here as well.
pub(crate) const KW_TYPE_IN_PORT_STR:  &'static str = "in";
pub(crate) const KW_TYPE_OUT_PORT_STR: &'static str = "out";
pub(crate) const KW_TYPE_MESSAGE_STR:  &'static str = "msg";
pub(crate) const KW_TYPE_BOOL_STR:     &'static str = "bool";
pub(crate) const KW_TYPE_UINT8_STR:    &'static str = "u8";
pub(crate) const KW_TYPE_UINT16_STR:   &'static str = "u16";
pub(crate) const KW_TYPE_UINT32_STR:   &'static str = "u32";
pub(crate) const KW_TYPE_UINT64_STR:   &'static str = "u64";
pub(crate) const KW_TYPE_SINT8_STR:    &'static str = "s8";
pub(crate) const KW_TYPE_SINT16_STR:   &'static str = "s16";
pub(crate) const KW_TYPE_SINT32_STR:   &'static str = "s32";
pub(crate) const KW_TYPE_SINT64_STR:   &'static str = "s64";
pub(crate) const KW_TYPE_CHAR_STR:     &'static str = "char";
pub(crate) const KW_TYPE_STRING_STR:   &'static str = "string";
pub(crate) const KW_TYPE_INFERRED_STR: &'static str = "auto";

pub(crate) const KW_TYPE_IN_PORT:  &'static [u8] = KW_TYPE_IN_PORT_STR.as_bytes();
pub(crate) const KW_TYPE_OUT_PORT: &'static [u8] = KW_TYPE_OUT_PORT_STR.as_bytes();
pub(crate) const KW_TYPE_MESSAGE:  &'static [u8] = KW_TYPE_MESSAGE_STR.as_bytes();
pub(crate) const KW_TYPE_BOOL:     &'static [u8] = KW_TYPE_BOOL_STR.as_bytes();
pub(crate) const KW_TYPE_UINT8:    &'static [u8] = KW_TYPE_UINT8_STR.as_bytes();
pub(crate) const KW_TYPE_UINT16:   &'static [u8] = KW_TYPE_UINT16_STR.as_bytes();
pub(crate) const KW_TYPE_UINT32:   &'static [u8] = KW_TYPE_UINT32_STR.as_bytes();
pub(crate) const KW_TYPE_UINT64:   &'static [u8] = KW_TYPE_UINT64_STR.as_bytes();
pub(crate) const KW_TYPE_SINT8:    &'static [u8] = KW_TYPE_SINT8_STR.as_bytes();
pub(crate) const KW_TYPE_SINT16:   &'static [u8] = KW_TYPE_SINT16_STR.as_bytes();
pub(crate) const KW_TYPE_SINT32:   &'static [u8] = KW_TYPE_SINT32_STR.as_bytes();
pub(crate) const KW_TYPE_SINT64:   &'static [u8] = KW_TYPE_SINT64_STR.as_bytes();
pub(crate) const KW_TYPE_CHAR:     &'static [u8] = KW_TYPE_CHAR_STR.as_bytes();
pub(crate) const KW_TYPE_STRING:   &'static [u8] = KW_TYPE_STRING_STR.as_bytes();
pub(crate) const KW_TYPE_INFERRED: &'static [u8] = KW_TYPE_INFERRED_STR.as_bytes();

/// A special trait for when consuming comma-separated things such that we can
/// push them onto a `Vec` and onto a `ScopedSection`. As we monomorph for
/// very specific comma-separated cases I don't expect polymorph bloat.
/// Also, I really don't like this solution.
pub(crate) trait Extendable {
    type Value;

    fn push(&mut self, v: Self::Value);
}

impl<T> Extendable for Vec<T> {
    type Value = T;

    #[inline]
    fn push(&mut self, v: Self::Value) {
        (self as &mut Vec<T>).push(v);
    }
}

impl<T: Sized> Extendable for ScopedSection<T> {
    type Value = T;

    #[inline]
    fn push(&mut self, v: Self::Value) {
        (self as &mut ScopedSection<T>).push(v);
    }
}

/// Consumes a domain-name identifier: identifiers separated by a dot. For
/// simplification of later parsing and span identification the domain-name may
/// contain whitespace, but must reside on the same line.
pub(crate) fn consume_domain_ident<'a>(
    source: &'a InputSource, iter: &mut TokenIter
) -> Result<(&'a [u8], InputSpan), ParseError> {
    let (_, mut span) = consume_ident(source, iter)?;
    while let Some(TokenKind::Dot) = iter.next() {
        iter.consume();
        let (_, new_span) = consume_ident(source, iter)?;
        span.end = new_span.end;
    }

    // Not strictly necessary, but probably a reasonable restriction: this
    // simplifies parsing of module naming and imports.
    if span.begin.line != span.end.line {
        return Err(ParseError::new_error_str_at_span(source, span, "module names may not span multiple lines"));
    }

    // If module name consists of a single identifier, then it may not match any
    // of the reserved keywords
    let section = source.section_at_pos(span.begin, span.end);
    if is_reserved_keyword(section) {
        return Err(ParseError::new_error_str_at_span(source, span, "encountered reserved keyword"));
    }

    Ok((source.section_at_pos(span.begin, span.end), span))
}

/// Consumes a specific expected token. Be careful to only call this with tokens
/// that do not have a variable length.
pub(crate) fn consume_token(source: &InputSource, iter: &mut TokenIter, expected: TokenKind) -> Result<InputSpan, ParseError> {
    if Some(expected) != iter.next() {
        return Err(ParseError::new_error_at_pos(
            source, iter.last_valid_pos(),
            format!("expected '{}'", expected.token_chars())
        ));
    }
    let span = iter.next_span();
    iter.consume();
    Ok(span)
}

/// Consumes a comma separated list until the closing delimiter is encountered
pub(crate) fn consume_comma_separated_until<T, F, E>(
    close_delim: TokenKind, source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx,
    mut consumer_fn: F, target: &mut E, item_name_and_article: &'static str,
    close_pos: Option<&mut InputPosition>
) -> Result<(), ParseError>
    where F: FnMut(&InputSource, &mut TokenIter, &mut PassCtx) -> Result<T, ParseError>,
          E: Extendable<Value=T>
{
    let mut had_comma = true;
    let mut next;
    loop {
        next = iter.next();
        if Some(close_delim) == next {
            if let Some(close_pos) = close_pos {
                // If requested return the position of the closing delimiter
                let (_, new_close_pos) = iter.next_positions();
                *close_pos = new_close_pos;
            }
            iter.consume();
            break;
        } else if !had_comma || next.is_none() {
            return Err(ParseError::new_error_at_pos(
                source, iter.last_valid_pos(),
                format!("expected a '{}', or {}", close_delim.token_chars(), item_name_and_article)
            ));
        }

        let new_item = consumer_fn(source, iter, ctx)?;
        target.push(new_item);

        next = iter.next();
        had_comma = next == Some(TokenKind::Comma);
        if had_comma {
            iter.consume();
        }
    }

    Ok(())
}

/// Consumes a comma-separated list of items if the opening delimiting token is
/// encountered. If not, then the iterator will remain at its current position.
/// Note that the potential cases may be:
/// - No opening delimiter encountered, then we return `false`.
/// - Both opening and closing delimiter encountered, but no items.
/// - Opening and closing delimiter encountered, and items were processed.
/// - Found an opening delimiter, but processing an item failed.
pub(crate) fn maybe_consume_comma_separated<T, F, E>(
    open_delim: TokenKind, close_delim: TokenKind, source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx,
    consumer_fn: F, target: &mut E, item_name_and_article: &'static str,
    close_pos: Option<&mut InputPosition>
) -> Result<bool, ParseError>
    where F: FnMut(&InputSource, &mut TokenIter, &mut PassCtx) -> Result<T, ParseError>,
          E: Extendable<Value=T>
{
    if Some(open_delim) != iter.next() {
        return Ok(false);
    }

    // Opening delimiter encountered, so must parse the comma-separated list.
    iter.consume();
    consume_comma_separated_until(close_delim, source, iter, ctx, consumer_fn, target, item_name_and_article, close_pos)?;

    Ok(true)
}

pub(crate) fn maybe_consume_comma_separated_spilled<F: FnMut(&InputSource, &mut TokenIter, &mut PassCtx) -> Result<(), ParseError>>(
    open_delim: TokenKind, close_delim: TokenKind, source: &InputSource,
    iter: &mut TokenIter, ctx: &mut PassCtx,
    mut consumer_fn: F, item_name_and_article: &'static str
) -> Result<bool, ParseError> {
    let mut next = iter.next();
    if Some(open_delim) != next {
        return Ok(false);
    }

    iter.consume();
    let mut had_comma = true;
    loop {
        next = iter.next();
        if Some(close_delim) == next {
            iter.consume();
            break;
        } else if !had_comma {
            return Err(ParseError::new_error_at_pos(
                source, iter.last_valid_pos(),
                format!("expected a '{}', or {}", close_delim.token_chars(), item_name_and_article)
            ));
        }

        consumer_fn(source, iter, ctx)?;
        next = iter.next();
        had_comma = next == Some(TokenKind::Comma);
        if had_comma {
            iter.consume();
        }
    }

    Ok(true)
}

/// Consumes a comma-separated list and expected the opening and closing
/// characters to be present. The returned array may still be empty
pub(crate) fn consume_comma_separated<T, F, E>(
    open_delim: TokenKind, close_delim: TokenKind, source: &InputSource,
    iter: &mut TokenIter, ctx: &mut PassCtx,
    consumer_fn: F, target: &mut E, item_name_and_article: &'static str,
    list_name_and_article: &'static str, close_pos: Option<&mut InputPosition>
) -> Result<(), ParseError>
    where F: FnMut(&InputSource, &mut TokenIter, &mut PassCtx) -> Result<T, ParseError>,
          E: Extendable<Value=T>
{
    let first_pos = iter.last_valid_pos();
    match maybe_consume_comma_separated(
        open_delim, close_delim, source, iter, ctx, consumer_fn, target,
        item_name_and_article, close_pos
    ) {
        Ok(true) => Ok(()),
        Ok(false) => {
            return Err(ParseError::new_error_at_pos(
                source, first_pos,
                format!("expected {}", list_name_and_article)
            ));
        },
        Err(err) => Err(err)
    }
}

/// Consumes an integer literal, may be binary, octal, hexadecimal or decimal,
/// and may have separating '_'-characters.
/// TODO: @Cleanup, @Performance
pub(crate) fn consume_integer_literal(source: &InputSource, iter: &mut TokenIter, buffer: &mut String) -> Result<(u64, InputSpan), ParseError> {
    if Some(TokenKind::Integer) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected an integer literal"));
    }
    let integer_span = iter.next_span();
    iter.consume();

    let integer_text = source.section_at_span(integer_span);

    // Determine radix and offset from prefix
    let (radix, input_offset, radix_name) =
        if integer_text.starts_with(b"0b") || integer_text.starts_with(b"0B") {
            // Binary number
            (2, 2, "binary")
        } else if integer_text.starts_with(b"0o") || integer_text.starts_with(b"0O") {
            // Octal number
            (8, 2, "octal")
        } else if integer_text.starts_with(b"0x") || integer_text.starts_with(b"0X") {
            // Hexadecimal number
            (16, 2, "hexadecimal")
        } else {
            (10, 0, "decimal")
        };

    // Take out any of the separating '_' characters
    buffer.clear();
    for char_idx in input_offset..integer_text.len() {
        let char = integer_text[char_idx];
        if char == b'_' {
            continue;
        }

        if !((char >= b'0' && char <= b'9') || (char >= b'A' && char <= b'F') || (char >= b'a' || char <= b'f')) {
            return Err(ParseError::new_error_at_span(
                source, integer_span,
                format!("incorrectly formatted {} number", radix_name)
            ));
        }
        buffer.push(char::from(char));
    }

    // Use the cleaned up string to convert to integer
    match u64::from_str_radix(&buffer, radix) {
        Ok(number) => Ok((number, integer_span)),
        Err(_) => Err(ParseError::new_error_at_span(
            source, integer_span,
            format!("incorrectly formatted {} number", radix_name)
        )),
    }
}

/// Consumes a character literal. We currently support a limited number of
/// backslash-escaped characters
pub(crate) fn consume_character_literal(
    source: &InputSource, iter: &mut TokenIter
) -> Result<(char, InputSpan), ParseError> {
    if Some(TokenKind::Character) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected a character literal"));
    }
    let span = iter.next_span();
    iter.consume();

    let char_text = source.section_at_span(span);
    if !char_text.is_ascii() {
        return Err(ParseError::new_error_str_at_span(
            source, span, "expected an ASCII character literal"
        ));
    }

    match char_text.len() {
        0 => return Err(ParseError::new_error_str_at_span(source, span, "too little characters in character literal")),
        1 => {
            // We already know the text is ascii, so just throw an error if we have the escape
            // character.
            if char_text[0] == b'\\' {
                return Err(ParseError::new_error_str_at_span(source, span, "escape character without subsequent character"));
            }
            return Ok((char_text[0] as char, span));
        },
        2 => {
            if char_text[0] == b'\\' {
                let result = parse_escaped_character(source, iter.last_valid_pos(), char_text[1])?;
                return Ok((result, span))
            }
        },
        _ => {}
    }

    return Err(ParseError::new_error_str_at_span(source, span, "too many characters in character literal"))
}

/// Consumes a string literal. We currently support a limited number of
/// backslash-escaped characters. Note that the result is stored in the
/// buffer.
pub(crate) fn consume_string_literal(
    source: &InputSource, iter: &mut TokenIter, buffer: &mut String
) -> Result<InputSpan, ParseError> {
    if Some(TokenKind::String) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected a string literal"));
    }

    buffer.clear();
    let span = iter.next_span();
    iter.consume();

    let text = source.section_at_span(span);
    if !text.is_ascii() {
        return Err(ParseError::new_error_str_at_span(source, span, "expected an ASCII string literal"));
    }
    buffer.reserve(text.len());

    let mut was_escape = false;
    for idx in 0..text.len() {
        let cur = text[idx];
        if cur != b'\\' {
            if was_escape {
                let to_push = parse_escaped_character(source, iter.last_valid_pos(), cur)?;
                buffer.push(to_push);
            } else {
                buffer.push(cur as char);
            }
            was_escape = false;
        } else {
            was_escape = true;
        }
    }

    debug_assert!(!was_escape); // because otherwise we couldn't have ended the string literal

    Ok(span)
}

fn parse_escaped_character(source: &InputSource, pos: InputPosition, v: u8) -> Result<char, ParseError> {
    let result = match v {
        b'r' => '\r',
        b'n' => '\n',
        b't' => '\t',
        b'0' => '\0',
        b'\\' => '\\',
        b'\'' => '\'',
        b'"' => '"',
        v => return Err(ParseError::new_error_at_pos(
            source, pos, format!("unexpected escaped character '{}'", v)
        )),
    };
    Ok(result)
}

pub(crate) fn consume_pragma<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputPosition, InputPosition), ParseError> {
    if Some(TokenKind::Pragma) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected a pragma"));
    }
    let (pragma_start, pragma_end) = iter.next_positions();
    iter.consume();
    Ok((source.section_at_pos(pragma_start, pragma_end), pragma_start, pragma_end))
}

pub(crate) fn has_ident(source: &InputSource, iter: &mut TokenIter, expected: &[u8]) -> bool {
    peek_ident(source, iter).map_or(false, |section| section == expected)
}

pub(crate) fn peek_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Option<&'a [u8]> {
    if Some(TokenKind::Ident) == iter.next() {
        let (start, end) = iter.next_positions();
        return Some(source.section_at_pos(start, end))
    }

    None
}

/// Consumes any identifier and returns it together with its span. Does not
/// check if the identifier is a reserved keyword.
pub(crate) fn consume_any_ident<'a>(
    source: &'a InputSource, iter: &mut TokenIter
) -> Result<(&'a [u8], InputSpan), ParseError> {
    if Some(TokenKind::Ident) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected an identifier"));
    }
    let (ident_start, ident_end) = iter.next_positions();
    iter.consume();
    Ok((source.section_at_pos(ident_start, ident_end), InputSpan::from_positions(ident_start, ident_end)))
}

/// Consumes a specific identifier. May or may not be a reserved keyword.
pub(crate) fn consume_exact_ident(source: &InputSource, iter: &mut TokenIter, expected: &[u8]) -> Result<InputSpan, ParseError> {
    let (ident, pos) = consume_any_ident(source, iter)?;
    if ident != expected {
        debug_assert!(expected.is_ascii());
        return Err(ParseError::new_error_at_pos(
            source, iter.last_valid_pos(),
            format!("expected the text '{}'", &String::from_utf8_lossy(expected))
        ));
    }
    Ok(pos)
}

/// Consumes an identifier that is not a reserved keyword and returns it
/// together with its span.
pub(crate) fn consume_ident<'a>(
    source: &'a InputSource, iter: &mut TokenIter
) -> Result<(&'a [u8], InputSpan), ParseError> {
    let (ident, span) = consume_any_ident(source, iter)?;
    if is_reserved_keyword(ident) {
        return Err(ParseError::new_error_str_at_span(source, span, "encountered reserved keyword"));
    }

    Ok((ident, span))
}

/// Consumes an identifier and immediately intern it into the `StringPool`
pub(crate) fn consume_ident_interned(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx
) -> Result<Identifier, ParseError> {
    let (value, span) = consume_ident(source, iter)?;
    let value = ctx.pool.intern(value);
    Ok(Identifier{ span, value })
}

fn is_reserved_definition_keyword(text: &[u8]) -> bool {
    match text {
        KW_STRUCT | KW_ENUM | KW_UNION | KW_FUNCTION | KW_PRIMITIVE | KW_COMPOSITE => true,
        _ => false,
    }
}

fn is_reserved_statement_keyword(text: &[u8]) -> bool {
    match text {
        KW_IMPORT | KW_AS |
        KW_STMT_CHANNEL | KW_STMT_IF | KW_STMT_WHILE |
        KW_STMT_BREAK | KW_STMT_CONTINUE | KW_STMT_GOTO | KW_STMT_RETURN |
        KW_STMT_SYNC | KW_STMT_NEW => true,
        _ => false,
    }
}

fn is_reserved_expression_keyword(text: &[u8]) -> bool {
    match text {
        KW_LET | KW_CAST |
        KW_LIT_TRUE | KW_LIT_FALSE | KW_LIT_NULL |
        KW_FUNC_GET | KW_FUNC_PUT | KW_FUNC_FIRES | KW_FUNC_CREATE | KW_FUNC_ASSERT | KW_FUNC_LENGTH => true,
        _ => false,
    }
}

fn is_reserved_type_keyword(text: &[u8]) -> bool {
    match text {
        KW_TYPE_IN_PORT | KW_TYPE_OUT_PORT | KW_TYPE_MESSAGE | KW_TYPE_BOOL |
        KW_TYPE_UINT8 | KW_TYPE_UINT16 | KW_TYPE_UINT32 | KW_TYPE_UINT64 |
        KW_TYPE_SINT8 | KW_TYPE_SINT16 | KW_TYPE_SINT32 | KW_TYPE_SINT64 |
        KW_TYPE_CHAR | KW_TYPE_STRING |
        KW_TYPE_INFERRED => true,
        _ => false,
    }
}

fn is_reserved_keyword(text: &[u8]) -> bool {
    return
        is_reserved_definition_keyword(text) ||
        is_reserved_statement_keyword(text) ||
        is_reserved_expression_keyword(text) ||
        is_reserved_type_keyword(text);
}

pub(crate) fn seek_module(modules: &[Module], root_id: RootId) -> Option<&Module> {
    for module in modules {
        if module.root_id == root_id {
            return Some(module)
        }
    }

    return None
}

/// Constructs a human-readable message indicating why there is a conflict of
/// symbols.
// Note: passing the `module_idx` is not strictly necessary, but will prevent
// programmer mistakes during development: we get a conflict because we're
// currently parsing a particular module.
pub(crate) fn construct_symbol_conflict_error(
    modules: &[Module], module_idx: usize, ctx: &PassCtx, new_symbol: &Symbol, old_symbol: &Symbol
) -> ParseError {
    let module = &modules[module_idx];
    let get_symbol_span_and_msg = |symbol: &Symbol| -> (String, Option<InputSpan>) {
        match &symbol.variant {
            SymbolVariant::Module(module) => {
                let import = &ctx.heap[module.introduced_at];
                return (
                    format!("the module aliased as '{}' imported here", symbol.name.as_str()),
                    Some(import.as_module().span)
                );
            },
            SymbolVariant::Definition(definition) => {
                if definition.defined_in_module.is_invalid() {
                    // Must be a builtin thing
                    return (format!("the builtin '{}'", symbol.name.as_str()), None)
                } else {
                    if let Some(import_id) = definition.imported_at {
                        let import = &ctx.heap[import_id];
                        return (
                            format!("the type '{}' imported here", symbol.name.as_str()),
                            Some(import.as_symbols().span)
                        );
                    } else {
                        // This is a defined symbol. So this must mean that the
                        // error was caused by it being defined.
                        debug_assert_eq!(definition.defined_in_module, module.root_id);

                        return (
                            format!("the type '{}' defined here", symbol.name.as_str()),
                            Some(definition.identifier_span)
                        )
                    }
                }
            }
        }
    };

    let (new_symbol_msg, new_symbol_span) = get_symbol_span_and_msg(new_symbol);
    let (old_symbol_msg, old_symbol_span) = get_symbol_span_and_msg(old_symbol);
    let new_symbol_span = new_symbol_span.unwrap(); // because new symbols cannot be builtin

    match old_symbol_span {
        Some(old_symbol_span) => ParseError::new_error_at_span(
            &module.source, new_symbol_span, format!("symbol is defined twice: {}", new_symbol_msg)
        ).with_info_at_span(
            &module.source, old_symbol_span, format!("it conflicts with {}", old_symbol_msg)
        ),
        None => ParseError::new_error_at_span(
            &module.source, new_symbol_span,
            format!("symbol is defined twice: {} conflicts with {}", new_symbol_msg, old_symbol_msg)
        )
    }
}
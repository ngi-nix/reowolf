use crate::protocol::input_source::{
    InputSource as InputSource,
    ParseError,
    InputPosition as InputPosition,
};

use super::tokens::*;
use super::token_parsing::*;

/// Tokenizer is a reusable parser to tokenize multiple source files using the
/// same allocated buffers. In a well-formed program, we produce a consistent
/// tree of token ranges such that we may identify tokens that represent a
/// defintion or an import before producing the entire AST.
///
/// If the program is not well-formed then the tree may be inconsistent, but we
/// will detect this once we transform the tokens into the AST. To ensure a
/// consistent AST-producing phase we will require the import to have balanced
/// curly braces
pub(crate) struct PassTokenizer {
    // Stack of input positions of opening curly braces, used to detect
    // unmatched opening braces, unmatched closing braces are detected
    // immediately.
    curly_stack: Vec<InputPosition>,
    // Points to an element in the `TokenBuffer.ranges` variable.
    stack_idx: usize,
}

impl PassTokenizer {
    pub(crate) fn new() -> Self {
        Self{
            curly_stack: Vec::with_capacity(32),
            stack_idx: 0
        }
    }

    pub(crate) fn tokenize(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        // Assert source and buffer are at start
        debug_assert_eq!(source.pos().offset, 0);
        debug_assert!(target.tokens.is_empty());
        debug_assert!(target.ranges.is_empty());

        // Set up for tokenization by pushing the first range onto the stack.
        // This range may get transformed into the appropriate range kind later,
        // see `push_range` and `pop_range`.
        self.stack_idx = 0;
        target.ranges.push(TokenRange{
            parent_idx: 0,
            range_kind: TokenRangeKind::Module,
            curly_depth: 0,
            start: 0,
            end: 0,
            num_child_ranges: 0,
            first_child_idx: 0,
            last_child_idx: 0,
            next_sibling_idx: None,
        });

        // Main tokenization loop
        while let Some(c) = source.next() {
            let token_index = target.tokens.len() as u32;

            if is_char_literal_start(c) {
                self.consume_char_literal(source, target)?;
            } else if is_string_literal_start(c) {
                self.consume_string_literal(source, target)?;
            } else if is_identifier_start(c) {
                let ident = self.consume_identifier(source, target)?;

                if demarks_definition(ident) {
                    self.push_range(target, TokenRangeKind::Definition, token_index);
                } else if demarks_import(ident) {
                    self.push_range(target, TokenRangeKind::Import, token_index);
                }
            } else if is_integer_literal_start(c) {
                self.consume_number(source, target)?;
            } else if is_pragma_start_or_pound(c) {
                let was_pragma = self.consume_pragma_or_pound(c, source, target)?;
                if was_pragma {
                    self.push_range(target, TokenRangeKind::Pragma, token_index);
                }
            } else if self.is_line_comment_start(c, source) {
                self.consume_line_comment(source, target)?;
            } else if self.is_block_comment_start(c, source) {
                self.consume_block_comment(source, target)?;
            } else if is_whitespace(c) {
                let contained_newline = self.consume_whitespace(source);
                if contained_newline {
                    let range = &target.ranges[self.stack_idx];
                    if range.range_kind == TokenRangeKind::Pragma {
                        self.pop_range(target, target.tokens.len() as u32);
                    }
                }
            } else {
                let was_punctuation = self.maybe_parse_punctuation(c, source, target)?;
                if let Some((token, token_pos)) = was_punctuation {
                    if token == TokenKind::OpenCurly {
                        self.curly_stack.push(token_pos);
                    } else if token == TokenKind::CloseCurly {
                        // Check if this marks the end of a range we're
                        // currently processing
                        if self.curly_stack.is_empty() {
                            return Err(ParseError::new_error_str_at_pos(
                                source, token_pos, "unmatched closing curly brace '}'"
                            ));
                        }

                        self.curly_stack.pop();

                        let range = &target.ranges[self.stack_idx];
                        if range.range_kind == TokenRangeKind::Definition && range.curly_depth == self.curly_stack.len() as u32 {
                            self.pop_range(target, target.tokens.len() as u32);
                        }

                        // Exit early if we have more closing curly braces than
                        // opening curly braces
                    } else if token == TokenKind::SemiColon {
                        // Check if this marks the end of an import
                        let range = &target.ranges[self.stack_idx];
                        if range.range_kind == TokenRangeKind::Import {
                            self.pop_range(target, target.tokens.len() as u32);
                        }
                    }
                } else {
                    return Err(ParseError::new_error_str_at_pos(
                        source, source.pos(), "unexpected character"
                    ));
                }
            }
        }

        // End of file, check if our state is correct
        if let Some(error) = source.had_error.take() {
            return Err(error);
        }

        if !self.curly_stack.is_empty() {
            // Let's not add a lot of heuristics and just tell the programmer
            // that something is wrong
            let last_unmatched_open = self.curly_stack.pop().unwrap();
            return Err(ParseError::new_error_str_at_pos(
                source, last_unmatched_open, "unmatched opening curly brace '{'"
            ));
        }

        // TODO: @remove once I'm sure the algorithm works. For now it is better
        //  if the debugging is a little more expedient
        if cfg!(debug_assertions) {
            // For each range make sure its children make sense
            for parent_idx in 0..target.ranges.len() {
                let cur_range = &target.ranges[parent_idx];
                if cur_range.num_child_ranges == 0 {
                    assert_eq!(cur_range.first_child_idx, parent_idx as u32);
                    assert_eq!(cur_range.last_child_idx, parent_idx as u32);
                } else {
                    assert_ne!(cur_range.first_child_idx, parent_idx as u32);
                    assert_ne!(cur_range.last_child_idx, parent_idx as u32);

                    let mut child_counter = 0u32;
                    let last_child_idx = cur_range.first_child_idx;
                    let mut child_idx = Some(cur_range.first_child_idx);
                    while let Some(cur_child_idx) = child_idx {
                        let child_range = &target.ranges[cur_child_idx as usize];
                        assert_eq!(child_range.parent_idx, parent_idx);
                        child_idx = child_range.next_sibling_idx;
                        child_counter += 1;
                    }

                    assert_eq!(cur_range.last_child_idx, last_child_idx);
                    assert_eq!(cur_range.num_child_ranges, child_counter);
                }
            }
        }

        Ok(())
    }

    fn is_line_comment_start(&self, first_char: u8, source: &InputSource) -> bool {
        return first_char == b'/' && Some(b'/') == source.lookahead(1);
    }

    fn is_block_comment_start(&self, first_char: u8, source: &InputSource) -> bool {
        return first_char == b'/' && Some(b'*') == source.lookahead(1);
    }

    fn maybe_parse_punctuation(
        &mut self, first_char: u8, source: &mut InputSource, target: &mut TokenBuffer
    ) -> Result<Option<(TokenKind, InputPosition)>, ParseError> {
        debug_assert!(first_char != b'#', "'#' needs special handling");
        debug_assert!(first_char != b'\'', "'\'' needs special handling");
        debug_assert!(first_char != b'"', "'\"' needs special handling");

        let pos = source.pos();
        let token_kind;
        if first_char == b'!' {
            source.consume();
            if Some(b'=') == source.next() {
                source.consume();
                token_kind = TokenKind::NotEqual;
            } else {
                token_kind = TokenKind::Exclamation;
            }
        } else if first_char == b'%' {
            source.consume();
            if Some(b'=') == source.next() {
                source.consume();
                token_kind = TokenKind::PercentEquals;
            } else {
                token_kind = TokenKind::Percent;
            }
        } else if first_char == b'&' {
            source.consume();
            let next = source.next();
            if Some(b'&') == next {
                source.consume();
                token_kind = TokenKind::AndAnd;
            } else if Some(b'=') == next {
                source.consume();
                token_kind = TokenKind::AndEquals;
            } else {
                token_kind = TokenKind::And;
            }
        } else if first_char == b'(' {
            source.consume();
            token_kind = TokenKind::OpenParen;
        } else if first_char == b')' {
            source.consume();
            token_kind = TokenKind::CloseParen;
        } else if first_char == b'*' {
            source.consume();
            if let Some(b'=') = source.next() {
                source.consume();
                token_kind = TokenKind::StarEquals;
            } else {
                token_kind = TokenKind::Star;
            }
        } else if first_char == b'+' {
            source.consume();
            let next = source.next();
            if Some(b'+') == next {
                source.consume();
                token_kind = TokenKind::PlusPlus;
            } else if Some(b'=') == next {
                source.consume();
                token_kind = TokenKind::PlusEquals;
            } else {
                token_kind = TokenKind::Plus;
            }
        } else if first_char == b',' {
            source.consume();
            token_kind = TokenKind::Comma;
        } else if first_char == b'-' {
            source.consume();
            let next = source.next();
            if Some(b'-') == next {
                source.consume();
                token_kind = TokenKind::MinusMinus;
            } else if Some(b'>') == next {
                source.consume();
                token_kind = TokenKind::ArrowRight;
            } else if Some(b'=') == next {
                source.consume();
                token_kind = TokenKind::MinusEquals;
            } else {
                token_kind = TokenKind::Minus;
            }
        } else if first_char == b'.' {
            source.consume();
            if let Some(b'.') = source.next() {
                source.consume();
                token_kind = TokenKind::DotDot;
            } else {
                token_kind = TokenKind::Dot
            }
        } else if first_char == b'/' {
            source.consume();
            debug_assert_ne!(Some(b'/'), source.next());
            debug_assert_ne!(Some(b'*'), source.next());
            if let Some(b'=') = source.next() {
                source.consume();
                token_kind = TokenKind::SlashEquals;
            } else {
                token_kind = TokenKind::Slash;
            }
        } else if first_char == b':' {
            source.consume();
            if let Some(b':') = source.next() {
                source.consume();
                token_kind = TokenKind::ColonColon;
            } else {
                token_kind = TokenKind::Colon;
            }
        } else if first_char == b';' {
            source.consume();
            token_kind = TokenKind::SemiColon;
        } else if first_char == b'<' {
            source.consume();
            let next = source.next();
            if let Some(b'<') = next {
                source.consume();
                if let Some(b'=') = source.next() {
                    source.consume();
                    token_kind = TokenKind::ShiftLeftEquals;
                } else {
                    token_kind = TokenKind::ShiftLeft;
                }
            } else if let Some(b'=') = next {
                source.consume();
                token_kind = TokenKind::LessEquals;
            } else {
                token_kind = TokenKind::OpenAngle;
            }
        } else if first_char == b'=' {
            source.consume();
            if let Some(b'=') = source.next() {
                source.consume();
                token_kind = TokenKind::EqualEqual;
            } else {
                token_kind = TokenKind::Equal;
            }
        } else if first_char == b'>' {
            source.consume();
            let next = source.next();
            if Some(b'>') == next {
                source.consume();
                if Some(b'=') == source.next() {
                    source.consume();
                    token_kind = TokenKind::ShiftRightEquals;
                } else {
                    token_kind = TokenKind::ShiftRight;
                }
            } else if Some(b'=') == next {
                source.consume();
                token_kind = TokenKind::GreaterEquals;
            } else {
                token_kind = TokenKind::CloseAngle;
            }
        } else if first_char == b'?' {
            source.consume();
            token_kind = TokenKind::Question;
        } else if first_char == b'@' {
            source.consume();
            token_kind = TokenKind::At;
        } else if first_char == b'[' {
            source.consume();
            token_kind = TokenKind::OpenSquare;
        } else if first_char == b']' {
            source.consume();
            token_kind = TokenKind::CloseSquare;
        } else if first_char == b'^' {
            source.consume();
            if let Some(b'=') = source.next() {
                source.consume();
                token_kind = TokenKind::CaretEquals;
            } else {
                token_kind = TokenKind::Caret;
            }
        } else if first_char == b'{' {
            source.consume();
            token_kind = TokenKind::OpenCurly;
        } else if first_char == b'|' {
            source.consume();
            let next = source.next();
            if Some(b'|') == next {
                source.consume();
                token_kind = TokenKind::OrOr;
            } else if Some(b'=') == next {
                source.consume();
                token_kind = TokenKind::OrEquals;
            } else {
                token_kind = TokenKind::Or;
            }
        } else if first_char == b'}' {
            source.consume();
            token_kind = TokenKind::CloseCurly;
        } else if first_char == b'~' {
            source.consume();
            token_kind = TokenKind::Tilde;
        } else {
            self.check_ascii(source)?;
            return Ok(None);
        }

        target.tokens.push(Token::new(token_kind, pos));
        Ok(Some((token_kind, pos)))
    }

    fn consume_char_literal(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading quote
        debug_assert!(source.next().unwrap() == b'\'');
        source.consume();

        let mut prev_char = b'\'';
        while let Some(c) = source.next() {
            if !c.is_ascii() {
                return Err(ParseError::new_error_str_at_pos(source, source.pos(), "non-ASCII character in char literal"));
            }
            source.consume();

            // Make sure ending quote was not escaped
            if c == b'\'' && prev_char != b'\\' {
                prev_char = c;
                break;
            }

            prev_char = c;
        }

        if prev_char != b'\'' {
            // Unterminated character literal, reached end of file.
            return Err(ParseError::new_error_str_at_pos(source, begin_pos, "encountered unterminated character literal"));
        }

        let end_pos = source.pos();

        target.tokens.push(Token::new(TokenKind::Character, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    fn consume_string_literal(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading double quotes
        debug_assert!(source.next().unwrap() == b'"');
        source.consume();

        let mut prev_char = b'"';
        while let Some(c) = source.next() {
            if !c.is_ascii() {
                return Err(ParseError::new_error_str_at_pos(source, source.pos(), "non-ASCII character in string literal"));
            }

            source.consume();
            if c == b'"' && prev_char != b'\\' {
                prev_char = c;
                break;
            }

            prev_char = c;
        }

        if prev_char != b'"' {
            // Unterminated string literal
            return Err(ParseError::new_error_str_at_pos(source, begin_pos, "encountered unterminated string literal"));
        }

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::String, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    fn consume_pragma_or_pound(&mut self, first_char: u8, source: &mut InputSource, target: &mut TokenBuffer) -> Result<bool, ParseError> {
        let start_pos = source.pos();
        debug_assert_eq!(first_char, b'#');
        source.consume();

        let next = source.next();
        if next.is_none() || !is_identifier_start(next.unwrap()) {
            // Just a pound sign
            target.tokens.push(Token::new(TokenKind::Pound, start_pos));
            Ok(false)
        } else {
            // Pound sign followed by identifier
            source.consume();
            while let Some(c) = source.next() {
                if !is_identifier_remaining(c) {
                    break;
                }
                source.consume();
            }

            self.check_ascii(source)?;

            let end_pos = source.pos();
            target.tokens.push(Token::new(TokenKind::Pragma, start_pos));
            target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));
            Ok(true)
        }
    }

    fn consume_line_comment(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading "//"
        debug_assert!(source.next().unwrap() == b'/' && source.lookahead(1).unwrap() == b'/');
        source.consume();
        source.consume();

        let mut prev_char = b'/';
        let mut cur_char = b'/';
        while let Some(c) = source.next() {
            prev_char = cur_char;
            cur_char = c;

            if c == b'\n' {
                // End of line, note that the newline is not consumed
                break;
            }

            source.consume();
        }

        let mut end_pos = source.pos();
        debug_assert_eq!(begin_pos.line, end_pos.line);

        // Modify offset to not include the newline characters
        if cur_char == b'\n' {
            if prev_char == b'\r' {
                end_pos.offset -= 2;
            } else {
                end_pos.offset -= 1;
            }
            // Consume final newline
            source.consume();
        } else {
            // End of comment was due to EOF
            debug_assert!(source.next().is_none())
        }

        target.tokens.push(Token::new(TokenKind::LineComment, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    fn consume_block_comment(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading "/*"
        debug_assert!(source.next().unwrap() == b'/' && source.lookahead(1).unwrap() == b'*');
        source.consume();
        source.consume();

        // Explicitly do not put prev_char at "*", because then "/*/" would
        // represent a valid and closed block comment
        let mut prev_char = b' ';
        let mut is_closed = false;
        while let Some(c) = source.next() {
            source.consume();
            if prev_char == b'*' && c == b'/' {
                // End of block comment
                is_closed = true;
                break;
            }
            prev_char = c;
        }

        if !is_closed {
            return Err(ParseError::new_error_str_at_pos(
                source, source.pos(), "encountered unterminated block comment")
            );
        }

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::BlockComment, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    fn consume_identifier<'a>(&mut self, source: &'a mut InputSource, target: &mut TokenBuffer) -> Result<&'a [u8], ParseError> {
        let begin_pos = source.pos();
        debug_assert!(is_identifier_start(source.next().unwrap()));
        source.consume();

        // Keep reading until no more identifier
        while let Some(c) = source.next() {
            if !is_identifier_remaining(c) {
                break;
            }

            source.consume();
        }
        self.check_ascii(source)?;

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::Ident, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));
        Ok(source.section_at_pos(begin_pos, end_pos))
    }

    fn consume_number(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();
        debug_assert!(is_integer_literal_start(source.next().unwrap()));
        source.consume();

        // Keep reading until it doesn't look like a number anymore
        while let Some(c) = source.next() {
            if !maybe_number_remaining(c) {
                break;
            }

            source.consume();
        }
        self.check_ascii(source)?;

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::Integer, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    // Consumes whitespace and returns whether or not the whitespace contained
    // a newline.
    fn consume_whitespace(&self, source: &mut InputSource) -> bool {
        debug_assert!(is_whitespace(source.next().unwrap()));

        let mut has_newline = false;
        while let Some(c) = source.next() {
            if !is_whitespace(c) {
                break;
            }

            if c == b'\n' {
                has_newline = true;
            }
            source.consume();
        }

        has_newline
    }

    /// Pushes a new token range onto the stack in the buffers.
    fn push_range(&mut self, target: &mut TokenBuffer, range_kind: TokenRangeKind, first_token: u32) {
        let new_range_idx = target.ranges.len() as u32;
        let cur_range = &mut target.ranges[self.stack_idx];

        // If we have just popped a range and then push a new range, then the
        // first token is equal to the last token registered on the current
        // range. If not, then we had some intermediate tokens that did not
        // belong to a particular kind of token range: hence we insert an
        // intermediate "code" range.
        if cur_range.end != first_token {
            let code_start = cur_range.end;

            if cur_range.first_child_idx == self.stack_idx as u32 {
                // The parent of the new "code" range we're going to push does
                // not have any registered children yet.
                cur_range.first_child_idx = new_range_idx;
            }
            cur_range.last_child_idx = new_range_idx + 1;

            cur_range.end = first_token;
            cur_range.num_child_ranges += 1;
            target.ranges.push(TokenRange{
                parent_idx: self.stack_idx,
                range_kind: TokenRangeKind::Code,
                curly_depth: self.curly_stack.len() as u32,
                start: code_start,
                end: first_token,
                num_child_ranges: 0,
                first_child_idx: new_range_idx,
                last_child_idx: new_range_idx,
                next_sibling_idx: Some(new_range_idx + 1), // we're going to push this thing next
            });
        } else {
            // We're going to push the range in the code below, but while we
            // have the `cur_range` borrowed mutably, we fix up its children
            // indices.
            if cur_range.first_child_idx == self.stack_idx as u32 {
                cur_range.first_child_idx = new_range_idx;
            }
            cur_range.last_child_idx = new_range_idx;
        }

        // Insert a new range
        let parent_idx = self.stack_idx;
        self.stack_idx = target.ranges.len();
        let new_range_idx = self.stack_idx as u32;
        target.ranges.push(TokenRange{
            parent_idx,
            range_kind,
            curly_depth: self.curly_stack.len() as u32,
            start: first_token,
            end: first_token,
            num_child_ranges: 0,
            first_child_idx: new_range_idx,
            last_child_idx: new_range_idx,
            next_sibling_idx: None,
        });
    }

    fn pop_range(&mut self, target: &mut TokenBuffer, end_index: u32) {
        let last = &mut target.ranges[self.stack_idx];
        debug_assert!(self.stack_idx != last.parent_idx, "attempting to pop top-level range");

        // Fix up the current range before going back to parent
        last.end = end_index;
        debug_assert_ne!(last.start, end_index);

        // Go back to parent
        self.stack_idx = last.parent_idx;
        let parent = &mut target.ranges[self.stack_idx];
        parent.end = end_index;
        parent.num_child_ranges += 1;
    }


    fn check_ascii(&self, source: &InputSource) -> Result<(), ParseError> {
        match source.next() {
            Some(c) if !c.is_ascii() => {
                Err(ParseError::new_error_str_at_pos(source, source.pos(), "encountered a non-ASCII character"))
            },
            _else => {
                Ok(())
            },
        }
    }
}

// Helpers for characters
fn demarks_definition(ident: &[u8]) -> bool {
    return
        ident == KW_STRUCT ||
            ident == KW_ENUM ||
            ident == KW_UNION ||
            ident == KW_FUNCTION ||
            ident == KW_PRIMITIVE ||
            ident == KW_COMPOSITE
}

fn demarks_import(ident: &[u8]) -> bool {
    return ident == KW_IMPORT;
}

fn is_whitespace(c: u8) -> bool {
    c.is_ascii_whitespace()
}

fn is_char_literal_start(c: u8) -> bool {
    return c == b'\'';
}

fn is_string_literal_start(c: u8) -> bool {
    return c == b'"';
}

fn is_pragma_start_or_pound(c: u8) -> bool {
    return c == b'#';
}

fn is_identifier_start(c: u8) -> bool {
    return
        (c >= b'a' && c <= b'z') ||
            (c >= b'A' && c <= b'Z') ||
            c == b'_'
}

fn is_identifier_remaining(c: u8) -> bool {
    return
        (c >= b'0' && c <= b'9') ||
            (c >= b'a' && c <= b'z') ||
            (c >= b'A' && c <= b'Z') ||
            c == b'_'
}

fn is_integer_literal_start(c: u8) -> bool {
    return c >= b'0' && c <= b'9';
}

fn maybe_number_remaining(c: u8) -> bool {
    return
        (c == b'b' || c == b'B' || c == b'o' || c == b'O' || c == b'x' || c == b'X') ||
            (c >= b'0' && c <= b'9') ||
            c == b'_';
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Remove at some point
    #[test]
    fn test_tokenizer() {
        let mut source = InputSource::new_test("

        #version 500
        # hello 2

        import std.reo::*;

        struct Thing {
            int a: 5,
        }
        enum Hello {
            A,
            B
        }

        // Hello hello, is it me you are looking for?
        // I can seee it in your eeeyes

        func something(int a, int b, int c) -> byte {
            int a = 5;
            struct Inner {
                int a
            }
            struct City {
                int b
            }
            /* Waza
            How are you doing
            Things in here yo
            /* */ */

            a = a + 5 * 2;
            struct Pressure {
                int d
            }
        }
        ");
        let mut t = PassTokenizer::new();
        let mut buffer = TokenBuffer::new();
        t.tokenize(&mut source, &mut buffer).expect("tokenize");

        println!("Ranges:\n");
        for (idx, range) in buffer.ranges.iter().enumerate() {
            println!("[{}] {:?}", idx, range)
        }

        println!("Tokens:\n");
        let mut iter = buffer.tokens.iter().enumerate();
        while let Some((idx, token)) = iter.next() {
            match token.kind {
                TokenKind::Ident | TokenKind::Pragma | TokenKind::Integer |
                TokenKind::String | TokenKind::Character | TokenKind::LineComment |
                TokenKind::BlockComment => {
                    let (_, end) = iter.next().unwrap();
                    println!("[{}] {:?} ......", idx, token.kind);
                    assert_eq!(end.kind, TokenKind::SpanEnd);
                    let text = source.section_at_pos(token.pos, end.pos);
                    println!("{}", String::from_utf8_lossy(text));
                },
                _ => {
                    println!("[{}] {:?}", idx, token.kind);
                }
            }
        }
    }
}
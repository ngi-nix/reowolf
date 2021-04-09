
use crate::protocol::input_source2::{InputSource2 as InputSource, ParseError, InputPosition2 as InputPosition, InputSpan};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    // Variable-character tokens, followed by a SpanEnd token
    Ident,
    Integer,
    String,
    Character,
    LineComment,
    BlockComment,
    // Punctuation
    Exclamation,    // !
    Question,       // ?
    Pound,          // #
    OpenAngle,      // <
    OpenCurly,      // {
    OpenParen,      // (
    OpenSquare,     // [
    CloseAngle,     // >
    CloseCurly,     // }
    CloseParen,     // )
    CloseSquare,    // ]
    Colon,          // :
    ColonColon,     // ::
    Comma,          // ,
    Dot,            // .
    DotDot,         // ..
    SemiColon,      // ;
    Quote,          // '
    DoubleQuote,    // "
    // Operator-like
    At,             // @
    Plus,           // +
    PlusPlus,       // ++
    PlusEquals,     // +=
    Minus,          // -
    ArrowRight,     // ->
    MinusMinus,     // --
    MinusEquals,    // -=
    Star,           // *
    StarEquals,     // *=
    Slash,          // /
    SlashEquals,    // /=
    Percent,        // %
    PercentEquals,  // %=
    Caret,          // ^
    CaretEquals,    // ^=
    And,            // &
    AndAnd,         // &&
    AndEquals,      // &=
    Or,             // |
    OrOr,           // ||
    OrEquals,       // |=
    Tilde,          // ~
    Equal,          // =
    EqualEqual,     // ==
    NotEqual,       // !=
    ShiftLeft,      // <<
    ShiftLeftEquals,// <<=
    ShiftRight,     // >>
    ShiftRightEquals, // >>=
    // Special marker token to indicate end of variable-character tokens
    SpanEnd,
}

struct Token {
    kind: TokenKind,
    pos: InputPosition, // probably need something different
}

impl Token {
    fn new(kind: TokenKind, pos: InputPosition) -> Self {
        Self{ kind, pos }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TokenRangeKind {
    Module,
    Pragma,
    Import,
    Definition,
    Code,
}

struct TokenRange {
    // Index of parent in `TokenBuffer.ranges`, does not have a parent if the 
    // range kind is Module, in that case the parent index points to itself.
    parent_idx: usize,
    range_kind: TokenRangeKind,
    curly_depth: u8,
    start: usize,
    end: usize,
    subranges: usize,
}

struct TokenBuffer {
    tokens: Vec<Token>,
    ranges: Vec<TokenRange>,
}

struct ParseState {
    kind: TokenRangeKind,
    start: usize,
}

// Tokenizer is a reusable parser to tokenize multiple source files using the
// same allocated buffers. In a well-formed program, we produce a consistent
// tree of token ranges such that we may identify tokens that represent a 
// defintion or an import before producing the entire AST.
//
// If the program is not well-formed then the tree may be inconsistent, but we
// will detect this once we transform the tokens into the AST.
pub(crate) struct Tokenizer {
    curly_depth: u8,
    stack_idx: usize,
}

impl Tokenizer {
    pub(crate) fn tokenize(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        // Assert source and buffer are at start
        debug_assert_eq!(source.pos().offset, 0);
        debug_assert!(target.tokens.is_empty());
        debug_assert!(target.ranges.is_empty());

        // Set up for tokenization by pushing the first range onto the stack.
        // This range may get transformed into the appropriate range kind later,
        // see `push_range` and `pop_range`.
        self.curly_depth = 0;
        self.stack_idx = 1;
        target.ranges.push(TokenRange{
            parent_idx: 0,
            range_kind: TokenRangeKind::Module,
            curly_depth: 0,
            start: 0,
            end: 0,
            subranges: 0,
        });

        // Main processing loop
        while let Some(c) = source.next() {
            let token_index = target.tokens.len();

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
            } else if is_pragma_start(c) {
                self.consume_pragma(c, source, target);
                self.push_range(target, TokenRangeKind::Pragma, token_index);
            } else if self.is_line_comment_start(c, source) {
                self.consume_line_comment(source, target)?;
            } else if self.is_block_comment_start(c, source) {
                self.consume_block_comment(source, target)?;
            } else if is_whitespace(c) {
                let contained_newline = self.consume_whitespace(source);
            } else {
                let was_punctuation = self.maybe_parse_punctuation(c, source, target)?;
                if let Some(token) = was_punctuation {
                    if token == TokenKind::OpenCurly {
                        self.curly_depth += 1;
                    } else if token == TokenKind::CloseCurly {
                        // Check if this marks the end of a range we're 
                        // currently processing
                        self.curly_depth -= 1;

                        let range = &target.ranges[self.stack_idx];
                        if range.range_kind == TokenRangeKind::Definition && range.curly_depth == self.curly_depth {
                            self.pop_range(target, target.tokens.len());
                        }
                    } else if token == TokenKind::SemiColon {
                        // Check if this marks the end of an import
                        let range = &target.ranges[self.stack_idx];
                        if range.range_kind == TokenRangeKind::Import {
                            self.pop_range(target, target.tokens.len());
                        }
                    }
                } else {
                    return Err(ParseError::new_error(source, source.pos(), "unexpected character"));
                }
            }
        }

        // End of file, check if our state is correct
        Ok(())
    }

    fn is_line_comment_start(&self, first_char: u8, source: &InputSource) -> bool {
        return first_char == b'/' && Some(b'/') == source.lookahead(1);
    }

    fn is_block_comment_start(&self, first_char: u8, source: &InputSource) -> bool {
        return first_char == b'/' && Some(b'*') == source.lookahead(1);
    }

    pub(crate) fn maybe_parse_punctuation(&mut self, first_char: u8, source: &mut InputSource, target: &mut TokenBuffer) -> Result<Option<TokenKind>, ParseError> {
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
            if let Some(b'<') = source.next() {
                source.consume();
                if let Some(b'=') = source.next() {
                    source.consume();
                    token_kind = TokenKind::ShiftLeftEquals;
                } else {
                    token_kind = TokenKind::ShiftLeft;
                }
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
            if let Some(b'>') = source.next() {
                source.consume();
                if let Some(b'=') = source.next() {
                    source.consume();
                    token_kind = TokenKind::ShiftRightEquals;
                } else {
                    token_kind = TokenKind::ShiftRight;
                }
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
            token_kind = TokenKind::CloseCurly
        } else {
            self.check_ascii(source)?;
            return Ok(None);
        }

        target.tokens.push(Token::new(token_kind, pos));
        Ok(Some(token_kind))
    }

    pub(crate) fn consume_char_literal(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading quote
        debug_assert!(source.next().unwrap() == b'\'');
        source.consume();

        let mut prev_char = b'\'';
        while let Some(c) = source.next() {
            source.consume();
            if c == b'\'' && prev_char != b'\\' {
                prev_char = c;
                break;
            }

            prev_char = c;
        }

        if prev_char != b'\'' {
            // Unterminated character literal
            return Err(ParseError::new_error(source, begin_pos, "encountered unterminated character literal"));
        }

        let end_pos = source.pos();

        target.tokens.push(Token::new(TokenKind::Character, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    pub(crate) fn consume_string_literal(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading double quotes
        debug_assert!(source.next().unwrap() == b'"');
        source.consume();

        let mut prev_char = b'"';
        while let Some(c) = source.next() {
            source.consume();
            if c == b'"' && prev_char != b'\\' {
                prev_char = c;
                break;
            }

            prev_char = c;
        }

        if prev_char != b'"' {
            // Unterminated string literal
            return Err(ParseError::new_error(source, begin_pos, "encountered unterminated string literal"));
        }

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::String, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    fn consume_pragma(&mut self, first_char: u8, source: &mut InputSource, target: &mut TokenBuffer) {
        let pos = source.pos();
        debug_assert_eq!(first_char, b'#');
        source.consume();

        target.tokens.push(Token::new(TokenKind::Pound, pos));
    }

    pub(crate) fn consume_line_comment(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
        let begin_pos = source.pos();

        // Consume the leading "//"
        debug_assert!(source.next().unwrap() == b'/' && source.lookahead(1).unwrap() == b'/');
        source.consume();
        source.consume();

        let mut prev_char = b'/';
        let mut cur_char = b'/';
        while let Some(c) = source.next() {
            source.consume();
            cur_char = c;
            if c == b'\n' {
                // End of line
                break;
            }
            prev_char = c;
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
        } else {
            debug_assert!(source.next().is_none())
        }

        target.tokens.push(Token::new(TokenKind::LineComment, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    pub(crate) fn consume_block_comment(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
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
            return Err(ParseError::new_error(source, source.pos(), "encountered unterminated block comment"));
        }

        let end_pos = source.pos();
        target.tokens.push(Token::new(TokenKind::BlockComment, begin_pos));
        target.tokens.push(Token::new(TokenKind::SpanEnd, end_pos));

        Ok(())
    }

    pub(crate) fn consume_identifier<'a>(&mut self, source: &'a mut InputSource, target: &mut TokenBuffer) -> Result<&'a [u8], ParseError> {
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
        Ok(source.section(begin_pos.offset, end_pos.offset))
    }

    pub(crate) fn consume_number(&mut self, source: &mut InputSource, target: &mut TokenBuffer) -> Result<(), ParseError> {
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
        }

        has_newline
    }

    /// Pushes a new token range onto the stack in the buffers.
    fn push_range(&self, target: &mut TokenBuffer, range_kind: TokenRangeKind, first_token: usize) {
        let cur_range = &target.ranges[self.stack_idx];
        let parent_idx = cur_range.parent_idx;
        let parent_range = &target.ranges[parent_idx];
        if parent_range.end != first_token {
            // Insert intermediate range
            target.ranges.push(TokenRange{
                parent_idx,
                range_kind: TokenRangeKind::Code,
                curly_depth: cur_range.curly_depth,
                start: parent_range.end,
                end: first_token,
                subranges: 0,
            });
        }

        // Insert a new range
        let parent_idx = self.stack_idx;
        self.stack_idx = target.ranges.len();
        target.ranges.push(TokenRange{
            parent_idx,
            range_kind,
            curly_depth: self.curly_depth,
            start: first_token,
            end: first_token,
            subranges: 0,
        });
    }

    fn pop_range(&self, target: &mut TokenBuffer, end_index: usize) {
        // Pop all the dummy ranges that are left on the range stack
        let last = &mut target.ranges[self.stack_idx];
        debug_assert!(self.stack_idx != last.parent_idx, "attempting to pop top-level range");

        last.end = end_index;
        self.stack_idx = last.parent_idx as usize;
        let parent = &mut target.ranges[self.stack_idx];
        parent.end = end_index;
        parent.subranges += 1;
    }


    fn check_ascii(&self, source: &InputSource) -> Result<(), ParseError> {
        match source.next() {
            Some(c) if !c.is_ascii() => {
                Err(ParseError::new_error(source, source.pos(), "encountered a non-ASCII character"))
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
        ident == b"struct" ||
        ident == b"enum" ||
        ident == b"union" ||
        ident == b"func" ||
        ident == b"primitive" ||
        ident == b"composite"
}

fn demarks_import(ident: &[u8]) -> bool {
    return ident == b"import";
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

fn is_pragma_start(c: u8) -> bool {
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
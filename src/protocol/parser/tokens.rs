use crate::protocol::input_source2::{
    InputPosition2 as InputPosition,
    InputSpan
};

/// Represents a particular kind of token. Some tokens represent
/// variable-character tokens. Such a token is always followed by a
/// `TokenKind::SpanEnd` token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TokenKind {
    // Variable-character tokens, followed by a SpanEnd token
    Ident,          // regular identifier
    Pragma,         // identifier with prefixed `#`, range includes `#`
    Integer,        // integer literal
    String,         // string literal, range includes `"`
    Character,      // character literal, range includes `'`
    LineComment,    // line comment, range includes leading `//`, but not newline
    BlockComment,   // block comment, range includes leading `/*` and trailing `*/`
    // Punctuation (single character)
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
    Comma,          // ,
    Dot,            // .
    SemiColon,      // ;
    Quote,          // '
    DoubleQuote,    // "
    // Operator-like (single character)
    At,             // @
    Plus,           // +
    Minus,          // -
    Star,           // *
    Slash,          // /
    Percent,        // %
    Caret,          // ^
    And,            // &
    Or,             // |
    Tilde,          // ~
    Equal,          // =
    // Punctuation (two characters)
    ColonColon,     // ::
    DotDot,         // ..
    ArrowRight,     // ->
    // Operator-like (two characters)
    PlusPlus,       // ++
    PlusEquals,     // +=
    MinusMinus,     // --
    MinusEquals,    // -=
    StarEquals,     // *=
    SlashEquals,    // /=
    PercentEquals,  // %=
    CaretEquals,    // ^=
    AndAnd,         // &&
    AndEquals,      // &=
    OrOr,           // ||
    OrEquals,       // |=
    EqualEqual,     // ==
    NotEqual,       // !=
    ShiftLeft,      // <<
    ShiftRight,     // >>
    // Operator-like (three characters)
    ShiftLeftEquals,// <<=
    ShiftRightEquals, // >>=
    // Special marker token to indicate end of variable-character tokens
    SpanEnd,
}

impl TokenKind {
    /// Returns true if the next expected token is the special `TokenKind::SpanEnd` token. This is
    /// the case for tokens of variable length (e.g. an identifier).
    fn has_span_end(&self) -> bool {
        return *self <= TokenKind::BlockComment
    }

    /// Returns the number of characters associated with the token. May only be called on tokens
    /// that do not have a variable length.
    fn num_characters(&self) -> u32 {
        debug_assert!(!self.has_span_end() && *self != TokenKind::SpanEnd);
        if *self <= TokenKind::Equal {
            1
        } else if *self <= TokenKind::ShiftRight {
            2
        } else {
            3
        }
    }

    /// Returns the characters that are represented by the token, may only be called on tokens that
    /// do not have a variable length.
    pub fn token_chars(&self) -> &'static str {
        debug_assert!(!self.has_span_end() && *self != TokenKind::SpanEnd);
        use TokenKind as TK;
        match self {
            TK::Exclamation => "!",
            TK::Question => "?",
            TK::Pound => "#",
            TK::OpenAngle => "<",
            TK::OpenCurly => "{",
            TK::OpenParen => "(",
            TK::OpenSquare => "[",
            TK::CloseAngle => ">",
            TK::CloseCurly => "}",
            TK::CloseParen => ")",
            TK::CloseSquare => "]",
            TK::Colon => ":",
            TK::Comma => ",",
            TK::Dot => ".",
            TK::SemiColon => ";",
            TK::Quote => "'",
            TK::DoubleQuote => "\"",
            TK::At => "@",
            TK::Plus => "+",
            TK::Minus => "-",
            TK::Star => "*",
            TK::Slash => "/",
            TK::Percent => "%",
            TK::Caret => "^",
            TK::And => "&",
            TK::Or => "|",
            TK::Tilde => "~",
            TK::Equal => "=",
            TK::ColonColon => "::",
            TK::DotDot => "..",
            TK::ArrowRight => "->",
            TK::PlusPlus => "++",
            TK::PlusEquals => "+=",
            TK::MinusMinus => "--",
            TK::MinusEquals => "-=",
            TK::StarEquals => "*=",
            TK::SlashEquals => "/=",
            TK::PercentEquals => "%=",
            TK::CaretEquals => "^=",
            TK::AndAnd => "&&",
            TK::AndEquals => "&=",
            TK::OrOr => "||",
            TK::OrEquals => "|=",
            TK::EqualEqual => "==",
            TK::NotEqual => "!=",
            TK::ShiftLeft => "<<",
            TK::ShiftRight => ">>",
            TK::ShiftLeftEquals => "<<=",
            TK::ShiftRightEquals => ">>=",
            // Lets keep these in explicitly for now, in case we want to add more symbols
            TK::Ident | TK::Pragma | TK::Integer | TK::String | TK::Character |
            TK::LineComment | TK::BlockComment | TK::SpanEnd => unreachable!(),
        }
    }
}

/// Represents a single token at a particular position.
pub(crate) struct Token {
    pub kind: TokenKind,
    pub pos: InputPosition,
}

impl Token {
    pub(crate) fn new(kind: TokenKind, pos: InputPosition) -> Self {
        Self{ kind, pos }
    }
}

/// The kind of token ranges that are specially parsed by the tokenizer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TokenRangeKind {
    Module,
    Pragma,
    Import,
    Definition,
    Code,
}

/// A range of tokens with a specific meaning. Such a range is part of a tree
/// where each parent tree envelops all of its children.
#[derive(Debug)]
pub(crate) struct TokenRange {
    // Index of parent in `TokenBuffer.ranges`, does not have a parent if the
    // range kind is Module, in that case the parent index points to itself.
    pub parent_idx: usize,
    pub range_kind: TokenRangeKind,
    pub curly_depth: u32,
    // Offsets into `TokenBuffer.ranges`: the tokens belonging to this range.
    pub start: u32,             // first token (inclusive index)
    pub end: u32,               // last token (exclusive index)
    // Child ranges
    pub num_child_ranges: u32,  // Number of subranges
    pub first_child_idx: u32,   // First subrange (or points to self if no subranges)
    pub last_child_idx: u32,    // Last subrange (or points to self if no subranges)
    pub next_sibling_idx: Option<u32>,
}

pub(crate) struct TokenBuffer {
    pub tokens: Vec<Token>,
    pub ranges: Vec<TokenRange>,
}

impl TokenBuffer {
    pub(crate) fn new() -> Self {
        Self{ tokens: Vec::new(), ranges: Vec::new() }
    }

    pub(crate) fn iter_range<'a>(&'a self, range: &TokenRange) -> TokenIter<'a> {
        TokenIter::new(self, range.start as usize, range.end as usize)
    }

    pub(crate) fn start_pos(&self, range: &TokenRange) -> InputPosition {
        self.tokens[range.start].pos
    }

    pub(crate) fn end_pos(&self, range: &TokenRange) -> InputPosition {
        let last_token = &self.tokens[range.end - 1];
        if last_token.kind == TokenKind::SpanEnd {
            return last_token.pos
        } else {
            debug_assert!(!last_token.kind.has_span_end());
            return last_token.pos.with_offset(last_token.kind.num_characters());
        }
    }
}

/// Iterator over tokens within a specific `TokenRange`.
pub(crate) struct TokenIter<'a> {
    tokens: &'a Vec<Token>,
    cur: usize,
    end: usize,
}

impl<'a> TokenIter<'a> {
    fn new(buffer: &'a TokenBuffer, start: usize, end: usize) -> Self {
        Self{ tokens: &buffer.tokens, cur: start, end }
    }

    /// Returns the next token (may include comments), or `None` if at the end
    /// of the range.
    pub(crate) fn next_including_comments(&self) -> Option<TokenKind> {
        if self.cur >= self.end {
            return None;
        }

        let token = &self.tokens[self.cur];
        Some(token.kind)
    }

    /// Returns the next token (but skips over comments), or `None` if at the
    /// end of the range
    pub(crate) fn next(&mut self) -> Option<TokenKind> {
        while let Some(token_kind) = self.next() {
            if token_kind != TokenKind::LineComment && token_kind != TokenKind::BlockComment {
                return Some(token_kind);
            }
            self.consume();
        }

        return None
    }

    /// Returns the start position belonging to the token returned by `next`. If
    /// there is not a next token, then we return the end position of the
    /// previous token.
    pub(crate) fn last_valid_pos(&self) -> InputPosition {
        if self.cur < self.end {
            // Return token position
            return self.tokens[self.cur].pos
        }

        // Return previous token end
        let token = &self.tokens[self.cur - 1];
        return if token.kind == TokenKind::SpanEnd {
            token.pos
        } else {
            token.pos.with_offset(token.kind.num_characters());
        };
    }

    /// Returns the token range belonging to the token returned by `next`. This
    /// assumes that we're not at the end of the range we're iterating over.
    /// TODO: @cleanup Phase out?
    pub(crate) fn next_positions(&self) -> (InputPosition, InputPosition) {
        debug_assert!(self.cur < self.end);
        let token = &self.tokens[self.cur];
        if token.kind.has_span_end() {
            let span_end = &self.tokens[self.cur + 1];
            debug_assert_eq!(span_end.kind, TokenKind::SpanEnd);
            (token.pos, span_end.pos)
        } else {
            let offset = token.kind.num_characters();
            (token.pos, token.pos.with_offset(offset))
        }
    }

    /// See `next_positions`
    pub(crate) fn next_span(&self) -> InputSpan {
        let (begin, end) = self.next_positions();
        return InputSpan::from_positions(begin, end)
    }

    /// Advances the iterator to the next (meaningful) token.
    pub(crate) fn consume(&mut self) {
        if let Some(kind) = self.next() {
            if kind.has_span_end() {
                self.cur += 2;
            } else {
                self.cur += 1;
            }
        }
    }
}
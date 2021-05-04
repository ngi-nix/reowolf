use std::fmt;
use std::sync::{RwLock, RwLockReadGuard};
use std::fmt::Write;

#[derive(Debug, Clone, Copy)]
pub struct InputPosition {
    pub line: u32,
    pub offset: u32,
}

impl InputPosition {
    pub(crate) fn with_offset(&self, offset: u32) -> Self {
        InputPosition { line: self.line, offset: self.offset + offset }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputSpan {
    pub begin: InputPosition,
    pub end: InputPosition,
}

impl InputSpan {
    #[inline]
    pub fn from_positions(begin: InputPosition, end: InputPosition) -> Self {
        Self { begin, end }
    }
}

/// Wrapper around source file with optional filename. Ensures that the file is
/// only scanned once.
pub struct InputSource {
    pub(crate) filename: String,
    pub(crate) input: Vec<u8>,
    // Iteration
    line: u32,
    offset: usize,
    // State tracking
    pub(crate) had_error: Option<ParseError>,
    // The offset_lookup is built on-demand upon attempting to report an error.
    // Only one procedure will actually create the lookup, afterwards only read
    // locks will be held.
    offset_lookup: RwLock<Vec<u32>>,
}

impl InputSource {
    pub fn new(filename: String, input: Vec<u8>) -> Self {
        Self{
            filename,
            input,
            line: 1,
            offset: 0,
            had_error: None,
            offset_lookup: RwLock::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub fn new_test(input: &str) -> Self {
        let bytes = Vec::from(input.as_bytes());
        return Self::new(String::from("test"), bytes)
    }

    #[inline]
    pub fn pos(&self) -> InputPosition {
        InputPosition { line: self.line, offset: self.offset as u32 }
    }

    pub fn next(&self) -> Option<u8> {
        if self.offset < self.input.len() {
            Some(self.input[self.offset])
        } else {
            None
        }
    }

    pub fn lookahead(&self, offset: usize) -> Option<u8> {
        let offset_pos = self.offset + offset;
        if offset_pos < self.input.len() {
            Some(self.input[offset_pos])
        } else {
            None
        }
    }

    #[inline]
    pub fn section_at_pos(&self, start: InputPosition, end: InputPosition) -> &[u8] {
        &self.input[start.offset as usize..end.offset as usize]
    }

    #[inline]
    pub fn section_at_span(&self, span: InputSpan) -> &[u8] {
        &self.input[span.begin.offset as usize..span.end.offset as usize]
    }

    // Consumes the next character. Will check well-formedness of newlines: \r
    // must be followed by a \n, because this is used for error reporting. Will
    // not check for ascii-ness of the file, better left to a tokenizer.
    pub fn consume(&mut self) {
        match self.next() {
            Some(b'\r') => {
                if Some(b'\n') == self.lookahead(1) {
                    // Well formed file
                    self.offset += 1;
                } else {
                    // Not a well-formed file, pretend like we can continue
                    self.offset += 1;
                    self.set_error("Encountered carriage-feed without a following newline");
                }
            },
            Some(b'\n') => {
                self.line += 1;
                self.offset += 1;
            },
            Some(_) => {
                self.offset += 1;
            }
            None => {}
        }

        // Maybe we actually want to check this in release mode. Then again:
        // a 4 gigabyte source file... Really?
        debug_assert!(self.offset < u32::max_value() as usize);
    }

    fn set_error(&mut self, msg: &str) {
        if self.had_error.is_none() {
            self.had_error = Some(ParseError::new_error_str_at_pos(self, self.pos(), msg));
        }
    }

    fn get_lookup(&self) -> RwLockReadGuard<Vec<u32>> {
        // Once constructed the lookup always contains one element. We use this
        // to see if it is constructed already.
        {
            let lookup = self.offset_lookup.read().unwrap();
            if !lookup.is_empty() {
                return lookup;
            }
        }

        // Lookup was not constructed yet
        let mut lookup = self.offset_lookup.write().unwrap();
        if !lookup.is_empty() {
            // Somebody created it before we had the chance
            drop(lookup);
            let lookup = self.offset_lookup.read().unwrap();
            return lookup;
        }

        // Build the line number (!) to offset lookup, so offset by 1. We 
        // assume the entire source file is scanned (most common case) for
        // preallocation.
        lookup.reserve(self.line as usize + 2);
        lookup.push(0); // line 0: never used
        lookup.push(0); // first line: first character

        for char_idx in 0..self.input.len() {
            if self.input[char_idx] == b'\n' {
                lookup.push(char_idx as u32 + 1);
            }
        }

        lookup.push(self.input.len() as u32); // for lookup_line_end
        debug_assert_eq!(self.line as usize + 2, lookup.len(), "remove me: i am a testing assert and sometimes invalid");

        // Return created lookup
        drop(lookup);
        let lookup = self.offset_lookup.read().unwrap();
        return lookup;
    }

    /// Retrieves offset at which line starts (right after newline)
    fn lookup_line_start_offset(&self, line_number: u32) -> u32 {
        let lookup = self.get_lookup();
        lookup[line_number as usize]
    }

    /// Retrieves offset at which line ends (at the newline character or the
    /// preceding carriage feed for \r\n-encoded newlines)
    fn lookup_line_end_offset(&self, line_number: u32) -> u32 {
        let lookup = self.get_lookup();
        let offset = lookup[(line_number + 1) as usize] - 1;
        let offset_usize = offset as usize;

        // Compensate for newlines and a potential carriage feed
        if self.input[offset_usize] == b'\n' {
            if offset_usize > 0 && self.input[offset_usize] == b'\r' {
                offset - 2
            } else {
                offset - 1
            }
        } else {
            offset
        }
    }
}

#[derive(Debug)]
pub enum StatementKind {
    Info,
    Error
}

#[derive(Debug)]
pub enum ContextKind {
    SingleLine,
    MultiLine,
}

#[derive(Debug)]
pub struct ParseErrorStatement {
    pub(crate) statement_kind: StatementKind,
    pub(crate) context_kind: ContextKind,
    pub(crate) start_line: u32,
    pub(crate) start_column: u32,
    pub(crate) end_line: u32,
    pub(crate) end_column: u32,
    pub(crate) filename: String,
    pub(crate) context: String,
    pub(crate) message: String,
}

impl ParseErrorStatement {
    fn from_source_at_pos(statement_kind: StatementKind, source: &InputSource, position: InputPosition, message: String) -> Self {
        // Seek line start and end
        let line_start = source.lookup_line_start_offset(position.line);
        let line_end = source.lookup_line_end_offset(position.line);
        let context = Self::create_context(source, line_start as usize, line_end as usize);
        debug_assert!(position.offset >= line_start);
        let column = position.offset - line_start + 1;

        Self{
            statement_kind,
            context_kind: ContextKind::SingleLine,
            start_line: position.line,
            start_column: column,
            end_line: position.line,
            end_column: column + 1,
            filename: source.filename.clone(),
            context,
            message,
        }
    }

    fn from_source_at_span(statement_kind: StatementKind, source: &InputSource, span: InputSpan, message: String) -> Self {
        debug_assert!(span.end.line >= span.begin.line);
        debug_assert!(span.end.offset >= span.begin.offset);

        let first_line_start = source.lookup_line_start_offset(span.begin.line);
        let last_line_start = source.lookup_line_start_offset(span.end.line);
        let last_line_end = source.lookup_line_end_offset(span.end.line);
        let context = Self::create_context(source, first_line_start as usize, last_line_end as usize);
        debug_assert!(span.begin.offset >= first_line_start);
        let start_column = span.begin.offset - first_line_start + 1;
        let end_column = span.end.offset - last_line_start + 1;

        let context_kind = if span.begin.line == span.end.line {
            ContextKind::SingleLine
        } else {
            ContextKind::MultiLine
        };

        Self{
            statement_kind,
            context_kind,
            start_line: first_line_start,
            start_column,
            end_line: last_line_start,
            end_column,
            filename: source.filename.clone(),
            context,
            message,
        }
    }

    /// Produces context from source
    fn create_context(source: &InputSource, start: usize, end: usize) -> String {
        let context_raw = &source.input[start..end];
        String::from_utf8_lossy(context_raw).to_string()
    }
}

impl fmt::Display for ParseErrorStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write kind of statement and message
        match self.statement_kind {
            StatementKind::Info => f.write_str(" INFO: ")?,
            StatementKind::Error => f.write_str("ERROR: ")?,
        }
        f.write_str(&self.message)?;
        f.write_char('\n')?;

        // Write originating file/line/column
        f.write_str(" +- ")?;
        if !self.filename.is_empty() {
            write!(f, "in {} ", self.filename)?;
        }

        match self.context_kind {
            ContextKind::SingleLine => writeln!(f, " at {}:{}", self.start_line, self.start_column),
            ContextKind::MultiLine => writeln!(
                f, " from {}:{} to {}:{}",
                self.start_line, self.start_column, self.end_line, self.end_column
            )
        }?;

        // Helper function for writing context: converting tabs into 4 spaces
        // (oh, the controversy!) and creating an annotated line
        fn transform_context(source: &str, target: &mut String) {
            for char in source.chars() {
                if char == '\t' {
                    target.push_str("    ");
                } else {
                    target.push(char);
                }
            }
        }

        fn extend_annotation(first_col: u32, last_col: u32, source: &str, target: &mut String, extend_char: char) {
            debug_assert!(first_col > 0 && last_col > first_col);
            for (char_idx, char) in source.chars().enumerate().skip(first_col as usize - 1) {
                if char_idx == last_col as usize {
                    break;
                }

                if char == '\t' {
                    for _ in 0..4 { target.push(extend_char); }
                } else {
                    target.push(extend_char);
                }
            }
        }

        // Write source context
        writeln!(f, " | ")?;

        let mut context = String::with_capacity(128);
        let mut annotation = String::with_capacity(128);

        match self.context_kind {
            ContextKind::SingleLine => {
                // Write single line of context with indicator for the offending
                // span underneath.
                transform_context(&self.context, &mut context);
                context.push('\n');
                f.write_str(&context)?;

                annotation.push_str(" | ");
                extend_annotation(1, self.start_column, &self.context, &mut annotation, ' ');
                extend_annotation(self.start_column, self.end_column, &self.context, &mut annotation, '~');
                annotation.push('\n');

                f.write_str(&annotation)?;
            },
            ContextKind::MultiLine => {
                // Annotate all offending lines
                // - first line
                let mut lines = self.context.lines();
                let first_line = lines.next().unwrap();
                transform_context(first_line, &mut context);
                writeln!(f, " |- {}", &context)?;

                // - remaining lines
                let mut last_line = first_line;
                while let Some(cur_line) = lines.next() {
                    context.clear();
                    transform_context(cur_line, &mut context);
                    writeln!(f, " |  {}", &context)?;
                    last_line = cur_line;
                }

                // - underline beneath last line
                annotation.push_str(" \\__");
                extend_annotation(1, self.end_column, &last_line, &mut annotation, '_');
                annotation.push_str("/\n");
                f.write_str(&annotation)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ParseError {
    pub(crate) statements: Vec<ParseErrorStatement>
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.statements.is_empty() {
            return Ok(())
        }

        self.statements[0].fmt(f)?;
        for statement in self.statements.iter().skip(1) {
            writeln!(f)?;
            statement.fmt(f)?;
        }

        Ok(())
    }
}

impl ParseError {
    pub fn new_error_at_pos(source: &InputSource, position: InputPosition, message: String) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source_at_pos(
            StatementKind::Error, source, position, message
        )) }
    }

    pub fn new_error_str_at_pos(source: &InputSource, position: InputPosition, message: &str) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source_at_pos(
            StatementKind::Error, source, position, message.to_string()
        )) }
    }

    pub fn new_error_at_span(source: &InputSource, span: InputSpan, message: String) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source_at_span(
            StatementKind::Error, source, span, message
        )) }
    }

    pub fn new_error_str_at_span(source: &InputSource, span: InputSpan, message: &str) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source_at_span(
            StatementKind::Error, source, span, message.to_string()
        )) }
    }

    pub fn with_at_pos(mut self, error_type: StatementKind, source: &InputSource, position: InputPosition, message: String) -> Self {
        self.statements.push(ParseErrorStatement::from_source_at_pos(error_type, source, position, message));
        self
    }

    pub fn with_at_span(mut self, error_type: StatementKind, source: &InputSource, span: InputSpan, message: String) -> Self {
        self.statements.push(ParseErrorStatement::from_source_at_span(error_type, source, span, message.to_string()));
        self
    }

    pub fn with_info_at_pos(self, source: &InputSource, position: InputPosition, msg: String) -> Self {
        self.with_at_pos(StatementKind::Info, source, position, msg)
    }

    pub fn with_info_str_at_pos(self, source: &InputSource, position: InputPosition, msg: &str) -> Self {
        self.with_at_pos(StatementKind::Info, source, position, msg.to_string())
    }

    pub fn with_info_at_span(self, source: &InputSource, span: InputSpan, msg: String) -> Self {
        self.with_at_span(StatementKind::Info, source, span, msg)
    }

    pub fn with_info_str_at_span(self, source: &InputSource, span: InputSpan, msg: &str) -> Self {
        self.with_at_span(StatementKind::Info, source, span, msg.to_string())
    }
}

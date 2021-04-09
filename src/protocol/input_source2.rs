use std::fmt;
use std::cell::{Ref, RefCell};

#[derive(Debug, Clone, Copy)]
pub struct InputPosition2 {
    pub line: u32,
    pub offset: u32,
}

pub struct InputSpan {
    pub begin: InputPosition2,
    pub end: InputPosition2,
}

impl InputSpan {
    #[inline]
    fn from_positions(begin: InputPosition2, end: InputPosition2) -> Self {
        Self { begin, end }
    }
}

/// Wrapper around source file with optional filename. Ensures that the file is
/// only scanned once.
pub struct InputSource2 {
    pub(crate) filename: String,
    pub(crate) input: Vec<u8>,
    // Iteration
    line: u32,
    offset: usize,
    // State tracking
    had_error: Option<ParseError>,
    // The offset_lookup is built on-demand upon attempting to report an error.
    // As the compiler is currently not multithreaded, we simply put it in a 
    // RefCell to allow interior mutability.
    offset_lookup: RefCell<Vec<u32>>,
}

impl InputSource2 {
    pub fn new(filename: String, input: Vec<u8>) -> Self {
        Self{
            filename,
            input,
            line: 1,
            offset: 0,
            had_error: None,
            offset_lookup: RefCell::new(Vec::new()),
        }
    }

    #[inline]
    pub fn pos(&self) -> InputPosition2 {
        InputPosition2{ line: self.line, offset: self.offset as u32 }
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

    pub fn section(&self, start: u32, end: u32) -> &[u8] {
        &self.input[start as usize..end as usize]
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
            self.had_error = Some(ParseError::new_error(self, self.pos(), msg));
        }
    }

    fn get_lookup(&self) -> Ref<Vec<u32>> {
        // Once constructed the lookup always contains one element. We use this
        // to see if it is constructed already.
        let lookup = self.offset_lookup.borrow();
        if !lookup.is_empty() {
            return lookup;
        }

        // Build the line number (!) to offset lookup, so offset by 1. We 
        // assume the entire source file is scanned (most common case) for
        // preallocation.
        let lookup = self.offset_lookup.borrow_mut();
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
        let lookup = self.offset_lookup.borrow();
        return lookup;
    }

    fn lookup_line_start_offset(&self, line_number: u32) -> u32 {
        let lookup = self.get_lookup();
        lookup[line_number as usize]
    }

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
pub enum ParseErrorType {
    Info,
    Error
}

#[derive(Debug)]
pub struct ParseErrorStatement {
    pub(crate) error_type: ParseErrorType,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) offset: u32,
    pub(crate) filename: String,
    pub(crate) context: String,
    pub(crate) message: String,
}

impl ParseErrorStatement {
    fn from_source(error_type: ParseErrorType, source: &InputSource2, position: InputPosition2, msg: &str) -> Self {
        // Seek line start and end
        let line_start = source.lookup_line_start_offset(position.line);
        let line_end = source.lookup_line_end_offset(position.line);
        debug_assert!(position.offset >= line_start);
        let column = position.offset - line_start + 1;

        Self{
            error_type,
            line: position.line,
            column,
            offset: position.offset,
            filename: source.filename.clone(),
            context: String::from_utf8_lossy(&source.input[line_start as usize..line_end as usize]).to_string(),
            message: msg.to_string()
        }
    }
}

impl fmt::Display for ParseErrorStatement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Write message
        match self.error_type {
            ParseErrorType::Info => write!(f, " INFO: ")?,
            ParseErrorType::Error => write!(f, "ERROR: ")?,
        }
        writeln!(f, "{}", &self.message)?;

        // Write originating file/line/column
        if self.filename.is_empty() {
            writeln!(f, " +- at {}:{}", self.line, self.column)?;
        } else {
            writeln!(f, " +- at {}:{}:{}", self.filename, self.line, self.column)?;
        }

        // Write source context
        writeln!(f, " | ")?;
        writeln!(f, " | {}", self.context)?;

        // Write underline indicating where the error ocurred
        debug_assert!(self.column as usize <= self.context.chars().count());
        let mut arrow = String::with_capacity(self.context.len() + 3);
        arrow.push_str(" | ");
        let mut char_col = 1;
        for char in self.context.chars() {
            if char_col == self.column { break; }
            if char == '\t' {
                arrow.push('\t');
            } else {
                arrow.push(' ');
            }

            char_col += 1;
        }
        arrow.push('^');
        writeln!(f, "{}", arrow)?;

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
    pub fn empty() -> Self {
        Self{ statements: Vec::new() }
    }

    pub fn new_error(source: &InputSource2, position: InputPosition2, msg: &str) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source(ParseErrorType::Error, source, position, msg))}
    }

    pub fn with_prefixed(mut self, error_type: ParseErrorType, source: &InputSource2, position: InputPosition2, msg: &str) -> Self {
        self.statements.insert(0, ParseErrorStatement::from_source(error_type, source, position, msg));
        self
    }

    pub fn with_postfixed(mut self, error_type: ParseErrorType, source: &InputSource2, position: InputPosition2, msg: &str) -> Self {
        self.statements.push(ParseErrorStatement::from_source(error_type, source, position, msg));
        self
    }

    pub fn with_postfixed_info(self, source: &InputSource2, position: InputPosition2, msg: &str) -> Self {
        self.with_postfixed(ParseErrorType::Info, source, position, msg)
    }
}

use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;

use backtrace::Backtrace;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputSource {
    pub(crate) filename: String,
    pub(crate) input: Vec<u8>,
    line: usize,
    column: usize,
    offset: usize,
}

static STD_LIB_PDL: &'static [u8] = b"
primitive forward(in i, out o) {
    while(true) synchronous put(o, get(i));
}
primitive sync(in i, out o) {
    while(true) synchronous if(fires(i)) put(o, get(i));
}
primitive alternator(in i, out l, out r) {
    while(true) {
        synchronous if(fires(i)) put(l, get(i));
        synchronous if(fires(i)) put(r, get(i));
    }
}
primitive replicator(in i, out l, out r) {
    while(true) synchronous {
        if(fires(i)) {
            msg m = get(i);
            put(l, m);
            put(r, m);
        }
    }
}
primitive merger(in l, in r, out o) {
    while(true) synchronous {
        if(fires(l))      put(o, get(l));
        else if(fires(r)) put(o, get(r));
    }
}
";

impl InputSource {
    // Constructors
    pub fn new<R: io::Read, S: ToString>(filename: S, reader: &mut R) -> io::Result<InputSource> {
        let mut vec = Vec::new();
        reader.read_to_end(&mut vec)?;
        vec.extend(STD_LIB_PDL.to_vec());
        Ok(InputSource {
            filename: filename.to_string(),
            input: vec,
            line: 1,
            column: 1,
            offset: 0,
        })
    }
    // Constructor helpers
    pub fn from_file(path: &Path) -> io::Result<InputSource> {
        let filename = path.file_name();
        match filename {
            Some(filename) => {
                let mut f = File::open(path)?;
                InputSource::new(filename.to_string_lossy(), &mut f)
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, "Invalid path")),
        }
    }
    pub fn from_string(string: &str) -> io::Result<InputSource> {
        let buffer = Box::new(string);
        let mut bytes = buffer.as_bytes();
        InputSource::new(String::new(), &mut bytes)
    }
    pub fn from_buffer(buffer: &[u8]) -> io::Result<InputSource> {
        InputSource::new(String::new(), &mut Box::new(buffer))
    }
    // Internal methods
    pub fn pos(&self) -> InputPosition {
        InputPosition { line: self.line, column: self.column, offset: self.offset }
    }
    pub fn seek(&mut self, pos: InputPosition) {
        debug_assert!(pos.offset < self.input.len());
        self.line = pos.line;
        self.column = pos.column;
        self.offset = pos.offset;
    }
    // pub fn error<S: ToString>(&self, message: S) -> ParseError {
    //     self.pos().parse_error(message)
    // }
    pub fn is_eof(&self) -> bool {
        self.next() == None
    }

    pub fn next(&self) -> Option<u8> {
        if self.offset < self.input.len() {
            Some(self.input[self.offset])
        } else {
            None
        }
    }

    pub fn lookahead(&self, pos: usize) -> Option<u8> {
        let offset_pos = self.offset + pos;
        if offset_pos < self.input.len() {
            Some(self.input[offset_pos])
        } else {
            None
        }
    }

    pub fn has(&self, to_compare: &[u8]) -> bool {
        if self.offset + to_compare.len() <= self.input.len() {
            for idx in 0..to_compare.len() {
                if to_compare[idx] != self.input[self.offset + idx] {
                    return false;
                }
            }

            true
        } else {
            false
        }
    }

    pub fn consume(&mut self) {
        match self.next() {
            Some(x) if x == b'\r' && self.lookahead(1) != Some(b'\n') || x == b'\n' => {
                self.line += 1;
                self.offset += 1;
                self.column = 1;
            }
            Some(_) => {
                self.offset += 1;
                self.column += 1;
            }
            None => {}
        }
    }
}

impl fmt::Display for InputSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.pos().fmt(f)
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct InputPosition {
    line: usize,
    column: usize,
    offset: usize,
}

impl InputPosition {
    fn context<'a>(&self, source: &'a InputSource) -> &'a [u8] {
        let start = self.offset - (self.column - 1);
        let mut end = self.offset;
        while end < source.input.len() {
            let cur = (*source.input)[end];
            if cur == b'\n' || cur == b'\r' {
                break;
            }
            end += 1;
        }
        &source.input[start..end]
    }
    // fn parse_error<S: ToString>(&self, message: S) -> ParseError {
    //     ParseError { position: *self, message: message.to_string(), backtrace: Backtrace::new() }
    // }
    fn eval_error<S: ToString>(&self, message: S) -> EvalError {
        EvalError { position: *self, message: message.to_string(), backtrace: Backtrace::new() }
    }
}

impl Default for InputPosition {
    fn default() -> Self {
        Self{ line: 1, column: 1, offset: 0 }
    }
}

impl fmt::Display for InputPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

pub trait SyntaxElement {
    fn position(&self) -> InputPosition;
    fn error<S: ToString>(&self, message: S) -> EvalError {
        self.position().eval_error(message)
    }
}

#[derive(Debug)]
pub enum ParseErrorType {
    Info,
    Error
}

#[derive(Debug)]
pub struct ParseErrorStatement {
    error_type: ParseErrorType,
    position: InputPosition,
    filename: String,
    context: String,
    message: String,
}

impl ParseErrorStatement {
    fn from_source(error_type: ParseErrorType, source: &InputSource, position: InputPosition, msg: &str) -> Self {
        // Seek line start and end
        println!("DEBUG[1]:\nPos {}, msg: {},\nDEBUG[2]: In source:\n{}",
            position, msg, String::from_utf8_lossy(&source.input));
        debug_assert!(position.column < position.offset);
        let line_start = position.offset - (position.column - 1);
        let mut line_end = position.offset;
        while line_end < source.input.len() && source.input[line_end] != b'\n' {
            line_end += 1;
        }

        // Compensate for '\r\n'
        if line_end > line_start && source.input[line_end - 1] == b'\r' {
            line_end -= 1;
        }

        Self{
            error_type,
            position,
            filename: source.filename.clone(),
            context: String::from_utf8_lossy(&source.input[line_start..line_end]).to_string(),
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
        writeln!(f, "{}", &self.message);

        // Write originating file/line/column
        if self.filename.is_empty() {
            writeln!(f, " +- at {}:{}", self.position.line, self.position.column)?;
        } else {
            writeln!(f, " +- at {}:{}:{}", self.filename, self.position.line, self.position.column)?;
        }

        // Write source context
        writeln!(f, " | ")?;
        writeln!(f, " | {}", self.context)?;

        // Write underline indicating where the error ocurred
        debug_assert!(self.position.column <= self.context.chars().count());
        let mut arrow = String::with_capacity(self.context.len() + 3);
        arrow.push_str(" | ");
        let mut char_col = 1;
        for char in self.context.chars() {
            if char_col == self.position.column { break; }
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
pub struct ParseError2 {
    statements: Vec<ParseErrorStatement>
}

impl fmt::Display for ParseError2 {
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

impl ParseError2 {
    pub fn empty() -> Self {
        Self{ statements: Vec::new() }
    }

    pub fn new_error(source: &InputSource, position: InputPosition, msg: &str) -> Self {
        Self{ statements: vec!(ParseErrorStatement::from_source(ParseErrorType::Error, source, position, msg))}
    }

    pub fn with_prefixed(mut self, error_type: ParseErrorType, source: &InputSource, position: InputPosition, msg: &str) -> Self {
        self.statements.insert(0, ParseErrorStatement::from_source(error_type, source, position, msg));
        self
    }

    pub fn with_postfixed(mut self, error_type: ParseErrorType, source: &InputSource, position: InputPosition, msg: &str) -> Self {
        self.statements.push(ParseErrorStatement::from_source(error_type, source, position, msg));
        self
    }

    pub fn with_postfixed_info(self, source: &InputSource, position: InputPosition, msg: &str) -> Self {
        self.with_postfixed(ParseErrorType::Info, source, position, msg)
    }
}

#[derive(Debug, Clone)]
pub struct EvalError {
    position: InputPosition,
    message: String,
    backtrace: Backtrace,
}

impl EvalError {
    pub fn new<S: ToString>(position: InputPosition, message: S) -> EvalError {
        EvalError { position, message: message.to_string(), backtrace: Backtrace::new() }
    }
    // Diagnostic methods
    pub fn write<A: io::Write>(&self, source: &InputSource, writer: &mut A) -> io::Result<()> {
        if !source.filename.is_empty() {
            writeln!(
                writer,
                "Evaluation error at {}:{}: {}",
                source.filename, self.position, self.message
            )?;
        } else {
            writeln!(writer, "Evaluation error at {}: {}", self.position, self.message)?;
        }
        let line = self.position.context(source);
        writeln!(writer, "{}", String::from_utf8_lossy(line))?;
        let mut arrow: Vec<u8> = Vec::new();
        for pos in 1..self.position.column {
            let c = line[pos - 1];
            if c == b'\t' {
                arrow.push(b'\t')
            } else {
                arrow.push(b' ')
            }
        }
        arrow.push(b'^');
        writeln!(writer, "{}", String::from_utf8_lossy(&arrow))
    }
    pub fn print(&self, source: &InputSource) {
        self.write(source, &mut std::io::stdout()).unwrap()
    }
    pub fn display<'a>(&'a self, source: &'a InputSource) -> DisplayEvalError<'a> {
        DisplayEvalError::new(self, source)
    }
}

impl From<EvalError> for io::Error {
    fn from(_: EvalError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, "eval error")
    }
}

#[derive(Clone, Copy)]
pub struct DisplayEvalError<'a> {
    error: &'a EvalError,
    source: &'a InputSource,
}

impl DisplayEvalError<'_> {
    fn new<'a>(error: &'a EvalError, source: &'a InputSource) -> DisplayEvalError<'a> {
        DisplayEvalError { error, source }
    }
}

impl fmt::Display for DisplayEvalError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut vec: Vec<u8> = Vec::new();
        match self.error.write(self.source, &mut vec) {
            Err(_) => {
                return fmt::Result::Err(fmt::Error);
            }
            Ok(_) => {}
        }
        write!(f, "{}", String::from_utf8_lossy(&vec))
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_from_string() {
//         let mut is = InputSource::from_string("#version 100\n").unwrap();
//         assert!(is.input.len() == 13);
//         assert!(is.line == 1);
//         assert!(is.column == 1);
//         assert!(is.offset == 0);
//         let ps = is.pos();
//         assert!(ps.line == 1);
//         assert!(ps.column == 1);
//         assert!(ps.offset == 0);
//         assert!(is.next() == Some(b'#'));
//         is.consume();
//         assert!(is.next() == Some(b'v'));
//         assert!(is.lookahead(1) == Some(b'e'));
//         is.consume();
//         assert!(is.next() == Some(b'e'));
//         is.consume();
//         assert!(is.next() == Some(b'r'));
//         is.consume();
//         assert!(is.next() == Some(b's'));
//         is.consume();
//         assert!(is.next() == Some(b'i'));
//         is.consume();
//         {
//             let ps = is.pos();
//             assert_eq!(b"#version 100", ps.context(&is));
//             let er = is.error("hello world!");
//             let mut vec: Vec<u8> = Vec::new();
//             er.write(&is, &mut vec).unwrap();
//             assert_eq!(
//                 "Parse error at 1:7: hello world!\n#version 100\n      ^\n",
//                 String::from_utf8_lossy(&vec)
//             );
//         }
//         assert!(is.next() == Some(b'o'));
//         is.consume();
//         assert!(is.next() == Some(b'n'));
//         is.consume();
//         assert!(is.input.len() == 13);
//         assert!(is.line == 1);
//         assert!(is.column == 9);
//         assert!(is.offset == 8);
//         assert!(is.next() == Some(b' '));
//         is.consume();
//         assert!(is.next() == Some(b'1'));
//         is.consume();
//         assert!(is.next() == Some(b'0'));
//         is.consume();
//         assert!(is.next() == Some(b'0'));
//         is.consume();
//         assert!(is.input.len() == 13);
//         assert!(is.line == 1);
//         assert!(is.column == 13);
//         assert!(is.offset == 12);
//         assert!(is.next() == Some(b'\n'));
//         is.consume();
//         assert!(is.input.len() == 13);
//         assert!(is.line == 2);
//         assert!(is.column == 1);
//         assert!(is.offset == 13);
//         {
//             let ps = is.pos();
//             assert_eq!(b"", ps.context(&is));
//         }
//         assert!(is.next() == None);
//         is.consume();
//         assert!(is.next() == None);
//     }

//     #[test]
//     fn test_split() {
//         let mut is = InputSource::from_string("#version 100\n").unwrap();
//         let backup = is.clone();
//         assert!(is.next() == Some(b'#'));
//         is.consume();
//         assert!(is.next() == Some(b'v'));
//         is.consume();
//         assert!(is.next() == Some(b'e'));
//         is.consume();
//         is = backup;
//         assert!(is.next() == Some(b'#'));
//         is.consume();
//         assert!(is.next() == Some(b'v'));
//         is.consume();
//         assert!(is.next() == Some(b'e'));
//         is.consume();
//     }
// }

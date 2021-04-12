use crate::protocol::Heap;
use crate::protocol::tokenizer::{TokenBuffer, Token};
use crate::protocol::input_source2::{InputSource2 as InputSource, ParseError};

struct Ctx<'a> {
    heap: &'a mut Heap,
    source: &'a InputSource,
    tokens: &'a TokenBuffer,
}

// Lexes definitions. Should be the first pass over each of the module files 
// after tokenization. Only once all definitions are parsed can we do the full
// AST creation pass.
struct LexerDefinitions {

}

impl LexerDefinitions {
    pub(crate) fn parse(ctx: &mut Ctx) -> Result<(), ParseError> {
        debug_assert!(ctx.tokens.ranges.len() > 0);
    }

    pub(crate) fn parse_definition(heap: &mut Heap, source: &InputSource, range: &TokenRang)
}
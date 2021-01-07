use crate::protocol::ast::*;
use crate::protocol::input_source::*;
use crate::protocol::lexer::*;

type Unit = ();
type VisitorResult = Result<Unit, ParseError>;

trait ShallowVisitor: Sized {
    fn visit_protocol_description(&mut self, h: &mut Heap, pd: RootId) -> 
}
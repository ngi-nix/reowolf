/// eval
///
/// Evaluator of the generated AST. Note that we use some misappropriated terms
/// to describe where values live and what they do. This is a temporary
/// implementation of an evaluator until some kind of appropriate bytecode or
/// machine code is generated.
///
/// Code is always executed within a "frame". For Reowolf the first frame is
/// usually an executed component. All subsequent frames are function calls.
/// Simple values live on the "stack". Each variable/parameter has a place on
/// the stack where its values are stored. If the value is not a primitive, then
/// its value will be stored in the "heap". Expressions are treated differently
/// and use a separate "stack" for their evaluation.
///
/// Since this is a value-based language, most values are copied. One has to be
/// careful with values that reside in the "heap" and make sure that copies are
/// properly removed from the heap..
///
/// Just to reiterate: this is a temporary wasteful implementation. A proper
/// implementation would fully fill out the type table with alignment/size/
/// offset information and lay out bytecode.

mod value;
mod store;
mod executor;

pub use value::{Value, ValueGroup};
pub use executor::{EvalContinuation, Prompt};


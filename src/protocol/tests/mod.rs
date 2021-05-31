/**
 * protocol/tests.rs
 *
 * Contains tests for various parts of the lexer/parser and the evaluator of the
 * code. These are intended to be temporary tests such that we're sure that we
 * don't break existing functionality.
 *
 * In the future these should be replaced by proper testing protocols.
 *
 * If any of these tests fail, and you think they're not needed anymore, feel
 * free to cast them out into oblivion, where dead code goes to die.
 */

mod utils;
mod lexer;
mod parser_validation;
mod parser_inference;
mod parser_monomorphs;
mod parser_imports;
mod parser_binding;
mod parser_literals;
mod eval_operators;
mod eval_calls;
mod eval_casting;
mod eval_binding;
mod eval_silly;

pub(crate) use utils::{Tester}; // the testing harness
pub(crate) use crate::protocol::eval::value::*; // to test functions
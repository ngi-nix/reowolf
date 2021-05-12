/**
 * protocol/tests.rs
 *
 * Contains tests for various parts of the lexer/parser and the evaluator of the
 * code. These are intended to be temporary tests such that we're sure that we
 * don't break existing functionality.
 *
 * In the future these should be replaced by proper testing protocols.
 */

mod utils;
mod lexer;
mod parser_validation;
mod parser_inference;
mod parser_monomorphs;
mod parser_imports;
mod eval_operators;
mod eval_calls;

pub(crate) use utils::{Tester};
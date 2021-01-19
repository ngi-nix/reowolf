use crate::protocol::parser::{type_table::*, symbol_table::*, LexedModule};
use crate::protocol::inputsource::*;
use crate::protocol::ast::*;

type FnResult = Result<(), ParseError2>;

pub(crate) struct Ctx<'a> {
    module: &'a LexedModule,
    heap: &'a mut Heap,
    types: &'a TypeTable,
    symbols: &'a SymbolTable,
}
pub(crate) struct FunctionParser {

}

impl FunctionParser {
    pub(crate) fn new() -> Self {
        Self{}
    }

    pub(crate) fn visit_function_definition(&mut self, ctx: &mut Ctx, id: DefinitionId) -> FnResult {
        let body_id = ctx.heap[id].as_function().body;

        // Three passes:
        // 1. first pass: breadth first parsing of elements
        // 2. second pass: insert any new statments that we queued in the first pass
        // 3. third pass: traverse children

        for stmt_id in ctx.heap[body_id].as_block().statements.iter() {
            let stmt = &ctx.heap[*stmt_id];
            match stmt {
                Statement::Block(block) => {
                    // Mark for later traversal
                },
                Statement::Local(LocalStatement::Channel(stmt)) => {
                    return Err(ParseError2::new_error(&ctx.module.source, stmt.position, "Illegal channel instantiation in function"));
                },
                Statement::Local(LocalStatement::Memory(stmt)) => {
                    // Resolve type, add to locals
                },
                Statement::Skip(skip) => {},
                Statement::Labeled(label) => {
                    // Add label, then parse statement
                },
                Statement::If(stmt) => {
                    // Mark for later traversal,
                    // Mark for insertion of "EndIf"
                },
                Statement::EndIf(_) => {},
                Statement::While(stmt) => {
                    // Mark for later traversal
                    // Mark for insertion of "EndWhile"
                },
                Statement::EndWhile(_) => {},
                Statement::Break(stmt) => {
                    // Perform "break" resolving if labeled. If not labeled then
                    // use end-while statment
                },
                Statement::Continue(stmt) => {
                    // Perform "continue" resolving if labeled. If not labeled
                    // then use end-while statement
                },
                Statement::Synchronous(stmt) => {
                    // Illegal!
                },
                Statement::EndSynchronous(stmt) => {
                    // Illegal, but can't possibly be encountered here
                },
                Statement::Return(stmt) => {
                    // Return statement, should be used in control flow analysis
                    // somehow. Because we need every branch to have a "return"
                    // call in the language.
                },
                Statement::Assert(stmt) => {
                    // Not allowed in functions? Is allowed in functions?
                },
                Statement::Goto(stmt) => {
                    // Goto label resolving, may not skip definition of
                    // variables.
                },
                Statement::New(stmt) => {
                    // Illegal in functions?
                },
                Statement::Put(stmt) => {
                    // This really needs to be a builtin call...
                },
                Statement::Expression(expr) => {
                    // Bare expression, we must traverse in order to determine
                    // if all variables are properly resolved.
                }
            }
        }

        Ok(())
    }
}
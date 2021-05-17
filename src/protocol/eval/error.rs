use std::fmt;

use crate::protocol::{
    ast::*,
    Module,
    input_source::{ErrorStatement, StatementKind}
};
use super::executor::*;

/// Represents a stack frame recorded in an error
#[derive(Debug)]
pub struct EvalFrame {
    pub line: u32,
    pub module_name: String,
    pub procedure: String, // function or component
    pub is_func: bool,
}

impl fmt::Display for EvalFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let func_or_comp = if self.is_func {
            "function "
        } else {
            "component"
        };

        if self.module_name.is_empty() {
            write!(f, "{} {}:{}", func_or_comp, &self.procedure, self.line)
        } else {
            write!(f, "{} {}:{}:{}", func_or_comp, &self.module_name, &self.procedure, self.line)
        }
    }
}

/// Represents an error that ocurred during evaluation. Contains error
/// statements just like in parsing errors. Additionally may display the current
/// execution state.
#[derive(Debug)]
pub struct EvalError {
    pub(crate) statements: Vec<ErrorStatement>,
    pub(crate) frames: Vec<EvalFrame>,
}

impl EvalError {
    pub(crate) fn new_error_at_expr(prompt: &Prompt, modules: &[Module], heap: &Heap, expr_id: ExpressionId, msg: String) -> EvalError {
        // Create frames
        debug_assert!(!prompt.frames.is_empty());
        let mut frames = Vec::with_capacity(prompt.frames.len());
        let mut last_module_source = &modules[0].source;
        for frame in prompt.frames.iter() {
            let definition = &heap[frame.definition];
            let statement = &heap[frame.position];
            let statement_span = statement.span();

            let (root_id, procedure, is_func) = match definition {
                Definition::Function(def) => {
                    (def.defined_in, def.identifier.value.as_str().to_string(), true)
                },
                Definition::Component(def) => {
                    (def.defined_in, def.identifier.value.as_str().to_string(), false)
                },
                _ => unreachable!("construct stack frame with definition pointing to data type")
            };

            // Lookup module name, if it has one
            let module = modules.iter().find(|m| m.root_id == root_id).unwrap();
            let module_name = if let Some(name) = &module.name {
                name.as_str().to_string()
            } else {
                String::new()
            };

            last_module_source = &module.source;
            frames.push(EvalFrame{
                line: statement_span.begin.line,
                module_name,
                procedure,
                is_func
            });
        }

        let expr = &heap[expr_id];
        let statements = vec![
            ErrorStatement::from_source_at_span(StatementKind::Error, last_module_source, expr.span(), msg)
        ];

        EvalError{ statements, frames }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display error statement(s)
        self.statements[0].fmt(f)?;
        for statement in self.statements.iter().skip(1) {
            writeln!(f)?;
            statement.fmt(f)?;
        }

        // Display stack trace
        writeln!(f)?;
        writeln!(f, " +-  Stack trace:")?;
        for frame in self.frames.iter().rev() {
            write!(f, " | ")?;
            frame.fmt(f)?;
            writeln!(f)?;
        }

        Ok(())
    }
}
use std::fmt::{Display, Write};
use std::io::Write as IOWrite;

use super::ast::*;
use std::borrow::Borrow;

const INDENT: usize = 2;

const PREFIX_EMPTY: &'static str = "    ";
const PREFIX_ROOT_ID: &'static str = "Root";
const PREFIX_PRAGMA_ID: &'static str = "Prag";
const PREFIX_IMPORT_ID: &'static str = "Imp ";
const PREFIX_TYPE_ANNOT_ID: &'static str = "TyAn";
const PREFIX_VARIABLE_ID: &'static str = "Var ";
const PREFIX_PARAMETER_ID: &'static str = "Par ";
const PREFIX_LOCAL_ID: &'static str = "Loc ";
const PREFIX_DEFINITION_ID: &'static str = "Def ";
const PREFIX_STRUCT_ID: &'static str = "DefS";
const PREFIX_ENUM_ID: &'static str = "DefE";
const PREFIX_COMPONENT_ID: &'static str = "DefC";
const PREFIX_FUNCTION_ID: &'static str = "DefF";
const PREFIX_STMT_ID: &'static str = "Stmt";
const PREFIX_BLOCK_STMT_ID: &'static str = "SBl ";
const PREFIX_LOCAL_STMT_ID: &'static str = "SLoc";
const PREFIX_MEM_STMT_ID: &'static str = "SMem";
const PREFIX_CHANNEL_STMT_ID: &'static str = "SCha";
const PREFIX_SKIP_STMT_ID: &'static str = "SSki";
const PREFIX_LABELED_STMT_ID: &'static str = "SLab";
const PREFIX_IF_STMT_ID: &'static str = "SIf ";
const PREFIX_ENDIF_STMT_ID: &'static str = "SEIf";
const PREFIX_WHILE_STMT_ID: &'static str = "SWhi";
const PREFIX_ENDWHILE_STMT_ID: &'static str = "SEWh";
const PREFIX_BREAK_STMT_ID: &'static str = "SBre";
const PREFIX_CONTINUE_STMT_ID: &'static str = "SCon";
const PREFIX_SYNC_STMT_ID: &'static str = "SSyn";
const PREFIX_ENDSYNC_STMT_ID: &'static str = "SESy";
const PREFIX_RETURN_STMT_ID: &'static str = "SRet";
const PREFIX_ASSERT_STMT_ID: &'static str = "SAsr";
const PREFIX_GOTO_STMT_ID: &'static str = "SGot";
const PREFIX_NEW_STMT_ID: &'static str = "SNew";
const PREFIX_PUT_STMT_ID: &'static str = "SPut";
const PREFIX_EXPR_STMT_ID: &'static str = "SExp";
const PREFIX_ASSIGNMENT_EXPR_ID: &'static str = "EAsi";
const PREFIX_CONDITIONAL_EXPR_ID: &'static str = "ECnd";
const PREFIX_BINARY_EXPR_ID: &'static str = "EBin";
const PREFIX_UNARY_EXPR_ID: &'static str = "EUna";
const PREFIX_INDEXING_EXPR_ID: &'static str = "EIdx";
const PREFIX_SLICING_EXPR_ID: &'static str = "ESli";
const PREFIX_SELECT_EXPR_ID: &'static str = "ESel";
const PREFIX_ARRAY_EXPR_ID: &'static str = "EArr";
const PREFIX_CONST_EXPR_ID: &'static str = "ECns";
const PREFIX_CALL_EXPR_ID: &'static str = "ECll";
const PREFIX_VARIABLE_EXPR_ID: &'static str = "EVar";

pub(crate) struct ASTWriter {
    buffer: String,
    temp: String,
}

impl ASTWriter {
    pub(crate) fn new() -> Self {
        Self{ buffer: String::with_capacity(4096), temp: String::with_capacity(256) }
    }
    pub(crate) fn write_ast<W: IOWrite>(&mut self, w: &mut W, heap: &Heap) {
        for root_id in heap.protocol_descriptions.iter().map(|v| v.this) {
            self.write_module(heap, root_id);
            w.write_all(self.buffer.as_bytes()).expect("flush buffer");
            self.buffer.clear();
        }
    }

    //--------------------------------------------------------------------------
    // Top-level module writing
    //--------------------------------------------------------------------------

    fn write_module(&mut self, heap: &Heap, root_id: RootId) {
        self.write_id_and_indent(PREFIX_ROOT_ID, root_id.0.index, 0);
        self.buffer.write_str("Module:\n");

        let root = &heap[root_id];

        self.write_indent(0);
        write!(&mut self.buffer, "- ID: {}\n", root.this.0.index);

        self.write_indent(0);
        self.buffer.write_str("- Pragmas:\n");

        for pragma_id in &root.pragmas {
            self.write_pragma(&heap[*pragma_id], 1);
        }

        self.write_indent(0);
        self.buffer.write_str("- Imports:\n");

        for import_id in &root.imports {
            self.write_import(&heap[*import_id], 1);
        }

        self.write_indent(0);
        self.buffer.write_str("- Definitions:\n");

        for definition_id in &root.definitions {
            self.write_definition(heap, &heap[*definition_id], 1);
        }
    }

    fn write_pragma(&mut self, pragma: &Pragma, indent: usize) {
        match pragma {
            Pragma::Version(pragma) => {
                self.write_id_and_indent(PREFIX_PRAGMA_ID, pragma.this.0.index, indent);
                write!(&mut self.buffer, "- Version: {}\n", pragma.version);
            },
            Pragma::Module(pragma) => {
                self.write_id_and_indent(PREFIX_PRAGMA_ID, pragma.this.0.index, indent);
                write!(&mut self.buffer, "- Module: {}\n", String::from_utf8_lossy(&pragma.value));
            }
        }
    }

    fn write_import(&mut self, import: &Import, indent: usize) {
        let indent2 = indent + 1;
        match import {
            Import::Module(import) => {
                self.write_id_and_indent(PREFIX_IMPORT_ID, import.this.0.index, indent);
                write!(&mut self.buffer, "- ModuleImport:\n");

                self.write_indent(indent2);
                write!(&mut self.buffer, "- Name: {}\n", String::from_utf8_lossy(&import.module_name));
                self.write_indent(indent2);
                write!(&mut self.buffer, "- Alias: {}\n", String::from_utf8_lossy(&import.alias));
                self.write_indent(indent2);
                write_option(&mut self.temp, import.module_id.map(|v| v.0.index));
                write!(&mut self.buffer, "- Target: {}\n", &self.temp);
            },
            Import::Symbols(import) => {
                self.write_id_and_indent(PREFIX_IMPORT_ID, import.this.0.index, indent);
                write!(&mut self.buffer, "- SymbolImport:\n");

                self.write_indent(indent2);
                write!(&mut self.buffer, "- Name: {}\n", String::from_utf8_lossy(&import.module_name));
                self.write_indent(indent2);
                write_option(&mut self.temp, import.module_id.map(|v| v.0.index));
                write!(&mut self.buffer, "- Target: {}\n", &self.temp);

                self.write_indent(indent2);
                write!(&mut self.buffer, "- Symbols:\n");

                let indent3 = indent2 + 1;
                let indent4 = indent3 + 1;
                for symbol in &import.symbols {
                    self.write_indent(indent3);
                    write!(&mut self.buffer, "- AliasedSymbol:\n");
                    self.write_indent(indent4);
                    write!(&mut self.buffer, "- Name: {}\n", String::from_utf8_lossy(&symbol.name));
                    self.write_indent(indent4);
                    write!(&mut self.buffer, "- Alias: {}\n", String::from_utf8_lossy(&symbol.alias));
                    self.write_indent(indent4);
                    write_option(&mut self.temp, symbol.definition_id.map(|v| v.0.index));
                    write!(&mut self.buffer, "- Definition: {}\n", &self.temp);
                }
            }
        }
    }

    //--------------------------------------------------------------------------
    // Top-level definition writing
    //--------------------------------------------------------------------------

    fn write_definition(&mut self, heap: &Heap, def: &Definition, indent: usize) {
        match def {
            Definition::Struct(_) => todo!("implement Definition::Struct"),
            Definition::Enum(_) => todo!("implement Definition::Enum"),
            Definition::Function(_) => todo!("implement Definition::Function"),
            Definition::Component(def) => {
                self.write_id_and_indent(PREFIX_COMPONENT_ID, def.this.0.0.index, indent);
                write!(&mut self.buffer, "- Component:\n");

                let indent2 = indent + 1;
                let indent3 = indent2 + 1;
                let indent4 = indent3 + 1;
                self.write_indent(indent2);
                write!(&mut self.buffer, "- Name: {}\n", String::from_utf8_lossy(&def.identifier.value));
                self.write_indent(indent2);
                write!(&mut self.buffer, "- Variant: {:?}\n", &def.variant);
                self.write_indent(indent2);
                write!(&mut self.buffer, "- Parameters:\n");

                for parameter_id in &def.parameters {
                    let param = &heap[*parameter_id];
                    self.write_indent(indent3);
                    write!(&mut self.buffer, "- Parameter\n");

                    self.write_indent(indent4);
                    write!(&mut self.buffer, "- Name: {}\n", String::from_utf8_lossy(&param.identifier.value));
                    self.write_indent(indent4);
                    write!(&mut self.buffer, "- Type: ");
                    write_type(&mut self.buffer, &heap[param.type_annotation]);
                    self.buffer.push('\n');
                }

                self.write_indent(indent2);
                write!(&mut self.buffer, "- Body:\n");

                self.write_stmt(heap, def.body, indent3);
            }
        }
    }

    fn write_stmt(&mut self, heap: &Heap, stmt_id: StatementId, indent: usize) {
        let stmt = &heap[stmt_id];

        match stmt {
            Statement::Block(stmt) => {
                self.write_id_and_indent(PREFIX_BLOCK_STMT_ID, stmt.this.0.0.index, indent);
                write!(&mut self.buffer, "- Block:\n");
                for stmt_id in &stmt.statements {
                    self.write_stmt(heap, *stmt_id, indent + 1);
                }
            },
            Statement::Local(stmt) => {
                match stmt {
                    LocalStatement::Channel(stmt) => {
                        self.write_id_and_indent(PREFIX_CHANNEL_STMT_ID, stmt.this.0.0.0.index, indent);
                        write!(&mut self.buffer, "- Channel:\n");

                        let indent2 = indent + 1;
                        let indent3 = indent2 + 1;
                        let from = &heap[stmt.from];
                        self.write_id_and_indent(PREFIX_LOCAL_ID, stmt.from.0.0.index, indent2);
                    },
                    LocalStatement::Memory(stmt) => {

                    }
                }
            },
            Statement::Skip(stmt) => {
                self.write_id_and_indent(PREFIX_SKIP_STMT_ID, stmt.this.0.0.index, indent);
                write!(&mut self.buffer, "- Skip\n");
            },
            Statement::Labeled(stmt) => {
                self.write_id_and_indent(PREFIX_LABELED_STMT_ID, stmt.this.0.0.index, indent);
                write!(&mut self.buffer, "- LabeledStatement\n");

                let indent1 = indent + 1;
                let indent2 = indent + 2;
                self.write_indent(indent1);
                write!(&mut self.buffer, "- Label: {}\n", String::from_utf8_lossy(&stmt.label.value));
                self.write_indent(indent1);
                write!(&mut self.buffer, "- Statement:\n");
                self.write_stmt(heap, stmt.body, indent2);
            },
            Statement::If(stmt) => {

            },
            Statement::EndIf(stmt) => {

            },
            Statement::While(stmt) => {

            },
            Statement::EndWhile(stmt) => {

            },
            Statement::Break(stmt) => {

            },
            Statement::Continue(stmt) => {

            },
            Statement::Synchronous(stmt) => {

            },
            Statement::EndSynchronous(stmt) => {

            },
            Statement::Return(stmt) => {

            },
            Statement::Assert(stmt) => {

            },
            Statement::Goto(stmt) => {

            },
            Statement::New(stmt) => {

            },
            Statement::Put(stmt) => {

            },
            Statement::Expression(stmt) => {

            }
        }
    }

    fn write_local(&mut self, heap: &Heap, local_id: LocalId, indent: usize) {
        o
    }

    //--------------------------------------------------------------------------
    // Printing Utilities
    //--------------------------------------------------------------------------

    fn write_id_and_indent(&mut self, prefix: &'static str, id: u32, indent: usize) {
        write!(&mut self.buffer, "{}[{:04}] ", prefix, id);
        for _ in 0..indent*INDENT {
            self.buffer.push(' ');
        }
    }

    fn write_indent(&mut self, indent: usize) {
        write!(&mut self.buffer, "{}       ", PREFIX_EMPTY);
        for _ in 0..indent*INDENT {
            self.buffer.push(' ');
        }
    }

    fn flush<W: IOWrite>(&mut self, w: &mut W) {
        w.write(self.buffer.as_bytes()).unwrap();
        self.buffer.clear()
    }
}

fn write_option<V: Display>(target: &mut String, value: Option<V>) {
    target.clear();
    match &value {
        Some(v) => write!(target, "Some({})", v),
        None => target.write_str("None")
    };
}

fn write_type(target: &mut String, t: &TypeAnnotation) {
    match &t.the_type.primitive {
        PrimitiveType::Input => target.write_str("in"),
        PrimitiveType::Output => target.write_str("out"),
        PrimitiveType::Message => target.write_str("msg"),
        PrimitiveType::Boolean => target.write_str("bool"),
        PrimitiveType::Byte => target.write_str("byte"),
        PrimitiveType::Short => target.write_str("short"),
        PrimitiveType::Int => target.write_str("int"),
        PrimitiveType::Long => target.write_str("long"),
        PrimitiveType::Symbolic(symbolic) => {
            let mut temp = String::new();
            write_option(&mut temp, symbolic.definition.map(|v| v.0.index));
            write!(target, "Symbolic(name: {}, target: {})", String::from_utf8_lossy(&symbolic.identifier.value), &temp);
        }
    };

    if t.the_type.array {
        target.push_str("[]");
    }
}
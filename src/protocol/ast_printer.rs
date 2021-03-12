use std::fmt::{Debug, Display, Write};
use std::io::Write as IOWrite;

use super::ast::*;

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

struct KV<'a> {
    buffer: &'a mut String,
    prefix: Option<(&'static str, u32)>,
    indent: usize,
    temp_key: &'a mut String,
    temp_val: &'a mut String,
}

impl<'a> KV<'a> {
    fn new(buffer: &'a mut String, temp_key: &'a mut String, temp_val: &'a mut String, indent: usize) -> Self {
        temp_key.clear();
        temp_val.clear();
        KV{
            buffer,
            prefix: None,
            indent,
            temp_key,
            temp_val
        }
    }

    fn with_id(mut self, prefix: &'static str, id: u32) -> Self {
        self.prefix = Some((prefix, id));
        self
    }

    fn with_s_key(self, key: &str) -> Self {
        self.temp_key.push_str(key);
        self
    }

    fn with_d_key<D: Display>(mut self, key: &D) -> Self {
        write!(&mut self.temp_key, "{}", key);
        self
    }

    fn with_s_val(self, val: &str) -> Self {
        self.temp_val.push_str(val);
        self
    }

    fn with_disp_val<D: Display>(mut self, val: &D) -> Self {
        write!(&mut self.temp_val, "{}", val);
        self
    }

    fn with_debug_val<D: Debug>(mut self, val: &D) -> Self {
        write!(&mut self.temp_val, "{:?}", val);
        self
    }

    fn with_ascii_val(self, val: &[u8]) -> Self {
        self.temp_val.write_str(&*String::from_utf8_lossy(val));
        self
    }

    fn with_opt_disp_val<D: Display>(mut self, val: Option<&D>) -> Self {
        match val {
            Some(v) => { write!(&mut self.temp_val, "Some({})", v); },
            None => { self.temp_val.write_str("None"); }
        }
        self
    }

    fn with_opt_ascii_val(self, val: Option<&[u8]>) -> Self {
        match val {
            Some(v) => {
                self.temp_val.write_str("Some(");
                self.temp_val.write_str(&*String::from_utf8_lossy(v));
                self.temp_val.write_char(')');
            },
            None => {
                self.temp_val.write_str("None");
            }
        }
        self
    }

    fn with_custom_val<F: Fn(&mut String)>(mut self, val_fn: F) -> Self {
        val_fn(&mut self.temp_val);
        self
    }
}

impl<'a> Drop for KV<'a> {
    fn drop(&mut self) {
        // Prefix and indent
        if let Some((prefix, id)) = &self.prefix {
            write!(&mut self.buffer, "{}[{:04}] ", prefix, id);
        } else {
            write!(&mut self.buffer, "           ");
        }

        for _ in 0..self.indent * INDENT {
            self.buffer.push(' ');
        }

        // Leading dash
        self.buffer.write_str("- ");

        // Key and value
        self.buffer.write_str(self.temp_key);
        if self.temp_val.is_empty() {
            self.buffer.push(':');
        } else {
            self.buffer.push_str(": ");
            self.buffer.push_str(&self.temp_val);
        }
        self.buffer.push('\n');
    }
}

pub(crate) struct ASTWriter {
    buffer: String,
    temp1: String,
    temp2: String,
}

impl ASTWriter {
    pub(crate) fn new() -> Self {
        Self{
            buffer: String::with_capacity(4096),
            temp1: String::with_capacity(256),
            temp2: String::with_capacity(256),
        }
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
        self.kv(0).with_id(PREFIX_ROOT_ID, root_id.index)
            .with_s_key("Module");

        let root = &heap[root_id];
        self.kv(1).with_s_key("Pragmas");
        for pragma_id in &root.pragmas {
            self.write_pragma(heap, *pragma_id, 2);
        }

        self.kv(1).with_s_key("Imports");
        for import_id in &root.imports {
            self.write_import(heap, *import_id, 2);
        }

        self.kv(1).with_s_key("Definitions");
        for def_id in &root.definitions {
            self.write_definition(heap, *def_id, 2);
        }
    }

    fn write_pragma(&mut self, heap: &Heap, pragma_id: PragmaId, indent: usize) {
        match &heap[pragma_id] {
            Pragma::Version(pragma) => {
                self.kv(indent).with_id(PREFIX_PRAGMA_ID, pragma.this.index)
                    .with_s_key("PragmaVersion")
                    .with_disp_val(&pragma.version);
            },
            Pragma::Module(pragma) => {
                self.kv(indent).with_id(PREFIX_PRAGMA_ID, pragma.this.index)
                    .with_s_key("PragmaModule")
                    .with_ascii_val(&pragma.value);
            }
        }
    }

    fn write_import(&mut self, heap: &Heap, import_id: ImportId, indent: usize) {
        let import = &heap[import_id];
        let indent2 = indent + 1;

        match import {
            Import::Module(import) => {
                self.kv(indent).with_id(PREFIX_IMPORT_ID, import.this.index)
                    .with_s_key("ImportModule");

                self.kv(indent2).with_s_key("Name").with_ascii_val(&import.module_name);
                self.kv(indent2).with_s_key("Alias").with_ascii_val(&import.alias);
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(import.module_id.as_ref().map(|v| &v.index));
            },
            Import::Symbols(import) => {
                self.kv(indent).with_id(PREFIX_IMPORT_ID, import.this.index)
                    .with_s_key("ImportSymbol");

                self.kv(indent2).with_s_key("Name").with_ascii_val(&import.module_name);
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(import.module_id.as_ref().map(|v| &v.index));

                self.kv(indent2).with_s_key("Symbols");

                let indent3 = indent2 + 1;
                let indent4 = indent3 + 1;
                for symbol in &import.symbols {
                    self.kv(indent3).with_s_key("AliasedSymbol");
                    self.kv(indent4).with_s_key("Name").with_ascii_val(&symbol.name);
                    self.kv(indent4).with_s_key("Alias").with_ascii_val(&symbol.alias);
                    self.kv(indent4).with_s_key("Definition")
                        .with_opt_disp_val(symbol.definition_id.as_ref().map(|v| &v.index));
                }
            }
        }
    }

    //--------------------------------------------------------------------------
    // Top-level definition writing
    //--------------------------------------------------------------------------

    fn write_definition(&mut self, heap: &Heap, def_id: DefinitionId, indent: usize) {
        let indent2 = indent + 1;
        let indent3 = indent2 + 1;
        let indent4 = indent3 + 1;

        match &heap[def_id] {
            Definition::Struct(_) => todo!("implement Definition::Struct"),
            Definition::Enum(_) => todo!("implement Definition::Enum"),
            Definition::Function(_) => todo!("implement Definition::Function"),
            Definition::Component(def) => {
                self.kv(indent).with_id(PREFIX_COMPONENT_ID,def.this.0.index)
                    .with_s_key("DefinitionComponent");

                self.kv(indent2).with_s_key("Name").with_ascii_val(&def.identifier.value);
                self.kv(indent2).with_s_key("Variant").with_debug_val(&def.variant);

                self.kv(indent2).with_s_key("Parameters");
                for param_id in &def.parameters {
                    let param = &heap[*param_id];
                    self.kv(indent3).with_id(PREFIX_PARAMETER_ID, param_id.0.index)
                        .with_s_key("Parameter");

                    self.kv(indent4).with_s_key("Name").with_ascii_val(&param.identifier.value);
                    self.kv(indent4).with_s_key("Type").with_custom_val(|w| write_type(w, heap, &heap[param.parser_type]));
                }

                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, def.body, indent3);
            }
        }
    }

    fn write_stmt(&mut self, heap: &Heap, stmt_id: StatementId, indent: usize) {
        let stmt = &heap[stmt_id];
        let indent2 = indent + 1;
        let indent3 = indent2 + 1;

        match stmt {
            Statement::Block(stmt) => {
                self.kv(indent).with_id(PREFIX_BLOCK_STMT_ID, stmt.this.0.index)
                    .with_s_key("Block");

                for stmt_id in &stmt.statements {
                    self.write_stmt(heap, *stmt_id, indent2);
                }
            },
            Statement::Local(stmt) => {
                match stmt {
                    LocalStatement::Channel(stmt) => {
                        self.kv(indent).with_id(PREFIX_CHANNEL_STMT_ID, stmt.this.0.0.index)
                            .with_s_key("LocalChannel");

                        self.kv(indent2).with_s_key("From");
                        self.write_local(heap, stmt.from, indent3);
                        self.kv(indent2).with_s_key("To");
                        self.write_local(heap, stmt.to, indent3);
                        self.kv(indent2).with_s_key("Next")
                            .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
                    },
                    LocalStatement::Memory(stmt) => {
                        self.kv(indent).with_id(PREFIX_MEM_STMT_ID, stmt.this.0.0.index)
                            .with_s_key("LocalMemory");

                        self.kv(indent2).with_s_key("Variable");
                        self.write_local(heap, stmt.variable, indent3);
                        self.kv(indent2).with_s_key("initial");
                        self.write_expr(heap, stmt.initial, indent3);
                        self.kv(indent2).with_s_key("Next")
                            .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
                    }
                }
            },
            Statement::Skip(stmt) => {
                self.kv(indent).with_id(PREFIX_SKIP_STMT_ID, stmt.this.0.index)
                    .with_s_key("Skip");
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Labeled(stmt) => {
                self.kv(indent).with_id(PREFIX_LABELED_STMT_ID, stmt.this.0.index)
                    .with_s_key("Labeled");

                self.kv(indent2).with_s_key("Label").with_ascii_val(&stmt.label.value);
                self.kv(indent2).with_s_key("Statement");
                self.write_stmt(heap, stmt.body, indent3);
            },
            Statement::If(stmt) => {
                self.kv(indent).with_id(PREFIX_IF_STMT_ID, stmt.this.0.index)
                    .with_s_key("If");

                self.kv(indent2).with_s_key("EndIf")
                    .with_opt_disp_val(stmt.end_if.as_ref().map(|v| &v.0.index));

                self.kv(indent2).with_s_key("Condition");
                self.write_expr(heap, stmt.test, indent3);

                self.kv(indent2).with_s_key("TrueBody");
                self.write_stmt(heap, stmt.true_body, indent3);

                self.kv(indent2).with_s_key("FalseBody");
                self.write_stmt(heap, stmt.false_body, indent3);
            },
            Statement::EndIf(stmt) => {
                self.kv(indent).with_id(PREFIX_ENDIF_STMT_ID, stmt.this.0.index)
                    .with_s_key("EndIf");
                self.kv(indent2).with_s_key("StartIf").with_disp_val(&stmt.start_if.0.index);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::While(stmt) => {
                self.kv(indent).with_id(PREFIX_WHILE_STMT_ID, stmt.this.0.index)
                    .with_s_key("While");

                self.kv(indent2).with_s_key("EndWhile")
                    .with_opt_disp_val(stmt.end_while.as_ref().map(|v| &v.0.index));
                self.kv(indent2).with_s_key("InSync")
                    .with_opt_disp_val(stmt.in_sync.as_ref().map(|v| &v.0.index));
                self.kv(indent2).with_s_key("Condition");
                self.write_expr(heap, stmt.test, indent3);
                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, stmt.body, indent3);
            },
            Statement::EndWhile(stmt) => {
                self.kv(indent).with_id(PREFIX_ENDWHILE_STMT_ID, stmt.this.0.index)
                    .with_s_key("EndWhile");
                self.kv(indent2).with_s_key("StartWhile").with_disp_val(&stmt.start_while.0.index);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Break(stmt) => {
                self.kv(indent).with_id(PREFIX_BREAK_STMT_ID, stmt.this.0.index)
                    .with_s_key("Break");
                self.kv(indent2).with_s_key("Label")
                    .with_opt_ascii_val(stmt.label.as_ref().map(|v| v.value.as_slice()));
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(stmt.target.as_ref().map(|v| &v.0.index));
            },
            Statement::Continue(stmt) => {
                self.kv(indent).with_id(PREFIX_CONTINUE_STMT_ID, stmt.this.0.index)
                    .with_s_key("Continue");
                self.kv(indent2).with_s_key("Label")
                    .with_opt_ascii_val(stmt.label.as_ref().map(|v| v.value.as_slice()));
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(stmt.target.as_ref().map(|v| &v.0.index));
            },
            Statement::Synchronous(stmt) => {
                self.kv(indent).with_id(PREFIX_SYNC_STMT_ID, stmt.this.0.index)
                    .with_s_key("Synchronous");
                self.kv(indent2).with_s_key("EndSync")
                    .with_opt_disp_val(stmt.end_sync.as_ref().map(|v| &v.0.index));
                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, stmt.body, indent3);
            },
            Statement::EndSynchronous(stmt) => {
                self.kv(indent).with_id(PREFIX_ENDSYNC_STMT_ID, stmt.this.0.index)
                    .with_s_key("EndSynchronous");
                self.kv(indent2).with_s_key("StartSync").with_disp_val(&stmt.start_sync.0.index);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Return(stmt) => {
                self.kv(indent).with_id(PREFIX_RETURN_STMT_ID, stmt.this.0.index)
                    .with_s_key("Return");
                self.kv(indent2).with_s_key("Expression");
                self.write_expr(heap, stmt.expression, indent3);
            },
            Statement::Assert(stmt) => {
                self.kv(indent).with_id(PREFIX_ASSERT_STMT_ID, stmt.this.0.index)
                    .with_s_key("Assert");
                self.kv(indent2).with_s_key("Expression");
                self.write_expr(heap, stmt.expression, indent3);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Goto(stmt) => {
                self.kv(indent).with_id(PREFIX_GOTO_STMT_ID, stmt.this.0.index)
                    .with_s_key("Goto");
                self.kv(indent2).with_s_key("Label").with_ascii_val(&stmt.label.value);
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(stmt.target.as_ref().map(|v| &v.0.index));
            },
            Statement::New(stmt) => {
                self.kv(indent).with_id(PREFIX_NEW_STMT_ID, stmt.this.0.index)
                    .with_s_key("New");
                self.kv(indent2).with_s_key("Expression");
                self.write_expr(heap, stmt.expression.upcast(), indent3);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Put(stmt) => {
                self.kv(indent).with_id(PREFIX_PUT_STMT_ID, stmt.this.0.index)
                    .with_s_key("Put");
                self.kv(indent2).with_s_key("Port");
                self.write_expr(heap, stmt.port, indent3);
                self.kv(indent2).with_s_key("Message");
                self.write_expr(heap, stmt.message, indent3);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            },
            Statement::Expression(stmt) => {
                self.kv(indent).with_id(PREFIX_EXPR_STMT_ID, stmt.this.0.index)
                    .with_s_key("ExpressionStatement");
                self.write_expr(heap, stmt.expression, indent2);
                self.kv(indent2).with_s_key("Next")
                    .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
            }
        }
    }

    fn write_expr(&mut self, heap: &Heap, expr_id: ExpressionId, indent: usize) {
        let expr = &heap[expr_id];
        let indent2 = indent + 1;
        let indent3 = indent2 + 1;

        match expr {
            Expression::Assignment(expr) => {
                self.kv(indent).with_id(PREFIX_ASSIGNMENT_EXPR_ID, expr.this.0.index)
                    .with_s_key("AssignmentExpr");
                self.kv(indent2).with_s_key("Operation").with_debug_val(&expr.operation);
                self.kv(indent2).with_s_key("Left");
                self.write_expr(heap, expr.left, indent3);
                self.kv(indent2).with_s_key("Right");
                self.write_expr(heap, expr.right, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Conditional(expr) => {
                self.kv(indent).with_id(PREFIX_CONDITIONAL_EXPR_ID, expr.this.0.index)
                    .with_s_key("ConditionalExpr");
                self.kv(indent2).with_s_key("Condition");
                self.write_expr(heap, expr.test, indent3);
                self.kv(indent2).with_s_key("TrueExpression");
                self.write_expr(heap, expr.true_expression, indent3);
                self.kv(indent2).with_s_key("FalseExpression");
                self.write_expr(heap, expr.false_expression, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Binary(expr) => {
                self.kv(indent).with_id(PREFIX_BINARY_EXPR_ID, expr.this.0.index)
                    .with_s_key("BinaryExpr");
                self.kv(indent2).with_s_key("Operation").with_debug_val(&expr.operation);
                self.kv(indent2).with_s_key("Left");
                self.write_expr(heap, expr.left, indent3);
                self.kv(indent2).with_s_key("Right");
                self.write_expr(heap, expr.right, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Unary(expr) => {
                self.kv(indent).with_id(PREFIX_UNARY_EXPR_ID, expr.this.0.index)
                    .with_s_key("UnaryExpr");
                self.kv(indent2).with_s_key("Operation").with_debug_val(&expr.operation);
                self.kv(indent2).with_s_key("Argument");
                self.write_expr(heap, expr.expression, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Indexing(expr) => {
                self.kv(indent).with_id(PREFIX_INDEXING_EXPR_ID, expr.this.0.index)
                    .with_s_key("IndexingExpr");
                self.kv(indent2).with_s_key("Subject");
                self.write_expr(heap, expr.subject, indent3);
                self.kv(indent2).with_s_key("Index");
                self.write_expr(heap, expr.index, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Slicing(expr) => {
                self.kv(indent).with_id(PREFIX_SLICING_EXPR_ID, expr.this.0.index)
                    .with_s_key("SlicingExpr");
                self.kv(indent2).with_s_key("Subject");
                self.write_expr(heap, expr.subject, indent3);
                self.kv(indent2).with_s_key("FromIndex");
                self.write_expr(heap, expr.from_index, indent3);
                self.kv(indent2).with_s_key("ToIndex");
                self.write_expr(heap, expr.to_index, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Select(expr) => {
                self.kv(indent).with_id(PREFIX_SELECT_EXPR_ID, expr.this.0.index)
                    .with_s_key("SelectExpr");
                self.kv(indent2).with_s_key("Subject");
                self.write_expr(heap, expr.subject, indent3);

                match &expr.field {
                    Field::Length => {
                        self.kv(indent2).with_s_key("Field").with_s_val("length");
                    },
                    Field::Symbolic(field) => {
                        self.kv(indent2).with_s_key("Field").with_ascii_val(&field.value);
                    }
                }
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Array(expr) => {
                self.kv(indent).with_id(PREFIX_ARRAY_EXPR_ID, expr.this.0.index)
                    .with_s_key("ArrayExpr");
                self.kv(indent2).with_s_key("Elements");
                for expr_id in &expr.elements {
                    self.write_expr(heap, *expr_id, indent3);
                }

                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Constant(expr) => {
                self.kv(indent).with_id(PREFIX_CONST_EXPR_ID, expr.this.0.index)
                    .with_s_key("ConstantExpr");

                let val = self.kv(indent2).with_s_key("Value");
                match &expr.value {
                    Constant::Null => { val.with_s_val("null"); },
                    Constant::True => { val.with_s_val("true"); },
                    Constant::False => { val.with_s_val("false"); },
                    Constant::Character(char) => { val.with_ascii_val(char); },
                    Constant::Integer(int) => { val.with_disp_val(int); },
                }

                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Call(expr) => {
                self.kv(indent).with_id(PREFIX_CALL_EXPR_ID, expr.this.0.index)
                    .with_s_key("CallExpr");

                // Method
                let method = self.kv(indent2).with_s_key("Method");
                match &expr.method {
                    Method::Get => { method.with_s_val("get"); },
                    Method::Fires => { method.with_s_val("fires"); },
                    Method::Create => { method.with_s_val("create"); },
                    Method::Symbolic(symbolic) => {
                        method.with_s_val("symbolic");
                        self.kv(indent3).with_s_key("Name").with_ascii_val(&symbolic.identifier.value);
                        self.kv(indent3).with_s_key("Definition")
                            .with_opt_disp_val(symbolic.definition.as_ref().map(|v| &v.index));
                    }
                }

                // Arguments
                self.kv(indent2).with_s_key("Arguments");
                for arg_id in &expr.arguments {
                    self.write_expr(heap, *arg_id, indent3);
                }

                // Parent
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            },
            Expression::Variable(expr) => {
                self.kv(indent).with_id(PREFIX_VARIABLE_EXPR_ID, expr.this.0.index)
                    .with_s_key("VariableExpr");
                self.kv(indent2).with_s_key("Name").with_ascii_val(&expr.identifier.value);
                self.kv(indent2).with_s_key("Definition")
                    .with_opt_disp_val(expr.declaration.as_ref().map(|v| &v.index));
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
            }
        }
    }

    fn write_local(&mut self, heap: &Heap, local_id: LocalId, indent: usize) {
        let local = &heap[local_id];
        let indent2 = indent + 1;

        self.kv(indent).with_id(PREFIX_LOCAL_ID, local_id.0.index)
            .with_s_key("Local");

        self.kv(indent2).with_s_key("Name").with_ascii_val(&local.identifier.value);
        self.kv(indent2).with_s_key("Type")
            .with_custom_val(|w| write_type(w, heap, &heap[local.parser_type]));
    }

    //--------------------------------------------------------------------------
    // Printing Utilities
    //--------------------------------------------------------------------------

    fn kv(&mut self, indent: usize) -> KV {
        KV::new(&mut self.buffer, &mut self.temp1, &mut self.temp2, indent)
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

fn write_type(target: &mut String, heap: &Heap, t: &ParserType) {
    use ParserTypeVariant as PTV;

    let mut embedded = Vec::new();
    match &t.variant {
        PTV::Input(id) => { target.write_str("in"); embedded.push(*id); }
        PTV::Output(id) => { target.write_str("out"); embedded.push(*id) }
        PTV::Array(id) => { target.write_str("array"); embedded.push(*id) }
        PTV::Message => { target.write_str("msg"); }
        PTV::Bool => { target.write_str("bool"); }
        PTV::Byte => { target.write_str("byte"); }
        PTV::Short => { target.write_str("short"); }
        PTV::Int => { target.write_str("int"); }
        PTV::Long => { target.write_str("long"); }
        PTV::String => { target.write_str("str"); }
        PTV::IntegerLiteral => { target.write_str("int_lit"); }
        PTV::Inferred => { target.write_str("auto"); }
        PTV::Symbolic(symbolic) => {
            target.write_str(&String::from_utf8_lossy(&symbolic.identifier.value));
            embedded.extend(&symbolic.poly_args);
        }
    };

    if !embedded.is_empty() {
        target.write_str("<");
        for (idx, embedded_id) in embedded.into_iter().enumerate() {
            if idx != 0 { target.write_str(", "); }
            write_type(target, heap, &heap[embedded_id]);
        }
        target.write_str(">");
    }
}

fn write_expression_parent(target: &mut String, parent: &ExpressionParent) {
    use ExpressionParent as EP;

    *target = match parent {
        EP::None => String::from("None"),
        EP::Memory(id) => format!("MemoryStmt({})", id.0.0.index),
        EP::If(id) => format!("IfStmt({})", id.0.index),
        EP::While(id) => format!("WhileStmt({})", id.0.index),
        EP::Return(id) => format!("ReturnStmt({})", id.0.index),
        EP::Assert(id) => format!("AssertStmt({})", id.0.index),
        EP::New(id) => format!("NewStmt({})", id.0.index),
        EP::Put(id, idx) => format!("PutStmt({}, {})", id.0.index, idx),
        EP::ExpressionStmt(id) => format!("ExprStmt({})", id.0.index),
        EP::Expression(id, idx) => format!("Expr({}, {})", id.index, idx)
    };
}
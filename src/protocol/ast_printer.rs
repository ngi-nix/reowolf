use std::fmt::{Debug, Display, Write};
use std::io::Write as IOWrite;

use super::ast::*;
use super::token_parsing::*;

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
const PREFIX_UNION_ID: &'static str = "DefU";
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
const PREFIX_BINDING_EXPR_ID: &'static str = "EBnd";
const PREFIX_CONDITIONAL_EXPR_ID: &'static str = "ECnd";
const PREFIX_BINARY_EXPR_ID: &'static str = "EBin";
const PREFIX_UNARY_EXPR_ID: &'static str = "EUna";
const PREFIX_INDEXING_EXPR_ID: &'static str = "EIdx";
const PREFIX_SLICING_EXPR_ID: &'static str = "ESli";
const PREFIX_SELECT_EXPR_ID: &'static str = "ESel";
const PREFIX_LITERAL_EXPR_ID: &'static str = "ELit";
const PREFIX_CALL_EXPR_ID: &'static str = "ECll";
const PREFIX_VARIABLE_EXPR_ID: &'static str = "EVar";

struct KV<'a> {
    buffer: &'a mut String,
    prefix: Option<(&'static str, i32)>,
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

    fn with_id(mut self, prefix: &'static str, id: i32) -> Self {
        self.prefix = Some((prefix, id));
        self
    }

    fn with_s_key(self, key: &str) -> Self {
        self.temp_key.push_str(key);
        self
    }

    fn with_d_key<D: Display>(self, key: &D) -> Self {
        self.temp_key.push_str(&key.to_string());
        self
    }

    fn with_s_val(self, val: &str) -> Self {
        self.temp_val.push_str(val);
        self
    }

    fn with_disp_val<D: Display>(self, val: &D) -> Self {
        self.temp_val.push_str(&format!("{}", val));
        self
    }

    fn with_debug_val<D: Debug>(self, val: &D) -> Self {
        self.temp_val.push_str(&format!("{:?}", val));
        self
    }

    fn with_identifier_val(self, val: &Identifier) -> Self {
        self.temp_val.push_str(val.value.as_str());
        self
    }

    fn with_opt_disp_val<D: Display>(self, val: Option<&D>) -> Self {
        match val {
            Some(v) => { self.temp_val.push_str(&format!("Some({})", v)); },
            None => { self.temp_val.push_str("None"); }
        }
        self
    }

    fn with_opt_identifier_val(self, val: Option<&Identifier>) -> Self {
        match val {
            Some(v) => {
                self.temp_val.push_str("Some(");
                self.temp_val.push_str(v.value.as_str());
                self.temp_val.push(')');
            },
            None => {
                self.temp_val.push_str("None");
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
            self.buffer.push_str(&format!("{}[{:04}]", prefix, id));
        } else {
            self.buffer.push_str("           ");
        }

        for _ in 0..self.indent * INDENT {
            self.buffer.push(' ');
        }

        // Leading dash
        self.buffer.push_str("- ");

        // Key and value
        self.buffer.push_str(self.temp_key);
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
    cur_definition: Option<DefinitionId>,
    buffer: String,
    temp1: String,
    temp2: String,
}

impl ASTWriter {
    pub(crate) fn new() -> Self {
        Self{
            cur_definition: None,
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
                    .with_identifier_val(&pragma.value);
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

                self.kv(indent2).with_s_key("Name").with_identifier_val(&import.module);
                self.kv(indent2).with_s_key("Alias").with_identifier_val(&import.alias);
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(import.module_id.as_ref().map(|v| &v.index));
            },
            Import::Symbols(import) => {
                self.kv(indent).with_id(PREFIX_IMPORT_ID, import.this.index)
                    .with_s_key("ImportSymbol");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&import.module);
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(import.module_id.as_ref().map(|v| &v.index));

                self.kv(indent2).with_s_key("Symbols");

                let indent3 = indent2 + 1;
                let indent4 = indent3 + 1;
                for symbol in &import.symbols {
                    self.kv(indent3).with_s_key("AliasedSymbol");
                    self.kv(indent4).with_s_key("Name").with_identifier_val(&symbol.name);
                    self.kv(indent4).with_s_key("Alias").with_opt_identifier_val(symbol.alias.as_ref());
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
        self.cur_definition = Some(def_id);
        let indent2 = indent + 1;
        let indent3 = indent2 + 1;
        let indent4 = indent3 + 1;

        match &heap[def_id] {
            Definition::Struct(def) => {
                self.kv(indent).with_id(PREFIX_STRUCT_ID, def.this.0.index)
                    .with_s_key("DefinitionStruct");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&def.identifier);
                for poly_var_id in &def.poly_vars {
                    self.kv(indent3).with_s_key("PolyVar").with_identifier_val(&poly_var_id);
                }

                self.kv(indent2).with_s_key("Fields");
                for field in &def.fields {
                    self.kv(indent3).with_s_key("Field");
                    self.kv(indent4).with_s_key("Name")
                        .with_identifier_val(&field.field);
                    self.kv(indent4).with_s_key("Type")
                        .with_custom_val(|s| write_parser_type(s, heap, &field.parser_type));
                }
            },
            Definition::Enum(def) => {
                self.kv(indent).with_id(PREFIX_ENUM_ID, def.this.0.index)
                    .with_s_key("DefinitionEnum");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&def.identifier);
                for poly_var_id in &def.poly_vars {
                    self.kv(indent3).with_s_key("PolyVar").with_identifier_val(&poly_var_id);
                }

                self.kv(indent2).with_s_key("Variants");
                for variant in &def.variants {
                    self.kv(indent3).with_s_key("Variant");
                    self.kv(indent4).with_s_key("Name")
                        .with_identifier_val(&variant.identifier);
                    let variant_value = self.kv(indent4).with_s_key("Value");
                    match &variant.value {
                        EnumVariantValue::None => variant_value.with_s_val("None"),
                        EnumVariantValue::Integer(value) => variant_value.with_disp_val(value),
                    };
                }
            },
            Definition::Union(def) => {
                self.kv(indent).with_id(PREFIX_UNION_ID, def.this.0.index)
                    .with_s_key("DefinitionUnion");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&def.identifier);
                for poly_var_id in &def.poly_vars {
                    self.kv(indent3).with_s_key("PolyVar").with_identifier_val(&poly_var_id);
                }

                self.kv(indent2).with_s_key("Variants");
                for variant in &def.variants {
                    self.kv(indent3).with_s_key("Variant");
                    self.kv(indent4).with_s_key("Name")
                        .with_identifier_val(&variant.identifier);
                        
                    match &variant.value {
                        UnionVariantValue::None => {
                            self.kv(indent4).with_s_key("Value").with_s_val("None");
                        }
                        UnionVariantValue::Embedded(embedded) => {
                            self.kv(indent4).with_s_key("Values");
                            for embedded in embedded {
                                self.kv(indent4+1).with_s_key("Value")
                                    .with_custom_val(|v| write_parser_type(v, heap, embedded));
                            }
                        }
                    }
                }
            }
            Definition::Function(def) => {
                self.kv(indent).with_id(PREFIX_FUNCTION_ID, def.this.0.index)
                    .with_s_key("DefinitionFunction");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&def.identifier);
                for poly_var_id in &def.poly_vars {
                    self.kv(indent3).with_s_key("PolyVar").with_identifier_val(&poly_var_id);
                }

                self.kv(indent2).with_s_key("ReturnParserTypes");
                for return_type in &def.return_types {
                    self.kv(indent3).with_s_key("ReturnParserType")
                        .with_custom_val(|s| write_parser_type(s, heap, return_type));
                }

                self.kv(indent2).with_s_key("Parameters");
                for param_id in &def.parameters {
                    self.write_parameter(heap, *param_id, indent3);
                }

                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, def.body.upcast(), indent3);
            },
            Definition::Component(def) => {
                self.kv(indent).with_id(PREFIX_COMPONENT_ID,def.this.0.index)
                    .with_s_key("DefinitionComponent");

                self.kv(indent2).with_s_key("Name").with_identifier_val(&def.identifier);
                self.kv(indent2).with_s_key("Variant").with_debug_val(&def.variant);

                self.kv(indent2).with_s_key("PolymorphicVariables");
                for poly_var_id in &def.poly_vars {
                    self.kv(indent3).with_s_key("PolyVar").with_identifier_val(&poly_var_id);
                }

                self.kv(indent2).with_s_key("Parameters");
                for param_id in &def.parameters {
                    self.write_parameter(heap, *param_id, indent3)
                }

                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, def.body.upcast(), indent3);
            }
        }
    }

    fn write_parameter(&mut self, heap: &Heap, param_id: ParameterId, indent: usize) {
        let indent2 = indent + 1;
        let param = &heap[param_id];

        self.kv(indent).with_id(PREFIX_PARAMETER_ID, param_id.0.index)
            .with_s_key("Parameter");
        self.kv(indent2).with_s_key("Name").with_identifier_val(&param.identifier);
        self.kv(indent2).with_s_key("ParserType").with_custom_val(|w| write_parser_type(w, heap, &param.parser_type));
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
                        self.kv(indent2).with_s_key("Next")
                            .with_opt_disp_val(stmt.next.as_ref().map(|v| &v.index));
                    }
                }
            },
            Statement::Labeled(stmt) => {
                self.kv(indent).with_id(PREFIX_LABELED_STMT_ID, stmt.this.0.index)
                    .with_s_key("Labeled");

                self.kv(indent2).with_s_key("Label").with_identifier_val(&stmt.label);
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
                self.write_stmt(heap, stmt.true_body.upcast(), indent3);

                if let Some(false_body) = stmt.false_body {
                    self.kv(indent2).with_s_key("FalseBody");
                    self.write_stmt(heap, false_body.upcast(), indent3);
                }
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
                self.write_stmt(heap, stmt.body.upcast(), indent3);
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
                    .with_opt_identifier_val(stmt.label.as_ref());
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(stmt.target.as_ref().map(|v| &v.0.index));
            },
            Statement::Continue(stmt) => {
                self.kv(indent).with_id(PREFIX_CONTINUE_STMT_ID, stmt.this.0.index)
                    .with_s_key("Continue");
                self.kv(indent2).with_s_key("Label")
                    .with_opt_identifier_val(stmt.label.as_ref());
                self.kv(indent2).with_s_key("Target")
                    .with_opt_disp_val(stmt.target.as_ref().map(|v| &v.0.index));
            },
            Statement::Synchronous(stmt) => {
                self.kv(indent).with_id(PREFIX_SYNC_STMT_ID, stmt.this.0.index)
                    .with_s_key("Synchronous");
                self.kv(indent2).with_s_key("EndSync")
                    .with_opt_disp_val(stmt.end_sync.as_ref().map(|v| &v.0.index));
                self.kv(indent2).with_s_key("Body");
                self.write_stmt(heap, stmt.body.upcast(), indent3);
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
            Statement::Goto(stmt) => {
                self.kv(indent).with_id(PREFIX_GOTO_STMT_ID, stmt.this.0.index)
                    .with_s_key("Goto");
                self.kv(indent2).with_s_key("Label").with_identifier_val(&stmt.label);
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
        let def_id = self.cur_definition.unwrap();

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
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            },
            Expression::Binding(expr) => {
                self.kv(indent).with_id(PREFIX_BINARY_EXPR_ID, expr.this.0.index)
                    .with_s_key("BindingExpr");
                self.kv(indent2).with_s_key("LeftExpression");
                self.write_expr(heap, expr.left.upcast(), indent3);
                self.kv(indent2).with_s_key("RightExpression");
                self.write_expr(heap, expr.right, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
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
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
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
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            },
            Expression::Unary(expr) => {
                self.kv(indent).with_id(PREFIX_UNARY_EXPR_ID, expr.this.0.index)
                    .with_s_key("UnaryExpr");
                self.kv(indent2).with_s_key("Operation").with_debug_val(&expr.operation);
                self.kv(indent2).with_s_key("Argument");
                self.write_expr(heap, expr.expression, indent3);
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
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
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
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
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
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
                        self.kv(indent2).with_s_key("Field").with_identifier_val(&field.identifier);
                        self.kv(indent3).with_s_key("Definition").with_opt_disp_val(field.definition.as_ref().map(|v| &v.index));
                        self.kv(indent3).with_s_key("Index").with_disp_val(&field.field_idx);
                    }
                }
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            },
            Expression::Literal(expr) => {
                self.kv(indent).with_id(PREFIX_LITERAL_EXPR_ID, expr.this.0.index)
                    .with_s_key("LiteralExpr");

                let val = self.kv(indent2).with_s_key("Value");
                match &expr.value {
                    Literal::Null => { val.with_s_val("null"); },
                    Literal::True => { val.with_s_val("true"); },
                    Literal::False => { val.with_s_val("false"); },
                    Literal::Character(data) => { val.with_disp_val(data); },
                    Literal::String(data) => { val.with_disp_val(data.as_str()); },
                    Literal::Integer(data) => { val.with_debug_val(data); },
                    Literal::Struct(data) => {
                        val.with_s_val("Struct");
                        let indent4 = indent3 + 1;

                        self.kv(indent3).with_s_key("ParserType")
                            .with_custom_val(|t| write_parser_type(t, heap, &data.parser_type));
                        self.kv(indent3).with_s_key("Definition").with_custom_val(|s| {
                            write_option(s, data.definition.as_ref().map(|v| &v.index));
                        });

                        for field in &data.fields {
                            self.kv(indent3).with_s_key("Field");
                            self.kv(indent4).with_s_key("Name").with_identifier_val(&field.identifier);
                            self.kv(indent4).with_s_key("Index").with_disp_val(&field.field_idx);
                            self.kv(indent4).with_s_key("ParserType");
                            self.write_expr(heap, field.value, indent4 + 1);
                        }
                    },
                    Literal::Enum(data) => {
                        val.with_s_val("Enum");

                        self.kv(indent3).with_s_key("ParserType")
                            .with_custom_val(|t| write_parser_type(t, heap, &data.parser_type));
                        self.kv(indent3).with_s_key("Definition").with_custom_val(|s| {
                            write_option(s, data.definition.as_ref().map(|v| &v.index))
                        });
                        self.kv(indent3).with_s_key("VariantIdx").with_disp_val(&data.variant_idx);
                    },
                    Literal::Union(data) => {
                        val.with_s_val("Union");
                        let indent4 = indent3 + 1;

                        self.kv(indent3).with_s_key("ParserType")
                            .with_custom_val(|t| write_parser_type(t, heap, &data.parser_type));
                        self.kv(indent3).with_s_key("Definition").with_custom_val(|s| {
                            write_option(s, data.definition.as_ref().map(|v| &v.index));
                        });
                        self.kv(indent3).with_s_key("VariantIdx").with_disp_val(&data.variant_idx);

                        for value in &data.values {
                            self.kv(indent3).with_s_key("Value");
                            self.write_expr(heap, *value, indent4);
                        }
                    }
                    Literal::Array(data) => {
                        val.with_s_val("Array");
                        let indent4 = indent3 + 1;

                        self.kv(indent3).with_s_key("Elements");
                        for expr_id in data {
                            self.write_expr(heap, *expr_id, indent4);
                        }
                    }
                }

                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            },
            Expression::Call(expr) => {
                self.kv(indent).with_id(PREFIX_CALL_EXPR_ID, expr.this.0.index)
                    .with_s_key("CallExpr");

                let definition = &heap[expr.definition];
                match definition {
                    Definition::Component(definition) => {
                        self.kv(indent2).with_s_key("BuiltIn").with_disp_val(&false);
                        self.kv(indent2).with_s_key("Variant").with_debug_val(&definition.variant);
                    },
                    Definition::Function(definition) => {
                        self.kv(indent2).with_s_key("BuiltIn").with_disp_val(&definition.builtin);
                        self.kv(indent2).with_s_key("Variant").with_s_val("Function");
                    },
                    _ => unreachable!()
                }
                self.kv(indent2).with_s_key("MethodName").with_identifier_val(definition.identifier());
                self.kv(indent2).with_s_key("ParserType")
                    .with_custom_val(|t| write_parser_type(t, heap, &expr.parser_type));

                // Arguments
                self.kv(indent2).with_s_key("Arguments");
                for arg_id in &expr.arguments {
                    self.write_expr(heap, *arg_id, indent3);
                }

                // Parent
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            },
            Expression::Variable(expr) => {
                self.kv(indent).with_id(PREFIX_VARIABLE_EXPR_ID, expr.this.0.index)
                    .with_s_key("VariableExpr");
                self.kv(indent2).with_s_key("Name").with_identifier_val(&expr.identifier);
                self.kv(indent2).with_s_key("Definition")
                    .with_opt_disp_val(expr.declaration.as_ref().map(|v| &v.index));
                self.kv(indent2).with_s_key("Parent")
                    .with_custom_val(|v| write_expression_parent(v, &expr.parent));
                self.kv(indent2).with_s_key("ConcreteType")
                    .with_custom_val(|v| write_concrete_type(v, heap, def_id, &expr.concrete_type));
            }
        }
    }

    fn write_local(&mut self, heap: &Heap, local_id: LocalId, indent: usize) {
        let local = &heap[local_id];
        let indent2 = indent + 1;

        self.kv(indent).with_id(PREFIX_LOCAL_ID, local_id.0.index)
            .with_s_key("Local");

        self.kv(indent2).with_s_key("Name").with_identifier_val(&local.identifier);
        self.kv(indent2).with_s_key("ParserType")
            .with_custom_val(|w| write_parser_type(w, heap, &local.parser_type));
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
        Some(v) => target.push_str(&format!("Some({})", v)),
        None => target.push_str("None")
    };
}

fn write_parser_type(target: &mut String, heap: &Heap, t: &ParserType) {
    use ParserTypeVariant as PTV;

    fn push_bytes(target: &mut String, msg: &[u8]) {
        target.push_str(&String::from_utf8_lossy(msg));
    }

    fn write_element(target: &mut String, heap: &Heap, t: &ParserType, mut element_idx: usize) -> usize {
        let element = &t.elements[element_idx];
        match &element.variant {
            PTV::Message => { push_bytes(target, KW_TYPE_MESSAGE); },
            PTV::Bool => { push_bytes(target, KW_TYPE_BOOL); },
            PTV::UInt8 => { push_bytes(target, KW_TYPE_UINT8); },
            PTV::UInt16 => { push_bytes(target, KW_TYPE_UINT16); },
            PTV::UInt32 => { push_bytes(target, KW_TYPE_UINT32); },
            PTV::UInt64 => { push_bytes(target, KW_TYPE_UINT64); },
            PTV::SInt8 => { push_bytes(target, KW_TYPE_SINT8); },
            PTV::SInt16 => { push_bytes(target, KW_TYPE_SINT16); },
            PTV::SInt32 => { push_bytes(target, KW_TYPE_SINT32); },
            PTV::SInt64 => { push_bytes(target, KW_TYPE_SINT64); },
            PTV::Character => { push_bytes(target, KW_TYPE_CHAR); },
            PTV::String => { push_bytes(target, KW_TYPE_STRING); },
            PTV::IntegerLiteral => { target.push_str("int_literal"); },
            PTV::Inferred => { push_bytes(target, KW_TYPE_INFERRED); },
            PTV::Array => {
                element_idx = write_element(target, heap, t, element_idx + 1);
                target.push_str("[]");
            },
            PTV::Input => {
                push_bytes(target, KW_TYPE_IN_PORT);
                target.push('<');
                element_idx = write_element(target, heap, t, element_idx + 1);
                target.push('>');
            },
            PTV::Output => {
                push_bytes(target, KW_TYPE_OUT_PORT);
                target.push('<');
                element_idx = write_element(target, heap, t, element_idx + 1);
                target.push('>');
            },
            PTV::PolymorphicArgument(definition_id, arg_idx) => {
                let definition = &heap[*definition_id];
                let poly_var = &definition.poly_vars()[*arg_idx].value;
                target.write_str(poly_var.as_str());
            },
            PTV::Definition(definition_id, num_embedded) => {
                let definition = &heap[*definition_id];
                let definition_ident = definition.identifier().value.as_str();
                target.write_str(definition_ident);

                let num_embedded = *num_embedded;
                if num_embedded != 0 {
                    target.push('<');
                    for embedded_idx in 0..num_embedded {
                        if embedded_idx != 0 {
                            target.push(',');
                        }
                        element_idx = write_element(target, heap, t, element_idx + 1);
                    }
                    target.push('>');
                }
            }
        }

        element_idx
    }

    write_element(target, heap, t, 0);
}

fn write_concrete_type(target: &mut String, heap: &Heap, def_id: DefinitionId, t: &ConcreteType) {
    use ConcreteTypePart as CTP;

    fn write_concrete_part(target: &mut String, heap: &Heap, def_id: DefinitionId, t: &ConcreteType, mut idx: usize) -> usize {
        if idx >= t.parts.len() {
            return idx;
        }

        match &t.parts[idx] {
            CTP::Marker(marker) => {
                // Marker points to polymorphic variable index
                let definition = &heap[def_id];
                let poly_var_ident = &definition.poly_vars()[*marker];
                target.push_str(poly_var_ident.value.as_str());
                idx = write_concrete_part(target, heap, def_id, t, idx + 1);
            },
            CTP::Void => target.push_str("void"),
            CTP::Message => target.push_str("msg"),
            CTP::Bool => target.push_str("bool"),
            CTP::Byte => target.push_str("byte"),
            CTP::Short => target.push_str("short"),
            CTP::Int => target.push_str("int"),
            CTP::Long => target.push_str("long"),
            CTP::String => target.push_str("string"),
            CTP::Array => {
                idx = write_concrete_part(target, heap, def_id, t, idx + 1);
                target.push_str("[]");
            },
            CTP::Slice => {
                idx = write_concrete_part(target, heap, def_id, t, idx + 1);
                target.push_str("[..]");
            }
            CTP::Input => {
                target.push_str("in<");
                idx = write_concrete_part(target, heap, def_id, t, idx + 1);
                target.push('>');
            },
            CTP::Output => {
                target.push_str("out<");
                idx = write_concrete_part(target, heap, def_id, t, idx + 1);
                target.push('>')
            },
            CTP::Instance(definition_id, num_embedded) => {
                let identifier = heap[*definition_id].identifier();
                target.push_str(identifier.value.as_str());
                target.push('<');
                for idx_embedded in 0..*num_embedded {
                    if idx_embedded != 0 {
                        target.push_str(", ");
                    }
                    idx = write_concrete_part(target, heap, def_id, t, idx + 1);
                }
                target.push('>');
            }
        }

        idx + 1
    }

    write_concrete_part(target, heap, def_id, t, 0);
}

fn write_expression_parent(target: &mut String, parent: &ExpressionParent) {
    use ExpressionParent as EP;

    *target = match parent {
        EP::None => String::from("None"),
        EP::If(id) => format!("IfStmt({})", id.0.index),
        EP::While(id) => format!("WhileStmt({})", id.0.index),
        EP::Return(id) => format!("ReturnStmt({})", id.0.index),
        EP::Assert(id) => format!("AssertStmt({})", id.0.index),
        EP::New(id) => format!("NewStmt({})", id.0.index),
        EP::ExpressionStmt(id) => format!("ExprStmt({})", id.0.index),
        EP::Expression(id, idx) => format!("Expr({}, {})", id.index, idx)
    };
}
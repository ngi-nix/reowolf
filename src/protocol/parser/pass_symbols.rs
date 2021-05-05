use crate::protocol::ast::*;
use super::symbol_table::*;
use crate::protocol::input_source::{ParseError, InputSpan};
use super::tokens::*;
use super::token_parsing::*;
use super::{Module, ModuleCompilationPhase, PassCtx};

/// Scans the module and finds all module-level type definitions. These will be
/// added to the symbol table such that during AST-construction we know which
/// identifiers point to types. Will also parse all pragmas to determine module
/// names.
pub(crate) struct PassSymbols {
    symbols: Vec<Symbol>,
    pragmas: Vec<PragmaId>,
    imports: Vec<ImportId>,
    definitions: Vec<DefinitionId>,
    buffer: String,
    has_pragma_version: bool,
    has_pragma_module: bool,
}

impl PassSymbols {
    pub(crate) fn new() -> Self {
        Self{
            symbols: Vec::with_capacity(128),
            pragmas: Vec::with_capacity(8),
            imports: Vec::with_capacity(32),
            definitions: Vec::with_capacity(128),
            buffer: String::with_capacity(128),
            has_pragma_version: false,
            has_pragma_module: false,
        }
    }

    fn reset(&mut self) {
        self.symbols.clear();
        self.pragmas.clear();
        self.imports.clear();
        self.definitions.clear();
        self.has_pragma_version = false;
        self.has_pragma_module = false;
    }

    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut PassCtx) -> Result<(), ParseError> {
        self.reset();

        let module = &mut modules[module_idx];
        let module_range = &module.tokens.ranges[0];

        debug_assert_eq!(module.phase, ModuleCompilationPhase::Tokenized);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);
        debug_assert!(module.root_id.is_invalid()); // not set yet,

        // Preallocate root in the heap
        let root_id = ctx.heap.alloc_protocol_description(|this| {
            Root{
                this,
                pragmas: Vec::new(),
                imports: Vec::new(),
                definitions: Vec::new(),
            }
        });
        module.root_id = root_id;

        // Retrieve first range index, then make immutable borrow
        let mut range_idx = module_range.first_child_idx;
        let module = &modules[module_idx];

        // Visit token ranges to detect definitions and pragmas
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];
            let next_sibling_idx = cur_range.next_sibling_idx;
            let range_kind = cur_range.range_kind;

            // Parse if it is a definition or a pragma
            if range_kind == TokenRangeKind::Definition {
                self.visit_definition_range(modules, module_idx, ctx, range_idx_usize)?;
            } else if range_kind == TokenRangeKind::Pragma {
                self.visit_pragma_range(modules, module_idx, ctx, range_idx_usize)?;
            }

            if next_sibling_idx == NO_SIBLING {
                break;
            } else {
                range_idx = next_sibling_idx;
            }
        }

        // Add the module's symbol scope and the symbols we just parsed
        let module_scope = SymbolScope::Module(root_id);
        ctx.symbols.insert_scope(None, module_scope);
        for symbol in self.symbols.drain(..) {
            ctx.symbols.insert_scope(Some(module_scope), SymbolScope::Definition(symbol.variant.as_definition().definition_id));
            if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(module_scope, symbol) {
                return Err(construct_symbol_conflict_error(modules, module_idx, ctx, &new_symbol, &old_symbol))
            }
        }

        // Modify the preallocated root
        let root = &mut ctx.heap[root_id];
        root.pragmas.extend(self.pragmas.drain(..));
        root.definitions.extend(self.definitions.drain(..));

        let module = &mut modules[module_idx];
        module.phase = ModuleCompilationPhase::SymbolsScanned;

        Ok(())
    }

    fn visit_pragma_range(&mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let range = &module.tokens.ranges[range_idx];
        let mut iter = module.tokens.iter_range(range);

        // Consume pragma name
        let (pragma_section, pragma_start, _) = consume_pragma(&module.source, &mut iter)?;

        // Consume pragma values
        if pragma_section == b"#module" {
            // Check if name is defined twice within the same file
            if self.has_pragma_module {
                return Err(ParseError::new_error_str_at_pos(&module.source, pragma_start, "module name is defined twice"));
            }

            // Consume the domain-name
            let (module_name, module_span) = consume_domain_ident(&module.source, &mut iter)?;
            if iter.next().is_some() {
                return Err(ParseError::new_error_str_at_pos(&module.source, iter.last_valid_pos(), "expected end of #module pragma after module name"));
            }

            // Add to heap and symbol table
            let pragma_span = InputSpan::from_positions(pragma_start, module_span.end);
            let module_name = ctx.pool.intern(module_name);
            let pragma_id = ctx.heap.alloc_pragma(|this| Pragma::Module(PragmaModule{
                this,
                span: pragma_span,
                value: Identifier{ span: module_span, value: module_name.clone() },
            }));
            self.pragmas.push(pragma_id);

            if let Err(other_module_root_id) = ctx.symbols.insert_module(module_name, module.root_id) {
                // Naming conflict
                let this_module = &modules[module_idx];
                let other_module = seek_module(modules, other_module_root_id).unwrap();
                let other_module_pragma_id = other_module.name.as_ref().map(|v| (*v).0).unwrap();
                let other_pragma = ctx.heap[other_module_pragma_id].as_module();
                return Err(ParseError::new_error_str_at_span(
                    &this_module.source, pragma_span, "conflict in module name"
                ).with_info_str_at_span(
                    &other_module.source, other_pragma.span, "other module is defined here"
                ));
            }
            self.has_pragma_module = true;
        } else if pragma_section == b"#version" {
            // Check if version is defined twice within the same file
            if self.has_pragma_version {
                return Err(ParseError::new_error_str_at_pos(&module.source, pragma_start, "module version is defined twice"));
            }

            // Consume the version pragma
            let (version, version_span) = consume_integer_literal(&module.source, &mut iter, &mut self.buffer)?;
            let pragma_id = ctx.heap.alloc_pragma(|this| Pragma::Version(PragmaVersion{
                this,
                span: InputSpan::from_positions(pragma_start, version_span.end),
                version,
            }));
            self.pragmas.push(pragma_id);
            self.has_pragma_version = true;
        } else {
            // Custom pragma, maybe we support this in the future, but for now
            // we don't.
            return Err(ParseError::new_error_str_at_pos(&module.source, pragma_start, "illegal pragma name"));
        }

        Ok(())
    }

    fn visit_definition_range(&mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let range = &module.tokens.ranges[range_idx];
        let definition_span = InputSpan::from_positions(
            module.tokens.start_pos(range),
            module.tokens.end_pos(range)
        );
        let mut iter = module.tokens.iter_range(range);

        // First ident must be type of symbol
        let (kw_text, _) = consume_any_ident(&module.source, &mut iter).unwrap();

        // Retrieve identifier of definition
        let identifier = consume_ident_interned(&module.source, &mut iter, ctx)?;
        let mut poly_vars = Vec::new();
        maybe_consume_comma_separated(
            TokenKind::OpenAngle, TokenKind::CloseAngle, &module.source, &mut iter, ctx,
            |source, iter, ctx| consume_ident_interned(source, iter, ctx),
            &mut poly_vars, "a polymorphic variable", None
        )?;
        let ident_text = identifier.value.clone(); // because we need it later
        let ident_span = identifier.span.clone();

        // Reserve space in AST for definition and add it to the symbol table
        let definition_class;
        let ast_definition_id;
        match kw_text {
            KW_STRUCT => {
                let struct_def_id = ctx.heap.alloc_struct_definition(|this| {
                    StructDefinition::new_empty(this, module.root_id, definition_span, identifier, poly_vars)
                });
                definition_class = DefinitionClass::Struct;
                ast_definition_id = struct_def_id.upcast();
            },
            KW_ENUM => {
                let enum_def_id = ctx.heap.alloc_enum_definition(|this| {
                    EnumDefinition::new_empty(this, module.root_id, definition_span, identifier, poly_vars)
                });
                definition_class = DefinitionClass::Enum;
                ast_definition_id = enum_def_id.upcast();
            },
            KW_UNION => {
                let union_def_id = ctx.heap.alloc_union_definition(|this| {
                    UnionDefinition::new_empty(this, module.root_id, definition_span, identifier, poly_vars)
                });
                definition_class = DefinitionClass::Union;
                ast_definition_id = union_def_id.upcast()
            },
            KW_FUNCTION => {
                let func_def_id = ctx.heap.alloc_function_definition(|this| {
                    FunctionDefinition::new_empty(this, module.root_id, definition_span, identifier, poly_vars)
                });
                definition_class = DefinitionClass::Function;
                ast_definition_id = func_def_id.upcast();
            },
            KW_PRIMITIVE | KW_COMPOSITE => {
                let component_variant = if kw_text == KW_PRIMITIVE {
                    ComponentVariant::Primitive
                } else {
                    ComponentVariant::Composite
                };
                let comp_def_id = ctx.heap.alloc_component_definition(|this| {
                    ComponentDefinition::new_empty(this, module.root_id, definition_span, component_variant, identifier, poly_vars)
                });
                definition_class = DefinitionClass::Component;
                ast_definition_id = comp_def_id.upcast();
            },
            _ => unreachable!("encountered keyword '{}' in definition range", String::from_utf8_lossy(kw_text)),
        }

        let symbol = Symbol{
            name: ident_text,
            variant: SymbolVariant::Definition(SymbolDefinition{
                defined_in_module: module.root_id,
                defined_in_scope: SymbolScope::Module(module.root_id),
                definition_span,
                identifier_span: ident_span,
                imported_at: None,
                class: definition_class,
                definition_id: ast_definition_id,
            }),
        };
        self.symbols.push(symbol);
        self.definitions.push(ast_definition_id);

        Ok(())
    }
}
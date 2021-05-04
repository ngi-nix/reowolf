use crate::protocol::ast::*;
use super::symbol_table::*;
use super::{Module, ModuleCompilationPhase, PassCtx};
use super::tokens::*;
use super::token_parsing::*;
use crate::protocol::input_source::{InputSource as InputSource, InputSpan, ParseError};
use crate::collections::*;

/// Parses all the imports in the module tokens. Is applied after the
/// definitions and name of modules are resolved. Hence we should be able to
/// resolve all symbols to their appropriate module/definition.
pub(crate) struct PassImport {
    imports: Vec<ImportId>,
    found_symbols: Vec<(AliasedSymbol, SymbolDefinition)>,
    scoped_symbols: Vec<Symbol>,
}

impl PassImport {
    pub(crate) fn new() -> Self {
        Self{
            imports: Vec::with_capacity(32),
            found_symbols: Vec::with_capacity(32),
            scoped_symbols: Vec::with_capacity(32),
        }
    }
    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut PassCtx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert!(modules.iter().all(|m| m.phase >= ModuleCompilationPhase::SymbolsScanned));
        debug_assert_eq!(module.phase, ModuleCompilationPhase::SymbolsScanned);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);

        let mut range_idx = module_range.first_child_idx;
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];

            if cur_range.range_kind == TokenRangeKind::Import {
                self.visit_import_range(modules, module_idx, ctx, range_idx_usize)?;
            }

            match cur_range.next_sibling_idx {
                Some(idx) => { range_idx = idx; },
                None => { break; }
            }
        }

        let root = &mut ctx.heap[module.root_id];
        root.imports.extend(self.imports.drain(..));

        let module = &mut modules[module_idx];
        module.phase = ModuleCompilationPhase::ImportsResolved;

        Ok(())
    }

    pub(crate) fn visit_import_range(
        &mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize
    ) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let import_range = &module.tokens.ranges[range_idx];
        debug_assert_eq!(import_range.range_kind, TokenRangeKind::Import);

        let mut iter = module.tokens.iter_range(import_range);

        // Consume "import"
        let (_import_ident, import_span) =
            consume_any_ident(&module.source, &mut iter)?;
        debug_assert_eq!(_import_ident, KW_IMPORT);

        // Consume module name
        let (module_name, module_name_span) = consume_domain_ident(&module.source, &mut iter)?;
        let target_root_id = ctx.symbols.get_module_by_name(module_name);
        if target_root_id.is_none() {
            return Err(ParseError::new_error_at_span(
                &module.source, module_name_span,
                format!("could not resolve module '{}'", String::from_utf8_lossy(module_name))
            ));
        }
        let module_name = ctx.pool.intern(module_name);
        let module_identifier = Identifier{ span: module_name_span, value: module_name };
        let target_root_id = target_root_id.unwrap();

        // Check for subsequent characters (alias, multiple imported symbols)
        let next = iter.next();
        let import_id;

        if has_ident(&module.source, &mut iter, b"as") {
            // Alias for module
            iter.consume();
            let alias_identifier = consume_ident_interned(&module.source, &mut iter, ctx)?;
            let alias_name = alias_identifier.value.clone();

            import_id = ctx.heap.alloc_import(|this| Import::Module(ImportModule{
                this,
                span: import_span,
                module: module_identifier,
                alias: alias_identifier,
                module_id: target_root_id
            }));
            ctx.symbols.insert_symbol(SymbolScope::Module(module.root_id), Symbol{
                name: alias_name,
                variant: SymbolVariant::Module(SymbolModule{
                    root_id: target_root_id,
                    introduced_at: import_id,
                }),
            });
        } else if Some(TokenKind::ColonColon) == next {
            iter.consume();

            // Helper function to consume symbols, their alias, and the
            // definition the symbol is pointing to.
            fn consume_symbol_and_maybe_alias<'a>(
                source: &'a InputSource, iter: &mut TokenIter, ctx: &mut PassCtx,
                module_name: &StringRef<'static>, module_root_id: RootId,
            ) -> Result<(AliasedSymbol, SymbolDefinition), ParseError> {
                // Consume symbol name and make sure it points to an existing definition
                let symbol_identifier = consume_ident_interned(source, iter, ctx)?;

                // Consume alias text if specified
                let alias_identifier = if peek_ident(source, iter) == Some(b"as") {
                    // Consume alias
                    iter.consume();
                    Some(consume_ident_interned(source, iter, ctx)?)
                } else {
                    None
                };

                let target = ctx.symbols.get_symbol_by_name_defined_in_scope(
                    SymbolScope::Module(module_root_id), symbol_identifier.value.as_bytes()
                );

                if target.is_none() {
                    return Err(ParseError::new_error_at_span(
                        source, symbol_identifier.span,
                        format!(
                            "could not find symbol '{}' within module '{}'",
                            symbol_identifier.value.as_str(), module_name.as_str()
                        )
                    ));
                }
                let target = target.unwrap();
                debug_assert_ne!(target.class(), SymbolClass::Module);
                let target_definition = target.variant.as_definition();

                Ok((
                    AliasedSymbol{
                        name: symbol_identifier,
                        alias: alias_identifier,
                        definition_id: target_definition.definition_id,
                    },
                    target_definition.clone()
                ))
            }

            let next = iter.next();

            if Some(TokenKind::Ident) == next {
                // Importing a single symbol
                iter.consume();
                let (imported_symbol, symbol_definition) = consume_symbol_and_maybe_alias(
                    &module.source, &mut iter, ctx, &module_identifier.value, target_root_id
                )?;

                let alias_identifier = match imported_symbol.alias.as_ref() {
                    Some(alias) => alias.clone(),
                    None => imported_symbol.name.clone(),
                };

                import_id = ctx.heap.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    span: InputSpan::from_positions(import_span.begin, alias_identifier.span.end),
                    module: module_identifier,
                    module_id: target_root_id,
                    symbols: vec![imported_symbol],
                }));
                if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(
                    SymbolScope::Module(module.root_id),
                    Symbol{
                        name: alias_identifier.value,
                        variant: SymbolVariant::Definition(symbol_definition.into_imported(import_id))
                    }
                ) {
                    return Err(construct_symbol_conflict_error(
                        modules, module_idx, ctx, &new_symbol, &old_symbol
                    ));
                }
            } else if Some(TokenKind::OpenCurly) == next {
                // Importing multiple symbols
                let mut end_of_list = iter.last_valid_pos();
                consume_comma_separated(
                    TokenKind::OpenCurly, TokenKind::CloseCurly, &module.source, &mut iter, ctx,
                    |source, iter, ctx| consume_symbol_and_maybe_alias(
                        source, iter, ctx, &module_identifier.value, target_root_id
                    ),
                    &mut self.found_symbols, "a symbol", "a list of symbols to import", Some(&mut end_of_list)
                )?;

                // Preallocate import
                import_id = ctx.heap.alloc_import(|this| Import::Symbols(ImportSymbols {
                    this,
                    span: InputSpan::from_positions(import_span.begin, end_of_list),
                    module: module_identifier,
                    module_id: target_root_id,
                    symbols: Vec::with_capacity(self.found_symbols.len()),
                }));

                // Fill import symbols while inserting symbols in the
                // appropriate scope in the symbol table.
                let import = ctx.heap[import_id].as_symbols_mut();

                for (imported_symbol, symbol_definition) in self.found_symbols.drain(..) {
                    let import_name = match imported_symbol.alias.as_ref() {
                        Some(import) => import.value.clone(),
                        None => imported_symbol.name.value.clone()
                    };

                    import.symbols.push(imported_symbol);
                    if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(
                        SymbolScope::Module(module.root_id), Symbol{
                            name: import_name,
                            variant: SymbolVariant::Definition(symbol_definition.into_imported(import_id))
                        }
                    ) {
                        return Err(construct_symbol_conflict_error(modules, module_idx, ctx, &new_symbol, &old_symbol));
                    }
                }
            } else if Some(TokenKind::Star) == next {
                // Import all symbols from the module
                let star_span = iter.next_span();

                iter.consume();
                self.scoped_symbols.clear();
                let _found = ctx.symbols.get_all_symbols_defined_in_scope(
                    SymbolScope::Module(target_root_id),
                    &mut self.scoped_symbols
                );
                debug_assert!(_found); // even modules without symbols should have a scope

                // Preallocate import
                import_id = ctx.heap.alloc_import(|this| Import::Symbols(ImportSymbols{
                    this,
                    span: InputSpan::from_positions(import_span.begin, star_span.end),
                    module: module_identifier,
                    module_id: target_root_id,
                    symbols: Vec::with_capacity(self.scoped_symbols.len())
                }));

                // Fill import AST node and symbol table
                let import = ctx.heap[import_id].as_symbols_mut();

                for symbol in self.scoped_symbols.drain(..) {
                    let symbol_name = symbol.name;
                    match symbol.variant {
                        SymbolVariant::Definition(symbol_definition) => {
                            import.symbols.push(AliasedSymbol{
                                name: Identifier{ span: star_span, value: symbol_name.clone() },
                                alias: None,
                                definition_id: symbol_definition.definition_id,
                            });

                            if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(
                                SymbolScope::Module(module.root_id),
                                Symbol{
                                    name: symbol_name,
                                    variant: SymbolVariant::Definition(symbol_definition.into_imported(import_id))
                                }
                            ) {
                                return Err(construct_symbol_conflict_error(modules, module_idx, ctx, &new_symbol, &old_symbol));
                            }
                        },
                        _ => unreachable!(),
                    }
                }
            } else {
                return Err(ParseError::new_error_str_at_pos(
                    &module.source, iter.last_valid_pos(), "expected symbol name, '{' or '*'"
                ));
            }
        } else {
            // Assume implicit alias
            let module_name_str = module_identifier.value.clone();
            let last_ident_start = module_name_str.as_str().rfind('.').map_or(0, |v| v + 1);
            let alias_text = &module_name_str.as_bytes()[last_ident_start..];
            let alias = ctx.pool.intern(alias_text);
            let alias_span = InputSpan::from_positions(
                module_name_span.begin.with_offset(last_ident_start as u32),
                module_name_span.end
            );
            let alias_identifier = Identifier{ span: alias_span, value: alias.clone() };

            import_id = ctx.heap.alloc_import(|this| Import::Module(ImportModule{
                this,
                span: InputSpan::from_positions(import_span.begin, module_identifier.span.end),
                module: module_identifier,
                alias: alias_identifier,
                module_id: target_root_id,
            }));
            if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(SymbolScope::Module(module.root_id), Symbol{
                name: alias,
                variant: SymbolVariant::Module(SymbolModule{
                    root_id: target_root_id,
                    introduced_at: import_id
                })
            }) {
                return Err(construct_symbol_conflict_error(modules, module_idx, ctx, &new_symbol, &old_symbol));
            }
        }

        // By now the `import_id` is set, just need to make sure that the import
        // properly ends with a semicolon
        consume_token(&module.source, &mut iter, TokenKind::SemiColon)?;
        self.imports.push(import_id);

        Ok(())
    }
}

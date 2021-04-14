use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::protocol::input_source2::*;
use crate::protocol::ast::*;
use crate::collections::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SymbolScope {
    Module(RootId),
    Definition(DefinitionId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SymbolClass {
    Module,
    Struct,
    Enum,
    Union,
    Function,
    Component
}

struct ScopedSymbols {
    scope: SymbolScope,
    parent_scope: Option<SymbolScope>,
    child_scopes: Vec<SymbolScope>,
    start: usize,
    end: usize,
}

pub struct Symbol {
    // Definition location
    pub defined_in_module: RootId,
    pub defined_in_scope: SymbolScope,
    pub definition_span: InputSpan, // full span of definition
    // Introduction location (if imported instead of defined)

    // Symbol properties
    pub class: SymbolClass,
    pub name: StringRef,
    pub definition: Option<DefinitionId>,
}

impl Symbol {
    pub(crate) fn new(root_id: RootId, scope: SymbolScope, span: InputSpan, class: SymbolClass, name: StringRef) -> Self {
        Self{
            defined_in_module: root_id,
            defined_in_scope: scope,
            definition_span: span,
            class,
            name,
            definition: None,
        }
    }
}

pub struct SymbolTable {
    module_lookup: HashMap<StringRef, RootId>,
    scope_lookup: HashMap<SymbolScope, ScopedSymbols>,
    symbols: Vec<Symbol>,
}

impl SymbolTable {
    /// Inserts a new module by its name. Upon module naming conflict the
    /// previously associated `RootId` will be returned.
    pub(crate) fn insert_module(&mut self, module_name: StringRef, root_id: RootId) -> Result<(), RootId> {
        match self.module_lookup.entry(module_name) {
            Entry::Occupied(v) => {
                Err(*v.get())
            },
            Entry::Vacant(v) => {
                v.insert(root_id);
                Ok(())
            }
        }
    }

    /// Inserts a new scope with defined symbols. The `parent_scope` must
    /// already be added to the symbol table. The symbols are expected to come
    /// from a temporary buffer and are copied inside the symbol table. Will
    /// return an error if there is a naming conflict.
    pub(crate) fn insert_scoped_symbols(
        &mut self, parent_scope: Option<SymbolScope>, within_scope: SymbolScope, symbols: &[Symbol]
    ) -> Result<(), ParseError> {
        // Add scoped symbols
        let old_num_symbols = self.symbols.len();

        let new_scope = ScopedSymbols {
            scope: within_scope,
            parent_scope,
            child_scopes: Vec::new(),
            start: old_num_symbols,
            end: old_num_symbols + symbols.len(),
        };

        self.symbols.extend(symbols);
        self.scope_lookup.insert(within_scope, new_scope);

        if let Some(parent_scope) = parent_scope.as_ref() {
            let parent = self.scope_lookup.get_mut(parent_scope).unwrap();
            parent.child_scopes.push(within_scope);
        }

        Ok(())
    }
}
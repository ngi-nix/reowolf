/// symbol_table.rs
///
/// The datastructure used to lookup symbols within particular scopes. Scopes
/// may be module-level or definition level, although imports and definitions
/// within definitions are currently not allowed.
///
/// TODO: Once the compiler has matured, find out ways to optimize to prevent
///     the repeated HashMap lookup.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::protocol::input_source2::*;
use crate::protocol::ast::*;
use crate::collections::*;

const RESERVED_SYMBOLS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    symbols: Vec<Symbol>,
}

pub enum SymbolDefinition {
    Module(RootId),
    Struct(StructDefinitionId),
    Enum(EnumDefinitionId),
    Union(UnionDefinitionId),
    Function(FunctionDefinitionId),
    Component(ComponentDefinitionId),
}

impl SymbolDefinition {
    pub fn symbol_class(&self) -> SymbolClass {
        use SymbolDefinition as SD;
        use SymbolClass as SC;

        match self {
            SD::Module(_) => SC::Module,
            SD::Struct(_) => SC::Struct,
            SD::Enum(_) => SC::Enum,
            SD::Union(_) => SC::Union,
            SD::Function(_) => SC::Function,
            SD::Component(_) => SC::Component,
        }
    }
}

pub enum SymbolData {
    
}

pub struct Symbol {
    // Definition location (may be different from the scope/module in which it
    // is used if the symbol is imported)
    pub defined_in_module: RootId,
    pub defined_in_scope: SymbolScope,
    pub definition_span: InputSpan, // full span of definition, not just the name
    pub identifier_span: InputSpan, // span of just the identifier
    // Introduction location (if imported instead of defined)
    pub introduced_at: Option<ImportId>,
    // Symbol properties
    pub name: StringRef<'static>,
    pub definition: SymbolDefinition,
}

pub struct SymbolTable {
    module_lookup: HashMap<StringRef<'static>, RootId>,
    scope_lookup: HashMap<SymbolScope, ScopedSymbols>,
}

impl SymbolTable {
    /// Inserts a new module by its name. Upon module naming conflict the
    /// previously associated `RootId` will be returned.
    pub(crate) fn insert_module(&mut self, module_name: StringRef<'static>, root_id: RootId) -> Result<(), RootId> {
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

    /// Retrieves module `RootId` by name
    pub(crate) fn get_module_by_name(&mut self, name: &[u8]) -> Option<RootId> {
        let string_ref = StringRef::new(name);
        self.module_lookup.get(&string_ref).map(|v| *v)
    }

    /// Inserts a new symbol scope. The parent must have been added to the
    /// symbol table before.
    pub(crate) fn insert_scope(&mut self, parent_scope: Option<SymbolScope>, new_scope: SymbolScope) {
        debug_assert!(
            parent_scope.is_none() || self.scope_lookup.contains_key(parent_scope.as_ref().unwrap()),
            "inserting scope {:?} but parent {:?} does not exist", new_scope, parent_scope
        );
        debug_assert!(!self.scope_lookup.contains_key(&new_scope), "inserting scope {:?}, but it already exists", new_scope);

        if let Some(parent_scope) = parent_scope {
            let parent = self.scope_lookup.get_mut(&parent_scope).unwrap();
            parent.child_scopes.push(new_scope);
        }

        let scope = ScopedSymbols {
            scope: new_scope,
            parent_scope,
            child_scopes: Vec::with_capacity(RESERVED_SYMBOLS),
            symbols: Vec::with_capacity(RESERVED_SYMBOLS)
        };
        self.scope_lookup.insert(new_scope, scope);
    }

    /// Inserts a symbol into a particular scope. The symbol's name may not
    /// exist in the scope or any of its parents. If it does collide then the
    /// symbol will be returned, together with the symbol that has the same
    /// name.
    pub(crate) fn insert_symbol(&mut self, in_scope: SymbolScope, symbol: Symbol) -> Result<(), (Symbol, &Symbol)> {
        debug_assert!(self.scope_lookup.contains_key(&in_scope), "inserting symbol {}, but scope {:?} does not exist", symbol.name.as_str(), in_scope);
        let mut seek_scope = in_scope;
        loop {
            let scoped_symbols = self.scope_lookup.get(&seek_scope).unwrap();
            for existing_symbol in scoped_symbols.symbols.iter() {
                if symbol.name == existing_symbol.name {
                    return Err((symbol, existing_symbol))
                }
            }

            match scoped_symbols.parent_scope {
                Some(parent_scope) => { seek_scope = parent_scope; },
                None => { break; }
            }
        }

        // If here, then there is no collision
        let scoped_symbols = self.scope_lookup.get_mut(&in_scope).unwrap();
        scoped_symbols.symbols.push(symbol);
        Ok(())
    }

    /// Retrieves a particular scope. As this will be called by the compiler to
    /// retrieve scopes that MUST exist, this function will panic if the
    /// indicated scope does not exist.
    pub(crate) fn get_scope_by_id(&mut self, scope: &SymbolScope) -> &mut ScopedSymbols {
        debug_assert!(self.scope_lookup.contains_key(scope), "retrieving scope {:?}, but it doesn't exist", scope);
        self.scope_lookup.get_mut(scope).unwrap()
    }
}
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

use crate::protocol::input_source::*;
use crate::protocol::ast::*;
use crate::collections::*;

const RESERVED_SYMBOLS: usize = 32;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum SymbolScope {
    Global,
    Module(RootId),
    Definition(DefinitionId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolClass {
    Module,
    Struct,
    Enum,
    Union,
    Function,
    Component
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionClass {
    Struct,
    Enum,
    Union,
    Function,
    Component,
}

impl DefinitionClass {
    fn as_symbol_class(&self) -> SymbolClass {
        match self {
            DefinitionClass::Struct => SymbolClass::Struct,
            DefinitionClass::Enum => SymbolClass::Enum,
            DefinitionClass::Union => SymbolClass::Union,
            DefinitionClass::Function => SymbolClass::Function,
            DefinitionClass::Component => SymbolClass::Component,
        }
    }
}

struct ScopedSymbols {
    scope: SymbolScope,
    parent_scope: Option<SymbolScope>,
    child_scopes: Vec<SymbolScope>,
    symbols: Vec<Symbol>,
}

impl ScopedSymbols {
    fn get_symbol<'a>(&'a self, name: &StringRef) -> Option<&'a Symbol> {
        for symbol in self.symbols.iter() {
            if symbol.name == *name {
                return Some(symbol);
            }
        }

        None
    }
}

#[derive(Debug, Clone)]
pub struct SymbolModule {
    pub root_id: RootId,
    pub introduced_at: ImportId,
}

#[derive(Debug, Clone)]
pub struct SymbolDefinition {
    // Definition location (not necessarily the place where the symbol
    // is introduced, as it may be imported). Builtin symbols will have invalid
    // spans and module IDs
    pub defined_in_module: RootId,
    pub defined_in_scope: SymbolScope,
    pub definition_span: InputSpan, // full span of definition
    pub identifier_span: InputSpan, // span of just the identifier
    // Location where the symbol is introduced in its scope
    pub imported_at: Option<ImportId>,
    // Definition in the heap, with a utility enum to determine its
    // class if the ID is not needed.
    pub class: DefinitionClass,
    pub definition_id: DefinitionId,
}

impl SymbolDefinition {
    /// Clones the entire data structure, but replaces the `imported_at` field
    /// with the supplied `ImportId`.
    pub(crate) fn into_imported(mut self, imported_at: ImportId) -> Self {
        self.imported_at = Some(imported_at);
        self
    }
}

#[derive(Debug, Clone)]
pub enum SymbolVariant {
    Module(SymbolModule),
    Definition(SymbolDefinition),
}

impl SymbolVariant {
    /// Returns the span at which the item was introduced. For an imported
    /// item (all modules, and imported types) this returns the span of the
    /// import. For a defined type this returns the span of the identifier
    pub(crate) fn span_of_introduction(&self, heap: &Heap) -> InputSpan {
        match self {
            SymbolVariant::Module(v) => heap[v.introduced_at].span(),
            SymbolVariant::Definition(v) => if let Some(import_id) = v.imported_at {
                heap[import_id].span()
            } else {
                v.identifier_span
            },
        }
    }

    pub(crate) fn as_module(&self) -> &SymbolModule {
        match self {
            SymbolVariant::Module(v) => v,
            SymbolVariant::Definition(_) => unreachable!("called 'as_module' on {:?}", self),
        }
    }

    pub(crate) fn as_definition(&self) -> &SymbolDefinition {
        match self {
            SymbolVariant::Module(v) => unreachable!("called 'as_definition' on {:?}", self),
            SymbolVariant::Definition(v) => v,
        }
    }

    pub(crate) fn as_definition_mut(&mut self) -> &mut SymbolDefinition {
        match self {
            SymbolVariant::Module(v) => unreachable!("called 'as_definition_mut' on {:?}", self),
            SymbolVariant::Definition(v) => v,
        }
    }
}

/// TODO: @Cleanup - remove clone everywhere
#[derive(Clone)]
pub struct Symbol {
    pub name: StringRef<'static>,
    pub variant: SymbolVariant,
}

impl Symbol {
    pub(crate) fn class(&self) -> SymbolClass {
        match &self.variant {
            SymbolVariant::Module(_) => SymbolClass::Module,
            SymbolVariant::Definition(data) => data.class.as_symbol_class(),
        }
    }
}

pub struct SymbolTable {
    module_lookup: HashMap<StringRef<'static>, RootId>,
    scope_lookup: HashMap<SymbolScope, ScopedSymbols>,
}

impl SymbolTable {
    pub(crate) fn new() -> Self {
        Self{
            module_lookup: HashMap::new(),
            scope_lookup: HashMap::new(),
        }
    }
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

    /// Retrieves a symbol by name by searching in a particular scope and that scope's parents. The
    /// returned symbol may both be imported as defined within any of the searched scopes.
    pub(crate) fn get_symbol_by_name(
        &self, mut in_scope: SymbolScope, name: &[u8]
    ) -> Option<&Symbol> {
        let string_ref = StringRef::new(name);
        loop {
            let scope = self.scope_lookup.get(&in_scope);
            if scope.is_none() {
                return None;
            }
            let scope = scope.unwrap();

            if let Some(symbol) = scope.get_symbol(&string_ref) {
                return Some(symbol);
            } else {
                // Could not find symbol in current scope, seek in the parent scope if it exists
                match &scope.parent_scope {
                    Some(parent_scope) => { in_scope = *parent_scope; },
                    None => return None,
                }
            }
        }
    }

    /// Retrieves a symbol by name by searching in a particular scope and that scope's parents. The
    /// returned symbol must be defined within any of the searched scopes and may not be imported.
    /// In case such an imported symbol exists then this function still returns `None`.
    pub(crate) fn get_symbol_by_name_defined_in_scope(
        &self, in_scope: SymbolScope, name: &[u8]
    ) -> Option<&Symbol> {
        match self.get_symbol_by_name(in_scope, name) {
            Some(symbol) => {
                match &symbol.variant {
                    SymbolVariant::Module(_) => {
                        None // in-scope modules are always imported
                    },
                    SymbolVariant::Definition(variant) => {
                        if variant.imported_at.is_some() || variant.defined_in_scope == SymbolScope::Global {
                            // Symbol is imported or lives in the global scope.
                            // Things in the global scope are defined by the
                            // compiler.
                            None
                        } else {
                            Some(symbol)
                        }
                    }
                }
            },
            None => None,
        }
    }

    /// Retrieves all symbols that are defined within a particular scope. Imported symbols are
    /// ignored. Returns `true` if the scope was found (which may contain 0 defined symbols) and
    /// `false` if the scope was not found.
    pub(crate) fn get_all_symbols_defined_in_scope(&self, in_scope: SymbolScope, target: &mut Vec<Symbol>) -> bool {
        match self.scope_lookup.get(&in_scope) {
            Some(scope) => {
                for symbol in &scope.symbols {
                    if let SymbolVariant::Definition(definition) = &symbol.variant {
                        if definition.imported_at.is_some() {
                            continue;
                        }

                        // Defined in scope, so push onto target
                        target.push(symbol.clone());
                    }
                }

                true
            },
            None => false,
        }
    }
}
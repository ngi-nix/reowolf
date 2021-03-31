// TODO: Maybe allow namespaced-aliased imports. It is currently not possible
//  to express the following:
//      import Module.Submodule as SubMod
//      import SubMod::{Symbol}
//  And it is especially not possible to express the following:
//      import SubMod::{Symbol}
//      import Module.Submodule as SubMod
use crate::protocol::ast::*;
use crate::protocol::inputsource::*;

use std::collections::{HashMap, hash_map::Entry};
use crate::protocol::parser::LexedModule;

#[derive(PartialEq, Eq, Hash)]
struct SymbolKey {
    module_id: RootId,
    symbol_name: Vec<u8>,
}

pub(crate) enum Symbol {
    Namespace(RootId),
    Definition((RootId, DefinitionId)),
}

pub(crate) struct SymbolValue {
    // Position is the place where the symbol is introduced to a module (this
    // position always corresponds to the module whose RootId is stored in the
    // `SymbolKey` associated with this `SymbolValue`). For a definition this
    // is the position where the symbol is defined, for an import this is the
    // position of the import statement.
    pub(crate) position: InputPosition,
    pub(crate) symbol: Symbol,
}

impl SymbolValue {
    pub(crate) fn is_namespace(&self) -> bool {
        match &self.symbol {
            Symbol::Namespace(_) => true,
            _ => false
        }
    }
    pub(crate) fn as_namespace(&self) -> Option<RootId> {
        match &self.symbol {
            Symbol::Namespace(root_id) => Some(*root_id),
            _ => None,
        }
    }

    pub(crate) fn as_definition(&self) -> Option<(RootId, DefinitionId)> {
        match &self.symbol {
            Symbol::Definition((root_id, definition_id)) => Some((*root_id, *definition_id)),
            _ => None,
        }
    }
}
/// `SymbolTable` is responsible for two parts of the parsing process: firstly
/// it ensures that there are no clashing symbol definitions within each file,
/// and secondly it will resolve all symbols within a module to their
/// appropriate definitions (in case of enums, functions, etc.) and namespaces
/// (currently only external modules can act as namespaces). If a symbol clashes
/// or if a symbol cannot be resolved this will be an error.
///
/// Within the compilation process the symbol table is responsible for resolving
/// namespaced identifiers (e.g. Module::Enum::EnumVariant) to the appropriate
/// definition (i.e. not namespaces; as the language has no way to use
/// namespaces except for using them in namespaced identifiers).
pub(crate) struct SymbolTable {
    // Lookup from module name (not any aliases) to the root id
    module_lookup: HashMap<Vec<u8>, RootId>,
    // Lookup from within a module, to a particular imported (potentially
    // aliased) or defined symbol. Basically speaking: if the source code of a
    // module contains correctly imported/defined symbols, then this lookup
    // will always return the corresponding definition
    symbol_lookup: HashMap<SymbolKey, SymbolValue>,
}

impl SymbolTable {
    pub(crate) fn new() -> Self {
        Self{ module_lookup: HashMap::new(), symbol_lookup: HashMap::new() }
    }

    pub(crate) fn build(&mut self, heap: &Heap, modules: &[LexedModule]) -> Result<(), ParseError2> {
        // Sanity checks
        debug_assert!(self.module_lookup.is_empty());
        debug_assert!(self.symbol_lookup.is_empty());
        if cfg!(debug_assertions) {
            for (index, module) in modules.iter().enumerate() {
                debug_assert_eq!(
                    index, module.root_id.index as usize,
                    "module RootId does not correspond to LexedModule index"
                )
            }
        }

        // Preparation: create a lookup from module name to root id. This does
        // not take aliasing into account.
        self.module_lookup.reserve(modules.len());
        for module in modules {
            // TODO: Maybe put duplicate module name checking here?
            // TODO: @string
            self.module_lookup.insert(module.module_name.clone(), module.root_id);
        }

        // Preparation: determine total number of imports we will be inserting
        // into the lookup table. We could just iterate over the arena, but then
        // we don't know the source file the import belongs to.
        let mut lookup_reserve_size = 0;
        for module in modules {
            let module_root = &heap[module.root_id];
            for import_id in &module_root.imports {
                match &heap[*import_id] {
                    Import::Module(_) => lookup_reserve_size += 1,
                    Import::Symbols(import) => {
                        if import.symbols.is_empty() {
                            // Add all symbols from the other module
                            match self.module_lookup.get(&import.module_name) {
                                Some(target_module_id) => {
                                    lookup_reserve_size += heap[*target_module_id].definitions.len()
                                },
                                None => {
                                    return Err(
                                        ParseError2::new_error(&module.source, import.position, "Cannot resolve module")
                                    );
                                }
                            }
                        } else {
                            lookup_reserve_size += import.symbols.len();
                        }
                    }
                }
            }

            lookup_reserve_size += module_root.definitions.len();
        }

        self.symbol_lookup.reserve(lookup_reserve_size);

        // First pass: we go through all of the modules and add lookups to
        // symbols that are defined within that module. Cross-module imports are
        // not yet resolved
        for module in modules {
            let root = &heap[module.root_id];
            for definition_id in &root.definitions {
                let definition = &heap[*definition_id];
                let identifier = definition.identifier();
                if let Err(previous_position) = self.add_definition_symbol(
                    module.root_id, identifier.position, &identifier.value,
                    module.root_id, *definition_id
                ) {
                    return Err(
                        ParseError2::new_error(&module.source, definition.position(), "Symbol is multiply defined")
                            .with_postfixed_info(&module.source, previous_position, "Previous definition was here")
                    )
                }
            }
        }

        // Second pass: now that we can find symbols in modules, we can resolve
        // all imports (if they're correct, that is)
        for module in modules {
            let root = &heap[module.root_id];
            for import_id in &root.imports {
                let import = &heap[*import_id];
                match import {
                    Import::Module(import) => {
                        // Find the module using its name
                        let target_root_id = self.resolve_module(&import.module_name);
                        if target_root_id.is_none() {
                            return Err(ParseError2::new_error(&module.source, import.position, "Could not resolve module"));
                        }
                        let target_root_id = target_root_id.unwrap();
                        if target_root_id == module.root_id {
                            return Err(ParseError2::new_error(&module.source, import.position, "Illegal import of self"));
                        }

                        // Add the target module under its alias
                        if let Err(previous_position) = self.add_namespace_symbol(
                            module.root_id, import.position,
                            &import.alias, target_root_id
                        ) {
                            return Err(
                                ParseError2::new_error(&module.source, import.position, "Symbol is multiply defined")
                                    .with_postfixed_info(&module.source, previous_position, "Previous definition was here")
                            );
                        }
                    },
                    Import::Symbols(import) => {
                        // Find the target module using its name
                        let target_root_id = self.resolve_module(&import.module_name);
                        if target_root_id.is_none() {
                            return Err(ParseError2::new_error(&module.source, import.position, "Could not resolve module of symbol imports"));
                        }
                        let target_root_id = target_root_id.unwrap();
                        if target_root_id == module.root_id {
                            return Err(ParseError2::new_error(&module.source, import.position, "Illegal import of self"));
                        }

                        // Determine which symbols to import
                        if import.symbols.is_empty() {
                            // Import of all symbols, not using any aliases
                            for definition_id in &heap[target_root_id].definitions {
                                let definition = &heap[*definition_id];
                                let identifier = definition.identifier();
                                if let Err(previous_position) = self.add_definition_symbol(
                                    module.root_id, import.position, &identifier.value,
                                    target_root_id, *definition_id
                                ) {
                                    return Err(
                                        ParseError2::new_error(
                                            &module.source, import.position,
                                            &format!("Imported symbol '{}' is already defined", String::from_utf8_lossy(&identifier.value))
                                        )
                                        .with_postfixed_info(
                                            &modules[target_root_id.index as usize].source,
                                            definition.position(),
                                            "The imported symbol is defined here"
                                        )
                                        .with_postfixed_info(
                                            &module.source, previous_position, "And is previously defined here"
                                        )
                                    )
                                }
                            }
                        } else {
                            // Import of specific symbols, optionally using aliases
                            for symbol in &import.symbols {
                                // Because we have already added per-module definitions, we can use
                                // the table to lookup this particular symbol. Note: within a single
                                // module a namespace-import and a symbol-import may not collide.
                                // Hence per-module symbols are unique.
                                // However: if we import a symbol from another module, we don't want
                                // to "import a module's imported symbol". And so if we do find
                                // a symbol match, we need to make sure it is a definition from
                                // within that module by checking `source_root_id == target_root_id`
                                let target_symbol = self.resolve_symbol(target_root_id, &symbol.name);
                                let symbol_definition_id = match target_symbol {
                                    Some(target_symbol) => {
                                        match target_symbol.symbol {
                                            Symbol::Definition((symbol_root_id, symbol_definition_id)) => {
                                                if symbol_root_id == target_root_id {
                                                    Some(symbol_definition_id)
                                                } else {
                                                    // This is imported within the target module, and not
                                                    // defined within the target module
                                                    None
                                                }
                                            },
                                            Symbol::Namespace(_) => {
                                                // We don't import a module's "module import"
                                                None
                                            }
                                        }
                                    },
                                    None => None
                                };

                                if symbol_definition_id.is_none() {
                                    return Err(
                                        ParseError2::new_error(&module.source, symbol.position, "Could not resolve symbol")
                                    )
                                }
                                let symbol_definition_id = symbol_definition_id.unwrap();

                                if let Err(previous_position) = self.add_definition_symbol(
                                    module.root_id, symbol.position, &symbol.alias,
                                    target_root_id, symbol_definition_id
                                ) {
                                    return Err(
                                        ParseError2::new_error(&module.source, symbol.position, "Symbol is multiply defined")
                                            .with_postfixed_info(&module.source, previous_position, "Previous definition was here")
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
        fn find_name(heap: &Heap, root_id: RootId) -> String {
            let root = &heap[root_id];
            for pragma_id in &root.pragmas {
                match &heap[*pragma_id] {
                    Pragma::Module(module) => {
                        return String::from_utf8_lossy(&module.value).to_string()
                    },
                    _ => {},
                }
            }

            return String::from("Unknown")
        }

        debug_assert_eq!(
            self.symbol_lookup.len(), lookup_reserve_size,
            "miscalculated reserved size for symbol lookup table"
        );
        Ok(())
    }

    /// Resolves a module by its defined name
    pub(crate) fn resolve_module(&self, identifier: &Vec<u8>) -> Option<RootId> {
        self.module_lookup.get(identifier).map(|v| *v)
    }

    /// Resolves a symbol within a particular module, indicated by its RootId,
    /// with a single non-namespaced identifier
    pub(crate) fn resolve_symbol(&self, within_module_id: RootId, identifier: &Vec<u8>) -> Option<&SymbolValue> {
        self.symbol_lookup.get(&SymbolKey{ module_id: within_module_id, symbol_name: identifier.clone() })
    }

    /// Resolves a namespaced symbol. This method will go as far as possible in
    /// going to the right symbol. It will halt the search when:
    /// 1. Polymorphic arguments are encountered on the identifier.
    /// 2. A non-namespace symbol is encountered.
    /// 3. A part of the identifier couldn't be resolved to anything
    pub(crate) fn resolve_namespaced_symbol<'t, 'i>(
        &'t self, root_module_id: RootId, identifier: &'i NamespacedIdentifier2
    ) -> (Option<&'t Symbol>, &'i NamespacedIdentifier2Iter) {
        let mut iter = identifier.iter();
        let mut symbol: Option<&SymbolValue> = None;
        let mut within_module_id = root_module_id;

        while let Some((partial, poly_args)) = iter.next() {
            // Lookup the symbol within the currently iterated upon module
            let lookup_key = SymbolKey{ module_id: within_module_id, symbol_name: Vec::from(partial) };
            let new_symbol = self.symbol_lookup.get(&lookup_key);
            
            match new_symbol {
                None => {
                    // Can't find anything
                    break;
                },
                Some(new_symbol) => {
                    // Found something, but if we already moved to another
                    // module then we don't want to keep jumping across modules,
                    // we're only interested in symbols defined within that
                    // module.
                    match &new_symbol.symbol {
                        Symbol::Namespace(new_root_id) => {
                            if root_module_id != within_module_id {
                            within_module_id = *new_root_id;
                            symbol = Some(new_symbol);
                        },
                        Symbol::Definition((definition_root_id, _)) => {
                            // Found a definition, but if we already jumped
                            // modules, then this must be defined within that
                            // module.
                            if root_module_id != within_module_id && within_module_id != *definition_root_id {
                                // This is an imported definition within the module
                                // TODO: Maybe factor out? Dunno...
                                debug_assert!(symbol.is_some());
                                debug_assert!(symbol.unwrap().is_namespace());
                                debug_assert!(iter.num_returned() > 1);
                                let to_skip = iter.num_returned() - 1;
                                iter = identifier.iter();
                                for _ in 0..to_skip { iter.next(); }
                                break;
                            }
                            symbol = Some(new_symbol);
                            break;
                        }
                    }
                }
            }
        }

        match symbol {
            None => Ok(None),
            Some(symbol) => Ok(Some((symbol, iter)))
        }
    }

    /// Attempts to add a namespace symbol. Returns `Ok` if the symbol was
    /// inserted. If the symbol already exists then `Err` will be returned
    /// together with the previous definition's source position (in the origin
    /// module's source file).
    // Note: I would love to return a reference to the value, but Rust is
    // preventing me from doing so... That, or I'm not smart enough...
    fn add_namespace_symbol(
        &mut self, origin_module_id: RootId, origin_position: InputPosition, symbol_name: &Vec<u8>, target_module_id: RootId
    ) -> Result<(), InputPosition> {
        let key = SymbolKey{
            module_id: origin_module_id,
            symbol_name: symbol_name.clone()
        };
        match self.symbol_lookup.entry(key) {
            Entry::Occupied(o) => Err(o.get().position),
            Entry::Vacant(v) => {
                v.insert(SymbolValue{
                    position: origin_position,
                    symbol: Symbol::Namespace(target_module_id)
                });
                Ok(())
            }
        }
    }

    /// Attempts to add a definition symbol. Returns `Ok` if the symbol was
    /// inserted. If the symbol already exists then `Err` will be returned
    /// together with the previous definition's source position (in the origin
    /// module's source file).
    fn add_definition_symbol(
        &mut self, origin_module_id: RootId, origin_position: InputPosition, symbol_name: &Vec<u8>,
        target_module_id: RootId, target_definition_id: DefinitionId,
    ) -> Result<(), InputPosition> {
        let key = SymbolKey{
            module_id: origin_module_id,
            symbol_name: symbol_name.clone()
        };
        match self.symbol_lookup.entry(key) {
            Entry::Occupied(o) => Err(o.get().position),
            Entry::Vacant(v) => {
                v.insert(SymbolValue {
                    position: origin_position,
                    symbol: Symbol::Definition((target_module_id, target_definition_id))
                });
                Ok(())
            }
        }
    }
}
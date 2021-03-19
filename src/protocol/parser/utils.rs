use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use super::symbol_table::*;
use super::type_table::*;

/// Utility result type.
pub(crate) enum FindTypeResult<'t, 'i> {
    // Found the type exactly
    Found(&'t DefinedType),
    // Could not match symbol
    SymbolNotFound{ident_pos: InputPosition},
    // Matched part of the namespaced identifier, but not completely
    SymbolPartial{ident_pos: InputPosition, symbol_pos: InputPosition, ident_iter: NamespacedIdentifierIter<'i>},
    // Symbol matched, but points to a namespace/module instead of a type
    SymbolNamespace{ident_pos: InputPosition, symbol_pos: InputPosition},
}

// TODO: @cleanup Find other uses of this pattern
impl<'t, 'i> FindTypeResult<'t, 'i> {
    /// Utility function to transform the `FindTypeResult` into a `Result` where
    /// `Ok` contains the resolved type, and `Err` contains a `ParseError` which
    /// can be readily returned. This is the most common use.
    pub(crate) fn as_parse_error(self, module_source: &InputSource) -> Result<&'t DefinedType, ParseError2> {
        match self {
            FindTypeResult::Found(defined_type) => Ok(defined_type),
            FindTypeResult::SymbolNotFound{ident_pos} => {
                Err(ParseError2::new_error(
                    module_source, ident_pos,
                    "Could not resolve this identifier to a symbol"
                ))
            },
            FindTypeResult::SymbolPartial{ident_pos, symbol_pos, ident_iter} => {
                Err(ParseError2::new_error(
                    module_source, ident_pos, 
                    "Could not fully resolve this identifier to a symbol"
                ).with_postfixed_info(
                    module_source, symbol_pos, 
                    &format!(
                        "The partial identifier '{}' was matched to this symbol",
                        String::from_utf8_lossy(ident_iter.returned_section()),
                    )
                ))
            },
            FindTypeResult::SymbolNamespace{ident_pos, symbol_pos} => {
                Err(ParseError2::new_error(
                    module_source, ident_pos,
                    "This identifier was resolved to a namespace instead of a type"
                ).with_postfixed_info(
                    module_source, symbol_pos,
                    "This is the referenced namespace"
                ))
            }
        }
    }
}

/// Attempt to find the type pointer to by a (root, identifier) combination. The
/// type must match exactly (no parts in the namespace iterator remaining) and
/// must be a type, not a namespace. 
pub(crate) fn find_type_definition<'t, 'i>(
    symbols: &SymbolTable, types: &'t TypeTable, 
    root_id: RootId, identifier: &'i NamespacedIdentifier
) -> FindTypeResult<'t, 'i> {
    // Lookup symbol
    let symbol = symbols.resolve_namespaced_symbol(root_id, identifier);
    if symbol.is_none() { 
        return FindTypeResult::SymbolNotFound{ident_pos: identifier.position};
    }
    
    // Make sure we resolved it exactly
    let (symbol, ident_iter) = symbol.unwrap();
    if ident_iter.num_remaining() != 0 { 
        return FindTypeResult::SymbolPartial{
            ident_pos: identifier.position, 
            symbol_pos: symbol.position, 
            ident_iter
        };
    }

    match symbol.symbol {
        Symbol::Namespace(_) => {
            FindTypeResult::SymbolNamespace{
                ident_pos: identifier.position, 
                symbol_pos: symbol.position
            }
        },
        Symbol::Definition((_, definition_id)) => {
            // If this function is called correctly, then we should always be
            // able to match the definition's ID to an entry in the type table.
            let definition = types.get_base_definition(&definition_id);
            debug_assert!(definition.is_some());
            FindTypeResult::Found(definition.unwrap())
        }
    }
}
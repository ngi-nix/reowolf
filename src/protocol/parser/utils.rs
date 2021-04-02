    use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use super::symbol_table::*;
use super::type_table::*;

/// Utility result type.
pub(crate) enum FindTypeResult<'t, 'i> {
    // Found the type exactly
    Found((&'t DefinedType, NamespacedIdentifierIter<'i>)),
    // Could not match symbol
    SymbolNotFound{ident_pos: InputPosition},
    // Matched part of the namespaced identifier, but not completely
    SymbolPartial{ident_pos: InputPosition, ident_iter: NamespacedIdentifierIter<'i>},
    // Symbol matched, but points to a namespace/module instead of a type
    SymbolNamespace{ident_pos: InputPosition, symbol_pos: InputPosition},
}

// TODO: @cleanup Find other uses of this pattern
// TODO: Hindsight is 20/20: this belongs in the visitor_linker, not in a 
//  separate file.
impl<'t, 'i> FindTypeResult<'t, 'i> {
    /// Utility function to transform the `FindTypeResult` into a `Result` where
    /// `Ok` contains the resolved type, and `Err` contains a `ParseError` which
    /// can be readily returned. This is the most common use.
    pub(crate) fn as_parse_error(self, module_source: &InputSource) -> Result<(&'t DefinedType, NamespacedIdentifierIter<'i>), ParseError> {
        match self {
            FindTypeResult::Found(defined_type) => Ok(defined_type),
            FindTypeResult::SymbolNotFound{ident_pos} => {
                Err(ParseError::new_error(
                    module_source, ident_pos,
                    "Could not resolve this identifier to a symbol"
                ))
            },
            FindTypeResult::SymbolPartial{ident_pos, ident_iter} => {
                Err(ParseError::new_error(
                    module_source, ident_pos, 
                    &format!(
                        "Could not fully resolve this identifier to a symbol, was only able to match '{}'",
                        &String::from_utf8_lossy(ident_iter.returned_section())
                    )
                ))
            },
            FindTypeResult::SymbolNamespace{ident_pos, symbol_pos} => {
                Err(ParseError::new_error(
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
    let (symbol, ident_iter) = symbols.resolve_namespaced_identifier(root_id, identifier);
    if symbol.is_none() { 
        return FindTypeResult::SymbolNotFound{ident_pos: identifier.position};
    }
    
    // Make sure we resolved it exactly
    let symbol = symbol.unwrap();
    if ident_iter.num_remaining() != 0 { 
        return FindTypeResult::SymbolPartial{
            ident_pos: identifier.position,
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
            FindTypeResult::Found((definition.unwrap(), ident_iter))
        }
    }
}

pub(crate) enum MatchPolymorphResult<'t> {
    Matching,
    InferAll(usize),
    Mismatch{defined_type: &'t DefinedType, ident_position: InputPosition, num_specified: usize},
    NoneExpected{defined_type: &'t DefinedType, ident_position: InputPosition},
}

impl<'t> MatchPolymorphResult<'t> {
    pub(crate) fn as_parse_error(self, heap: &Heap, module_source: &InputSource) -> Result<usize, ParseError> {
        match self {
            MatchPolymorphResult::Matching => Ok(0),
            MatchPolymorphResult::InferAll(count) => {
                debug_assert!(count > 0);
                Ok(count)
            },
            MatchPolymorphResult::Mismatch{defined_type, ident_position, num_specified} => {
                let type_identifier = heap[defined_type.ast_definition].identifier();
                let args_name = if defined_type.poly_vars.len() == 1 {
                    "argument"
                } else {
                    "arguments"
                };

                return Err(ParseError::new_error(
                    module_source, ident_position,
                    &format!(
                        "expected {} polymorphic {} (or none, to infer them) for the type {}, but {} were specified",
                        defined_type.poly_vars.len(), args_name, 
                        &String::from_utf8_lossy(&type_identifier.value),
                        num_specified
                    )
                ))
            },
            MatchPolymorphResult::NoneExpected{defined_type, ident_position, ..} => {
                let type_identifier = heap[defined_type.ast_definition].identifier();
                return Err(ParseError::new_error(
                    module_source, ident_position,
                    &format!(
                        "the type {} is not polymorphic",
                        &String::from_utf8_lossy(&type_identifier.value)
                    )
                ))
            }
        }
    }
}

/// Attempt to match the polymorphic arguments to the number of polymorphic
/// variables in the definition.
pub(crate) fn match_polymorphic_args_to_vars<'t>(
    defined_type: &'t DefinedType, poly_args: Option<&[ParserTypeId]>, ident_position: InputPosition
) -> MatchPolymorphResult<'t> {
    if defined_type.poly_vars.is_empty() {
        // No polymorphic variables on type
        if poly_args.is_some() {
            return MatchPolymorphResult::NoneExpected{
                defined_type,
                ident_position, 
            };
        }
    } else {
        // Polymorphic variables on type
        let has_specified = poly_args.map_or(false, |a| a.len() != 0);
        if !has_specified {
            // Implicitly infer all of the polymorphic arguments
            return MatchPolymorphResult::InferAll(defined_type.poly_vars.len());
        }

        let num_specified = poly_args.unwrap().len();
        if num_specified != defined_type.poly_vars.len() {
            return MatchPolymorphResult::Mismatch{
                defined_type,
                ident_position,
                num_specified,
            };
        }
    }

    MatchPolymorphResult::Matching
}
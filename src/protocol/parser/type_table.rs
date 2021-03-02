// TODO: @fix PrimitiveType for enums/unions
use crate::protocol::ast::*;
use crate::protocol::parser::symbol_table::{SymbolTable, Symbol};
use crate::protocol::inputsource::*;
use crate::protocol::parser::*;

use std::collections::HashMap;


pub enum DefinedType {
    Enum(EnumType),
    Union(UnionType),
    Struct(StructType),
    Function(FunctionType),
    Component(ComponentType)
}

// TODO: Also support maximum u64 value
pub struct EnumVariant {
    identifier: Identifier,
    value: i64,
}

/// `EnumType` is the classical C/C++ enum type. It has various variants with
/// an assigned integer value. The integer values may be user-defined,
/// compiler-defined, or a mix of the two. If a user assigns the same enum
/// value multiple times, we assume the user is an expert and we consider both
/// variants to be equal to one another.
pub struct EnumType {
    variants: Vec<EnumVariant>,
    representation: PrimitiveType,
}

pub struct UnionVariant {
    identifier: Identifier,
    embedded_type: Option<TypeAnnotationId>,
    tag_value: i64,
}

pub struct UnionType {
    variants: Vec<UnionVariant>,
    tag_representation: PrimitiveType
}

pub struct StructField {
    identifier: Identifier,
    field_type: TypeAnnotationId,
}

pub struct StructType {
    fields: Vec<StructField>,
}

pub struct FunctionArgument {
    identifier: Identifier,
    argument_type: TypeAnnotationId,
}

pub struct FunctionType {
    return_type: TypeAnnotationId,
    arguments: Vec<FunctionArgument>
}

pub struct ComponentType {
    variant: ComponentVariant,
    arguments: Vec<FunctionArgument>
}

pub struct TypeTable {
    lookup: HashMap<DefinitionId, DefinedType>,
}

enum LookupResult {
    BuiltIn,
    Resolved((RootId, DefinitionId)),
    Unresolved((RootId, DefinitionId)),
    Error((InputPosition, String)),
}

/// `TypeTable` is responsible for walking the entire lexed AST and laying out
/// the various user-defined types in terms of bytes and offsets. This process
/// may be pseudo-recursive (meaning: it is implemented in a recursive fashion,
/// but not in the recursive-function-call kind of way) in case a type depends
/// on another type to be resolved.
/// TODO: Distinction between resolved types and unresolved types (regarding the
///     mixed use of BuiltIns and resolved types) is a bit yucky at the moment,
///     will have to come up with something nice in the future.
/// TODO: Need to factor out the repeated lookup in some kind of state machine.
impl TypeTable {
    pub(crate) fn new(
        symbols: &SymbolTable, heap: &Heap, modules: &[LexedModule]
    ) -> Result<TypeTable, ParseError2> {
        if cfg!(debug_assertions) {
            for (index, module) in modules.iter().enumerate() {
                debug_assert_eq!(index, module.root_id.0.index as usize)
            }
        }

        // Estimate total number of definitions we will encounter
        let num_definitions = heap.definitions.len();
        let mut table = TypeTable{
            lookup: HashMap::with_capacity(num_definitions),
        };

        // Perform the breadcrumb-based type parsing. For now we do not allow
        // cyclic types. 
        // TODO: Allow cyclic types. However, we want to have an implementation
        //  that is somewhat efficient: if a type is cyclic, then we have to
        //  insert a pointer somewhere. However, we don't want to insert them
        //  everywhere, for each possible type. Without any further context the
        //  decision to place a pointer somewhere is "random", we need to know
        //  how the type is used to have an informed opinion on where to place
        //  the pointer.
        enum Breadcrumb {
            Linear((usize, usize)),
            Jumping((RootId, DefinitionId))
        }

        // Helper to handle the return value from lookup_type_definition. If
        // resolved/builtin we return `true`, if an error we complete the error
        // and return it. If unresolved then we check for cyclic types and
        // return an error if cyclic, otherwise return false (indicating we need
        // to continue in the breadcrumb-based resolving loop)
        let handle_lookup_result = |
            heap: &Heap, all_modules: &[LexedModule], cur_module: &LexedModule,
            breadcrumbs: &mut Vec<Breadcrumb>, result: LookupResult
        | -> Result<bool, ParseError2> {
            return match result {
                LookupResult::BuiltIn | LookupResult::Resolved(_) => Ok(true),
                LookupResult::Unresolved((new_root_id, new_definition_id)) => {
                    // Check for cyclic dependencies
                    let mut is_cyclic = false;
                    for breadcrumb in breadcrumbs.iter() {
                        let (root_id, definition_id) = match breadcrumb {
                            Breadcrumb::Linear((root_index, definition_index)) => {
                                let root_id = all_modules[*root_index].root_id;
                                let definition_id = heap[root_id].definitions[*definition_index];
                                (root_id, definition_id)
                            },
                            Breadcrumb::Jumping(root_id_and_definition_id) => *root_id_and_definition_id
                        };

                        if root_id == new_root_id && definition_id == new_definition_id {
                            // Oh noes!
                            is_cyclic = true;
                            break;
                        }
                    }

                    if is_cyclic {
                        let mut error = ParseError2::new_error(
                            &all_modules[new_root_id.0.index as usize].source,
                            heap[new_definition_id].position(),
                            "Evaluating this definition results in a a cyclic dependency"
                        );
                        for (index, breadcrumb) in breadcrumbs.iter().enumerate() {
                            match breadcrumb {
                                Breadcrumb::Linear((root_index, definition_index)) => {
                                    debug_assert_eq!(index, 0);
                                    error = error.with_postfixed_info(
                                        &all_modules[*root_index].source,
                                        heap[heap[all_modules[*root_index].root_id].definitions[*definition_index]].position(),
                                        "The cycle started with this definition"
                                    )
                                },
                                Breadcrumb::Jumping((root_id, definition_id)) => {
                                    debug_assert!(index > 0);
                                    error = error.with_postfixed_info(
                                        &all_modules[root_id.0.index as usize].source,
                                        heap[*definition_id].position(),
                                        "Which depends on this definition"
                                    )
                                }
                            }
                        }
                        Err(error)
                    } else {
                        breadcrumbs.push(Breadcrumb::Jumping((new_root_id, new_definition_id)));
                        Ok(false)
                    }
                },
                LookupResult::Error((position, message)) => {
                    Err(ParseError2::new_error(&cur_module.source, position, &message))
                }
            }
        };

        let mut module_index = 0;
        let mut definition_index = 0;
        let mut breadcrumbs = Vec::with_capacity(32); // if a user exceeds this, the user sucks at programming
        while module_index < modules.len() {
            // Go to next module if needed
            {
                let root = &heap[modules[module_index].root_id];
                if definition_index >= root.definitions.len() {
                    module_index += 1;
                    definition_index = 0;
                    continue;
                }
            }

            // Construct breadcrumbs in case we need to follow some types around
            debug_assert!(breadcrumbs.is_empty());
            breadcrumbs.push(Breadcrumb::Linear((module_index, definition_index)));
            'resolve_loop: while !breadcrumbs.is_empty() {
                // Retrieve module, the module's root and the definition
                let (module, definition_id) = match breadcrumbs.last().unwrap() {
                    Breadcrumb::Linear((module_index, definition_index)) => {
                        let module = &modules[*module_index];
                        let root = &heap[module.root_id];
                        let definition_id = root.definitions[*definition_index];
                        (module, definition_id)
                    },
                    Breadcrumb::Jumping((root_id, definition_id)) => {
                        let module = &modules[root_id.0.index as usize];
                        debug_assert_eq!(module.root_id, *root_id);
                        (module, *definition_id)
                    }
                };

                let definition = &heap[definition_id];

                // Because we might have chased around to this particular 
                // definition before, we check if we haven't resolved the type
                // already.
                if table.lookup.contains_key(&definition_id) {
                    breadcrumbs.pop();
                    continue;
                }

                match definition {
                    Definition::Enum(definition) => {
                        // Check the definition to see if we're dealing with an
                        // enum or a union. If we find any union variants then
                        // we immediately check if the type is already resolved.
                        let mut has_tag_values = None;
                        let mut has_int_values = None;
                        for variant in &definition.variants {
                            match &variant.value {
                                EnumVariantValue::None => {},
                                EnumVariantValue::Integer(_) => {
                                    if has_int_values.is_none() { has_int_values = Some(variant.position); }
                                 },
                                EnumVariantValue::Type(variant_type) => { 
                                    if has_tag_values.is_none() { has_tag_values = Some(variant.position); }

                                    let variant_type = &heap[*variant_type];

                                    let lookup_result = lookup_type_definition(
                                        heap, &table, symbols, module.root_id,
                                        &variant_type.the_type.primitive
                                    );
                                    if !handle_lookup_result(heap, modules, module, &mut breadcrumbs, lookup_result)? {
                                        continue 'resolve_loop;
                                    }
                                },
                            }
                        }

                        if has_tag_values.is_some() && has_int_values.is_some() {
                            // Not entirely illegal, but probably not desired
                            let tag_pos = has_tag_values.unwrap();
                            let int_pos = has_int_values.unwrap();
                            return Err(
                                ParseError2::new_error(&module.source, definition.position, "Illegal combination of enum integer variant(s) and enum union variant(s)")
                                .with_postfixed_info(&module.source, int_pos, "Explicitly assigning an integer value here")
                                .with_postfixed_info(&module.source, tag_pos, "Explicitly declaring a union variant here")
                            )
                        }

                        // If here, then the definition is a valid discriminated
                        // union with all of its types resolved, or a valid
                        // enum.

                        // Decide whether to implement as enum or as union
                        let is_union = has_tag_values.is_some();
                        if is_union {
                            // Implement as discriminated union. Because we 
                            // checked the availability of types above, we are
                            // safe to lookup type definitions
                            let mut tag_value = -1;
                            let mut variants = Vec::with_capacity(definition.variants.len());
                            for variant in &definition.variants {
                                tag_value += 1;
                                let embedded_type = match &variant.value {
                                    EnumVariantValue::None => {
                                        None
                                    },
                                    EnumVariantValue::Type(type_annotation_id) => {
                                        // Type should be resolvable, we checked this above
                                        let type_annotation = &heap[*type_annotation_id];
                                        // TODO: Remove the assert once I'm clear on how to layout "the types" of types
                                        if cfg!(debug_assertions) {
                                            ensure_type_definition(heap, &table, symbols, module.root_id, &type_annotation.the_type.primitive);
                                        }

                                        Some(*type_annotation_id)
                                    },
                                    EnumVariantValue::Integer(_) => {
                                        debug_assert!(false, "Encountered `Integer` variant after asserting enum is a discriminated union");
                                        unreachable!();
                                    }
                                };

                                variants.push(UnionVariant{
                                    identifier: variant.identifier.clone(),
                                    embedded_type,
                                    tag_value,
                                })
                            }

                            table.add_definition(definition_id, DefinedType::Union(UnionType{
                                variants,
                                tag_representation: enum_representation(tag_value)
                            }));
                        } else {
                            // Implement as regular enum
                            let mut enum_value = -1; // TODO: allow u64 max size
                            let mut variants = Vec::with_capacity(definition.variants.len());
                            for variant in &definition.variants {
                                enum_value += 1;
                                match &variant.value {
                                    EnumVariantValue::None => {
                                        variants.push(EnumVariant{
                                            identifier: variant.identifier.clone(),
                                            value: enum_value,
                                        });
                                    },
                                    EnumVariantValue::Integer(override_value) => {
                                        enum_value = *override_value;
                                        variants.push(EnumVariant{
                                            identifier: variant.identifier.clone(),
                                            value: enum_value,
                                        });
                                    },
                                    EnumVariantValue::Type(_) => {
                                        debug_assert!(false, "Encountered `Type` variant after asserting enum is not a discriminated union");
                                        unreachable!();
                                    }
                                }
                            }

                            table.add_definition(definition_id, DefinedType::Enum(EnumType{
                                variants,
                                representation: enum_representation(enum_value),
                            }));
                        }
                    },
                    Definition::Struct(definition) => {
                        // Before we start allocating fields, make sure we can
                        // actually resolve all of the field types
                        for field_definition in &definition.fields {
                            let type_definition = &heap[field_definition.the_type];
                            let lookup_result = lookup_type_definition(
                                heap, &table, symbols, module.root_id,
                                &type_definition.the_type.primitive
                            );
                            if !handle_lookup_result(heap, modules, module, &mut breadcrumbs, lookup_result)? {
                                continue 'resolve_loop;
                            }
                        }

                        // We can resolve everything
                        let mut fields = Vec::with_capacity(definition.fields.len());
                        for field_definition in &definition.fields {
                            let type_annotation = &heap[field_definition.the_type];
                            if cfg!(debug_assertions) {
                                ensure_type_definition(heap, &table, symbols, module.root_id, &type_annotation.the_type.primitive);
                            }

                            fields.push(StructField{
                                identifier: field_definition.field.clone(),
                                field_type: field_definition.the_type
                            });
                        }

                        table.add_definition(definition_id, DefinedType::Struct(StructType{
                            fields,
                        }));
                    },
                    Definition::Component(definition) => {
                        // As always, ensure all parameter types are resolved
                        for parameter_id in &definition.parameters {
                            let parameter = &heap[*parameter_id];
                            let type_definition = &heap[parameter.type_annotation];
                            let lookup_result = lookup_type_definition(
                                heap, &table, symbols, module.root_id,
                                &type_definition.the_type.primitive
                            );
                            if !handle_lookup_result(heap, modules, module, &mut breadcrumbs, lookup_result)? {
                                continue 'resolve_loop;
                            }
                        }

                        // We can resolve everything
                        let mut parameters = Vec::with_capacity(definition.parameters.len());
                        for parameter_id in &definition.parameters {
                            let parameter = &heap[*parameter_id];
                            let type_definition = &heap[parameter.type_annotation];
                            if cfg!(debug_assertions) {
                                ensure_type_definition(heap, &table, symbols, module.root_id, &type_definition.the_type.primitive);
                            }

                            parameters.push(FunctionArgument{
                                identifier: parameter.identifier.clone(),
                                argument_type: parameter.type_annotation,
                            });
                        }

                        table.add_definition(definition_id, DefinedType::Component(ComponentType{
                            variant: definition.variant,
                            arguments: parameters, // Arguments, parameters, tomayto, tomahto
                        }));
                    },
                    Definition::Function(definition) => {
                        // Handle checking the return type
                        let lookup_result = lookup_type_definition(
                            heap, &table, symbols, module.root_id,
                            &heap[definition.return_type].the_type.primitive
                        );
                        if !handle_lookup_result(heap, modules, module, &mut breadcrumbs, lookup_result)? {
                            continue 'resolve_loop;
                        }

                        for parameter_id in &definition.parameters {
                            let parameter = &heap[*parameter_id];
                            let type_definition = &heap[parameter.type_annotation];
                            let lookup_result = lookup_type_definition(
                                heap, &table, symbols, module.root_id,
                                &type_definition.the_type.primitive
                            );
                            if !handle_lookup_result(heap, modules, module, &mut breadcrumbs, lookup_result)? {
                                continue 'resolve_loop;
                            }
                        }

                        // Resolve function's types
                        let mut parameters = Vec::with_capacity(definition.parameters.len());
                        for parameter_id in &definition.parameters {
                            let parameter = &heap[*parameter_id];
                            let type_definition = &heap[parameter.type_annotation];
                            if cfg!(debug_assertions) {
                                ensure_type_definition(heap, &table, symbols, module.root_id, &type_definition.the_type.primitive);
                            }

                            parameters.push(FunctionArgument{
                                identifier: parameter.identifier.clone(),
                                argument_type: parameter.type_annotation
                            });
                        }

                        table.add_definition(definition_id, DefinedType::Function(FunctionType{
                            return_type: definition.return_type.clone(),
                            arguments: parameters,
                        }));
                    },
                }

                // If here, then we layed out the current type definition under
                // investigation, so:
                debug_assert!(!breadcrumbs.is_empty());
                breadcrumbs.pop();
            }

            // Go to next definition
            definition_index += 1;
        }

        debug_assert_eq!(
            num_definitions, table.lookup.len(),
            "expected {} (reserved) definitions in table, got {}",
            num_definitions, table.lookup.len()
        );

        Ok(table)
    }

    pub(crate) fn get_definition(&self, definition_id: &DefinitionId) -> Option<&DefinedType> {
        self.lookup.get(definition_id)
    }

    fn add_definition(&mut self, definition_id: DefinitionId, definition: DefinedType) {
        debug_assert!(!self.lookup.contains_key(&definition_id), "already added definition");
        self.lookup.insert(definition_id, definition);
    }
}

/// Attempts to lookup a type definition using a namespaced identifier. We have
/// three success cases: the first is simply that the type is a `BuiltIn` type.
/// In the two other cases both we find the definition of the type in the symbol
/// table, but in one case it is already `Resolved` in the type table, and in
/// the other case it is `Unresolved`. In this last case the type has to be
/// resolved before we're able to use it in the `TypeTable` construction
/// algorithm.
/// In the `Error` case something goes wrong with resolving the type. The error
/// message aims to be as helpful as possible to the user.
/// The caller should ensure that the `module_root` is where the `identifier`
/// lives.
fn lookup_type_definition(
    heap: &Heap, types: &TypeTable, symbols: &SymbolTable,
    module_root: RootId, type_to_resolve: &PrimitiveType
) -> LookupResult {
    if let PrimitiveType::Symbolic(type_to_resolve) = type_to_resolve {
        let identifier = &type_to_resolve.identifier;

        match symbols.resolve_namespaced_symbol(module_root, identifier) {
            None => {
                // Failed to find anything at all
                LookupResult::Error((identifier.position, String::from("Unknown type")))
            },
            Some((symbol, mut identifier_iter)) => {
                match symbol.symbol {
                    Symbol::Namespace(_root_id) => {
                        // Reference to a namespace, which is not a type. However,
                        // the error message depends on whether we have identifiers
                        // remaining or not
                        if identifier_iter.num_remaining() == 0 {
                            LookupResult::Error((
                                identifier.position,
                                String::from("Expected a type, got a module name")
                            ))
                        } else {
                            let next_identifier = identifier_iter.next().unwrap();
                            LookupResult::Error((
                                identifier.position,
                                format!("Cannot find symbol '{}' in this module", String::from_utf8_lossy(next_identifier))
                            ))
                        }
                    },
                    Symbol::Definition((definition_root_id, definition_id)) => {
                        // Got a definition, but we may also have more identifier's
                        // remaining in the identifier iterator
                        let definition = &heap[definition_id];
                        if identifier_iter.num_remaining() == 0 {
                            // See if the type is resolved, and make sure it is
                            // a non-function, non-component type. Ofcourse
                            // these are valid types, but we cannot (yet?) use
                            // them as function arguments, struct fields or enum
                            // variants
                            match definition {
                                Definition::Component(definition) => {
                                    return LookupResult::Error((
                                        identifier.position,
                                        format!(
                                            "Cannot use the component '{}' as an embedded type",
                                            String::from_utf8_lossy(&definition.identifier.value)
                                        )
                                    ));
                                },
                                Definition::Function(definition) => {
                                    return LookupResult::Error((
                                        identifier.position,
                                        format!(
                                            "Cannot use the function '{}' as an embedded type",
                                            String::from_utf8_lossy(&definition.identifier.value)
                                        )
                                    ));
                                },
                                Definition::Enum(_) | Definition::Struct(_) => {}
                            }

                            return if types.lookup.contains_key(&definition_id) {
                                LookupResult::Resolved((definition_root_id, definition_id))
                            } else {
                                LookupResult::Unresolved((definition_root_id, definition_id))
                            }
                        } else if identifier_iter.num_remaining() == 1 {
                            // This is always invalid, but if the type is an
                            // enumeration or a union then we want to return a
                            // different error message.
                            if definition.is_enum() {
                                let last_identifier = identifier_iter.next().unwrap();
                                return LookupResult::Error((
                                    identifier.position,
                                    format!(
                                        "Expected a type, but got a (possible) enum variant '{}'. Only the enum '{}' itself can be used as a type",
                                        String::from_utf8_lossy(last_identifier),
                                        String::from_utf8_lossy(&definition.identifier().value)
                                    )
                                ));
                            }
                        }

                        // Too much identifiers (>1) for an enumeration, or not an
                        // enumeration and we had more identifiers remaining
                        LookupResult::Error((
                            identifier.position,
                            format!(
                                "Unknown type '{}', did you mean to use '{}'?",
                                String::from_utf8_lossy(&identifier.value),
                                String::from_utf8_lossy(&definition.identifier().value)
                            )
                        ))
                    }
                }
            }
        }
    } else {
        LookupResult::BuiltIn
    }
}

/// Debugging function to ensure a type is resolved (calling
/// `lookup_type_definition` and ensuring its return value is `BuiltIn` or
/// `Resolved`
#[cfg(debug_assertions)]
fn ensure_type_definition(
    heap: &Heap, types: &TypeTable, symbols: &SymbolTable,
    module_root: RootId, type_to_resolve: &PrimitiveType
) {
    match lookup_type_definition(heap, types, symbols, module_root, type_to_resolve) {
        LookupResult::BuiltIn | LookupResult::Resolved(_) => {},
        LookupResult::Unresolved((_, definition_id)) => {
            assert!(
                false,
                "Expected that type definition for {} was resolved by the type table, but it wasn't",
                String::from_utf8_lossy(&heap[definition_id].identifier().value)
            )
        },
        LookupResult::Error((_, error)) => {
            let message = if let PrimitiveType::Symbolic(symbolic) = type_to_resolve {
                format!(
                    "Expected that type definition for {} was resolved by the type table, but it returned: {}",
                    String::from_utf8_lossy(&symbolic.identifier.value), &error
                )
            } else {
                format!(
                    "Expected (non-symbolic!?) type definition to be resolved, but it returned: {}",
                    &error
                )
            };
            assert!(false, "{}", message)
        }
    }
}

/// Determines an enumeration's integer representation type, or a union's tag
/// type, using the maximum value of the tag. The returned type is always a
/// builtin type.
/// TODO: Fix for maximum u64 value
fn enum_representation(max_tag_value: i64) -> PrimitiveType {
    if max_tag_value <= u8::max_value() as i64 {
        PrimitiveType::Byte
    } else if max_tag_value <= u16::max_value() as i64 {
        PrimitiveType::Short
    } else if max_tag_value <= u32::max_value() as i64 {
        PrimitiveType::Int
    } else {
        PrimitiveType::Long
    }
}
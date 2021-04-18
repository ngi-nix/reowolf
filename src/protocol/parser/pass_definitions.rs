use crate::protocol::ast::*;
use super::symbol_table2::*;
use super::{Module, ModuleCompilationPhase, PassCtx};
use super::tokens::*;
use super::token_parsing::*;
use crate::protocol::input_source2::{InputSource2 as InputSource, InputSpan, ParseError};
use crate::collections::*;

/// Parses all the tokenized definitions into actual AST nodes.
pub(crate) struct PassDefinitions {
    identifiers: Vec<Identifier>,
    struct_fields: Vec<StructFieldDefinition>,
}

impl PassDefinitions {
    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut PassCtx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert_eq!(module.phase, ModuleCompilationPhase::ImportsResolved);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);

        let mut range_idx = module_range.first_child_idx;
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];

            if cur_range.range_kind == TokenRangeKind::Definition {
                self.visit_definition_range(modules, module_idx, ctx, range_idx_usize)?;
            }

            match cur_range.next_sibling_idx {
                Some(idx) => { range_idx = idx; },
                None => { break; },
            }
        }



        Ok(())
    }

    fn visit_definition_range(
        &mut self, modules: &[Module], module_idx: usize, ctx: &mut PassCtx, range_idx: usize
    ) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let cur_range = &module.tokens.ranges[range_idx];
        debug_assert_eq!(cur_range.range_kind, TokenRangeKind::Definition);

        // Detect which definition we're parsing
        let mut iter = module.tokens.iter_range(cur_range);
        let keyword = peek_ident(&module.source, &mut iter).unwrap();
        match keyword {
            KW_STRUCT => {

            },
            KW_ENUM => {

            },
            KW_UNION => {

            },
            KW_FUNCTION => {

            },
            KW_PRIMITIVE => {

            },
            KW_COMPOSITE => {

            },
            _ => unreachable!("encountered keyword '{}' in definition range", String::from_utf8_lossy(keyword)),
        };

        Ok(())
    }

    fn visit_struct_definition(
        &mut self, module: &Module, iter: &mut TokenIter, ctx: &mut PassCtx
    ) -> Result<(), ParseError> {
        // Consume struct and name of struct
        let struct_span = consume_exact_ident(&module.source, iter, b"struct");
        let (ident_text, _) = consume_ident(&module.source, iter)?;

        // We should have preallocated the definition in the heap, retrieve its identifier
        let definition_id = ctx.symbols.get_symbol_by_name_defined_in_scope(SymbolScope::Module(module.root_id), ident_text)
            .unwrap().variant.as_definition().definition_id;

        consume_polymorphic_vars(source, iter, ctx, &mut self.identifiers)?;
        debug_assert!(self.struct_fields.is_empty());
        consume_comma_separated(
            TokenKind::OpenCurly, TokenKind::CloseCurly, source, iter,
            |source, iter| {
                let field = consume_ident_interned(source, iter, ctx)?;

                StructFieldDefinition{ field, parser_type }
            },
            &mut self.struct_fields, "a struct field", "a list of struct fields"
        )
    }
}

enum TypeKind {
    Message,
    Bool,
    UInt8, UInt16, UInt32, UInt64,
    SInt8, SInt16, SInt32, SInt64,
    Character, String,
    Inferred,
    Array,
    Input,
    Output,
    SymbolicDefinition(DefinitionId),
    SymbolicPolyArg(DefinitionId, usize),
}

/// Consumes a type. A type always starts with an identifier which may indicate
/// a builtin type or a user-defined type. The fact that it may contain
/// polymorphic arguments makes it a tree-like structure. Because we cannot rely
/// on knowing the exact number of polymorphic arguments we do not check for
/// these.
fn consume_parser_type(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx, poly_vars: &[Identifier]
) -> Result<(), ParseError> {
    struct StackEntry {
        angle_depth: i32,
    }
    let mut type_stack = Vec::new();

    Ok(())
}

fn consume_parser_type_ident(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx,
    mut scope: SymbolScope, wrapping_definition: DefinitionId, poly_vars: &[Identifier]
) -> Result<(TypeKind, InputSpan), ParseError> {
    let (type_text, type_span) = consume_any_ident(source, iter)?;

    let type_kind = match type_text {
        KW_TYPE_MESSAGE => TypeKind::Message,
        KW_TYPE_BOOL => TypeKind::Bool,
        KW_TYPE_UINT8 => TypeKind::UInt8,
        KW_TYPE_UINT16 => TypeKind::UInt16,
        KW_TYPE_UINT32 => TypeKind::UInt32,
        KW_TYPE_UINT64 => TypeKind::UInt64,
        KW_TYPE_SINT8 => TypeKind::SInt8,
        KW_TYPE_SINT16 => TypeKind::SInt16,
        KW_TYPE_SINT32 => TypeKind::SInt32,
        KW_TYPE_SINT64 => TypeKind::SInt64,
        KW_TYPE_IN_PORT => TypeKind::Input,
        KW_TYPE_OUT_PORT => TypeKind::Output,
        _ => {
            // Must be some kind of symbolic type
            let mut type_kind = None;
            for (poly_idx, poly_var) in poly_vars.iter().enumerate() {
                if poly_var.value.as_bytes() == type_text {
                    type_kind = Some(TypeKind::SymbolicPolyArg(wrapping_definition, poly_idx));
                }
            }

            if type_kind.is_none() {
                // Check symbol table for definition
                let last_symbol = ctx.symbols.get_symbol_by_name(scope, type_text);
                if last_symbol.is_none() {
                    return Err(ParseError::new_error_str_at_span(source, type_span, "unknown type"));
                }
                let last_symbol = last_symbol.unwrap();
                match last_symbol.variant {
                    SymbolVariant::Module(symbol_module) => {
                        // Keep seeking
                    },
                    SymbolVariant::Definition(symbol_definition) => {

                    }
                }
            }
        }
    }


    Ok(())
}

/// Consumes polymorphic variables (i.e. a list of identifiers). If the list is
/// absent then we simply return an empty array.
fn consume_polymorphic_vars(
    source: &InputSource, iter: &mut TokenIter, ctx: &mut PassCtx, target: &mut Vec<Identifier>
) -> Result<(), ParseError> {
    // Note: because this is just a list of identifiers, we don't have to take
    // two `TokenKind::CloseAngle` interpreted as `TokenKind::ShiftRight` into
    // account.
    debug_assert!(target.is_empty());
    maybe_consume_comma_separated(
        TokenKind::OpenAngle, TokenKind::CloseAngle, source, iter,
        |source, iter| consume_ident_interned(source, iter, ctx),
        target, "a polymorphic variable"
    )?;

    Ok(())
}
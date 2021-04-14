use crate::protocol::ast::*;
use crate::protocol::Heap;
use crate::collections::{StringPool, StringRef};
use crate::protocol::tokenizer::*;
use crate::protocol::input_source2::{InputSource2 as InputSource, InputPosition2 as InputPosition, InputSpan, ParseError};
use crate::protocol::symbol_table2::*;

#[derive(PartialEq, Eq)]
enum ModuleCompilationPhase {
    Source,                 // only source is set
    Tokenized,              // source is tokenized
    DefinitionsScanned,     // all definitions are linked to their type class
    ImportsResolved,        // all imports are added to the symbol table
    Parsed,                 // produced the AST for the module
    ValidatedAndLinked,     // AST is traversed and has linked the required AST nodes
    Typed,                  // Type inference and checking has been performed
}

enum KeywordDefinition {
    Struct,
    Enum,
    Union,
    Function,
    Primitive,
    Composite,
}

impl KeywordDefinition {
    fn as_symbol_class(&self) -> SymbolClass {
        use KeywordDefinition as KD;
        use SymbolClass as SC;

        match self {
            KD::Struct => SC::Struct,
            KD::Enum => SC::Enum,
            KD::Union => SC::Union,
            KD::Function => SC::Function,
            KD::Primitive | KD::Composite => SC::Component,
        }
    }
}

struct Module {
    // Buffers
    source: InputSource,
    tokens: TokenBuffer,
    // Identifiers
    root_id: RootId,
    name: Option<(PragmaId, StringRef)>,
    version: Option<(PragmaId, i64)>,
    phase: ModuleCompilationPhase,
}

struct Ctx<'a> {
    heap: &'a mut Heap,
    symbols: &'a mut SymbolTable,
    pool: &'a mut StringPool,
}

/// Scans the module and finds all module-level type definitions. These will be
/// added to the symbol table such that during AST-construction we know which
/// identifiers point to types. Will also parse all pragmas to determine module
/// names.
pub(crate) struct ASTSymbolPrePass {
    symbols: Vec<Symbol>,
    pragmas: Vec<PragmaId>,
    buffer: String,
    has_pragma_version: bool,
    has_pragma_module: bool,
}

impl ASTSymbolPrePass {
    pub(crate) fn new() -> Self {
        Self{
            symbols: Vec::with_capacity(128),
            pragmas: Vec::with_capacity(8),
            buffer: String::with_capacity(128),
            has_pragma_version: false,
            has_pragma_module: false,
        }
    }

    fn reset(&mut self) {
        self.symbols.clear();
        self.pragmas.clear();
        self.has_pragma_version = false;
        self.has_pragma_module = false;
    }

    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx) -> Result<(), ParseError> {
        self.reset();

        let module = &mut modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        let expected_parent_idx = 0;
        let expected_subranges = module_range.subranges;
        debug_assert_eq!(module.phase, ModuleCompilationPhase::Tokenized);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);
        debug_assert_eq!(module.root_id.index, 0);

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

        // Visit token ranges to detect defintions
        let mut visited_subranges = 0;
        for range_idx in expected_parent_idx + 1..module.tokens.ranges.len() {
            // Skip any ranges that do not belong to the module
            let cur_range = &module.tokens.ranges[range_idx];
            if cur_range.parent_idx != expected_parent_idx {
                continue;
            }

            // Parse if it is a definition or a pragma
            if cur_range.range_kind == TokenRangeKind::Definition {
                self.visit_definition_range(modules, module_idx, ctx, range_idx)?;
            } else if cur_range.range_kind == TokenRangeKind::Pragma {
                self.visit_pragma_range(modules, module_idx, ctx, range_idx)?;
            }

            visited_subranges += 1;
            if visited_subranges == expected_subranges {
                break;
            }
        }

        // By now all symbols should have been found: add to symbol table and
        // add the parsed pragmas to the preallocated root in the heap.
        debug_assert_eq!(visited_subranges, expected_subranges);
        ctx.symbols.insert_scoped_symbols(None, SymbolScope::Module(module.root_id), &self.symbols);

        let root = &mut ctx.heap[root_id];
        debug_assert!(root.pragmas.is_empty());
        root.pragmas.extend(&self.pragmas);

        module.phase = ModuleCompilationPhase::DefinitionsScanned;

        Ok(())
    }

    fn visit_pragma_range(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx, range_idx: usize) -> Result<(), ParseError> {
        let module = &mut modules[module_idx];
        let range = &module.tokens.ranges[range_idx];
        let mut iter = module.tokens.iter_range(range);

        // Consume pragma name
        let (pragma_section, pragma_start, _) = consume_pragma(&self.source, &mut iter)?;

        // Consume pragma values
        if pragma_section == "#module" {
            // Check if name is defined twice within the same file
            if self.has_pragma_module {
                return Err(ParseError::new_error(&module.source, pragma_start, "module name is defined twice"));
            }

            // Consume the domain-name
            let (module_name, module_span) = consume_domain_ident(&module.source, &mut iter)?;
            if iter.next().is_some() {
                return Err(ParseError::new_error(&module.source, iter.last_valid_pos(), "expected end of #module pragma after module name"));
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
                let (other_module_pragma_id, _) = other_module.name.unwrap();
                let other_pragma = ctx.heap[other_module_pragma_id].as_module();
                return Err(ParseError::new_error_str_at_span(
                    &this_module.source, pragma_span, "conflict in module name"
                ).with_info_str_at_span(
                    &other_module.source, other_pragma.span, "other module is defined here"
                ));
            }
            self.has_pragma_module = true;
        } else if pragma_section == "#version" {
            // Check if version is defined twice within the same file
            if self.has_pragma_version {
                return Err(ParseError::new_error(&module.source, pragma_start, "module version is defined twice"));
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
            return Err(ParseError::new_error(&module.source, pragma_start, "illegal pragma name"));
        }

        Ok(())
    }

    fn visit_definition_range(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx, range_idx: usize) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let range = &module.tokens.ranges[range_idx];
        let definition_span = InputSpan::from_positions(
            module.tokens.start_pos(range),
            module.tokens.end_pos(range)
        );
        let mut iter = module.tokens.iter_range(range);

        // Because we're visiting a definition, we expect an ident that resolves
        // to a keyword indicating a definition.
        let kw_text = consume_ident_text(&module.source, &mut iter).unwrap();
        let kw = parse_definition_keyword(kw_text).unwrap();

        // Retrieve identifier and put in temp symbol table
        let definition_ident = consume_ident_text(&module.source, &mut iter)?;
        let definition_ident = ctx.pool.intern(definition_ident);
        let symbol_class = kw.as_symbol_class();

        // Get the token indicating the end of the definition to get the full
        // span of the definition
        let last_token = &module.tokens.tokens[range.end - 1];
        debug_assert_eq!(last_token.kind, TokenKind::CloseCurly);

        self.symbols.push(Symbol::new(
            module.root_id,
            SymbolScope::Module(module.root_id),
            definition_span,
            symbol_class,
            definition_ident
        ));

        Ok(())
    }
}

pub(crate) struct ASTImportPrePass {
}

impl ASTImportPrePass {
    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert_eq!(module.phase, ModuleCompilationPhase::DefinitionsScanned);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);

        let expected_parent_idx = 0;
        let expected_subranges = module_range.subranges;
        let mut visited_subranges = 0;

        for range_idx in expected_parent_idx + 1..module.tokens.ranges.len() {
            let cur_range = &module.tokens.ranges[range_idx];
            if cur_range.parent_idx != expected_parent_idx {
                continue;
            }

            visited_subranges += 1;
            if cur_range.range_kind == TokenRangeKind::Import {
                self.visit_import_range(modules, module_idx, ctx, range_idx)?;
            }

            if visited_subranges == expected_subranges {
                break;
            }
        }

        Ok(())
    }

    pub(crate) fn visit_import_range(
        &mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx, range_idx: usize
    ) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let import_range = &module.tokens.ranges[range_idx];
        debug_assert_eq!(import_range.range_kind, TokenRangeKind::Import);

        let mut iter = module.tokens.iter_range(import_range);

        // Consume "import"
        let _import_ident = consume_ident_text(&module.source, &mut iter)?;
        debug_assert_eq!(_import_ident, KW_IMPORT);

        // Consume module name
        let (module_name, _) = consume_domain_ident(&module.source, &mut iter)?;


        Ok(())
    }
}

fn consume_domain_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputSpan), ParseError> {
    let (_, name_start, mut name_end) = consume_ident(source, iter)?;
    while let Some(TokenKind::Dot) = iter.next() {
        consume_dot(source, iter)?;
        let (_, _, new_end) = consume_ident(source, iter)?;
        name_end = new_end;
    }

    Ok((source.section(name_start, name_end), InputSpan::from_positions(name_start, name_end)))
}

fn consume_dot<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(), ParseError> {
    if Some(TokenKind::Dot) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected a dot"));
    }
    iter.consume();
    Ok(())
}

fn consume_integer_literal(source: &InputSource, iter: &mut TokenIter, buffer: &mut String) -> Result<(u64, InputSpan), ParseError> {
    if Some(TokenKind::Integer) != iter.next() {
        return Err(ParseError::new_error_str_at_pos(source, iter.last_valid_pos(), "expected an integer literal"));
    }
    let (start_pos, end_pos) = iter.next_range();
    iter.consume();

    let integer_text = source.section(start_pos, end_pos);

    // Determine radix and offset from prefix
    let (radix, input_offset, radix_name) =
        if integer_text.starts_with(b"0b") || integer_text.starts_with(b"0B") {
            // Binary number
            (2, 2, "binary")
        } else if integer_text.starts_with(b"0o") || integer_text.starts_with(b"0O") {
            // Octal number
            (8, 2, "octal")
        } else if integer_text.starts_with(b"0x") || integer_text.starts_with(b"0X") {
            // Hexadecimal number
            (16, 2, "hexadecimal")
        } else {
            (10, 0, "decimal")
        };

    // Take out any of the separating '_' characters
    buffer.clear();
    for char_idx in input_offset..integer_text.len() {
        let char = integer_text[char_idx];
        if char == b'_' {
            continue;
        }
        if !char.is_ascii_digit() {
            return Err(ParseError::new_error(source, start_pos, "incorrectly formatted integer"));
        }
        buffer.push(char::from(char));
    }

    // Use the cleaned up string to convert to integer
    match u64::from_str_radix(&buffer, radix) {
        Ok(number) => Ok((number, InputSpan::from_positions(start_pos, end_pos))),
        Err(_) => Err(
            ParseError::new_error(source, start_pos, "incorrectly formatted integer")
        ),
    }
}

fn seek_module(modules: &[Module], root_id: RootId) -> Option<&Module> {
    for module in modules {
        if module.root_id == root_id {
            return Some(module)
        }
    }

    return None
}

fn consume_pragma<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputPosition, InputPosition), ParseError> {
    if Some(TokenKind::Pragma) != iter.next() {
        return Err(ParseError::new_error(source, iter.last_valid_pos(), "expected a pragma"));
    }
    let (pragma_start, pragma_end) = iter.next_range();
    iter.consume();
    Ok((source.section(pragma_start, pragma_end), pragma_start, pragma_end))
}

fn consume_ident_text<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<&'a [u8], ParseError> {
    if Some(TokenKind::Ident) != iter.next() {
        return Err(ParseError::new_error(source, iter.last_valid_pos(), "expected an identifier"));
    }
    let (ident_start, ident_end) = iter.next_range();
    iter.consume();
    Ok(source.section(ident_start, ident_end))
}

fn consume_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputPosition, InputPosition), ParseError> {
    if Some(TokenKind::Ident) != iter.next() {
        return Err(ParseError::new_error(sourcee, iter.last_valid_pos(), "expected an identifier"));
    }
    let (ident_start, ident_end) = iter.next_range();
    iter.consume();
    Ok((source.section(ident_start, ident_end), ident_start, ident_end))
}

fn parse_definition_keyword(keyword: &[u8]) -> Option<KeywordDefinition> {
    match keyword {
        KW_STRUCT =>    Some(Keyword::Struct),
        KW_ENUM =>      Some(Keyword::Enum),
        KW_UNION =>     Some(Keyword::Union),
        KW_FUNCTION =>  Some(Keyword::Function),
        KW_PRIMITIVE => Some(Keyword::Primitive),
        KW_COMPOSITE => Some(Keyword::Composite),
        _ => None
    }
}
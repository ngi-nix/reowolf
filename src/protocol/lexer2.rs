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

struct Module {
    // Buffers
    source: InputSource,
    tokens: TokenBuffer,
    // Identifiers
    root_id: RootId,
    name: Option<(PragmaId, StringRef<'static>)>,
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
pub(crate) struct PassPreSymbol {
    symbols: Vec<Symbol>,
    pragmas: Vec<PragmaId>,
    imports: Vec<ImportId>,
    definitions: Vec<DefinitionId>,
    buffer: String,
    has_pragma_version: bool,
    has_pragma_module: bool,
}

impl PassPreSymbol {
    pub(crate) fn new() -> Self {
        Self{
            symbols: Vec::with_capacity(128),
            pragmas: Vec::with_capacity(8),
            imports: Vec::with_capacity(32),
            definitions: Vec::with_capacity(128),
            buffer: String::with_capacity(128),
            has_pragma_version: false,
            has_pragma_module: false,
        }
    }

    fn reset(&mut self) {
        self.symbols.clear();
        self.pragmas.clear();
        self.imports.clear();
        self.definitions.clear();
        self.has_pragma_version = false;
        self.has_pragma_module = false;
    }

    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx) -> Result<(), ParseError> {
        self.reset();

        let module = &mut modules[module_idx];
        let module_range = &module.tokens.ranges[0];

        debug_assert_eq!(module.phase, ModuleCompilationPhase::Tokenized);
        debug_assert_eq!(module_range.range_kind, TokenRangeKind::Module);
        debug_assert!(module.root_id.is_invalid()); // not set yet,

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

        // Visit token ranges to detect definitions and pragmas
        let mut range_idx = module_range.first_child_idx;
        loop {
            let range_idx_usize = range_idx as usize;
            let cur_range = &module.tokens.ranges[range_idx_usize];

            // Parse if it is a definition or a pragma
            if cur_range.range_kind == TokenRangeKind::Definition {
                self.visit_definition_range(modules, module_idx, ctx, range_idx_usize)?;
            } else if cur_range.range_kind == TokenRangeKind::Pragma {
                self.visit_pragma_range(modules, module_idx, ctx, range_idx_usize)?;
            }

            match cur_range.next_sibling_idx {
                Some(idx) => { range_idx = idx; },
                None => { break; },
            }
        }

        // Add the module's symbol scope and the symbols we just parsed
        let module_scope = SymbolScope::Module(root_id);
        ctx.symbols.insert_scope(None, module_scope);
        for symbol in self.symbols.drain(..) {
            if let Err((new_symbol, old_symbol)) = ctx.symbols.insert_symbol(module_scope, symbol) {
                return Err(construct_symbol_conflict_error(modules, module_idx, ctx, &new_symbol, old_symbol))
            }
        }

        // Modify the preallocated root
        let root = &mut ctx.heap[root_id];
        root.pragmas.extend(self.pragmas.drain(..));
        root.definitions.extend(self.definitions.drain(..));
        module.phase = ModuleCompilationPhase::DefinitionsScanned;

        Ok(())
    }

    fn visit_pragma_range(&mut self, modules: &[Module], module_idx: usize, ctx: &mut Ctx, range_idx: usize) -> Result<(), ParseError> {
        let module = &modules[module_idx];
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

    fn visit_definition_range(&mut self, modules: &[Module], module_idx: usize, ctx: &mut Ctx, range_idx: usize) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let range = &module.tokens.ranges[range_idx];
        let definition_span = InputSpan::from_positions(
            module.tokens.start_pos(range),
            module.tokens.end_pos(range)
        );
        let mut iter = module.tokens.iter_range(range);

        // First ident must be type of symbol
        let (kw_text, _) = consume_any_ident(&module.source, &mut iter).unwrap();
        let kw = parse_definition_keyword(kw_text).unwrap();

        // Retrieve identifier of definition
        let (identifier_text, identifier_span) = consume_ident(&module.source, &mut iter)?;
        let ident_text = ctx.pool.intern(identifier_text);
        let identifier = Identifier{ span: identifier_span, value: ident_text };

        // Reserve space in AST for definition and add it to the symbol table
        let symbol_definition;
        let ast_definition_id;
        match kw {
            KeywordDefinition::Struct => {
                let struct_def_id = ctx.heap.alloc_struct_definition(|this| {
                    StructDefinition::new_empty(this, definition_span, identifier)
                });
                symbol_definition = SymbolDefinition::Struct(struct_def_id);
                ast_definition_id = struct_def_id.upcast();
            },
            KeywordDefinition::Enum => {
                let enum_def_id = ctx.heap.alloc_enum_definition(|this| {
                    EnumDefinition::new_empty(this, definition_span, identifier)
                });
                symbol_definition = SymbolDefinition::Enum(enum_def_id);
                ast_definition_id = enum_def_id.upcast();
            },
            KeywordDefinition::Union => {
                let union_def_id = ctx.heap.alloc_union_definition(|this| {
                    UnionDefinition::new_empty(this, definition_span, identifier)
                });
                symbol_definition = SymbolDefinition::Union(union_def_id);
                ast_definition_id = union_def_id.upcast()
            },
            KeywordDefinition::Function => {
                let func_def_id = ctx.heap.alloc_function_definition(|this| {
                    FunctionDefinition::new_empty(this, definition_span, identifier)
                });
                symbol_definition = SymbolDefinition::Function(func_def_id);
                ast_definition_id = func_def_id.upcast();
            },
            KeywordDefinition::Primitive | KeywordDefinition::Composite => {
                let component_variant = if kw == KeywordDefinition::Primitive {
                    ComponentVariant::Primitive
                } else {
                    ComponentVariant::Composite
                };
                let comp_def_id = ctx.heap.alloc_component_definition(|this| {
                    ComponentDefinition::new_empty(this, definition_span, component_variant, identifier)
                });
                symbol_definition = SymbolDefinition::Component(comp_def_id);
                ast_definition_id = comp_def_id.upcast();
            }
        }

        let symbol = Symbol{
            defined_in_module: module.root_id,
            defined_in_scope: SymbolScope::Module(module.root_id),
            definition_span,
            identifier_span,
            introduced_at: None,
            name: definition_ident,
            definition: symbol_definition
        };
        self.symbols.push(symbol);
        self.definitions.push(ast_definition_id);

        Ok(())
    }
}

/// Parses all the imports in the module tokens. Is applied after the
/// definitions and name of modules are resolved. Hence we should be able to
/// resolve all symbols to their appropriate module/definition.
pub(crate) struct PassImport {
    imports: Vec<ImportId>,
}

impl PassImport {
    pub(crate) fn new() -> Self {
        Self{ imports: Vec::with_capacity(32) }
    }
    pub(crate) fn parse(&mut self, modules: &mut [Module], module_idx: usize, ctx: &mut Ctx) -> Result<(), ParseError> {
        let module = &modules[module_idx];
        let module_range = &module.tokens.ranges[0];
        debug_assert!(modules.iter().all(|m| m.phase >= ModuleCompilationPhase::DefinitionsScanned));
        debug_assert_eq!(module.phase, ModuleCompilationPhase::DefinitionsScanned);
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
        let (_import_ident, import_span) =
            consume_ident(&module.source, &mut iter)?;
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
        let target_root_id = target_root_id.unwrap();

        // Check for subsequent characters
        let next = iter.next();
        if has_ident(&module.source, &mut iter, b"as") {
            iter.consume();
            let (alias_text, alias_span) = consume_ident(source, &mut iter)?;
            let alias = ctx.pool.intern(alias_text);

            let import_id = ctx.heap.alloc_import(|this| Import::Module(ImportModule{
                this,
                span: import_span,
                module_name: Identifier{ span: module_name_span, value: module_name },
                alias: Identifier{ span: alias_span, value: alias },
                module_id: target_root_id
            }));
            ctx.symbols.insert_symbol(SymbolScope::Module(module.root_id), Symbol{
                defined_in_module: target_root_id,
                defined_in_scope: SymbolScope::Module(target_root_id),
                definition_span
            })
        } else if Some(TokenKind::ColonColon) == next {
            iter.consume();
        } else {
            // Assume implicit alias, then check if we get the semicolon next
            let module_name_str = module_name.as_str();
            let last_ident_start = module_name_str.rfind('.').map_or(0, |v| v + 1);
            let alias_text = &module_name_str.as_bytes()[last_ident_start..];
            let alias = ctx.pool.intern(alias_text);
            let alias_span = InputSpan::from_positions(
                module_name_span.begin.with_offset(last_ident_start as u32),
                module_name_span.end
            );
        }

        Ok(())
    }
}

fn consume_domain_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputSpan), ParseError> {
    let (_, mut span) = consume_ident(source, iter)?;
    while let Some(TokenKind::Dot) = iter.next() {
        consume_dot(source, iter)?;
        let (_, new_span) = consume_ident(source, iter)?;
        span.end = new_span.end;
    }

    // Not strictly necessary, but probably a reasonable restriction: this
    // simplifies parsing of module naming and imports.
    if span.begin.line != span.end.line {
        return Err(ParseError::new_error_str_at_span(source, span, "module names may not span multiple lines"));
    }

    // If module name consists of a single identifier, then it may not match any
    // of the reserved keywords
    let section = source.section(span.begin, span.end);
    if is_reserved_keyword(section) {
        return Err(ParseError::new_error_str_at_span(source, span, "encountered reserved keyword"));
    }

    Ok((source.section(span.begin, span.end), span))
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

fn has_ident(source: &InputSource, iter: &mut TokenIter, expected: &[u8]) -> bool {
    if Some(TokenKind::Ident) == iter.next() {
        let (start, end) = iter.next_range();
        return source.section(start, end) == expected;
    }

    false
}

fn peek_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Option<&'a [u8]> {
    if Some(TokenKind::Ident) == iter.next() {
        let (start, end) = iter.next_range();
        return Some(source.section(start, end))
    }

    None
}

/// Consumes any identifier and returns it together with its span. Does not
/// check if the identifier is a reserved keyword.
fn consume_any_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputSpan), ParseError> {
    if Some(TokenKind::Ident) != iter.next() {
        return Err(ParseError::new_error(sourcee, iter.last_valid_pos(), "expected an identifier"));
    }
    let (ident_start, ident_end) = iter.next_range();
    iter.consume();
    Ok((source.section(ident_start, ident_end), InputSpan::from_positions(ident_start, ident_end)))
}

/// Consumes an identifier that is not a reserved keyword and returns it
/// together with its span.
fn consume_ident<'a>(source: &'a InputSource, iter: &mut TokenIter) -> Result<(&'a [u8], InputSpan), ParseError> {
    let (ident, span) = consume_any_ident(source, iter)?;
    if is_reserved_keyword(ident) {
        return Err(ParseError::new_error_str_at_span(source, span, "encountered reserved keyword"));
    }

    Ok((ident, span))
}

fn is_reserved_definition_keyword(text: &[u8]) -> bool {
    return ([
        b"struct", b"enum", b"union", b"function", b"primitive"
    ] as &[[u8]]).contains(text)
}

fn is_reserved_statement_keyword(text: &[u8]) -> bool {
    return ([
        b"channel", b"import", b"as",
        b"if", b"while", b"break", b"continue", b"goto", b"return",
        b"synchronous", b"assert", b"new",
    ] as &[[u8]]).contains(text)
}

fn is_reserved_expression_keyword(text: &[u8]) -> bool {
    return ([
        b"let", b"true", b"false", b"null", // literals
        b"get", b"put", b"fires", b"create", b"length", // functions
    ] as &[[u8]]).contains(text)
}

fn is_reserved_type_keyword(text: &[u8]) -> bool {
    return ([
        b"in", b"out", b"msg",
        b"bool",
        b"u8", b"u16", b"u32", b"u64",
        b"s8", b"s16", b"s32", b"s64",
        b"auto"
    ] as &[[u8]]).contains(text)
}

fn is_reserved_keyword(text: &[u8]) -> bool {
    return
        is_reserved_definition_keyword(text) ||
        is_reserved_statement_keyword(text) ||
        is_reserved_type_keyword(text);
}

/// Constructs a human-readable message indicating why there is a conflict of
/// symbols.
// Note: passing the `module_idx` is not strictly necessary, but will prevent
// programmer mistakes during development: we get a conflict because we're
// currently parsing a particular module.
fn construct_symbol_conflict_error(modules: &[Module], module_idx: usize, ctx: &Ctx, new_symbol: &Symbol, old_symbol: &Symbol) -> ParseError {
    let module = &modules[module_idx];
    let get_symbol_span_and_msg = |symbol: &Symbol| -> (String, InputSpan) {
        match symbol.introduced_at {
            Some(import_id) => {
                // Symbol is being imported
                let import = &ctx.heap[import_id];
                match import {
                    Import::Module(import) => (
                        format!("the module aliased as '{}' imported here", symbol.name.as_str()),
                        import.span
                    ),
                    Import::Symbols(symbols) => (
                        format!("the type '{}' imported here", symbol.name.as_str()),
                        symbols.span
                    ),
                }
            },
            None => {
                // Symbol is being defined
                debug_assert_eq!(symbol.defined_in_module, module.root_id);
                debug_assert_ne!(symbol.definition.symbol_class(), SymbolClass::Module);
                (
                    format!("the type '{}' defined here", symbol.name.as_str()),
                    symbol.identifier_span
                )
            }
        }
    };

    let (new_symbol_msg, new_symbol_span) = get_symbol_span_and_msg(new_symbol);
    let (old_symbol_msg, old_symbol_span) = get_symbol_span_and_msg(old_symbol);
    return ParseError::new_error_at_span(
        &module.source, new_symbol_span, format!("symbol is defined twice: {}", new_symbol_msg)
    ).with_info_at_span(
        &module.source, old_symbol_span, format!("it conflicts with {}", old_symbol_msg)
    )
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
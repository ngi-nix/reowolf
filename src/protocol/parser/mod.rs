mod depth_visitor;
mod symbol_table;
mod type_table;
mod type_resolver;
mod visitor;

use depth_visitor::*;
use symbol_table::SymbolTable;
use visitor::{Visitor2, ValidityAndLinkerVisitor};
use type_table::TypeTable;

use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use crate::protocol::lexer::*;

use std::collections::HashMap;
use crate::protocol::parser::visitor::Ctx;
use crate::protocol::ast_printer::ASTWriter;

// TODO: @fixme, pub qualifier
pub(crate) struct LexedModule {
    pub(crate) source: InputSource,
    module_name: Vec<u8>,
    version: Option<u64>,
    root_id: RootId,
}

pub struct Parser {
    pub(crate) heap: Heap,
    pub(crate) modules: Vec<LexedModule>,
    pub(crate) module_lookup: HashMap<Vec<u8>, usize>, // from (optional) module name to `modules` idx
}

impl Parser {
    pub fn new() -> Self {
        Parser{
            heap: Heap::new(),
            modules: Vec::new(),
            module_lookup: HashMap::new()
        }
    }

    // TODO: @fix, temporary implementation to keep code compilable
    pub fn new_with_source(source: InputSource) -> Result<Self, ParseError2> {
        let mut parser = Parser::new();
        parser.feed(source)?;
        Ok(parser)
    }

    pub fn feed(&mut self, mut source: InputSource) -> Result<RootId, ParseError2> {
        // Lex the input source
        let mut lex = Lexer::new(&mut source);
        let pd = lex.consume_protocol_description(&mut self.heap)?;

        // Seek the module name and version
        let root = &self.heap[pd];
        let mut module_name_pos = InputPosition::default();
        let mut module_name = Vec::new();
        let mut module_version_pos = InputPosition::default();
        let mut module_version = None;

        for pragma in &root.pragmas {
            match &self.heap[*pragma] {
                Pragma::Module(module) => {
                    if !module_name.is_empty() {
                        return Err(
                            ParseError2::new_error(&source, module.position, "Double definition of module name in the same file")
                                .with_postfixed_info(&source, module_name_pos, "Previous definition was here")
                        )
                    }

                    module_name_pos = module.position.clone();
                    module_name = module.value.clone();
                },
                Pragma::Version(version) => {
                    if module_version.is_some() {
                        return Err(
                            ParseError2::new_error(&source, version.position, "Double definition of module version")
                                .with_postfixed_info(&source, module_version_pos, "Previous definition was here")
                        )
                    }

                    module_version_pos = version.position.clone();
                    module_version = Some(version.version);
                },
            }
        }

        // Add module to list of modules and prevent naming conflicts
        let cur_module_idx = self.modules.len();
        if let Some(prev_module_idx) = self.module_lookup.get(&module_name) {
            // Find `#module` statement in other module again
            let prev_module = &self.modules[*prev_module_idx];
            let prev_module_pos = self.heap[prev_module.root_id].pragmas
                .iter()
                .find_map(|p| {
                    match &self.heap[*p] {
                        Pragma::Module(module) => Some(module.position.clone()),
                        _ => None
                    }
                })
                .unwrap_or(InputPosition::default());

            let module_name_msg = if module_name.is_empty() {
                format!("a nameless module")
            } else {
                format!("module '{}'", String::from_utf8_lossy(&module_name))
            };

            return Err(
                ParseError2::new_error(&source, module_name_pos, &format!("Double definition of {} across files", module_name_msg))
                    .with_postfixed_info(&prev_module.source, prev_module_pos, "Other definition was here")
            );
        }

        self.modules.push(LexedModule{
            source,
            module_name: module_name.clone(),
            version: module_version,
            root_id: pd
        });
        self.module_lookup.insert(module_name, cur_module_idx);
        Ok(pd)
    }

    pub fn compile(&mut self) {
        // Build module lookup
    }

    fn resolve_symbols_and_types(&mut self) -> Result<(SymbolTable, TypeTable), ParseError2> {
        // Construct the symbol table to resolve any imports and/or definitions,
        // then use the symbol table to actually annotate all of the imports.
        // If the type table is constructed correctly then all imports MUST be
        // resolvable.
        // TODO: Update once namespaced identifiers are implemented
        let symbol_table = SymbolTable::new(&self.heap, &self.modules)?;

        // Not pretty, but we need to work around rust's borrowing rules, it is
        // totally safe to mutate the contents of an AST element that we are
        // not borrowing anywhere else.
        // TODO: Maybe directly access heap's members to allow borrowing from
        //  mutliple members of Heap? Not pretty though...
        let mut module_index = 0;
        let mut import_index = 0;
        loop {
            if module_index >= self.modules.len() {
                break;
            }

            let module_root_id = self.modules[module_index].root_id;
            let import_id = {
                let root = &self.heap[module_root_id];
                if import_index >= root.imports.len() {
                    module_index += 1;
                    import_index = 0;
                    continue
                }
                root.imports[import_index]
            };

            let import = &mut self.heap[import_id];
            match import {
                Import::Module(import) => {
                    debug_assert!(import.module_id.is_none(), "module import already resolved");
                    let target_module_id = symbol_table.resolve_module(&import.module_name)
                        .expect("module import is resolved by symbol table");
                    import.module_id = Some(target_module_id)
                },
                Import::Symbols(import) => {
                    debug_assert!(import.module_id.is_none(), "module of symbol import already resolved");
                    let target_module_id = symbol_table.resolve_module(&import.module_name)
                        .expect("symbol import's module is resolved by symbol table");
                    import.module_id = Some(target_module_id);

                    for symbol in &mut import.symbols {
                        debug_assert!(symbol.definition_id.is_none(), "symbol import already resolved");
                        let (_, target_definition_id) = symbol_table.resolve_symbol(module_root_id, &symbol.alias)
                            .expect("symbol import is resolved by symbol table")
                            .as_definition()
                            .expect("symbol import does not resolve to namespace symbol");
                        symbol.definition_id = Some(target_definition_id);
                    }
                }
            }
        }

        // All imports in the AST are now annotated. We now use the symbol table
        // to construct the type table.
        let type_table = TypeTable::new(&symbol_table, &self.heap, &self.modules)?;

        Ok((symbol_table, type_table))
    }

    // TODO: @fix, temporary impl to keep code compilable
    pub fn parse(&mut self) -> Result<RootId, ParseError2> {
        assert_eq!(self.modules.len(), 1, "Fix meeeee");
        let root_id = self.modules[0].root_id;

        let (mut symbol_table, mut type_table) = self.resolve_symbols_and_types()?;

        // TODO: @cleanup
        let mut ctx = visitor::Ctx{
            heap: &mut self.heap,
            module: &self.modules[0],
            symbols: &mut symbol_table,
            types: &mut type_table,
        };
        let mut visit = ValidityAndLinkerVisitor::new();
        visit.visit_module(&mut ctx)?;

        if let Err((position, message)) = Self::parse_inner(&mut self.heap, root_id) {
            return Err(ParseError2::new_error(&self.modules[0].source, position, &message))
        }

        // let mut writer = ASTWriter::new();
        // let mut file = std::fs::File::create(std::path::Path::new("ast.txt")).unwrap();
        // writer.write_ast(&mut file, &self.heap);

        Ok(root_id)
    }

    pub fn parse_inner(h: &mut Heap, pd: RootId) -> VisitorResult {
        // TODO: @cleanup, slowly phasing out old compiler
        // NestedSynchronousStatements::new().visit_protocol_description(h, pd)?;
        // ChannelStatementOccurrences::new().visit_protocol_description(h, pd)?;
        // FunctionStatementReturns::new().visit_protocol_description(h, pd)?;
        // ComponentStatementReturnNew::new().visit_protocol_description(h, pd)?;
        // CheckBuiltinOccurrences::new().visit_protocol_description(h, pd)?;
        // BuildSymbolDeclarations::new().visit_protocol_description(h, pd)?;
        // LinkCallExpressions::new().visit_protocol_description(h, pd)?;
        // BuildScope::new().visit_protocol_description(h, pd)?;
        // ResolveVariables::new().visit_protocol_description(h, pd)?;
        LinkStatements::new().visit_protocol_description(h, pd)?;
        // BuildLabels::new().visit_protocol_description(h, pd)?;
        // ResolveLabels::new().visit_protocol_description(h, pd)?;
        AssignableExpressions::new().visit_protocol_description(h, pd)?;
        IndexableExpressions::new().visit_protocol_description(h, pd)?;
        SelectableExpressions::new().visit_protocol_description(h, pd)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;

    use super::*;

    // #[test]
    fn positive_tests() {
        for resource in TestFileIter::new("testdata/parser/positive", "pdl") {
            let resource = resource.expect("read testdata filepath");
            // println!(" * running: {}", &resource);
            let path = Path::new(&resource);
            let source = InputSource::from_file(&path).unwrap();
            // println!("DEBUG -- input:\n{}", String::from_utf8_lossy(&source.input));
            let mut parser = Parser::new_with_source(source).expect("parse source");
            match parser.parse() {
                Ok(_) => {}
                Err(err) => {
                    println!(" > file: {}", &resource);
                    println!("{}", err);
                    assert!(false);
                }
            }
        }
    }

    // #[test]
    fn negative_tests() {
        for resource in TestFileIter::new("testdata/parser/negative", "pdl") {
            let resource = resource.expect("read testdata filepath");
            let path = Path::new(&resource);
            let expect = path.with_extension("txt");
            let mut source = InputSource::from_file(&path).unwrap();
            let mut parser = Parser::new_with_source(source).expect("construct parser");
            match parser.parse() {
                Ok(pd) => {
                    println!("Expected parse error:");

                    let mut cev: Vec<u8> = Vec::new();
                    let mut f = File::open(expect).unwrap();
                    f.read_to_end(&mut cev).unwrap();
                    println!("{}", String::from_utf8_lossy(&cev));
                    assert!(false);
                }
                Err(err) => {
                    let expected = format!("{}", err);
                    println!("{}", &expected);

                    let mut cev: Vec<u8> = Vec::new();
                    let mut f = File::open(expect).unwrap();
                    f.read_to_end(&mut cev).unwrap();
                    println!("{}", String::from_utf8_lossy(&cev));

                    assert_eq!(expected.as_bytes(), cev);
                }
            }
        }
    }

    // #[test]
    fn counterexample_tests() {
        for resource in TestFileIter::new("testdata/parser/counterexamples", "pdl") {
            let resource = resource.expect("read testdata filepath");
            let path = Path::new(&resource);
            let source = InputSource::from_file(&path).unwrap();
            let mut parser = Parser::new_with_source(source).expect("construct parser");

            fn print_header(s: &str) {
                println!("{}", "=".repeat(80));
                println!(" > File: {}", s);
                println!("{}", "=".repeat(80));
            }

            match parser.parse() {
                Ok(parsed) => {
                    print_header(&resource);
                    println!("\n  SUCCESS\n\n --- source:\n{}", String::from_utf8_lossy(&parser.modules[0].source.input));
                },
                Err(err) => {
                    print_header(&resource);
                    println!(
                        "\n  FAILURE\n\n --- error:\n{}\n --- source:\n{}",
                        err,
                        String::from_utf8_lossy(&parser.modules[0].source.input)
                    )
                }
            }
        }
    }

    struct TestFileIter {
        iter: std::fs::ReadDir,
        root: String,
        extension: String
    }

    impl TestFileIter {
        fn new(root_dir: &str, extension: &str) -> Self {
            let path = Path::new(root_dir);
            assert!(path.is_dir(), "root '{}' is not a directory", root_dir);

            let iter = std::fs::read_dir(path).expect("list dir contents");

            Self {
                iter,
                root: root_dir.to_string(),
                extension: extension.to_string(),
            }
        }
    }

    impl Iterator for TestFileIter {
        type Item = Result<String, String>;

        fn next(&mut self) -> Option<Self::Item> {
            while let Some(entry) = self.iter.next() {
                if let Err(e) = entry {
                    return Some(Err(format!("failed to read dir entry, because: {}", e)));
                }
                let entry = entry.unwrap();

                let path = entry.path();
                if !path.is_file() { continue; }

                let extension = path.extension();
                if extension.is_none() { continue; }
                let extension = extension.unwrap().to_string_lossy();
                if extension != self.extension { continue; }

                return Some(Ok(path.to_string_lossy().to_string()));
            }

            None
        }
    }
}

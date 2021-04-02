mod depth_visitor;
pub(crate) mod symbol_table;
pub(crate) mod type_table;
mod type_resolver;
mod visitor;
mod visitor_linker;
mod utils;

use depth_visitor::*;
use symbol_table::SymbolTable;
use visitor::Visitor2;
use visitor_linker::ValidityAndLinkerVisitor;
use type_resolver::{TypeResolvingVisitor, ResolveQueue};
use type_table::{TypeTable, TypeCtx};

use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use crate::protocol::lexer::*;

use std::collections::HashMap;
use crate::protocol::ast_printer::ASTWriter;

// TODO: @fixme, pub qualifier
pub(crate) struct LexedModule {
    pub(crate) source: InputSource,
    module_name: Vec<u8>,
    version: Option<u64>,
    pub(crate) root_id: RootId,
}

pub struct Parser {
    pub(crate) heap: Heap,
    pub(crate) modules: Vec<LexedModule>,
    pub(crate) module_lookup: HashMap<Vec<u8>, usize>, // from (optional) module name to `modules` idx
    pub(crate) symbol_table: SymbolTable,
    pub(crate) type_table: TypeTable,
}

impl Parser {
    pub fn new() -> Self {
        Parser{
            heap: Heap::new(),
            modules: Vec::new(),
            module_lookup: HashMap::new(),
            symbol_table: SymbolTable::new(),
            type_table: TypeTable::new(),
        }
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

    fn resolve_symbols_and_types(&mut self) -> Result<(), ParseError2> {
        // Construct the symbol table to resolve any imports and/or definitions,
        // then use the symbol table to actually annotate all of the imports.
        // If the type table is constructed correctly then all imports MUST be
        // resolvable.
        self.symbol_table.build(&self.heap, &self.modules)?;

        // Not pretty, but we need to work around rust's borrowing rules, it is
        // totally safe to mutate the contents of an AST element that we are
        // not borrowing anywhere else.
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
                    let target_module_id = self.symbol_table.resolve_module(&import.module_name)
                        .expect("module import is resolved by symbol table");
                    import.module_id = Some(target_module_id)
                },
                Import::Symbols(import) => {
                    debug_assert!(import.module_id.is_none(), "module of symbol import already resolved");
                    let target_module_id = self.symbol_table.resolve_module(&import.module_name)
                        .expect("symbol import's module is resolved by symbol table");
                    import.module_id = Some(target_module_id);

                    for symbol in &mut import.symbols {
                        debug_assert!(symbol.definition_id.is_none(), "symbol import already resolved");
                        let (_, target_definition_id) = self.symbol_table.resolve_identifier(module_root_id, &symbol.alias)
                            .expect("symbol import is resolved by symbol table")
                            .as_definition()
                            .expect("symbol import does not resolve to namespace symbol");
                        symbol.definition_id = Some(target_definition_id);
                    }
                }
            }

            import_index += 1;
        }

        // All imports in the AST are now annotated. We now use the symbol table
        // to construct the type table.
        let mut type_ctx = TypeCtx::new(&self.symbol_table, &mut self.heap, &self.modules);
        self.type_table.build_base_types(&mut type_ctx)?;

        Ok(())
    }

    pub fn parse(&mut self) -> Result<(), ParseError2> {
        self.resolve_symbols_and_types()?;

        // Validate and link all modules
        let mut visit = ValidityAndLinkerVisitor::new();
        for module in &self.modules {
            let mut ctx = visitor::Ctx{
                heap: &mut self.heap,
                module,
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            visit.visit_module(&mut ctx)?;
        }

        // Perform typechecking on all modules
        let mut visit = TypeResolvingVisitor::new();
        let mut queue = ResolveQueue::new();
        for module in &self.modules {
            let ctx = visitor::Ctx{
                heap: &mut self.heap,
                module,
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            TypeResolvingVisitor::queue_module_definitions(&ctx, &mut queue);   
        };
        while !queue.is_empty() {
            let top = queue.pop().unwrap();
            let mut ctx = visitor::Ctx{
                heap: &mut self.heap,
                module: &self.modules[top.root_id.index as usize],
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            visit.handle_module_definition(&mut ctx, &mut queue, top)?;
        }

        // Perform remaining steps
        // TODO: Phase out at some point
        for module in &self.modules {
            let root_id = module.root_id;
            if let Err((position, message)) = Self::parse_inner(&mut self.heap, root_id) {
                return Err(ParseError2::new_error(&self.modules[0].source, position, &message))
            }
        }

        // let mut writer = ASTWriter::new();
        // let mut file = std::fs::File::create(std::path::Path::new("ast.txt")).unwrap();
        // writer.write_ast(&mut file, &self.heap);

        Ok(())
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
mod depth_visitor;
pub(crate) mod symbol_table;
pub(crate) mod type_table;
pub(crate) mod tokens;
pub(crate) mod token_parsing;
pub(crate) mod pass_tokenizer;
pub(crate) mod pass_symbols;
pub(crate) mod pass_imports;
pub(crate) mod pass_definitions;
pub(crate) mod pass_validation_linking;
pub(crate) mod pass_typing;
mod visitor;

use depth_visitor::*;
use tokens::*;
use crate::collections::*;
use symbol_table::SymbolTable;
use visitor::Visitor2;
use pass_tokenizer::PassTokenizer;
use pass_symbols::PassSymbols;
use pass_imports::PassImport;
use pass_definitions::PassDefinitions;
use pass_validation_linking::PassValidationLinking;
use pass_typing::{PassTyping, ResolveQueue};
use type_table::TypeTable;

use crate::protocol::ast::*;
use crate::protocol::input_source::*;

use crate::protocol::ast_printer::ASTWriter;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleCompilationPhase {
    Source,                 // only source is set
    Tokenized,              // source is tokenized
    SymbolsScanned,         // all definitions are linked to their type class
    ImportsResolved,        // all imports are added to the symbol table
    DefinitionsParsed,      // produced the AST for the entire module
    TypesAddedToTable,      // added all definitions to the type table
    ValidatedAndLinked,     // AST is traversed and has linked the required AST nodes
    Typed,                  // Type inference and checking has been performed
}

pub struct Module {
    // Buffers
    pub source: InputSource,
    pub tokens: TokenBuffer,
    // Identifiers
    pub root_id: RootId,
    pub name: Option<(PragmaId, StringRef<'static>)>,
    pub version: Option<(PragmaId, i64)>,
    pub phase: ModuleCompilationPhase,
}

pub struct PassCtx<'a> {
    heap: &'a mut Heap,
    symbols: &'a mut SymbolTable,
    pool: &'a mut StringPool,
}

pub struct Parser {
    pub(crate) heap: Heap,
    pub(crate) string_pool: StringPool,
    pub(crate) modules: Vec<Module>,
    pub(crate) symbol_table: SymbolTable,
    pub(crate) type_table: TypeTable,
    // Compiler passes
    pass_tokenizer: PassTokenizer,
    pass_symbols: PassSymbols,
    pass_import: PassImport,
    pass_definitions: PassDefinitions,
    pass_validation: PassValidationLinking,
    pass_typing: PassTyping,
}

impl Parser {
    pub fn new() -> Self {
        Parser{
            heap: Heap::new(),
            string_pool: StringPool::new(),
            modules: Vec::new(),
            symbol_table: SymbolTable::new(),
            type_table: TypeTable::new(),
            pass_tokenizer: PassTokenizer::new(),
            pass_symbols: PassSymbols::new(),
            pass_import: PassImport::new(),
            pass_definitions: PassDefinitions::new(),
            pass_validation: PassValidationLinking::new(),
            pass_typing: PassTyping::new(),
        }
    }

    pub fn feed(&mut self, mut source: InputSource) -> Result<(), ParseError> {
        // TODO: @Optimize
        let mut token_buffer = TokenBuffer::new();
        self.pass_tokenizer.tokenize(&mut source, &mut token_buffer)?;

        let module = Module{
            source,
            tokens: token_buffer,
            root_id: RootId::new_invalid(),
            name: None,
            version: None,
            phase: ModuleCompilationPhase::Tokenized,
        };
        self.modules.push(module);

        Ok(())
    }

    pub fn parse(&mut self) -> Result<(), ParseError> {
        let mut pass_ctx = PassCtx{
            heap: &mut self.heap,
            symbols: &mut self.symbol_table,
            pool: &mut self.string_pool,
        };

        // Advance all modules to the phase where all symbols are scanned
        for module_idx in 0..self.modules.len() {
            self.pass_symbols.parse(&mut self.modules, module_idx, &mut pass_ctx)?;
        }

        // With all symbols scanned, perform further compilation until we can
        // add all base types to the type table.
        for module_idx in 0..self.modules.len() {
            self.pass_import.parse(&mut self.modules, module_idx, &mut pass_ctx)?;
            self.pass_definitions.parse(&mut self.modules, module_idx, &mut pass_ctx)?;
        }

        // Add every known type to the type table
        self.type_table.build_base_types(&mut self.modules, &mut pass_ctx)?;

        // Continue compilation with the remaining phases now that the types
        // are all in the type table
        for module_idx in 0..self.modules.len() {
            let mut ctx = visitor::Ctx{
                heap: &mut self.heap,
                module: &self.modules[module_idx],
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            self.pass_validation.visit_module(&mut ctx)?;
        }

        // Perform typechecking on all modules
        let mut queue = ResolveQueue::new();
        for module in &self.modules {
            let ctx = visitor::Ctx{
                heap: &mut self.heap,
                module,
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            PassTyping::queue_module_definitions(&ctx, &mut queue);
        };
        while !queue.is_empty() {
            let top = queue.pop().unwrap();
            let mut ctx = visitor::Ctx{
                heap: &mut self.heap,
                module: &self.modules[top.root_id.index as usize],
                symbols: &mut self.symbol_table,
                types: &mut self.type_table,
            };
            self.pass_typing.handle_module_definition(&mut ctx, &mut queue, top)?;
        }

        // Perform remaining steps
        // TODO: Phase out at some point
        for module in &self.modules {
            let root_id = module.root_id;
            if let Err((position, message)) = Self::parse_inner(&mut self.heap, root_id) {
                return Err(ParseError::new_error_str_at_pos(&self.modules[0].source, position, &message))
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
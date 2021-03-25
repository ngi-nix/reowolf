// TODO: @cleanup, rigorous cleanup of dead code and silly object-oriented
//  trait impls where I deem them unfit.

use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Index, IndexMut};

use super::arena::{Arena, Id};
// use super::containers::StringAllocator;

// TODO: @cleanup, transform wrapping types into type aliases where possible
use crate::protocol::inputsource::*;

/// Helper macro that defines a type alias for a AST element ID. In this case 
/// only used to alias the `Id<T>` types.
macro_rules! define_aliased_ast_id {
    // Variant where we just defined the alias, without any indexing
    ($name:ident, $parent:ty) => {
        pub type $name = $parent;
    };
    // Variant where we define the type, and the Index and IndexMut traits
    ($name:ident, $parent:ty, $indexed_type:ty, $indexed_arena:ident) => {
        define_aliased_ast_id!($name, $parent);
        impl Index<$name> for Heap {
            type Output = $indexed_type;
            fn index(&self, index: $name) -> &Self::Output {
                &self.$indexed_arena[index]
            }
        }

        impl IndexMut<$name> for Heap {
            fn index_mut(&mut self, index: $name) -> &mut Self::Output {
                &mut self.$indexed_arena[index]
            }
        }
    }
}

/// Helper macro that defines a wrapper type for a particular variant of an AST
/// element ID. Only used to define single-wrapping IDs.
macro_rules! define_new_ast_id {
    // Variant where we just defined the new type, without any indexing
    ($name:ident, $parent:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        pub struct $name (pub(crate) $parent);

        impl $name {
            pub fn upcast(self) -> $parent {
                self.0
            }
        }
    };
    // Variant where we define the type, and the Index and IndexMut traits
    ($name:ident, $parent:ty, $indexed_type:ty, $wrapper_type:path, $indexed_arena:ident) => {
        define_new_ast_id!($name, $parent);
        impl Index<$name> for Heap {
            type Output = $indexed_type;
            fn index(&self, index: $name) -> &Self::Output {
                if let $wrapper_type(v) = &self.$indexed_arena[index.0] {
                    v
                } else {
                    unreachable!()
                }
            }
        }

        impl IndexMut<$name> for Heap {
            fn index_mut(&mut self, index: $name) -> &mut Self::Output {
                if let $wrapper_type(v) = &mut self.$indexed_arena[index.0] {
                    v
                } else {
                    unreachable!()
                }
            }
        }
    }
}

define_aliased_ast_id!(RootId, Id<Root>, Root, protocol_descriptions);
define_aliased_ast_id!(PragmaId, Id<Pragma>, Pragma, pragmas);
define_aliased_ast_id!(ImportId, Id<Import>, Import, imports);
define_aliased_ast_id!(ParserTypeId, Id<ParserType>, ParserType, parser_types);

define_aliased_ast_id!(VariableId, Id<Variable>, Variable, variables);
define_new_ast_id!(ParameterId, VariableId, Parameter, Variable::Parameter, variables);
define_new_ast_id!(LocalId, VariableId, Local, Variable::Local, variables);

define_aliased_ast_id!(DefinitionId, Id<Definition>, Definition, definitions);
define_new_ast_id!(StructId, DefinitionId, StructDefinition, Definition::Struct, definitions);
define_new_ast_id!(EnumId, DefinitionId, EnumDefinition, Definition::Enum, definitions);
define_new_ast_id!(ComponentId, DefinitionId, Component, Definition::Component, definitions);
define_new_ast_id!(FunctionId, DefinitionId, Function, Definition::Function, definitions);

define_aliased_ast_id!(StatementId, Id<Statement>, Statement, statements);
define_new_ast_id!(BlockStatementId, StatementId, BlockStatement, Statement::Block, statements);
define_new_ast_id!(LocalStatementId, StatementId, LocalStatement, Statement::Local, statements);
define_new_ast_id!(MemoryStatementId, LocalStatementId);
define_new_ast_id!(ChannelStatementId, LocalStatementId);
define_new_ast_id!(SkipStatementId, StatementId, SkipStatement, Statement::Skip, statements);
define_new_ast_id!(LabeledStatementId, StatementId, LabeledStatement, Statement::Labeled, statements);
define_new_ast_id!(IfStatementId, StatementId, IfStatement, Statement::If, statements);
define_new_ast_id!(EndIfStatementId, StatementId, EndIfStatement, Statement::EndIf, statements);
define_new_ast_id!(WhileStatementId, StatementId, WhileStatement, Statement::While, statements);
define_new_ast_id!(EndWhileStatementId, StatementId, EndWhileStatement, Statement::EndWhile, statements);
define_new_ast_id!(BreakStatementId, StatementId, BreakStatement, Statement::Break, statements);
define_new_ast_id!(ContinueStatementId, StatementId, ContinueStatement, Statement::Continue, statements);
define_new_ast_id!(SynchronousStatementId, StatementId, SynchronousStatement, Statement::Synchronous, statements);
define_new_ast_id!(EndSynchronousStatementId, StatementId, EndSynchronousStatement, Statement::EndSynchronous, statements);
define_new_ast_id!(ReturnStatementId, StatementId, ReturnStatement, Statement::Return, statements);
define_new_ast_id!(AssertStatementId, StatementId, AssertStatement, Statement::Assert, statements);
define_new_ast_id!(GotoStatementId, StatementId, GotoStatement, Statement::Goto, statements);
define_new_ast_id!(NewStatementId, StatementId, NewStatement, Statement::New, statements);
define_new_ast_id!(ExpressionStatementId, StatementId, ExpressionStatement, Statement::Expression, statements);

define_aliased_ast_id!(ExpressionId, Id<Expression>, Expression, expressions);
define_new_ast_id!(AssignmentExpressionId, ExpressionId, AssignmentExpression, Expression::Assignment, expressions);
define_new_ast_id!(ConditionalExpressionId, ExpressionId, ConditionalExpression, Expression::Conditional, expressions);
define_new_ast_id!(BinaryExpressionId, ExpressionId, BinaryExpression, Expression::Binary, expressions);
define_new_ast_id!(UnaryExpressionId, ExpressionId, UnaryExpression, Expression::Unary, expressions);
define_new_ast_id!(IndexingExpressionId, ExpressionId, IndexingExpression, Expression::Indexing, expressions);
define_new_ast_id!(SlicingExpressionId, ExpressionId, SlicingExpression, Expression::Slicing, expressions);
define_new_ast_id!(SelectExpressionId, ExpressionId, SelectExpression, Expression::Select, expressions);
define_new_ast_id!(ArrayExpressionId, ExpressionId, ArrayExpression, Expression::Array, expressions);
define_new_ast_id!(LiteralExpressionId, ExpressionId, LiteralExpression, Expression::Literal, expressions);
define_new_ast_id!(CallExpressionId, ExpressionId, CallExpression, Expression::Call, expressions);
define_new_ast_id!(VariableExpressionId, ExpressionId, VariableExpression, Expression::Variable, expressions);

// TODO: @cleanup - pub qualifiers can be removed once done
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Heap {
    // Root arena, contains the entry point for different modules. Each root
    // contains lists of IDs that correspond to the other arenas.
    pub(crate) protocol_descriptions: Arena<Root>,
    // Contents of a file, these are the elements the `Root` elements refer to
    pragmas: Arena<Pragma>,
    pub(crate) imports: Arena<Import>,
    identifiers: Arena<Identifier>,
    pub(crate) parser_types: Arena<ParserType>,
    pub(crate) variables: Arena<Variable>,
    pub(crate) definitions: Arena<Definition>,
    pub(crate) statements: Arena<Statement>,
    pub(crate) expressions: Arena<Expression>,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            // string_alloc: StringAllocator::new(),
            protocol_descriptions: Arena::new(),
            pragmas: Arena::new(),
            imports: Arena::new(),
            identifiers: Arena::new(),
            parser_types: Arena::new(),
            variables: Arena::new(),
            definitions: Arena::new(),
            statements: Arena::new(),
            expressions: Arena::new(),
        }
    }
    pub fn alloc_parser_type(
        &mut self,
        f: impl FnOnce(ParserTypeId) -> ParserType,
    ) -> ParserTypeId {
        self.parser_types.alloc_with_id(|id| f(id))
    }

    pub fn alloc_parameter(&mut self, f: impl FnOnce(ParameterId) -> Parameter) -> ParameterId {
        ParameterId(
            self.variables.alloc_with_id(|id| Variable::Parameter(f(ParameterId(id)))),
        )
    }
    pub fn alloc_local(&mut self, f: impl FnOnce(LocalId) -> Local) -> LocalId {
        LocalId(
            self.variables.alloc_with_id(|id| Variable::Local(f(LocalId(id)))),
        )
    }
    pub fn alloc_assignment_expression(
        &mut self,
        f: impl FnOnce(AssignmentExpressionId) -> AssignmentExpression,
    ) -> AssignmentExpressionId {
        AssignmentExpressionId(
            self.expressions.alloc_with_id(|id| {
                Expression::Assignment(f(AssignmentExpressionId(id)))
            })
        )
    }
    pub fn alloc_conditional_expression(
        &mut self,
        f: impl FnOnce(ConditionalExpressionId) -> ConditionalExpression,
    ) -> ConditionalExpressionId {
        ConditionalExpressionId(
            self.expressions.alloc_with_id(|id| {
                Expression::Conditional(f(ConditionalExpressionId(id)))
            })
        )
    }
    pub fn alloc_binary_expression(
        &mut self,
        f: impl FnOnce(BinaryExpressionId) -> BinaryExpression,
    ) -> BinaryExpressionId {
        BinaryExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Binary(f(BinaryExpressionId(id)))),
        )
    }
    pub fn alloc_unary_expression(
        &mut self,
        f: impl FnOnce(UnaryExpressionId) -> UnaryExpression,
    ) -> UnaryExpressionId {
        UnaryExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Unary(f(UnaryExpressionId(id)))),
        )
    }
    pub fn alloc_slicing_expression(
        &mut self,
        f: impl FnOnce(SlicingExpressionId) -> SlicingExpression,
    ) -> SlicingExpressionId {
        SlicingExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Slicing(f(SlicingExpressionId(id)))),
        )
    }
    pub fn alloc_indexing_expression(
        &mut self,
        f: impl FnOnce(IndexingExpressionId) -> IndexingExpression,
    ) -> IndexingExpressionId {
        IndexingExpressionId(
            self.expressions.alloc_with_id(|id| {
                Expression::Indexing(f(IndexingExpressionId(id)))
            }),
        )
    }
    pub fn alloc_select_expression(
        &mut self,
        f: impl FnOnce(SelectExpressionId) -> SelectExpression,
    ) -> SelectExpressionId {
        SelectExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Select(f(SelectExpressionId(id)))),
        )
    }
    pub fn alloc_array_expression(
        &mut self,
        f: impl FnOnce(ArrayExpressionId) -> ArrayExpression,
    ) -> ArrayExpressionId {
        ArrayExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Array(f(ArrayExpressionId(id)))),
        )
    }
    pub fn alloc_literal_expression(
        &mut self,
        f: impl FnOnce(LiteralExpressionId) -> LiteralExpression,
    ) -> LiteralExpressionId {
        LiteralExpressionId(
            self.expressions.alloc_with_id(|id| {
                Expression::Literal(f(LiteralExpressionId(id)))
            }),
        )
    }
    pub fn alloc_call_expression(
        &mut self,
        f: impl FnOnce(CallExpressionId) -> CallExpression,
    ) -> CallExpressionId {
        CallExpressionId(
            self.expressions
                .alloc_with_id(|id| Expression::Call(f(CallExpressionId(id)))),
        )
    }
    pub fn alloc_variable_expression(
        &mut self,
        f: impl FnOnce(VariableExpressionId) -> VariableExpression,
    ) -> VariableExpressionId {
        VariableExpressionId(
            self.expressions.alloc_with_id(|id| {
                Expression::Variable(f(VariableExpressionId(id)))
            }),
        )
    }
    pub fn alloc_block_statement(
        &mut self,
        f: impl FnOnce(BlockStatementId) -> BlockStatement,
    ) -> BlockStatementId {
        BlockStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Block(f(BlockStatementId(id)))),
        )
    }
    pub fn alloc_memory_statement(
        &mut self,
        f: impl FnOnce(MemoryStatementId) -> MemoryStatement,
    ) -> MemoryStatementId {
        MemoryStatementId(LocalStatementId(self.statements.alloc_with_id(|id| {
            Statement::Local(LocalStatement::Memory(
                f(MemoryStatementId(LocalStatementId(id)))
            ))
        })))
    }
    pub fn alloc_channel_statement(
        &mut self,
        f: impl FnOnce(ChannelStatementId) -> ChannelStatement,
    ) -> ChannelStatementId {
        ChannelStatementId(LocalStatementId(self.statements.alloc_with_id(|id| {
            Statement::Local(LocalStatement::Channel(
                f(ChannelStatementId(LocalStatementId(id)))
            ))
        })))
    }
    pub fn alloc_skip_statement(
        &mut self,
        f: impl FnOnce(SkipStatementId) -> SkipStatement,
    ) -> SkipStatementId {
        SkipStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Skip(f(SkipStatementId(id)))),
        )
    }
    pub fn alloc_if_statement(
        &mut self,
        f: impl FnOnce(IfStatementId) -> IfStatement,
    ) -> IfStatementId {
        IfStatementId(
            self.statements.alloc_with_id(|id| Statement::If(f(IfStatementId(id)))),
        )
    }
    pub fn alloc_end_if_statement(
        &mut self,
        f: impl FnOnce(EndIfStatementId) -> EndIfStatement,
    ) -> EndIfStatementId {
        EndIfStatementId(
            self.statements
                .alloc_with_id(|id| Statement::EndIf(f(EndIfStatementId(id)))),
        )
    }
    pub fn alloc_while_statement(
        &mut self,
        f: impl FnOnce(WhileStatementId) -> WhileStatement,
    ) -> WhileStatementId {
        WhileStatementId(
            self.statements
                .alloc_with_id(|id| Statement::While(f(WhileStatementId(id)))),
        )
    }
    pub fn alloc_end_while_statement(
        &mut self,
        f: impl FnOnce(EndWhileStatementId) -> EndWhileStatement,
    ) -> EndWhileStatementId {
        EndWhileStatementId(
            self.statements
                .alloc_with_id(|id| Statement::EndWhile(f(EndWhileStatementId(id)))),
        )
    }
    pub fn alloc_break_statement(
        &mut self,
        f: impl FnOnce(BreakStatementId) -> BreakStatement,
    ) -> BreakStatementId {
        BreakStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Break(f(BreakStatementId(id)))),
        )
    }
    pub fn alloc_continue_statement(
        &mut self,
        f: impl FnOnce(ContinueStatementId) -> ContinueStatement,
    ) -> ContinueStatementId {
        ContinueStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Continue(f(ContinueStatementId(id)))),
        )
    }
    pub fn alloc_synchronous_statement(
        &mut self,
        f: impl FnOnce(SynchronousStatementId) -> SynchronousStatement,
    ) -> SynchronousStatementId {
        SynchronousStatementId(self.statements.alloc_with_id(|id| {
            Statement::Synchronous(f(SynchronousStatementId(id)))
        }))
    }
    pub fn alloc_end_synchronous_statement(
        &mut self,
        f: impl FnOnce(EndSynchronousStatementId) -> EndSynchronousStatement,
    ) -> EndSynchronousStatementId {
        EndSynchronousStatementId(self.statements.alloc_with_id(|id| {
            Statement::EndSynchronous(f(EndSynchronousStatementId(id)))
        }))
    }
    pub fn alloc_return_statement(
        &mut self,
        f: impl FnOnce(ReturnStatementId) -> ReturnStatement,
    ) -> ReturnStatementId {
        ReturnStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Return(f(ReturnStatementId(id)))),
        )
    }
    pub fn alloc_assert_statement(
        &mut self,
        f: impl FnOnce(AssertStatementId) -> AssertStatement,
    ) -> AssertStatementId {
        AssertStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Assert(f(AssertStatementId(id)))),
        )
    }
    pub fn alloc_goto_statement(
        &mut self,
        f: impl FnOnce(GotoStatementId) -> GotoStatement,
    ) -> GotoStatementId {
        GotoStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Goto(f(GotoStatementId(id)))),
        )
    }
    pub fn alloc_new_statement(
        &mut self,
        f: impl FnOnce(NewStatementId) -> NewStatement,
    ) -> NewStatementId {
        NewStatementId(
            self.statements.alloc_with_id(|id| Statement::New(f(NewStatementId(id)))),
        )
    }
    pub fn alloc_labeled_statement(
        &mut self,
        f: impl FnOnce(LabeledStatementId) -> LabeledStatement,
    ) -> LabeledStatementId {
        LabeledStatementId(
            self.statements
                .alloc_with_id(|id| Statement::Labeled(f(LabeledStatementId(id)))),
        )
    }
    pub fn alloc_expression_statement(
        &mut self,
        f: impl FnOnce(ExpressionStatementId) -> ExpressionStatement,
    ) -> ExpressionStatementId {
        ExpressionStatementId(
            self.statements.alloc_with_id(|id| {
                Statement::Expression(f(ExpressionStatementId(id)))
            }),
        )
    }
    pub fn alloc_struct_definition(&mut self, f: impl FnOnce(StructId) -> StructDefinition) -> StructId {
        StructId(self.definitions.alloc_with_id(|id| {
            Definition::Struct(f(StructId(id)))
        }))
    }
    pub fn alloc_enum_definition(&mut self, f: impl FnOnce(EnumId) -> EnumDefinition) -> EnumId {
        EnumId(self.definitions.alloc_with_id(|id| {
            Definition::Enum(f(EnumId(id)))
        }))
    }
    pub fn alloc_component(&mut self, f: impl FnOnce(ComponentId) -> Component) -> ComponentId {
        ComponentId(self.definitions.alloc_with_id(|id| {
            Definition::Component(f(ComponentId(id)))
        }))
    }
    pub fn alloc_function(&mut self, f: impl FnOnce(FunctionId) -> Function) -> FunctionId {
        FunctionId(
            self.definitions
                .alloc_with_id(|id| Definition::Function(f(FunctionId(id)))),
        )
    }
    pub fn alloc_pragma(&mut self, f: impl FnOnce(PragmaId) -> Pragma) -> PragmaId {
        self.pragmas.alloc_with_id(|id| f(id))
    }
    pub fn alloc_import(&mut self, f: impl FnOnce(ImportId) -> Import) -> ImportId {
        self.imports.alloc_with_id(|id| f(id))
    }
    pub fn alloc_protocol_description(&mut self, f: impl FnOnce(RootId) -> Root) -> RootId {
        self.protocol_descriptions.alloc_with_id(|id| f(id))
    }
}

impl Index<MemoryStatementId> for Heap {
    type Output = MemoryStatement;
    fn index(&self, index: MemoryStatementId) -> &Self::Output {
        &self.statements[index.0.0].as_memory()
    }
}

impl Index<ChannelStatementId> for Heap {
    type Output = ChannelStatement;
    fn index(&self, index: ChannelStatementId) -> &Self::Output {
        &self.statements[index.0.0].as_channel()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Root {
    pub this: RootId,
    // Phase 1: parser
    pub position: InputPosition,
    pub pragmas: Vec<PragmaId>,
    pub imports: Vec<ImportId>,
    pub definitions: Vec<DefinitionId>,
}

impl Root {
    pub fn get_definition_ident(&self, h: &Heap, id: &[u8]) -> Option<DefinitionId> {
        for &def in self.definitions.iter() {
            if h[def].identifier().value == id {
                return Some(def);
            }
        }
        None
    }
}

impl SyntaxElement for Root {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Pragma {
    Version(PragmaVersion),
    Module(PragmaModule)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PragmaVersion {
    pub this: PragmaId,
    // Phase 1: parser
    pub position: InputPosition,
    pub version: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PragmaModule {
    pub this: PragmaId,
    // Phase 1: parser
    pub position: InputPosition,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PragmaOld {
    pub this: PragmaId,
    // Phase 1: parser
    pub position: InputPosition,
    pub value: Vec<u8>,
}

impl SyntaxElement for PragmaOld {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Import {
    Module(ImportModule),
    Symbols(ImportSymbols)
}

impl Import {
    pub(crate) fn as_module(&self) -> &ImportModule {
        match self {
            Import::Module(m) => m,
            _ => panic!("Unable to cast 'Import' to 'ImportModule'")
        }
    }
    pub(crate) fn as_symbols(&self) -> &ImportSymbols {
        match self {
            Import::Symbols(m) => m,
            _ => panic!("Unable to cast 'Import' to 'ImportSymbols'")
        }
    }
}

impl SyntaxElement for Import {
    fn position(&self) -> InputPosition {
        match self {
            Import::Module(m) => m.position,
            Import::Symbols(m) => m.position
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportModule {
    pub this: ImportId,
    // Phase 1: parser
    pub position: InputPosition,
    pub module_name: Vec<u8>,
    pub alias: Vec<u8>,
    // Phase 2: module resolving
    pub module_id: Option<RootId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AliasedSymbol {
    // Phase 1: parser
    pub position: InputPosition,
    pub name: Vec<u8>,
    pub alias: Vec<u8>,
    // Phase 2: symbol resolving
    pub definition_id: Option<DefinitionId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportSymbols {
    pub this: ImportId,
    // Phase 1: parser
    pub position: InputPosition,
    pub module_name: Vec<u8>,
    // Phase 2: module resolving
    pub module_id: Option<RootId>,
    // Phase 1&2
    // if symbols is empty, then we implicitly import all symbols without any
    // aliases for them. If it is not empty, then symbols are explicitly
    // specified, and optionally given an alias.
    pub symbols: Vec<AliasedSymbol>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Identifier {
    pub position: InputPosition,
    pub value: Vec<u8>
}

impl PartialEq for Identifier {
    fn eq(&self, other: &Self) -> bool {
        return self.value == other.value
    }
}

impl PartialEq<NamespacedIdentifier> for Identifier {
    fn eq(&self, other: &NamespacedIdentifier) -> bool {
        return self.value == other.value
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NamespacedIdentifier {
    pub position: InputPosition,
    pub num_namespaces: u8,
    pub value: Vec<u8>,
}

impl NamespacedIdentifier {
    pub(crate) fn iter(&self) -> NamespacedIdentifierIter {
        NamespacedIdentifierIter{
            value: &self.value,
            cur_offset: 0,
            num_returned: 0,
            num_total: self.num_namespaces
        }
    }
}

impl PartialEq for NamespacedIdentifier {
    fn eq(&self, other: &Self) -> bool {
        return self.value == other.value
    }
}

impl PartialEq<Identifier> for NamespacedIdentifier {
    fn eq(&self, other: &Identifier) -> bool {
        return self.value == other.value;
    }
}

// TODO: Just keep ref to NamespacedIdentifier
pub(crate) struct NamespacedIdentifierIter<'a> {
    value: &'a Vec<u8>,
    cur_offset: usize,
    num_returned: u8,
    num_total: u8,
}

impl<'a> NamespacedIdentifierIter<'a> {
    pub(crate) fn num_returned(&self) -> u8 {
        return self.num_returned;
    }
    pub(crate) fn num_remaining(&self) -> u8 {
        return self.num_total - self.num_returned
    }
    pub(crate) fn returned_section(&self) -> &[u8] {
        // Offset always includes the two trailing ':' characters
        let end = if self.cur_offset >= 2 { self.cur_offset - 2 } else { self.cur_offset };
        return &self.value[..end]
    }
}

impl<'a> Iterator for NamespacedIdentifierIter<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur_offset >= self.value.len() {
            debug_assert_eq!(self.num_returned, self.num_total);
            None
        } else {
            debug_assert!(self.num_returned < self.num_total);
            let start = self.cur_offset;
            let mut end = start;
            while end < self.value.len() - 1 {
                if self.value[end] == b':' && self.value[end + 1] == b':' {
                    self.cur_offset = end + 2;
                    self.num_returned += 1;
                    return Some(&self.value[start..end]);
                }
                end += 1;
            }

            // If NamespacedIdentifier is constructed properly, then we cannot
            // end with "::" in the value, so
            debug_assert!(end == 0 || (self.value[end - 1] != b':' && self.value[end] != b':'));
            debug_assert_eq!(self.num_returned + 1, self.num_total);
            self.cur_offset = self.value.len();
            self.num_returned += 1;
            return Some(&self.value[start..]);
        }
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // A source identifier is in ASCII range.
        write!(f, "{}", String::from_utf8_lossy(&self.value))
    }
}

/// TODO: @types Remove the Message -> Byte hack at some point...
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParserTypeVariant {
    // Basic builtin
    Message,
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // Literals (need to get concrete builtin type during typechecking)
    IntegerLiteral,
    Inferred,
    // Complex builtins
    Array(ParserTypeId), // array of a type
    Input(ParserTypeId), // typed input endpoint of a channel
    Output(ParserTypeId), // typed output endpoint of a channel
    Symbolic(SymbolicParserType), // symbolic type (definition or polyarg)
}

impl ParserTypeVariant {
    pub(crate) fn supports_polymorphic_args(&self) -> bool {
        use ParserTypeVariant::*;
        match self {
            Message | Bool | Byte | Short | Int | Long | String | IntegerLiteral | Inferred => false,
            _ => true
        }
    }
}

/// ParserType is a specification of a type during the parsing phase and initial
/// linker/validator phase of the compilation process. These types may be
/// (partially) inferred or represent literals (e.g. a integer whose bytesize is
/// not yet determined).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParserType {
    pub this: ParserTypeId,
    pub pos: InputPosition,
    pub variant: ParserTypeVariant,
}

/// SymbolicParserType is the specification of a symbolic type. During the
/// parsing phase we will only store the identifier of the type. During the
/// validation phase we will determine whether it refers to a user-defined type,
/// or a polymorphic argument. After the validation phase it may still be the
/// case that the resulting `variant` will not pass the typechecker.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolicParserType {
    // Phase 1: parser
    pub identifier: NamespacedIdentifier,
    /// The user-specified polymorphic arguments. Zero-length implies that the
    /// user did not specify any of them, and they're either not needed or all
    /// need to be inferred. Otherwise the number of polymorphic arguments must
    /// match those of the corresponding definition
    pub poly_args: Vec<ParserTypeId>,
    // Phase 2: validation/linking (for types in function/component bodies) and
    //  type table construction (for embedded types of structs/unions)
    pub variant: Option<SymbolicParserTypeVariant>
}

/// Specifies whether the symbolic type points to an actual user-defined type,
/// or whether it points to a polymorphic argument within the definition (e.g.
/// a defined variable `T var` within a function `int func<T>()`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SymbolicParserTypeVariant {
    Definition(DefinitionId),
    // TODO: figure out if I need the DefinitionId here
    PolyArg(DefinitionId, usize), // index of polyarg in the definition
}

/// ConcreteType is the representation of a type after resolving symbolic types
/// and performing type inference
#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ConcreteTypePart {
    // Markers for the use of polymorphic types within a procedure's body that
    // refer to polymorphic variables on the procedure's definition. Different
    // from markers in the `InferenceType`, these will not contain nested types.
    Marker(usize),
    // Special types (cannot be explicitly constructed by the programmer)
    Void,
    // Builtin types without nested types
    Message,
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // Builtin types with one nested type
    Array,
    Slice,
    Input,
    Output,
    // User defined type with any number of nested types
    Instance(DefinitionId, usize),
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConcreteType {
    pub(crate) parts: Vec<ConcreteTypePart>
}

impl Default for ConcreteType {
    fn default() -> Self {
        Self{ parts: Vec::new() }
    }
}

impl ConcreteType {
    pub(crate) fn has_marker(&self) -> bool {
        self.parts
            .iter()
            .any(|p| {
                if let ConcreteTypePart::Marker(_) = p { true } else { false }
            })
    }
}

// TODO: Remove at some point
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PrimitiveType {
    Unassigned,
    Input,
    Output,
    Message,
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Symbolic(PrimitiveSymbolic)
}

// TODO: @cleanup, remove PartialEq implementations
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrimitiveSymbolic {
    // Phase 1: parser
    pub(crate) identifier: NamespacedIdentifier,
    // Phase 2: typing
    pub(crate) definition: Option<DefinitionId>
}

impl PartialEq for PrimitiveSymbolic {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
    }
}
impl Eq for PrimitiveSymbolic{}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Type {
    pub primitive: PrimitiveType,
    pub array: bool,
}

#[allow(dead_code)]
impl Type {
    pub const UNASSIGNED: Type = Type { primitive: PrimitiveType::Unassigned, array: false };

    pub const INPUT: Type = Type { primitive: PrimitiveType::Input, array: false };
    pub const OUTPUT: Type = Type { primitive: PrimitiveType::Output, array: false };
    pub const MESSAGE: Type = Type { primitive: PrimitiveType::Message, array: false };
    pub const BOOLEAN: Type = Type { primitive: PrimitiveType::Boolean, array: false };
    pub const BYTE: Type = Type { primitive: PrimitiveType::Byte, array: false };
    pub const SHORT: Type = Type { primitive: PrimitiveType::Short, array: false };
    pub const INT: Type = Type { primitive: PrimitiveType::Int, array: false };
    pub const LONG: Type = Type { primitive: PrimitiveType::Long, array: false };

    pub const INPUT_ARRAY: Type = Type { primitive: PrimitiveType::Input, array: true };
    pub const OUTPUT_ARRAY: Type = Type { primitive: PrimitiveType::Output, array: true };
    pub const MESSAGE_ARRAY: Type = Type { primitive: PrimitiveType::Message, array: true };
    pub const BOOLEAN_ARRAY: Type = Type { primitive: PrimitiveType::Boolean, array: true };
    pub const BYTE_ARRAY: Type = Type { primitive: PrimitiveType::Byte, array: true };
    pub const SHORT_ARRAY: Type = Type { primitive: PrimitiveType::Short, array: true };
    pub const INT_ARRAY: Type = Type { primitive: PrimitiveType::Int, array: true };
    pub const LONG_ARRAY: Type = Type { primitive: PrimitiveType::Long, array: true };
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.primitive {
            PrimitiveType::Unassigned => {
                write!(f, "unassigned")?;
            }
            PrimitiveType::Input => {
                write!(f, "in")?;
            }
            PrimitiveType::Output => {
                write!(f, "out")?;
            }
            PrimitiveType::Message => {
                write!(f, "msg")?;
            }
            PrimitiveType::Boolean => {
                write!(f, "boolean")?;
            }
            PrimitiveType::Byte => {
                write!(f, "byte")?;
            }
            PrimitiveType::Short => {
                write!(f, "short")?;
            }
            PrimitiveType::Int => {
                write!(f, "int")?;
            }
            PrimitiveType::Long => {
                write!(f, "long")?;
            }
            PrimitiveType::Symbolic(data) => {
                // Type data is in ASCII range.
                if let Some(id) = &data.definition {
                    write!(
                        f, "Symbolic({}, id: {})", 
                        String::from_utf8_lossy(&data.identifier.value),
                        id.index
                    )?;
                } else {
                    write!(
                        f, "Symbolic({}, id: Unresolved)",
                        String::from_utf8_lossy(&data.identifier.value)
                    )?;
                }
            }
        }
        if self.array {
            write!(f, "[]")
        } else {
            Ok(())
        }
    }
}

type LiteralCharacter = Vec<u8>;
type LiteralInteger = i64; // TODO: @int_literal

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Literal {
    Null, // message
    True,
    False,
    Character(LiteralCharacter),
    Integer(LiteralInteger),
    Struct(LiteralStruct),
}

impl Literal {
    pub(crate) fn as_struct(&self) -> &LiteralStruct {
        if let Literal::Struct(literal) = self{
            literal
        } else {
            unreachable!("Attempted to obtain {:?} as Literal::Struct", self)
        }
    }

    pub(crate) fn as_struct_mut(&mut self) -> &mut LiteralStruct {
        if let Literal::Struct(literal) = self{
            literal
        } else {
            unreachable!("Attempted to obtain {:?} as Literal::Struct", self)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiteralStructField {
    // Phase 1: parser
    pub(crate) identifier: Identifier,
    pub(crate) value: ExpressionId,
    // Phase 2: linker
    pub(crate) field_idx: usize, // in struct definition
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiteralStruct {
    // Phase 1: parser
    pub(crate) identifier: NamespacedIdentifier,
    pub(crate) poly_args: Vec<ParserTypeId>,
    pub(crate) fields: Vec<LiteralStructField>,
    // Phase 2: linker
    pub(crate) definition: Option<DefinitionId>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Method {
    Get,
    Put,
    Fires,
    Create,
    Symbolic(MethodSymbolic)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MethodSymbolic {
    pub(crate) identifier: NamespacedIdentifier,
    pub(crate) definition: Option<DefinitionId>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Field {
    Length,
    Symbolic(Identifier),
}
impl Field {
    pub fn is_length(&self) -> bool {
        match self {
            Field::Length => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Scope {
    Definition(DefinitionId),
    Regular(BlockStatementId),
    Synchronous((SynchronousStatementId, BlockStatementId)),
}

impl Scope {
    pub fn is_block(&self) -> bool {
        match &self {
            Scope::Definition(_) => false,
            Scope::Regular(_) => true,
            Scope::Synchronous(_) => true,
        }
    }
    pub fn to_block(&self) -> BlockStatementId {
        match &self {
            Scope::Regular(id) => *id,
            Scope::Synchronous((_, id)) => *id,
            _ => panic!("unable to get BlockStatement from Scope")
        }
    }
}

pub trait VariableScope {
    fn parent_scope(&self, h: &Heap) -> Option<Scope>;
    fn get_variable(&self, h: &Heap, id: &Identifier) -> Option<VariableId>;
}

impl VariableScope for Scope {
    fn parent_scope(&self, h: &Heap) -> Option<Scope> {
        match self {
            Scope::Definition(def) => h[*def].parent_scope(h),
            Scope::Regular(stmt) => h[*stmt].parent_scope(h),
            Scope::Synchronous((stmt, _)) => h[*stmt].parent_scope(h),
        }
    }
    fn get_variable(&self, h: &Heap, id: &Identifier) -> Option<VariableId> {
        match self {
            Scope::Definition(def) => h[*def].get_variable(h, id),
            Scope::Regular(stmt) => h[*stmt].get_variable(h, id),
            Scope::Synchronous((stmt, _)) => h[*stmt].get_variable(h, id),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Variable {
    Parameter(Parameter),
    Local(Local),
}

impl Variable {
    pub fn identifier(&self) -> &Identifier {
        match self {
            Variable::Parameter(var) => &var.identifier,
            Variable::Local(var) => &var.identifier,
        }
    }
    pub fn is_parameter(&self) -> bool {
        match self {
            Variable::Parameter(_) => true,
            _ => false,
        }
    }
    pub fn as_parameter(&self) -> &Parameter {
        match self {
            Variable::Parameter(result) => result,
            _ => panic!("Unable to cast `Variable` to `Parameter`"),
        }
    }
    pub fn as_local(&self) -> &Local {
        match self {
            Variable::Local(result) => result,
            _ => panic!("Unable to cast `Variable` to `Local`"),
        }
    }
    pub fn as_local_mut(&mut self) -> &mut Local {
        match self {
            Variable::Local(result) => result,
            _ => panic!("Unable to cast 'Variable' to 'Local'"),
        }
    }
}

impl SyntaxElement for Variable {
    fn position(&self) -> InputPosition {
        match self {
            Variable::Parameter(decl) => decl.position(),
            Variable::Local(decl) => decl.position(),
        }
    }
}

/// TODO: Remove distinction between parameter/local and add an enum to indicate
///     the distinction between the two
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Parameter {
    pub this: ParameterId,
    // Phase 1: parser
    pub position: InputPosition,
    pub parser_type: ParserTypeId,
    pub identifier: Identifier,
}

impl SyntaxElement for Parameter {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Local {
    pub this: LocalId,
    // Phase 1: parser
    pub position: InputPosition,
    pub parser_type: ParserTypeId,
    pub identifier: Identifier,
    // Phase 2: linker
    pub relative_pos_in_block: u32,
}
impl SyntaxElement for Local {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Definition {
    Struct(StructDefinition),
    Enum(EnumDefinition),
    Component(Component),
    Function(Function),
}

impl Definition {
    pub fn is_struct(&self) -> bool {
        match self {
            Definition::Struct(_) => true,
            _ => false
        }
    }
    pub fn as_struct(&self) -> &StructDefinition {
        match self {
            Definition::Struct(result) => result,
            _ => panic!("Unable to cast 'Definition' to 'StructDefinition'"),
        }
    }
    pub fn is_enum(&self) -> bool {
        match self {
            Definition::Enum(_) => true,
            _ => false,
        }
    }
    pub fn as_enum(&self) -> &EnumDefinition {
        match self {
            Definition::Enum(result) => result,
            _ => panic!("Unable to cast 'Definition' to 'EnumDefinition'"),
        }
    }
    pub fn is_component(&self) -> bool {
        match self {
            Definition::Component(_) => true,
            _ => false,
        }
    }
    pub fn as_component(&self) -> &Component {
        match self {
            Definition::Component(result) => result,
            _ => panic!("Unable to cast `Definition` to `Component`"),
        }
    }
    pub fn is_function(&self) -> bool {
        match self {
            Definition::Function(_) => true,
            _ => false,
        }
    }
    pub fn as_function(&self) -> &Function {
        match self {
            Definition::Function(result) => result,
            _ => panic!("Unable to cast `Definition` to `Function`"),
        }
    }
    pub fn identifier(&self) -> &Identifier {
        match self {
            Definition::Struct(def) => &def.identifier,
            Definition::Enum(def) => &def.identifier,
            Definition::Component(com) => &com.identifier,
            Definition::Function(fun) => &fun.identifier,
        }
    }
    pub fn parameters(&self) -> &Vec<ParameterId> {
        // TODO: Fix this
        static EMPTY_VEC: Vec<ParameterId> = Vec::new();
        match self {
            Definition::Component(com) => &com.parameters,
            Definition::Function(fun) => &fun.parameters,
            _ => &EMPTY_VEC,
        }
    }
    pub fn body(&self) -> StatementId {
        // TODO: Fix this
        match self {
            Definition::Component(com) => com.body,
            Definition::Function(fun) => fun.body,
            _ => panic!("cannot retrieve body (for EnumDefinition or StructDefinition)")
        }
    }
}

impl SyntaxElement for Definition {
    fn position(&self) -> InputPosition {
        match self {
            Definition::Struct(def) => def.position,
            Definition::Enum(def) => def.position,
            Definition::Component(def) => def.position(),
            Definition::Function(def) => def.position(),
        }
    }
}

impl VariableScope for Definition {
    fn parent_scope(&self, _h: &Heap) -> Option<Scope> {
        None
    }
    fn get_variable(&self, h: &Heap, id: &Identifier) -> Option<VariableId> {
        for &parameter_id in self.parameters().iter() {
            let parameter = &h[parameter_id];
            if parameter.identifier == *id {
                return Some(parameter_id.0);
            }
        }
        None
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StructFieldDefinition {
    pub position: InputPosition,
    pub field: Identifier,
    pub parser_type: ParserTypeId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StructDefinition {
    pub this: StructId,
    // Phase 1: parser
    pub position: InputPosition,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub fields: Vec<StructFieldDefinition>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum EnumVariantValue {
    None,
    Integer(i64),
    Type(ParserTypeId),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnumVariantDefinition {
    pub position: InputPosition,
    pub identifier: Identifier,
    pub value: EnumVariantValue,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnumDefinition {
    pub this: EnumId,
    // Phase 1: parser
    pub position: InputPosition,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub variants: Vec<EnumVariantDefinition>,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ComponentVariant {
    Primitive,
    Composite,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Component {
    pub this: ComponentId,
    // Phase 1: parser
    pub position: InputPosition,
    pub variant: ComponentVariant,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub parameters: Vec<ParameterId>,
    pub body: StatementId,
}

impl SyntaxElement for Component {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Function {
    pub this: FunctionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub return_type: ParserTypeId,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub parameters: Vec<ParameterId>,
    pub body: StatementId,
}

impl SyntaxElement for Function {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Statement {
    Block(BlockStatement),
    Local(LocalStatement),
    Skip(SkipStatement),
    Labeled(LabeledStatement),
    If(IfStatement),
    EndIf(EndIfStatement),
    While(WhileStatement),
    EndWhile(EndWhileStatement),
    Break(BreakStatement),
    Continue(ContinueStatement),
    Synchronous(SynchronousStatement),
    EndSynchronous(EndSynchronousStatement),
    Return(ReturnStatement),
    Assert(AssertStatement),
    Goto(GotoStatement),
    New(NewStatement),
    Expression(ExpressionStatement),
}

impl Statement {
    pub fn as_block(&self) -> &BlockStatement {
        match self {
            Statement::Block(result) => result,
            _ => panic!("Unable to cast `Statement` to `BlockStatement`"),
        }
    }
    pub fn as_block_mut(&mut self) -> &mut BlockStatement {
        match self {
            Statement::Block(result) => result,
            _ => panic!("Unable to cast `Statement` to `BlockStatement`"),
        }
    }
    pub fn as_local(&self) -> &LocalStatement {
        match self {
            Statement::Local(result) => result,
            _ => panic!("Unable to cast `Statement` to `LocalStatement`"),
        }
    }
    pub fn as_memory(&self) -> &MemoryStatement {
        self.as_local().as_memory()
    }
    pub fn as_channel(&self) -> &ChannelStatement {
        self.as_local().as_channel()
    }
    pub fn as_skip(&self) -> &SkipStatement {
        match self {
            Statement::Skip(result) => result,
            _ => panic!("Unable to cast `Statement` to `SkipStatement`"),
        }
    }
    pub fn as_labeled(&self) -> &LabeledStatement {
        match self {
            Statement::Labeled(result) => result,
            _ => panic!("Unable to cast `Statement` to `LabeledStatement`"),
        }
    }
    pub fn as_labeled_mut(&mut self) -> &mut LabeledStatement {
        match self {
            Statement::Labeled(result) => result,
            _ => panic!("Unable to cast `Statement` to `LabeledStatement`"),
        }
    }
    pub fn as_if(&self) -> &IfStatement {
        match self {
            Statement::If(result) => result,
            _ => panic!("Unable to cast `Statement` to `IfStatement`"),
        }
    }
    pub fn as_if_mut(&mut self) -> &mut IfStatement {
        match self {
            Statement::If(result) => result,
            _ => panic!("Unable to cast 'Statement' to 'IfStatement'"),
        }
    }
    pub fn as_end_if(&self) -> &EndIfStatement {
        match self {
            Statement::EndIf(result) => result,
            _ => panic!("Unable to cast `Statement` to `EndIfStatement`"),
        }
    }
    pub fn is_while(&self) -> bool {
        match self {
            Statement::While(_) => true,
            _ => false,
        }
    }
    pub fn as_while(&self) -> &WhileStatement {
        match self {
            Statement::While(result) => result,
            _ => panic!("Unable to cast `Statement` to `WhileStatement`"),
        }
    }
    pub fn as_while_mut(&mut self) -> &mut WhileStatement {
        match self {
            Statement::While(result) => result,
            _ => panic!("Unable to cast `Statement` to `WhileStatement`"),
        }
    }
    pub fn as_end_while(&self) -> &EndWhileStatement {
        match self {
            Statement::EndWhile(result) => result,
            _ => panic!("Unable to cast `Statement` to `EndWhileStatement`"),
        }
    }
    pub fn as_break(&self) -> &BreakStatement {
        match self {
            Statement::Break(result) => result,
            _ => panic!("Unable to cast `Statement` to `BreakStatement`"),
        }
    }
    pub fn as_break_mut(&mut self) -> &mut BreakStatement {
        match self {
            Statement::Break(result) => result,
            _ => panic!("Unable to cast `Statement` to `BreakStatement`"),
        }
    }
    pub fn as_continue(&self) -> &ContinueStatement {
        match self {
            Statement::Continue(result) => result,
            _ => panic!("Unable to cast `Statement` to `ContinueStatement`"),
        }
    }
    pub fn as_continue_mut(&mut self) -> &mut ContinueStatement {
        match self {
            Statement::Continue(result) => result,
            _ => panic!("Unable to cast `Statement` to `ContinueStatement`"),
        }
    }
    pub fn as_synchronous(&self) -> &SynchronousStatement {
        match self {
            Statement::Synchronous(result) => result,
            _ => panic!("Unable to cast `Statement` to `SynchronousStatement`"),
        }
    }
    pub fn as_synchronous_mut(&mut self) -> &mut SynchronousStatement {
        match self {
            Statement::Synchronous(result) => result,
            _ => panic!("Unable to cast `Statement` to `SynchronousStatement`"),
        }
    }
    pub fn as_end_synchronous(&self) -> &EndSynchronousStatement {
        match self {
            Statement::EndSynchronous(result) => result,
            _ => panic!("Unable to cast `Statement` to `EndSynchronousStatement`"),
        }
    }
    pub fn as_return(&self) -> &ReturnStatement {
        match self {
            Statement::Return(result) => result,
            _ => panic!("Unable to cast `Statement` to `ReturnStatement`"),
        }
    }
    pub fn as_assert(&self) -> &AssertStatement {
        match self {
            Statement::Assert(result) => result,
            _ => panic!("Unable to cast `Statement` to `AssertStatement`"),
        }
    }
    pub fn as_goto(&self) -> &GotoStatement {
        match self {
            Statement::Goto(result) => result,
            _ => panic!("Unable to cast `Statement` to `GotoStatement`"),
        }
    }
    pub fn as_goto_mut(&mut self) -> &mut GotoStatement {
        match self {
            Statement::Goto(result) => result,
            _ => panic!("Unable to cast `Statement` to `GotoStatement`"),
        }
    }
    pub fn as_new(&self) -> &NewStatement {
        match self {
            Statement::New(result) => result,
            _ => panic!("Unable to cast `Statement` to `NewStatement`"),
        }
    }
    pub fn as_expression(&self) -> &ExpressionStatement {
        match self {
            Statement::Expression(result) => result,
            _ => panic!("Unable to cast `Statement` to `ExpressionStatement`"),
        }
    }
    pub fn link_next(&mut self, next: StatementId) {
        match self {
            Statement::Block(_) => todo!(),
            Statement::Local(stmt) => match stmt {
                LocalStatement::Channel(stmt) => stmt.next = Some(next),
                LocalStatement::Memory(stmt) => stmt.next = Some(next),
            },
            Statement::Skip(stmt) => stmt.next = Some(next),
            Statement::EndIf(stmt) => stmt.next = Some(next),
            Statement::EndWhile(stmt) => stmt.next = Some(next),
            Statement::EndSynchronous(stmt) => stmt.next = Some(next),
            Statement::Assert(stmt) => stmt.next = Some(next),
            Statement::New(stmt) => stmt.next = Some(next),
            Statement::Expression(stmt) => stmt.next = Some(next),
            Statement::Return(_)
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Synchronous(_)
            | Statement::Goto(_)
            | Statement::While(_)
            | Statement::Labeled(_)
            | Statement::If(_) => unreachable!(),
        }
    }
}

impl SyntaxElement for Statement {
    fn position(&self) -> InputPosition {
        match self {
            Statement::Block(stmt) => stmt.position(),
            Statement::Local(stmt) => stmt.position(),
            Statement::Skip(stmt) => stmt.position(),
            Statement::Labeled(stmt) => stmt.position(),
            Statement::If(stmt) => stmt.position(),
            Statement::EndIf(stmt) => stmt.position(),
            Statement::While(stmt) => stmt.position(),
            Statement::EndWhile(stmt) => stmt.position(),
            Statement::Break(stmt) => stmt.position(),
            Statement::Continue(stmt) => stmt.position(),
            Statement::Synchronous(stmt) => stmt.position(),
            Statement::EndSynchronous(stmt) => stmt.position(),
            Statement::Return(stmt) => stmt.position(),
            Statement::Assert(stmt) => stmt.position(),
            Statement::Goto(stmt) => stmt.position(),
            Statement::New(stmt) => stmt.position(),
            Statement::Expression(stmt) => stmt.position(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockStatement {
    pub this: BlockStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub statements: Vec<StatementId>,
    // Phase 2: linker
    pub parent_scope: Option<Scope>,
    pub relative_pos_in_parent: u32,
    pub locals: Vec<LocalId>,
    pub labels: Vec<LabeledStatementId>,
}

impl BlockStatement {
    pub fn parent_block(&self, h: &Heap) -> Option<BlockStatementId> {
        let parent = self.parent_scope.unwrap();
        match parent {
            Scope::Definition(_) => {
                // If the parent scope is a definition, then there is no
                // parent block.
                None
            }
            Scope::Synchronous((parent, _)) => {
                // It is always the case that when this function is called,
                // the parent of a synchronous statement is a block statement:
                // nested synchronous statements are flagged illegal,
                // and that happens before resolving variables that
                // creates the parent_scope references in the first place.
                Some(h[parent].parent_scope(h).unwrap().to_block())
            }
            Scope::Regular(parent) => {
                // A variable scope is either a definition, sync, or block.
                Some(parent)
            }
        }
    }
    pub fn first(&self) -> StatementId {
        // It is an invariant (guaranteed by the lexer) that block statements have at least one stmt
        *self.statements.first().unwrap()
    }
}

impl SyntaxElement for BlockStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

impl VariableScope for BlockStatement {
    fn parent_scope(&self, _h: &Heap) -> Option<Scope> {
        self.parent_scope.clone()
    }
    fn get_variable(&self, h: &Heap, id: &Identifier) -> Option<VariableId> {
        for local_id in self.locals.iter() {
            let local = &h[*local_id];
            if local.identifier == *id {
                return Some(local_id.0);
            }
        }
        None
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LocalStatement {
    Memory(MemoryStatement),
    Channel(ChannelStatement),
}

impl LocalStatement {
    pub fn this(&self) -> LocalStatementId {
        match self {
            LocalStatement::Memory(stmt) => stmt.this.upcast(),
            LocalStatement::Channel(stmt) => stmt.this.upcast(),
        }
    }
    pub fn as_memory(&self) -> &MemoryStatement {
        match self {
            LocalStatement::Memory(result) => result,
            _ => panic!("Unable to cast `LocalStatement` to `MemoryStatement`"),
        }
    }
    pub fn as_channel(&self) -> &ChannelStatement {
        match self {
            LocalStatement::Channel(result) => result,
            _ => panic!("Unable to cast `LocalStatement` to `ChannelStatement`"),
        }
    }
    pub fn next(&self) -> Option<StatementId> {
        match self {
            LocalStatement::Memory(stmt) => stmt.next,
            LocalStatement::Channel(stmt) => stmt.next,
        }
    }
}

impl SyntaxElement for LocalStatement {
    fn position(&self) -> InputPosition {
        match self {
            LocalStatement::Memory(stmt) => stmt.position(),
            LocalStatement::Channel(stmt) => stmt.position(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryStatement {
    pub this: MemoryStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub variable: LocalId,
    // Phase 2: linker
    pub next: Option<StatementId>,
}

impl SyntaxElement for MemoryStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

/// ChannelStatement is the declaration of an input and output port associated
/// with the same channel. Note that the polarity of the ports are from the
/// point of view of the component. So an output port is something that a
/// component uses to send data over (i.e. it is the "input end" of the
/// channel), and vice versa.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelStatement {
    pub this: ChannelStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub from: LocalId, // output
    pub to: LocalId,   // input
    // Phase 2: linker
    pub relative_pos_in_block: u32,
    pub next: Option<StatementId>,
}

impl SyntaxElement for ChannelStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkipStatement {
    pub this: SkipStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    // Phase 2: linker
    pub next: Option<StatementId>,
}

impl SyntaxElement for SkipStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LabeledStatement {
    pub this: LabeledStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub label: Identifier,
    pub body: StatementId,
    // Phase 2: linker
    pub relative_pos_in_block: u32,
    pub in_sync: Option<SynchronousStatementId>,
}

impl SyntaxElement for LabeledStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IfStatement {
    pub this: IfStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub test: ExpressionId,
    pub true_body: StatementId,
    pub false_body: StatementId,
    // Phase 2: linker
    pub end_if: Option<EndIfStatementId>,
}

impl SyntaxElement for IfStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndIfStatement {
    pub this: EndIfStatementId,
    // Phase 2: linker
    pub start_if: IfStatementId,
    pub position: InputPosition, // of corresponding if statement
    pub next: Option<StatementId>,
}

impl SyntaxElement for EndIfStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WhileStatement {
    pub this: WhileStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub test: ExpressionId,
    pub body: StatementId,
    // Phase 2: linker
    pub end_while: Option<EndWhileStatementId>,
    pub in_sync: Option<SynchronousStatementId>,
}

impl SyntaxElement for WhileStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndWhileStatement {
    pub this: EndWhileStatementId,
    // Phase 2: linker
    pub start_while: WhileStatementId,
    pub position: InputPosition, // of corresponding while
    pub next: Option<StatementId>,
}

impl SyntaxElement for EndWhileStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BreakStatement {
    pub this: BreakStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub label: Option<Identifier>,
    // Phase 2: linker
    pub target: Option<EndWhileStatementId>,
}

impl SyntaxElement for BreakStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContinueStatement {
    pub this: ContinueStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub label: Option<Identifier>,
    // Phase 2: linker
    pub target: Option<WhileStatementId>,
}

impl SyntaxElement for ContinueStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SynchronousStatement {
    pub this: SynchronousStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    // pub parameters: Vec<ParameterId>,
    pub body: StatementId,
    // Phase 2: linker
    pub end_sync: Option<EndSynchronousStatementId>,
    pub parent_scope: Option<Scope>,
}

impl SyntaxElement for SynchronousStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

impl VariableScope for SynchronousStatement {
    fn parent_scope(&self, _h: &Heap) -> Option<Scope> {
        self.parent_scope.clone()
    }
    fn get_variable(&self, _h: &Heap, _id: &Identifier) -> Option<VariableId> {
        // TODO: Another case of "where was this used for?"
        // for parameter_id in self.parameters.iter() {
        //     let parameter = &h[*parameter_id];
        //     if parameter.identifier.value == id.value {
        //         return Some(parameter_id.0);
        //     }
        // }
        None
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndSynchronousStatement {
    pub this: EndSynchronousStatementId,
    // Phase 2: linker
    pub position: InputPosition, // of corresponding sync statement
    pub start_sync: SynchronousStatementId,
    pub next: Option<StatementId>,
}

impl SyntaxElement for EndSynchronousStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReturnStatement {
    pub this: ReturnStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub expression: ExpressionId,
}

impl SyntaxElement for ReturnStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssertStatement {
    pub this: AssertStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub expression: ExpressionId,
    // Phase 2: linker
    pub next: Option<StatementId>,
}

impl SyntaxElement for AssertStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GotoStatement {
    pub this: GotoStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub label: Identifier,
    // Phase 2: linker
    pub target: Option<LabeledStatementId>,
}

impl SyntaxElement for GotoStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewStatement {
    pub this: NewStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub expression: CallExpressionId,
    // Phase 2: linker
    pub next: Option<StatementId>,
}

impl SyntaxElement for NewStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExpressionStatement {
    pub this: ExpressionStatementId,
    // Phase 1: parser
    pub position: InputPosition,
    pub expression: ExpressionId,
    // Phase 2: linker
    pub next: Option<StatementId>,
}

impl SyntaxElement for ExpressionStatement {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ExpressionParent {
    None, // only set during initial parsing
    If(IfStatementId),
    While(WhileStatementId),
    Return(ReturnStatementId),
    Assert(AssertStatementId),
    New(NewStatementId),
    ExpressionStmt(ExpressionStatementId),
    Expression(ExpressionId, u32) // index within expression (e.g LHS or RHS of expression)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Expression {
    Assignment(AssignmentExpression),
    Conditional(ConditionalExpression),
    Binary(BinaryExpression),
    Unary(UnaryExpression),
    Indexing(IndexingExpression),
    Slicing(SlicingExpression),
    Select(SelectExpression),
    Array(ArrayExpression),
    Literal(LiteralExpression),
    Call(CallExpression),
    Variable(VariableExpression),
}

impl Expression {
    pub fn as_assignment(&self) -> &AssignmentExpression {
        match self {
            Expression::Assignment(result) => result,
            _ => panic!("Unable to cast `Expression` to `AssignmentExpression`"),
        }
    }
    pub fn as_conditional(&self) -> &ConditionalExpression {
        match self {
            Expression::Conditional(result) => result,
            _ => panic!("Unable to cast `Expression` to `ConditionalExpression`"),
        }
    }
    pub fn as_binary(&self) -> &BinaryExpression {
        match self {
            Expression::Binary(result) => result,
            _ => panic!("Unable to cast `Expression` to `BinaryExpression`"),
        }
    }
    pub fn as_unary(&self) -> &UnaryExpression {
        match self {
            Expression::Unary(result) => result,
            _ => panic!("Unable to cast `Expression` to `UnaryExpression`"),
        }
    }
    pub fn as_indexing(&self) -> &IndexingExpression {
        match self {
            Expression::Indexing(result) => result,
            _ => panic!("Unable to cast `Expression` to `IndexingExpression`"),
        }
    }
    pub fn as_slicing(&self) -> &SlicingExpression {
        match self {
            Expression::Slicing(result) => result,
            _ => panic!("Unable to cast `Expression` to `SlicingExpression`"),
        }
    }
    pub fn as_select(&self) -> &SelectExpression {
        match self {
            Expression::Select(result) => result,
            _ => panic!("Unable to cast `Expression` to `SelectExpression`"),
        }
    }
    pub fn as_array(&self) -> &ArrayExpression {
        match self {
            Expression::Array(result) => result,
            _ => panic!("Unable to cast `Expression` to `ArrayExpression`"),
        }
    }
    pub fn as_constant(&self) -> &LiteralExpression {
        match self {
            Expression::Literal(result) => result,
            _ => panic!("Unable to cast `Expression` to `ConstantExpression`"),
        }
    }
    pub fn as_call(&self) -> &CallExpression {
        match self {
            Expression::Call(result) => result,
            _ => panic!("Unable to cast `Expression` to `CallExpression`"),
        }
    }
    pub fn as_call_mut(&mut self) -> &mut CallExpression {
        match self {
            Expression::Call(result) => result,
            _ => panic!("Unable to cast `Expression` to `CallExpression`"),
        }
    }
    pub fn as_variable(&self) -> &VariableExpression {
        match self {
            Expression::Variable(result) => result,
            _ => panic!("Unable to cast `Expression` to `VariableExpression`"),
        }
    }
    pub fn as_variable_mut(&mut self) -> &mut VariableExpression {
        match self {
            Expression::Variable(result) => result,
            _ => panic!("Unable to cast `Expression` to `VariableExpression`"),
        }
    }
    // TODO: @cleanup
    pub fn parent(&self) -> &ExpressionParent {
        match self {
            Expression::Assignment(expr) => &expr.parent,
            Expression::Conditional(expr) => &expr.parent,
            Expression::Binary(expr) => &expr.parent,
            Expression::Unary(expr) => &expr.parent,
            Expression::Indexing(expr) => &expr.parent,
            Expression::Slicing(expr) => &expr.parent,
            Expression::Select(expr) => &expr.parent,
            Expression::Array(expr) => &expr.parent,
            Expression::Literal(expr) => &expr.parent,
            Expression::Call(expr) => &expr.parent,
            Expression::Variable(expr) => &expr.parent,
        }
    }
    // TODO: @cleanup
    pub fn parent_expr_id(&self) -> Option<ExpressionId> {
        if let ExpressionParent::Expression(id, _) = self.parent() {
            Some(*id)
        } else {
            None
        }
    }
    // TODO: @cleanup
    pub fn set_parent(&mut self, parent: ExpressionParent) {
        match self {
            Expression::Assignment(expr) => expr.parent = parent,
            Expression::Conditional(expr) => expr.parent = parent,
            Expression::Binary(expr) => expr.parent = parent,
            Expression::Unary(expr) => expr.parent = parent,
            Expression::Indexing(expr) => expr.parent = parent,
            Expression::Slicing(expr) => expr.parent = parent,
            Expression::Select(expr) => expr.parent = parent,
            Expression::Array(expr) => expr.parent = parent,
            Expression::Literal(expr) => expr.parent = parent,
            Expression::Call(expr) => expr.parent = parent,
            Expression::Variable(expr) => expr.parent = parent,
        }
    }
    // TODO: @cleanup
    pub fn get_type_mut(&mut self) -> &mut ConcreteType {
        match self {
            Expression::Assignment(expr) => &mut expr.concrete_type,
            Expression::Conditional(expr) => &mut expr.concrete_type,
            Expression::Binary(expr) => &mut expr.concrete_type,
            Expression::Unary(expr) => &mut expr.concrete_type,
            Expression::Indexing(expr) => &mut expr.concrete_type,
            Expression::Slicing(expr) => &mut expr.concrete_type,
            Expression::Select(expr) => &mut expr.concrete_type,
            Expression::Array(expr) => &mut expr.concrete_type,
            Expression::Literal(expr) => &mut expr.concrete_type,
            Expression::Call(expr) => &mut expr.concrete_type,
            Expression::Variable(expr) => &mut expr.concrete_type,
        }
    }
}

impl SyntaxElement for Expression {
    fn position(&self) -> InputPosition {
        match self {
            Expression::Assignment(expr) => expr.position(),
            Expression::Conditional(expr) => expr.position(),
            Expression::Binary(expr) => expr.position(),
            Expression::Unary(expr) => expr.position(),
            Expression::Indexing(expr) => expr.position(),
            Expression::Slicing(expr) => expr.position(),
            Expression::Select(expr) => expr.position(),
            Expression::Array(expr) => expr.position(),
            Expression::Literal(expr) => expr.position(),
            Expression::Call(expr) => expr.position(),
            Expression::Variable(expr) => expr.position(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AssignmentOperator {
    Set,
    Multiplied,
    Divided,
    Remained,
    Added,
    Subtracted,
    ShiftedLeft,
    ShiftedRight,
    BitwiseAnded,
    BitwiseXored,
    BitwiseOred,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssignmentExpression {
    pub this: AssignmentExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub left: ExpressionId,
    pub operation: AssignmentOperator,
    pub right: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for AssignmentExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConditionalExpression {
    pub this: ConditionalExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub test: ExpressionId,
    pub true_expression: ExpressionId,
    pub false_expression: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for ConditionalExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BinaryOperator {
    Concatenate,
    LogicalOr,
    LogicalAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseAnd,
    Equality,
    Inequality,
    LessThan,
    GreaterThan,
    LessThanEqual,
    GreaterThanEqual,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BinaryExpression {
    pub this: BinaryExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub left: ExpressionId,
    pub operation: BinaryOperator,
    pub right: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for BinaryExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UnaryOperation {
    Positive,
    Negative,
    BitwiseNot,
    LogicalNot,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnaryExpression {
    pub this: UnaryExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub operation: UnaryOperation,
    pub expression: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for UnaryExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexingExpression {
    pub this: IndexingExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub subject: ExpressionId,
    pub index: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for IndexingExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SlicingExpression {
    pub this: SlicingExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub subject: ExpressionId,
    pub from_index: ExpressionId,
    pub to_index: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for SlicingExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SelectExpression {
    pub this: SelectExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub subject: ExpressionId,
    pub field: Field,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for SelectExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArrayExpression {
    pub this: ArrayExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub elements: Vec<ExpressionId>,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for ArrayExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallExpression {
    pub this: CallExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub method: Method,
    pub arguments: Vec<ExpressionId>,
    pub poly_args: Vec<ParserTypeId>,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for CallExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiteralExpression {
    pub this: LiteralExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub value: Literal,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for LiteralExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VariableExpression {
    pub this: VariableExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub identifier: NamespacedIdentifier,
    // Phase 2: linker
    pub declaration: Option<VariableId>,
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

impl SyntaxElement for VariableExpression {
    fn position(&self) -> InputPosition {
        self.position
    }
}

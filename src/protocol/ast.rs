// TODO: @cleanup, rigorous cleanup of dead code and silly object-oriented
//  trait impls where I deem them unfit.

use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Index, IndexMut};

use super::arena::{Arena, Id};
use crate::collections::StringRef;
use crate::protocol::inputsource::*;
use crate::protocol::input_source2::{InputPosition2, InputSpan};

/// Global limits to the AST, should be checked by lexer and parser. Some are
/// arbitrary
const MAX_LEVEL: usize = 128;
const MAX_NAMESPACES: usize = 64;

/// Helper macro that defines a type alias for a AST element ID. In this case 
/// only used to alias the `Id<T>` types.
macro_rules! define_aliased_ast_id {
    // Variant where we just defined the alias, without any indexing
    ($name:ident, $parent:ty) => {
        pub type $name = $parent;
    };
    // Variant where we define the type, and the Index and IndexMut traits
    (
        $name:ident, $parent:ty, 
        index($indexed_type:ty, $indexed_arena:ident)
    ) => {
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
    };
    // Variant where we define type, Index(Mut) traits and an allocation function
    (
        $name:ident, $parent:ty,
        index($indexed_type:ty, $indexed_arena:ident),
        alloc($fn_name:ident)
    ) => {
        define_aliased_ast_id!($name, $parent, index($indexed_type, $indexed_arena));
        impl Heap {
            pub fn $fn_name(&mut self, f: impl FnOnce($name) -> $indexed_type) -> $name {
                self.$indexed_arena.alloc_with_id(|id| f(id))
            }
        }
    };
}

/// Helper macro that defines a wrapper type for a particular variant of an AST
/// element ID. Only used to define single-wrapping IDs.
macro_rules! define_new_ast_id {
    // Variant where we just defined the new type, without any indexing
    ($name:ident, $parent:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name (pub(crate) $parent);

        impl $name {
            pub(crate) fn new_invalid() -> Self     { Self($parent::new_invalid()) }
            pub(crate) fn is_invalid(&self) -> bool { self.0.is_invalid() }
            pub fn upcast(self) -> $parent          { self.0 }
        }
    };
    // Variant where we define the type, and the Index and IndexMut traits
    (
        $name:ident, $parent:ty, 
        index($indexed_type:ty, $wrapper_type:path, $indexed_arena:ident)
    ) => {
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
    };
    // Variant where we define the type, the Index and IndexMut traits, and an allocation function
    (
        $name:ident, $parent:ty, 
        index($indexed_type:ty, $wrapper_type:path, $indexed_arena:ident),
        alloc($fn_name:ident)
    ) => {
        define_new_ast_id!($name, $parent, index($indexed_type, $wrapper_type, $indexed_arena));
        impl Heap {
            pub fn $fn_name(&mut self, f: impl FnOnce($name) -> $indexed_type) -> $name {
                $name(
                    self.$indexed_arena.alloc_with_id(|id| {
                        $wrapper_type(f($name(id)))
                    })
                )
            }
        }
    }
}

define_aliased_ast_id!(RootId, Id<Root>, index(Root, protocol_descriptions), alloc(alloc_protocol_description));
define_aliased_ast_id!(PragmaId, Id<Pragma>, index(Pragma, pragmas), alloc(alloc_pragma));
define_aliased_ast_id!(ImportId, Id<Import>, index(Import, imports), alloc(alloc_import));
define_aliased_ast_id!(ParserTypeId, Id<ParserType>, index(ParserType, parser_types), alloc(alloc_parser_type));

define_aliased_ast_id!(VariableId, Id<Variable>, index(Variable, variables));
define_new_ast_id!(ParameterId, VariableId, index(Parameter, Variable::Parameter, variables), alloc(alloc_parameter));
define_new_ast_id!(LocalId, VariableId, index(Local, Variable::Local, variables), alloc(alloc_local));

define_aliased_ast_id!(DefinitionId, Id<Definition>, index(Definition, definitions));
define_new_ast_id!(StructDefinitionId, DefinitionId, index(StructDefinition, Definition::Struct, definitions), alloc(alloc_struct_definition));
define_new_ast_id!(EnumDefinitionId, DefinitionId, index(EnumDefinition, Definition::Enum, definitions), alloc(alloc_enum_definition));
define_new_ast_id!(UnionDefinitionId, DefinitionId, index(UnionDefinition, Definition::Union, definitions), alloc(alloc_union_definition));
define_new_ast_id!(ComponentDefinitionId, DefinitionId, index(ComponentDefinition, Definition::Component, definitions), alloc(alloc_component_definition));
define_new_ast_id!(FunctionDefinitionId, DefinitionId, index(FunctionDefinition, Definition::Function, definitions), alloc(alloc_function_definition));

define_aliased_ast_id!(StatementId, Id<Statement>, index(Statement, statements));
define_new_ast_id!(BlockStatementId, StatementId, index(BlockStatement, Statement::Block, statements), alloc(alloc_block_statement));
define_new_ast_id!(LocalStatementId, StatementId, index(LocalStatement, Statement::Local, statements), alloc(alloc_local_statement));
define_new_ast_id!(MemoryStatementId, LocalStatementId);
define_new_ast_id!(ChannelStatementId, LocalStatementId);
define_new_ast_id!(SkipStatementId, StatementId, index(SkipStatement, Statement::Skip, statements), alloc(alloc_skip_statement));
define_new_ast_id!(LabeledStatementId, StatementId, index(LabeledStatement, Statement::Labeled, statements), alloc(alloc_labeled_statement));
define_new_ast_id!(IfStatementId, StatementId, index(IfStatement, Statement::If, statements), alloc(alloc_if_statement));
define_new_ast_id!(EndIfStatementId, StatementId, index(EndIfStatement, Statement::EndIf, statements), alloc(alloc_end_if_statement));
define_new_ast_id!(WhileStatementId, StatementId, index(WhileStatement, Statement::While, statements), alloc(alloc_while_statement));
define_new_ast_id!(EndWhileStatementId, StatementId, index(EndWhileStatement, Statement::EndWhile, statements), alloc(alloc_end_while_statement));
define_new_ast_id!(BreakStatementId, StatementId, index(BreakStatement, Statement::Break, statements), alloc(alloc_break_statement));
define_new_ast_id!(ContinueStatementId, StatementId, index(ContinueStatement, Statement::Continue, statements), alloc(alloc_continue_statement));
define_new_ast_id!(SynchronousStatementId, StatementId, index(SynchronousStatement, Statement::Synchronous, statements), alloc(alloc_synchronous_statement));
define_new_ast_id!(EndSynchronousStatementId, StatementId, index(EndSynchronousStatement, Statement::EndSynchronous, statements), alloc(alloc_end_synchronous_statement));
define_new_ast_id!(ReturnStatementId, StatementId, index(ReturnStatement, Statement::Return, statements), alloc(alloc_return_statement));
define_new_ast_id!(AssertStatementId, StatementId, index(AssertStatement, Statement::Assert, statements), alloc(alloc_assert_statement));
define_new_ast_id!(GotoStatementId, StatementId, index(GotoStatement, Statement::Goto, statements), alloc(alloc_goto_statement));
define_new_ast_id!(NewStatementId, StatementId, index(NewStatement, Statement::New, statements), alloc(alloc_new_statement));
define_new_ast_id!(ExpressionStatementId, StatementId, index(ExpressionStatement, Statement::Expression, statements), alloc(alloc_expression_statement));

define_aliased_ast_id!(ExpressionId, Id<Expression>, index(Expression, expressions));
define_new_ast_id!(AssignmentExpressionId, ExpressionId, index(AssignmentExpression, Expression::Assignment, expressions), alloc(alloc_assignment_expression));
define_new_ast_id!(BindingExpressionId, ExpressionId, index(BindingExpression, Expression::Binding, expressions), alloc(alloc_binding_expression));
define_new_ast_id!(ConditionalExpressionId, ExpressionId, index(ConditionalExpression, Expression::Conditional, expressions), alloc(alloc_conditional_expression));
define_new_ast_id!(BinaryExpressionId, ExpressionId, index(BinaryExpression, Expression::Binary, expressions), alloc(alloc_binary_expression));
define_new_ast_id!(UnaryExpressionId, ExpressionId, index(UnaryExpression, Expression::Unary, expressions), alloc(alloc_unary_expression));
define_new_ast_id!(IndexingExpressionId, ExpressionId, index(IndexingExpression, Expression::Indexing, expressions), alloc(alloc_indexing_expression));
define_new_ast_id!(SlicingExpressionId, ExpressionId, index(SlicingExpression, Expression::Slicing, expressions), alloc(alloc_slicing_expression));
define_new_ast_id!(SelectExpressionId, ExpressionId, index(SelectExpression, Expression::Select, expressions), alloc(alloc_select_expression));
define_new_ast_id!(ArrayExpressionId, ExpressionId, index(ArrayExpression, Expression::Array, expressions), alloc(alloc_array_expression));
define_new_ast_id!(LiteralExpressionId, ExpressionId, index(LiteralExpression, Expression::Literal, expressions), alloc(alloc_literal_expression));
define_new_ast_id!(CallExpressionId, ExpressionId, index(CallExpression, Expression::Call, expressions), alloc(alloc_call_expression));
define_new_ast_id!(VariableExpressionId, ExpressionId, index(VariableExpression, Expression::Variable, expressions), alloc(alloc_variable_expression));

// TODO: @cleanup - pub qualifiers can be removed once done
#[derive(Debug)]
pub struct Heap {
    // Root arena, contains the entry point for different modules. Each root
    // contains lists of IDs that correspond to the other arenas.
    pub(crate) protocol_descriptions: Arena<Root>,
    // Contents of a file, these are the elements the `Root` elements refer to
    pragmas: Arena<Pragma>,
    pub(crate) imports: Arena<Import>,
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
            parser_types: Arena::new(),
            variables: Arena::new(),
            definitions: Arena::new(),
            statements: Arena::new(),
            expressions: Arena::new(),
        }
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

#[derive(Debug, Clone)]
pub struct Root {
    pub this: RootId,
    // Phase 1: parser
    // pub position: InputPosition,
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

#[derive(Debug, Clone)]
pub enum Pragma {
    Version(PragmaVersion),
    Module(PragmaModule),
}

impl Pragma {
    pub(crate) fn as_module(&self) -> &PragmaModule {
        match self {
            Pragma::Module(pragma) => pragma,
            _ => unreachable!("Tried to obtain {:?} as PragmaModule", self),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PragmaVersion {
    pub this: PragmaId,
    // Phase 1: parser
    pub span: InputSpan, // of full pragma
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct PragmaModule {
    pub this: PragmaId,
    // Phase 1: parser
    pub span: InputSpan, // of full pragma
    pub value: Identifier,
}

#[derive(Debug, Clone)]
pub enum Import {
    Module(ImportModule),
    Symbols(ImportSymbols)
}

impl Import {
    pub(crate) fn as_module(&self) -> &ImportModule {
        match self {
            Import::Module(m) => m,
            _ => unreachable!("Unable to cast 'Import' to 'ImportModule'")
        }
    }
    pub(crate) fn as_symbols(&self) -> &ImportSymbols {
        match self {
            Import::Symbols(m) => m,
            _ => unreachable!("Unable to cast 'Import' to 'ImportSymbols'")
        }
    }
    pub(crate) fn as_symbols_mut(&mut self) -> &mut ImportSymbols {
        match self {
            Import::Symbols(m) => m,
            _ => unreachable!("Unable to cast 'Import' to 'ImportSymbols'")
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

#[derive(Debug, Clone)]
pub struct ImportModule {
    pub this: ImportId,
    // Phase 1: parser
    pub span: InputSpan,
    pub module: Identifier,
    pub alias: Identifier,
    pub module_id: RootId,
}

#[derive(Debug, Clone)]
pub struct AliasedSymbol {
    pub name: Identifier,
    pub alias: Option<Identifier>,
    pub definition_id: DefinitionId,
}

#[derive(Debug, Clone)]
pub struct ImportSymbols {
    pub this: ImportId,
    // Phase 1: parser
    pub span: InputSpan,
    pub module: Identifier,
    pub module_id: RootId,
    pub symbols: Vec<AliasedSymbol>,
}

#[derive(Debug, Clone)]
pub struct Identifier {
    pub span: InputSpan,
    pub value: StringRef<'static>,
}

impl PartialEq for Identifier {
    fn eq(&self, other: &Self) -> bool {
        return self.value == other.value
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // A source identifier is in ASCII range.
        write!(f, "{}", String::from_utf8_lossy(&self.value))
    }
}

#[derive(Debug, Clone)]
pub enum NamespacedIdentifierPart {
    // Regular identifier
    Identifier{start: u16, end: u16},
    // Polyargs associated with a preceding identifier
    PolyArgs{start: u16, end: u16},
}

impl NamespacedIdentifierPart {
    pub(crate) fn is_identifier(&self) -> bool {
        match self {
            NamespacedIdentifierPart::Identifier{..} => true,
            NamespacedIdentifierPart::PolyArgs{..} => false,
        }
    }

    pub(crate) fn as_identifier(&self) -> (u16, u16) {
        match self {
            NamespacedIdentifierPart::Identifier{start, end} => (*start, *end),
            NamespacedIdentifierPart::PolyArgs{..} => {
                unreachable!("Tried to obtain {:?} as Identifier", self);
            }
        }
    }

    pub(crate) fn as_poly_args(&self) -> (u16, u16) {
        match self {
            NamespacedIdentifierPart::PolyArgs{start, end} => (*start, *end),
            NamespacedIdentifierPart::Identifier{..} => {
                unreachable!("Tried to obtain {:?} as PolyArgs", self)
            }
        }
    }
}

/// An identifier with optional namespaces and polymorphic variables. Note that 
/// we allow each identifier to be followed by polymorphic arguments during the 
/// parsing phase (e.g. Foo<A,B>::Bar<C,D>::Qux). But in our current language 
/// implementation we can only have valid namespaced identifier that contain one
/// set of polymorphic arguments at the appropriate position.
/// TODO: @tokens Reimplement/rename once we have a tokenizer
#[derive(Debug, Clone)]
pub struct NamespacedIdentifier {
    pub position: InputPosition,
    pub value: Vec<u8>, // Full name as it resides in the input source
    pub poly_args: Vec<ParserTypeId>, // All poly args littered throughout the namespaced identifier
    pub parts: Vec<NamespacedIdentifierPart>, // Indices into value/poly_args
}

impl NamespacedIdentifier {
    /// Returns the identifier value without any of the specific polymorphic
    /// arguments.
    pub fn strip_poly_args(&self) -> Vec<u8> {
        debug_assert!(!self.parts.is_empty() && self.parts[0].is_identifier());

        let mut result = Vec::with_capacity(self.value.len());
        let mut iter = self.iter();
        let (first_ident, _) = iter.next().unwrap();
        result.extend(first_ident);

        for (ident, _) in iter.next() {
            result.push(b':');
            result.push(b':');
            result.extend(ident);
        }

        result
    }

    /// Returns an iterator of the elements in the namespaced identifier
    pub fn iter(&self) -> NamespacedIdentifierIter {
        return NamespacedIdentifierIter{
            identifier: self,
            element_idx: 0
        }
    }

    pub fn get_poly_args(&self) -> Option<&[ParserTypeId]> {
        let has_poly_args = self.parts.iter().any(|v| !v.is_identifier());
        if has_poly_args {
            Some(&self.poly_args)
        } else {
            None
        }
    }

    // Check if two namespaced identifiers match eachother when not considering
    // the polymorphic arguments
    pub fn matches_namespaced_identifier(&self, other: &Self) -> bool {
        let mut iter_self = self.iter();
        let mut iter_other = other.iter();

        loop {
            let val_self = iter_self.next();
            let val_other = iter_other.next();
            if val_self.is_some() != val_other.is_some() {
                // One is longer than the other
                return false;
            }
            if val_self.is_none() {
                // Both are none
                return true;
            }

            // Both are something
            let (val_self, _) = val_self.unwrap();
            let (val_other, _) = val_other.unwrap();
            if val_self != val_other { return false; }
        }
    }

    // Check if the namespaced identifier matches an identifier when not 
    // considering the polymorphic arguments
    pub fn matches_identifier(&self, other: &Identifier) -> bool {
        let mut iter = self.iter();
        let (first_ident, _) = iter.next().unwrap();
        if first_ident != other.value { 
            return false;
        }

        if iter.next().is_some() {
            return false;
        }

        return true;
    }
}

/// Iterator over elements of the namespaced identifier. The element index will
/// only ever be at the start of an identifier element.
#[derive(Debug)]
pub struct NamespacedIdentifierIter<'a> {
    identifier: &'a NamespacedIdentifier,
    element_idx: usize,
}

impl<'a> Iterator for NamespacedIdentifierIter<'a> {
    type Item = (&'a [u8], Option<&'a [ParserTypeId]>);
    fn next(&mut self) -> Option<Self::Item> {
        match self.get(self.element_idx) {
            Some((ident, poly)) => {
                self.element_idx += 1;
                if poly.is_some() {
                    self.element_idx += 1;
                }
                Some((ident, poly))
            },
            None => None
        }
    }
}

impl<'a> NamespacedIdentifierIter<'a> {
    /// Returns number of parts iterated over, may not correspond to number of
    /// times one called `next()` because returning an identifier with 
    /// polymorphic arguments increments the internal counter by 2.
    pub fn num_returned(&self) -> usize {
        return self.element_idx;
    }

    pub fn num_remaining(&self) -> usize {
        return self.identifier.parts.len() - self.element_idx;
    }

    pub fn returned_section(&self) -> &[u8] {
        if self.element_idx == 0 { return &self.identifier.value[0..0]; }

        let last_idx = match &self.identifier.parts[self.element_idx - 1] {
            NamespacedIdentifierPart::Identifier{end, ..} => *end,
            NamespacedIdentifierPart::PolyArgs{end, ..} => *end,
        };

        return &self.identifier.value[..last_idx as usize];
    }

    /// Returns a specific element from the namespaced identifier
    pub fn get(&self, idx: usize) -> Option<<Self as Iterator>::Item> {
        if idx >= self.identifier.parts.len() { 
            return None 
        }

        let cur_part = &self.identifier.parts[idx];
        let next_part = self.identifier.parts.get(idx + 1);

        let (ident_start, ident_end) = cur_part.as_identifier();
        let poly_slice = match next_part {
            Some(part) => match part {
                NamespacedIdentifierPart::Identifier{..} => None,
                NamespacedIdentifierPart::PolyArgs{start, end} => Some(
                    &self.identifier.poly_args[*start as usize..*end as usize]
                ),
            },
            None => None
        };

        Some((
            &self.identifier.value[ident_start as usize..ident_end as usize],
            poly_slice
        ))
    }

    /// Returns the previously returend index into the parts array of the 
    /// identifier.
    pub fn prev_idx(&self) -> Option<usize> {
        if self.element_idx == 0 { 
            return None;
        };
        
        if self.identifier.parts[self.element_idx - 1].is_identifier() { 
            return Some(self.element_idx - 1);
        }

        // Previous part had polymorphic arguments, so the one before that must
        // be an identifier (if well formed)
        debug_assert!(self.element_idx >= 2 && self.identifier.parts[self.element_idx - 2].is_identifier());
        return Some(self.element_idx - 2)
    }

    /// Returns the previously returned result from `next()`
    pub fn prev(&self) -> Option<<Self as Iterator>::Item> {
        match self.prev_idx() {
            None => None,
            Some(idx) => self.get(idx)
        }
    }
}

/// TODO: @types Remove the Message -> Byte hack at some point...
#[derive(Debug, Clone)]
pub enum ParserTypeVariant {
    // Basic builtin
    Message,
    Bool,
    UInt8, Uint16, UInt32, UInt64,
    SInt8, SInt16, SInt32, SInt64,
    Character, String,
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct SymbolicParserType {
    // Phase 1: parser
    pub identifier: NamespacedIdentifier,
    // Phase 2: validation/linking (for types in function/component bodies) and
    //  type table construction (for embedded types of structs/unions)
    pub poly_args2: Vec<ParserTypeId>, // taken from identifier or inferred
    pub variant: Option<SymbolicParserTypeVariant>
}

/// Specifies whether the symbolic type points to an actual user-defined type,
/// or whether it points to a polymorphic argument within the definition (e.g.
/// a defined variable `T var` within a function `int func<T>()`
#[derive(Debug, Clone)]
pub enum SymbolicParserTypeVariant {
    Definition(DefinitionId),
    // TODO: figure out if I need the DefinitionId here
    PolyArg(DefinitionId, usize), // index of polyarg in the definition
}

/// ConcreteType is the representation of a type after resolving symbolic types
/// and performing type inference
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
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

#[derive(Debug, Clone, Eq, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        }
        if self.array {
            write!(f, "[]")
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub enum Field {
    Length,
    Symbolic(FieldSymbolic),
}
impl Field {
    pub fn is_length(&self) -> bool {
        match self {
            Field::Length => true,
            _ => false,
        }
    }

    pub fn as_symbolic(&self) -> &FieldSymbolic {
        match self {
            Field::Symbolic(v) => v,
            _ => unreachable!("attempted to get Field::Symbolic from {:?}", self)
        }
    }
}

#[derive(Debug, Clone)]
pub struct FieldSymbolic {
    // Phase 1: Parser
    pub(crate) identifier: Identifier,
    // Phase 3: Typing
    pub(crate) definition: Option<DefinitionId>,
    pub(crate) field_idx: usize,
}

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub enum Definition {
    Struct(StructDefinition),
    Enum(EnumDefinition),
    Union(UnionDefinition),
    Component(ComponentDefinition),
    Function(FunctionDefinition),
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
    pub fn is_union(&self) -> bool {
        match self {
            Definition::Union(_) => true,
            _ => false,
        }
    }
    pub fn as_union(&self) -> &UnionDefinition {
        match self {
            Definition::Union(result) => result, 
            _ => panic!("Unable to cast 'Definition' to 'UnionDefinition'"),
        }
    }
    pub fn is_component(&self) -> bool {
        match self {
            Definition::Component(_) => true,
            _ => false,
        }
    }
    pub fn as_component(&self) -> &ComponentDefinition {
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
    pub fn as_function(&self) -> &FunctionDefinition {
        match self {
            Definition::Function(result) => result,
            _ => panic!("Unable to cast `Definition` to `Function`"),
        }
    }
    pub fn identifier(&self) -> &Identifier {
        match self {
            Definition::Struct(def) => &def.identifier,
            Definition::Enum(def) => &def.identifier,
            Definition::Union(def) => &def.identifier,
            Definition::Component(def) => &def.identifier,
            Definition::Function(def) => &def.identifier,
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
            Definition::Union(def) => def.position,
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

#[derive(Debug, Clone)]
pub struct StructFieldDefinition {
    pub field: Identifier,
    pub parser_type: ParserTypeId,
}

#[derive(Debug, Clone)]
pub struct StructDefinition {
    pub this: StructDefinitionId,
    // Phase 1: parser
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub fields: Vec<StructFieldDefinition>
}

impl StructDefinition {
    pub(crate) fn new_empty(this: StructDefinitionId, span: InputSpan, identifier: Identifier) -> Self {
        Self{ this, span, identifier, poly_vars: Vec::new(), fields: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub enum EnumVariantValue {
    None,
    Integer(i64),
}

#[derive(Debug, Clone)]
pub struct EnumVariantDefinition {
    pub position: InputPosition,
    pub identifier: Identifier,
    pub value: EnumVariantValue,
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub this: EnumDefinitionId,
    // Phase 1: parser
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub variants: Vec<EnumVariantDefinition>,
}

impl EnumDefinition {
    pub(crate) fn new_empty(this: EnumDefinitionId, span: InputSpan, identifier: Identifier) -> Self {
        Self{ this, span, identifier, poly_vars: Vec::new(), variants: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub enum UnionVariantValue {
    None,
    Embedded(Vec<ParserTypeId>),
}

#[derive(Debug, Clone)]
pub struct UnionVariantDefinition {
    pub position: InputPosition,
    pub identifier: Identifier,
    pub value: UnionVariantValue,
}

#[derive(Debug, Clone)]
pub struct UnionDefinition {
    pub this: UnionDefinitionId,
    // Phase 1: parser
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub variants: Vec<UnionVariantDefinition>,
}

impl UnionDefinition {
    pub(crate) fn new_empty(this: UnionDefinitionId, span: InputSpan, identifier: Identifier) -> Self {
        Self{ this, span, identifier, poly_vars: Vec::new(), variants: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ComponentVariant {
    Primitive,
    Composite,
}

#[derive(Debug, Clone)]
pub struct ComponentDefinition {
    pub this: ComponentDefinitionId,
    // Phase 1: parser
    pub span: InputSpan,
    pub variant: ComponentVariant,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub parameters: Vec<ParameterId>,
    pub body: StatementId,
}

impl ComponentDefinition {
    pub(crate) fn new_empty(this: ComponentDefinitionId, span: InputSpan, variant: ComponentVariant, identifier: Identifier) -> Self {
        Self{ 
            this, span, variant, identifier, 
            poly_vars: Vec::new(), 
            parameters: Vec::new(), 
            body: StatementId::new_invalid()
        }
    }
}

impl SyntaxElement for ComponentDefinition {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone)]
pub struct FunctionDefinition {
    pub this: FunctionDefinitionId,
    // Phase 1: parser
    pub span: InputSpan,
    pub return_type: ParserTypeId,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    pub parameters: Vec<ParameterId>,
    pub body: StatementId,
}

impl FunctionDefinition {
    pub(crate) fn new_empty(this: FunctionDefinitionId, span: InputSpan, identifier: Identifier) -> Self {
        Self {
            this, span, identifier,
            return_type: ParserTypeId::new_invalid(),
            poly_vars: Vec::new(),
            parameters: Vec::new(),
            body: StatementId::new_invalid(),
        }
    }
}

impl SyntaxElement for FunctionDefinition {
    fn position(&self) -> InputPosition {
        self.position
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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

#[derive(Debug, Clone)]
pub enum Expression {
    Assignment(AssignmentExpression),
    Binding(BindingExpression),
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
            Expression::Binding(expr) => &expr.parent,
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
            Expression::Binding(expr) => expr.parent = parent,
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
    pub fn get_type(&self) -> &ConcreteType {
        match self {
            Expression::Assignment(expr) => &expr.concrete_type,
            Expression::Binding(expr) => &expr.concrete_type,
            Expression::Conditional(expr) => &expr.concrete_type,
            Expression::Binary(expr) => &expr.concrete_type,
            Expression::Unary(expr) => &expr.concrete_type,
            Expression::Indexing(expr) => &expr.concrete_type,
            Expression::Slicing(expr) => &expr.concrete_type,
            Expression::Select(expr) => &expr.concrete_type,
            Expression::Array(expr) => &expr.concrete_type,
            Expression::Literal(expr) => &expr.concrete_type,
            Expression::Call(expr) => &expr.concrete_type,
            Expression::Variable(expr) => &expr.concrete_type,
        }
    }

    // TODO: @cleanup
    pub fn get_type_mut(&mut self) -> &mut ConcreteType {
        match self {
            Expression::Assignment(expr) => &mut expr.concrete_type,
            Expression::Binding(expr) => &mut expr.concrete_type,
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
            Expression::Binding(expr) => expr.position,
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct BindingExpression {
    pub this: BindingExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub left: LiteralExpressionId,
    pub right: ExpressionId,
    // Phase 2: linker
    pub parent: ExpressionParent,
    // Phase 3: type checking
    pub concrete_type: ConcreteType,
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

// TODO: @tokenizer Symbolic function calls are ambiguous with union literals
//  that accept embedded values (although the polymorphic arguments are placed
//  differently). To prevent double work we parse as CallExpression, and during
//  validation we may transform the expression into a union literal.
#[derive(Debug, Clone)]
pub struct CallExpression {
    pub this: CallExpressionId,
    // Phase 1: parser
    pub position: InputPosition,
    pub method: Method,
    pub arguments: Vec<ExpressionId>,
    pub poly_args: Vec<ParserTypeId>, // if symbolic will be determined during validation phase
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

#[derive(Debug, Clone)]
pub enum Method {
    Get,
    Put,
    Fires,
    Create,
    Symbolic(MethodSymbolic)
}

#[derive(Debug, Clone)]
pub struct MethodSymbolic {
    pub(crate) identifier: NamespacedIdentifier,
    pub(crate) definition: Option<DefinitionId>
}

#[derive(Debug, Clone)]
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

type LiteralCharacter = Vec<u8>;
type LiteralInteger = i64; // TODO: @int_literal

#[derive(Debug, Clone)]
pub enum Literal {
    Null, // message
    True,
    False,
    Character(LiteralCharacter),
    Integer(LiteralInteger),
    Struct(LiteralStruct),
    Enum(LiteralEnum),
    Union(LiteralUnion),
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

    pub(crate) fn as_enum(&self) -> &LiteralEnum {
        if let Literal::Enum(literal) = self {
            literal
        } else {
            unreachable!("Attempted to obtain {:?} as Literal::Enum", self)
        }
    }

    pub(crate) fn as_union(&self) -> &LiteralUnion {
        if let Literal::Union(literal) = self {
            literal
        } else {
            unreachable!("Attempted to obtain {:?} as Literal::Union", self)
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiteralStructField {
    // Phase 1: parser
    pub(crate) identifier: Identifier,
    pub(crate) value: ExpressionId,
    // Phase 2: linker
    pub(crate) field_idx: usize, // in struct definition
}

#[derive(Debug, Clone)]
pub struct LiteralStruct {
    // Phase 1: parser
    pub(crate) identifier: NamespacedIdentifier,
    pub(crate) fields: Vec<LiteralStructField>,
    // Phase 2: linker
    pub(crate) poly_args2: Vec<ParserTypeId>, // taken from identifier once linked to a definition
    pub(crate) definition: Option<DefinitionId>
}

// TODO: @tokenizer Enum literals are ambiguous with union literals that do not
//  accept embedded values. To prevent double work for now we parse as a 
//  LiteralEnum, and during validation we may transform the expression into a 
//  union literal.
#[derive(Debug, Clone)]
pub struct LiteralEnum {
    // Phase 1: parser
    pub(crate) identifier: NamespacedIdentifier,
    // Phase 2: linker
    pub(crate) poly_args2: Vec<ParserTypeId>, // taken from identifier once linked to a definition
    pub(crate) definition: Option<DefinitionId>,
    pub(crate) variant_idx: usize, // as present in the type table
}

#[derive(Debug, Clone)]
pub struct LiteralUnion {
    // Phase 1: parser
    pub(crate) identifier: NamespacedIdentifier,
    pub(crate) values: Vec<ExpressionId>,
    // Phase 2: linker
    pub(crate) poly_args2: Vec<ParserTypeId>, // taken from identifier once linked to a definition
    pub(crate) definition: Option<DefinitionId>,
    pub(crate) variant_idx: usize, // as present in type table
}

#[derive(Debug, Clone)]
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

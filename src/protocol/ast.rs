// TODO: @cleanup, rigorous cleanup of dead code and silly object-oriented
//  trait impls where I deem them unfit.

use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Index, IndexMut};

use super::arena::{Arena, Id};
use crate::collections::StringRef;
use crate::protocol::input_source::InputSpan;

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

        #[allow(dead_code)]
        impl $name {
            pub(crate) fn new_invalid() -> Self     { Self(<$parent>::new_invalid()) }
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
define_aliased_ast_id!(VariableId, Id<Variable>, index(Variable, variables), alloc(alloc_variable));

define_aliased_ast_id!(DefinitionId, Id<Definition>, index(Definition, definitions));
define_new_ast_id!(StructDefinitionId, DefinitionId, index(StructDefinition, Definition::Struct, definitions), alloc(alloc_struct_definition));
define_new_ast_id!(EnumDefinitionId, DefinitionId, index(EnumDefinition, Definition::Enum, definitions), alloc(alloc_enum_definition));
define_new_ast_id!(UnionDefinitionId, DefinitionId, index(UnionDefinition, Definition::Union, definitions), alloc(alloc_union_definition));
define_new_ast_id!(ComponentDefinitionId, DefinitionId, index(ComponentDefinition, Definition::Component, definitions), alloc(alloc_component_definition));
define_new_ast_id!(FunctionDefinitionId, DefinitionId, index(FunctionDefinition, Definition::Function, definitions), alloc(alloc_function_definition));

define_aliased_ast_id!(StatementId, Id<Statement>, index(Statement, statements));
define_new_ast_id!(BlockStatementId, StatementId, index(BlockStatement, Statement::Block, statements), alloc(alloc_block_statement));
define_new_ast_id!(EndBlockStatementId, StatementId, index(EndBlockStatement, Statement::EndBlock, statements), alloc(alloc_end_block_statement));
define_new_ast_id!(LocalStatementId, StatementId, index(LocalStatement, Statement::Local, statements), alloc(alloc_local_statement));
define_new_ast_id!(MemoryStatementId, LocalStatementId);
define_new_ast_id!(ChannelStatementId, LocalStatementId);
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
define_new_ast_id!(LiteralExpressionId, ExpressionId, index(LiteralExpression, Expression::Literal, expressions), alloc(alloc_literal_expression));
define_new_ast_id!(CastExpressionId, ExpressionId, index(CastExpression, Expression::Cast, expressions), alloc(alloc_cast_expression));
define_new_ast_id!(CallExpressionId, ExpressionId, index(CallExpression, Expression::Call, expressions), alloc(alloc_call_expression));
define_new_ast_id!(VariableExpressionId, ExpressionId, index(VariableExpression, Expression::Variable, expressions), alloc(alloc_variable_expression));

#[derive(Debug)]
pub struct Heap {
    // Root arena, contains the entry point for different modules. Each root
    // contains lists of IDs that correspond to the other arenas.
    pub(crate) protocol_descriptions: Arena<Root>,
    // Contents of a file, these are the elements the `Root` elements refer to
    pragmas: Arena<Pragma>,
    pub(crate) imports: Arena<Import>,
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
            if h[def].identifier().value.as_bytes() == id {
                return Some(def);
            }
        }
        None
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
    pub(crate) fn span(&self) -> InputSpan {
        match self {
            Import::Module(v) => v.span,
            Import::Symbols(v) => v.span,
        }
    }

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
        write!(f, "{}", self.value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserTypeVariant {
    // Special builtin, only usable by the compiler and not constructable by the
    // programmer
    Void,
    InputOrOutput,
    ArrayLike,
    IntegerLike,
    // Basic builtin
    Message,
    Bool,
    UInt8, UInt16, UInt32, UInt64,
    SInt8, SInt16, SInt32, SInt64,
    Character, String,
    // Literals (need to get concrete builtin type during typechecking)
    IntegerLiteral,
    // Marker for inference
    Inferred,
    // Builtins expecting one subsequent type
    Array,
    Input,
    Output,
    // User-defined types
    PolymorphicArgument(DefinitionId, u32), // u32 = index into polymorphic variables
    Definition(DefinitionId, u32), // u32 = number of subsequent types in the type tree.
}

impl ParserTypeVariant {
    pub(crate) fn num_embedded(&self) -> usize {
        use ParserTypeVariant::*;

        match self {
            Void | IntegerLike |
            Message | Bool |
            UInt8 | UInt16 | UInt32 | UInt64 |
            SInt8 | SInt16 | SInt32 | SInt64 |
            Character | String | IntegerLiteral |
            Inferred | PolymorphicArgument(_, _) =>
                0,
            ArrayLike | InputOrOutput | Array | Input | Output =>
                1,
            Definition(_, num) => *num as usize,
        }
    }
}

/// ParserTypeElement is an element of the type tree. An element may be
/// implicit, meaning that the user didn't specify the type, but it was set by
/// the compiler.
#[derive(Debug, Clone)]
pub struct ParserTypeElement {
    // TODO: @Fix span
    pub element_span: InputSpan, // span of this element, not including the child types
    pub variant: ParserTypeVariant,
}

/// ParserType is a specification of a type during the parsing phase and initial
/// linker/validator phase of the compilation process. These types may be
/// (partially) inferred or represent literals (e.g. a integer whose bytesize is
/// not yet determined).
///
/// Its contents are the depth-first serialization of the type tree. Each node
/// is a type that may accept polymorphic arguments. The polymorphic arguments
/// are then the children of the node.
#[derive(Debug, Clone)]
pub struct ParserType {
    pub elements: Vec<ParserTypeElement>,
    pub full_span: InputSpan,
}

impl ParserType {
    pub(crate) fn iter_embedded(&self, parent_idx: usize) -> ParserTypeIter {
        ParserTypeIter::new(&self.elements, parent_idx)
    }
}

/// Iterator over the embedded elements of a specific element.
pub struct ParserTypeIter<'a> {
    pub elements: &'a [ParserTypeElement],
    pub cur_embedded_idx: usize,
}

impl<'a> ParserTypeIter<'a> {
    fn new(elements: &'a [ParserTypeElement], parent_idx: usize) -> Self {
        debug_assert!(parent_idx < elements.len(), "parent index exceeds number of elements in ParserType");
        if elements[0].variant.num_embedded() == 0 {
            // Parent element does not have any embedded types, place
            // `cur_embedded_idx` at end so we will always return `None`
            Self{ elements, cur_embedded_idx: elements.len() }
        } else {
            // Parent element has an embedded type
            Self{ elements, cur_embedded_idx: parent_idx + 1 }
        }
    }
}

impl<'a> Iterator for ParserTypeIter<'a> {
    type Item = &'a [ParserTypeElement];

    fn next(&mut self) -> Option<Self::Item> {
        let elements_len = self.elements.len();
        if self.cur_embedded_idx >= elements_len {
            return None;
        }

        // Seek to the end of the subtree
        let mut depth = 1;
        let start_element = self.cur_embedded_idx;
        while self.cur_embedded_idx < elements_len {
            let cur_element = &self.elements[self.cur_embedded_idx];
            let depth_change = cur_element.variant.num_embedded() as i32 - 1;
            depth += depth_change;
            debug_assert!(depth >= 0, "illegally constructed ParserType: {:?}", self.elements);

            self.cur_embedded_idx += 1;
            if depth == 0 {
                break;
            }
        }

        debug_assert!(depth == 0, "illegally constructed ParserType: {:?}", self.elements);
        return Some(&self.elements[start_element..self.cur_embedded_idx]);
    }
}

/// ConcreteType is the representation of a type after the type inference and
/// checker is finished. These are fully typed.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ConcreteTypePart {
    // Special types (cannot be explicitly constructed by the programmer)
    Void,
    // Builtin types without nested types
    Message,
    Bool,
    UInt8, UInt16, UInt32, UInt64,
    SInt8, SInt16, SInt32, SInt64,
    Character, String,
    // Builtin types with one nested type
    Array,
    Slice,
    Input,
    Output,
    // User defined type with any number of nested types
    Instance(DefinitionId, u32),
}

impl ConcreteTypePart {
    fn num_embedded(&self) -> u32 {
        use ConcreteTypePart::*;

        match self {
            Void | Message | Bool |
            UInt8 | UInt16 | UInt32 | UInt64 |
            SInt8 | SInt16 | SInt32 | SInt64 |
            Character | String =>
                0,
            Array | Slice | Input | Output =>
                1,
            Instance(_, num_embedded) => *num_embedded
        }
    }
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
    /// Returns an iterator over the subtrees that are type arguments (e.g. an
    /// array element's type, or a polymorphic type's arguments) to the
    /// provided parent type (specified by its index in the `parts` array).
    pub(crate) fn embedded_iter<'a>(&'a self, parent_part_idx: usize) -> ConcreteTypeIter<'a> {
        let num_embedded = self.parts[parent_part_idx].num_embedded();
        return ConcreteTypeIter{
            concrete: self,
            idx_embedded: 0,
            num_embedded,
            part_idx: parent_part_idx + 1,
        }
    }

    /// Given the starting position of a type tree, determine the exclusive
    /// ending index.
    pub(crate) fn subtree_end_idx(&self, start_idx: usize) -> usize {
        let mut depth = 1;
        let num_parts = self.parts.len();
        debug_assert!(start_idx < num_parts);

        for part_idx in start_idx..self.parts.len() {
            let depth_change = self.parts[part_idx].num_embedded() as i32 - 1;
            depth += depth_change;
            debug_assert!(depth >= 0);

            if depth == 0 {
                return part_idx + 1;
            }
        }

        debug_assert!(false, "incorrectly constructed ConcreteType instance");
        return 0;
    }

    /// Construct a human-readable name for the type. Because this performs
    /// a string allocation don't use it for anything else then displaying the
    /// type to the user.
    pub(crate) fn display_name(&self, heap: &Heap) -> String {
        fn display_part(parts: &[ConcreteTypePart], heap: &Heap, mut idx: usize, target: &mut String) -> usize {
            use ConcreteTypePart as CTP;
            use crate::protocol::parser::token_parsing::*;

            let cur_idx = idx;
            idx += 1; // increment by 1, because it always happens

            match parts[cur_idx] {
                CTP::Void => { target.push_str("void"); },
                CTP::Message => { target.push_str(KW_TYPE_MESSAGE_STR); },
                CTP::Bool => { target.push_str(KW_TYPE_BOOL_STR); },
                CTP::UInt8 => { target.push_str(KW_TYPE_UINT8_STR); },
                CTP::UInt16 => { target.push_str(KW_TYPE_UINT16_STR); },
                CTP::UInt32 => { target.push_str(KW_TYPE_UINT32_STR); },
                CTP::UInt64 => { target.push_str(KW_TYPE_UINT64_STR); },
                CTP::SInt8 => { target.push_str(KW_TYPE_SINT8_STR); },
                CTP::SInt16 => { target.push_str(KW_TYPE_SINT16_STR); },
                CTP::SInt32 => { target.push_str(KW_TYPE_SINT32_STR); },
                CTP::SInt64 => { target.push_str(KW_TYPE_SINT64_STR); },
                CTP::Character => { target.push_str(KW_TYPE_CHAR_STR); },
                CTP::String => { target.push_str(KW_TYPE_STRING_STR); },
                CTP::Array | CTP::Slice => {
                    idx = display_part(parts, heap, idx, target);
                    target.push_str("[]");
                },
                CTP::Input => {
                    target.push_str(KW_TYPE_IN_PORT_STR);
                    target.push('<');
                    idx = display_part(parts, heap, idx, target);
                    target.push('>');
                },
                CTP::Output => {
                    target.push_str(KW_TYPE_OUT_PORT_STR);
                    target.push('<');
                    idx = display_part(parts, heap, idx, target);
                    target.push('>');
                },
                CTP::Instance(definition_id, num_poly_args) => {
                    let definition = &heap[definition_id];
                    target.push_str(definition.identifier().value.as_str());

                    if num_poly_args != 0 {
                        target.push('<');
                        for poly_arg_idx in 0..num_poly_args {
                            if poly_arg_idx != 0 {
                                target.push(',');
                                idx = display_part(parts, heap, idx, target);
                            }
                        }
                        target.push('>');
                    }
                }
            }

            idx
        }

        let mut name = String::with_capacity(128);
        let _final_idx = display_part(&self.parts, heap, 0, &mut name);
        debug_assert_eq!(_final_idx, self.parts.len());

        return name;
    }
}

#[derive(Debug)]
pub struct ConcreteTypeIter<'a> {
    concrete: &'a ConcreteType,
    idx_embedded: u32,
    num_embedded: u32,
    part_idx: usize,
}

impl<'a> Iterator for ConcreteTypeIter<'a> {
    type Item = &'a [ConcreteTypePart];

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx_embedded == self.num_embedded {
            return None;
        }

        // Retrieve the subtree of interest
        let start_idx = self.part_idx;
        let end_idx = self.concrete.subtree_end_idx(start_idx);

        self.idx_embedded += 1;
        self.part_idx = end_idx;

        return Some(&self.concrete.parts[start_idx..end_idx]);
    }
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

/// `ScopeNode` is a helper that links scopes in two directions. It doesn't
/// actually contain any information associated with the scope, this may be
/// found on the AST elements that `Scope` points to.
#[derive(Debug, Clone)]
pub struct ScopeNode {
    pub parent: Scope,
    pub nested: Vec<Scope>,
}

impl ScopeNode {
    pub(crate) fn new_invalid() -> Self {
        ScopeNode{
            parent: Scope::Definition(DefinitionId::new_invalid()),
            nested: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariableKind {
    Parameter,      // in parameter list of function/component
    Local,          // declared in function/component body
    Binding,        // may be bound to in a binding expression (determined in validator/linker)
}

#[derive(Debug, Clone)]
pub struct Variable {
    pub this: VariableId,
    // Parsing
    pub kind: VariableKind,
    pub parser_type: ParserType,
    pub identifier: Identifier,
    // Validator/linker
    pub relative_pos_in_block: u32,
    pub unique_id_in_scope: i32, // Temporary fix until proper bytecode/asm is generated
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
    pub(crate) fn as_struct(&self) -> &StructDefinition {
        match self {
            Definition::Struct(result) => result,
            _ => panic!("Unable to cast 'Definition' to 'StructDefinition'"),
        }
    }
    pub(crate) fn as_struct_mut(&mut self) -> &mut StructDefinition {
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
    pub(crate) fn as_enum(&self) -> &EnumDefinition {
        match self {
            Definition::Enum(result) => result,
            _ => panic!("Unable to cast 'Definition' to 'EnumDefinition'"),
        }
    }
    pub(crate) fn as_enum_mut(&mut self) -> &mut EnumDefinition {
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
    pub(crate) fn as_union(&self) -> &UnionDefinition {
        match self {
            Definition::Union(result) => result, 
            _ => panic!("Unable to cast 'Definition' to 'UnionDefinition'"),
        }
    }
    pub(crate) fn as_union_mut(&mut self) -> &mut UnionDefinition {
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
    pub(crate) fn as_component(&self) -> &ComponentDefinition {
        match self {
            Definition::Component(result) => result,
            _ => panic!("Unable to cast `Definition` to `Component`"),
        }
    }
    pub(crate) fn as_component_mut(&mut self) -> &mut ComponentDefinition {
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
    pub(crate) fn as_function(&self) -> &FunctionDefinition {
        match self {
            Definition::Function(result) => result,
            _ => panic!("Unable to cast `Definition` to `Function`"),
        }
    }
    pub(crate) fn as_function_mut(&mut self) -> &mut FunctionDefinition {
        match self {
            Definition::Function(result) => result,
            _ => panic!("Unable to cast `Definition` to `Function`"),
        }
    }
    pub fn parameters(&self) -> &Vec<VariableId> {
        match self {
            Definition::Component(def) => &def.parameters,
            Definition::Function(def) => &def.parameters,
            _ => panic!("Called parameters() on {:?}", self)
        }
    }
    pub fn defined_in(&self) -> RootId {
        match self {
            Definition::Struct(def) => def.defined_in,
            Definition::Enum(def) => def.defined_in,
            Definition::Union(def) => def.defined_in,
            Definition::Component(def) => def.defined_in,
            Definition::Function(def) => def.defined_in,
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
    pub fn poly_vars(&self) -> &Vec<Identifier> {
        match self {
            Definition::Struct(def) => &def.poly_vars,
            Definition::Enum(def) => &def.poly_vars,
            Definition::Union(def) => &def.poly_vars,
            Definition::Component(def) => &def.poly_vars,
            Definition::Function(def) => &def.poly_vars,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructFieldDefinition {
    pub span: InputSpan,
    pub field: Identifier,
    pub parser_type: ParserType,
}

#[derive(Debug, Clone)]
pub struct StructDefinition {
    pub this: StructDefinitionId,
    pub defined_in: RootId,
    // Symbol scanning
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    // Parsing
    pub fields: Vec<StructFieldDefinition>
}

impl StructDefinition {
    pub(crate) fn new_empty(
        this: StructDefinitionId, defined_in: RootId, span: InputSpan,
        identifier: Identifier, poly_vars: Vec<Identifier>
    ) -> Self {
        Self{ this, defined_in, span, identifier, poly_vars, fields: Vec::new() }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EnumVariantValue {
    None,
    Integer(i64),
}

#[derive(Debug, Clone)]
pub struct EnumVariantDefinition {
    pub identifier: Identifier,
    pub value: EnumVariantValue,
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub this: EnumDefinitionId,
    pub defined_in: RootId,
    // Symbol scanning
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    // Parsing
    pub variants: Vec<EnumVariantDefinition>,
}

impl EnumDefinition {
    pub(crate) fn new_empty(
        this: EnumDefinitionId, defined_in: RootId, span: InputSpan,
        identifier: Identifier, poly_vars: Vec<Identifier>
    ) -> Self {
        Self{ this, defined_in, span, identifier, poly_vars, variants: Vec::new() }
    }
}

#[derive(Debug, Clone)]
pub struct UnionVariantDefinition {
    pub span: InputSpan,
    pub identifier: Identifier,
    pub value: Vec<ParserType>, // if empty, then union variant does not contain any embedded types
}

#[derive(Debug, Clone)]
pub struct UnionDefinition {
    pub this: UnionDefinitionId,
    pub defined_in: RootId,
    // Phase 1: symbol scanning
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    // Phase 2: parsing
    pub variants: Vec<UnionVariantDefinition>,
}

impl UnionDefinition {
    pub(crate) fn new_empty(
        this: UnionDefinitionId, defined_in: RootId, span: InputSpan,
        identifier: Identifier, poly_vars: Vec<Identifier>
    ) -> Self {
        Self{ this, defined_in, span, identifier, poly_vars, variants: Vec::new() }
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
    pub defined_in: RootId,
    // Symbol scanning
    pub span: InputSpan,
    pub variant: ComponentVariant,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    // Parsing
    pub parameters: Vec<VariableId>,
    pub body: BlockStatementId,
    // Validation/linking
    pub num_expressions_in_body: i32,
}

impl ComponentDefinition {
    // Used for preallocation during symbol scanning
    pub(crate) fn new_empty(
        this: ComponentDefinitionId, defined_in: RootId, span: InputSpan,
        variant: ComponentVariant, identifier: Identifier, poly_vars: Vec<Identifier>
    ) -> Self {
        Self{ 
            this, defined_in, span, variant, identifier, poly_vars,
            parameters: Vec::new(), 
            body: BlockStatementId::new_invalid(),
            num_expressions_in_body: -1,
        }
    }
}

// Note that we will have function definitions for builtin functions as well. In
// that case the span, the identifier span and the body are all invalid.
#[derive(Debug, Clone)]
pub struct FunctionDefinition {
    pub this: FunctionDefinitionId,
    pub defined_in: RootId,
    // Symbol scanning
    pub builtin: bool,
    pub span: InputSpan,
    pub identifier: Identifier,
    pub poly_vars: Vec<Identifier>,
    // Parser
    pub return_types: Vec<ParserType>,
    pub parameters: Vec<VariableId>,
    pub body: BlockStatementId,
    // Validation/linking
    pub num_expressions_in_body: i32,
}

impl FunctionDefinition {
    pub(crate) fn new_empty(
        this: FunctionDefinitionId, defined_in: RootId, span: InputSpan,
        identifier: Identifier, poly_vars: Vec<Identifier>
    ) -> Self {
        Self {
            this, defined_in,
            builtin: false,
            span, identifier, poly_vars,
            return_types: Vec::new(),
            parameters: Vec::new(),
            body: BlockStatementId::new_invalid(),
            num_expressions_in_body: -1,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Statement {
    Block(BlockStatement),
    EndBlock(EndBlockStatement),
    Local(LocalStatement),
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

    pub fn as_new(&self) -> &NewStatement {
        match self {
            Statement::New(result) => result,
            _ => panic!("Unable to cast `Statement` to `NewStatement`"),
        }
    }

    pub fn span(&self) -> InputSpan {
        match self {
            Statement::Block(v) => v.span,
            Statement::Local(v) => v.span(),
            Statement::Labeled(v) => v.label.span,
            Statement::If(v) => v.span,
            Statement::While(v) => v.span,
            Statement::Break(v) => v.span,
            Statement::Continue(v) => v.span,
            Statement::Synchronous(v) => v.span,
            Statement::Return(v) => v.span,
            Statement::Goto(v) => v.span,
            Statement::New(v) => v.span,
            Statement::Expression(v) => v.span,
            Statement::EndBlock(_) | Statement::EndIf(_) | Statement::EndWhile(_) | Statement::EndSynchronous(_) => unreachable!(),
        }
    }
    pub fn link_next(&mut self, next: StatementId) {
        match self {
            Statement::Block(stmt) => stmt.next = next,
            Statement::EndBlock(stmt) => stmt.next = next,
            Statement::Local(stmt) => match stmt {
                LocalStatement::Channel(stmt) => stmt.next = next,
                LocalStatement::Memory(stmt) => stmt.next = next,
            },
            Statement::EndIf(stmt) => stmt.next = next,
            Statement::EndWhile(stmt) => stmt.next = next,
            Statement::EndSynchronous(stmt) => stmt.next = next,
            Statement::New(stmt) => stmt.next = next,
            Statement::Expression(stmt) => stmt.next = next,
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

#[derive(Debug, Clone)]
pub struct BlockStatement {
    pub this: BlockStatementId,
    // Phase 1: parser
    pub is_implicit: bool,
    pub span: InputSpan, // of the complete block
    pub statements: Vec<StatementId>,
    pub end_block: EndBlockStatementId,
    // Phase 2: linker
    pub scope_node: ScopeNode,
    pub first_unique_id_in_scope: i32, // Temporary fix until proper bytecode/asm is generated
    pub next_unique_id_in_scope: i32, // Temporary fix until proper bytecode/asm is generated
    pub relative_pos_in_parent: u32,
    pub locals: Vec<VariableId>,
    pub labels: Vec<LabeledStatementId>,
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct EndBlockStatement {
    pub this: EndBlockStatementId,
    // Parser
    pub start_block: BlockStatementId,
    // Validation/Linking
    pub next: StatementId,
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
    pub fn span(&self) -> InputSpan {
        match self {
            LocalStatement::Channel(v) => v.span,
            LocalStatement::Memory(v) => v.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemoryStatement {
    pub this: MemoryStatementId,
    // Phase 1: parser
    pub span: InputSpan,
    pub variable: VariableId,
    // Phase 2: linker
    pub next: StatementId,
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
    pub span: InputSpan, // of the "channel" keyword
    pub from: VariableId, // output
    pub to: VariableId,   // input
    // Phase 2: linker
    pub relative_pos_in_block: u32,
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct LabeledStatement {
    pub this: LabeledStatementId,
    // Phase 1: parser
    pub label: Identifier,
    pub body: StatementId,
    // Phase 2: linker
    pub relative_pos_in_block: u32,
    pub in_sync: SynchronousStatementId, // may be invalid
}

#[derive(Debug, Clone)]
pub struct IfStatement {
    pub this: IfStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "if" keyword
    pub test: ExpressionId,
    pub true_body: BlockStatementId,
    pub false_body: Option<BlockStatementId>,
    pub end_if: EndIfStatementId,
}

#[derive(Debug, Clone)]
pub struct EndIfStatement {
    pub this: EndIfStatementId,
    pub start_if: IfStatementId,
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct WhileStatement {
    pub this: WhileStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "while" keyword
    pub test: ExpressionId,
    pub body: BlockStatementId,
    pub end_while: EndWhileStatementId,
    pub in_sync: SynchronousStatementId, // may be invalid
}

#[derive(Debug, Clone)]
pub struct EndWhileStatement {
    pub this: EndWhileStatementId,
    pub start_while: WhileStatementId,
    // Phase 2: linker
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct BreakStatement {
    pub this: BreakStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "break" keyword
    pub label: Option<Identifier>,
    // Phase 2: linker
    pub target: Option<EndWhileStatementId>,
}

#[derive(Debug, Clone)]
pub struct ContinueStatement {
    pub this: ContinueStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "continue" keyword
    pub label: Option<Identifier>,
    // Phase 2: linker
    pub target: Option<WhileStatementId>,
}

#[derive(Debug, Clone)]
pub struct SynchronousStatement {
    pub this: SynchronousStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "sync" keyword
    pub body: BlockStatementId,
    // Phase 2: linker
    pub end_sync: EndSynchronousStatementId,
}

#[derive(Debug, Clone)]
pub struct EndSynchronousStatement {
    pub this: EndSynchronousStatementId,
    pub start_sync: SynchronousStatementId,
    // Phase 2: linker
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct ReturnStatement {
    pub this: ReturnStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "return" keyword
    pub expressions: Vec<ExpressionId>,
}

#[derive(Debug, Clone)]
pub struct GotoStatement {
    pub this: GotoStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "goto" keyword
    pub label: Identifier,
    // Phase 2: linker
    pub target: Option<LabeledStatementId>,
}

#[derive(Debug, Clone)]
pub struct NewStatement {
    pub this: NewStatementId,
    // Phase 1: parser
    pub span: InputSpan, // of the "new" keyword
    pub expression: CallExpressionId,
    // Phase 2: linker
    pub next: StatementId,
}

#[derive(Debug, Clone)]
pub struct ExpressionStatement {
    pub this: ExpressionStatementId,
    // Phase 1: parser
    pub span: InputSpan,
    pub expression: ExpressionId,
    // Phase 2: linker
    pub next: StatementId,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ExpressionParent {
    None, // only set during initial parsing
    If(IfStatementId),
    While(WhileStatementId),
    Return(ReturnStatementId),
    New(NewStatementId),
    ExpressionStmt(ExpressionStatementId),
    Expression(ExpressionId, u32) // index within expression (e.g LHS or RHS of expression)
}

impl ExpressionParent {
    pub fn is_new(&self) -> bool {
        match self {
            ExpressionParent::New(_) => true,
            _ => false,
        }
    }

    pub fn as_expression(&self) -> ExpressionId {
        match self {
            ExpressionParent::Expression(id, _) => *id,
            _ => panic!("called as_expression() on {:?}", self),
        }
    }
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
    Literal(LiteralExpression),
    Cast(CastExpression),
    Call(CallExpression),
    Variable(VariableExpression),
}

impl Expression {
    pub fn as_variable(&self) -> &VariableExpression {
        match self {
            Expression::Variable(result) => result,
            _ => panic!("Unable to cast `Expression` to `VariableExpression`"),
        }
    }

    /// Returns operator span, function name, a binding's "let" span, etc. An
    /// indicator for the kind of expression that is being applied.
    pub fn operation_span(&self) -> InputSpan {
        match self {
            Expression::Assignment(expr) => expr.operator_span,
            Expression::Binding(expr) => expr.operator_span,
            Expression::Conditional(expr) => expr.operator_span,
            Expression::Binary(expr) => expr.operator_span,
            Expression::Unary(expr) => expr.operator_span,
            Expression::Indexing(expr) => expr.operator_span,
            Expression::Slicing(expr) => expr.slicing_span,
            Expression::Select(expr) => expr.operator_span,
            Expression::Literal(expr) => expr.span,
            Expression::Cast(expr) => expr.cast_span,
            Expression::Call(expr) => expr.func_span,
            Expression::Variable(expr) => expr.identifier.span,
        }
    }

    /// Returns the span covering the entire expression (i.e. including the
    /// spans of the arguments as well).
    pub fn full_span(&self) -> InputSpan {
        match self {
            Expression::Assignment(expr) => expr.full_span,
            Expression::Binding(expr) => expr.full_span,
            Expression::Conditional(expr) => expr.full_span,
            Expression::Binary(expr) => expr.full_span,
            Expression::Unary(expr) => expr.full_span,
            Expression::Indexing(expr) => expr.full_span,
            Expression::Slicing(expr) => expr.full_span,
            Expression::Select(expr) => expr.full_span,
            Expression::Literal(expr) => expr.span,
            Expression::Cast(expr) => expr.full_span,
            Expression::Call(expr) => expr.full_span,
            Expression::Variable(expr) => expr.identifier.span,
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
            Expression::Literal(expr) => &expr.parent,
            Expression::Cast(expr) => &expr.parent,
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

    pub fn get_unique_id_in_definition(&self) -> i32 {
        match self {
            Expression::Assignment(expr) => expr.unique_id_in_definition,
            Expression::Binding(expr) => expr.unique_id_in_definition,
            Expression::Conditional(expr) => expr.unique_id_in_definition,
            Expression::Binary(expr) => expr.unique_id_in_definition,
            Expression::Unary(expr) => expr.unique_id_in_definition,
            Expression::Indexing(expr) => expr.unique_id_in_definition,
            Expression::Slicing(expr) => expr.unique_id_in_definition,
            Expression::Select(expr) => expr.unique_id_in_definition,
            Expression::Literal(expr) => expr.unique_id_in_definition,
            Expression::Cast(expr) => expr.unique_id_in_definition,
            Expression::Call(expr) => expr.unique_id_in_definition,
            Expression::Variable(expr) => expr.unique_id_in_definition,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AssignmentOperator {
    Set,
    Concatenated,
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
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub left: ExpressionId,
    pub operation: AssignmentOperator,
    pub right: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct BindingExpression {
    pub this: BindingExpressionId,
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub bound_to: ExpressionId,
    pub bound_from: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct ConditionalExpression {
    pub this: ConditionalExpressionId,
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub test: ExpressionId,
    pub true_expression: ExpressionId,
    pub false_expression: ExpressionId,
    // Validator/Linking
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub left: ExpressionId,
    pub operation: BinaryOperator,
    pub right: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Positive,
    Negative,
    BitwiseNot,
    LogicalNot,
}

#[derive(Debug, Clone)]
pub struct UnaryExpression {
    pub this: UnaryExpressionId,
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub operation: UnaryOperator,
    pub expression: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct IndexingExpression {
    pub this: IndexingExpressionId,
    // Parsing
    pub operator_span: InputSpan,
    pub full_span: InputSpan,
    pub subject: ExpressionId,
    pub index: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct SlicingExpression {
    pub this: SlicingExpressionId,
    // Parsing
    pub slicing_span: InputSpan, // from '[' to ']'
    pub full_span: InputSpan, // includes subject
    pub subject: ExpressionId,
    pub from_index: ExpressionId,
    pub to_index: ExpressionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct SelectExpression {
    pub this: SelectExpressionId,
    // Parsing
    pub operator_span: InputSpan, // of the '.'
    pub full_span: InputSpan, // includes subject and field
    pub subject: ExpressionId,
    pub field_name: Identifier,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct CastExpression {
    pub this: CastExpressionId,
    // Parsing
    pub cast_span: InputSpan, // of the "cast" keyword,
    pub full_span: InputSpan, // includes the cast subject
    pub to_type: ParserType,
    pub subject: ExpressionId,
    // Validator/linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub struct CallExpression {
    pub this: CallExpressionId,
    // Parsing
    pub func_span: InputSpan, // of the function name
    pub full_span: InputSpan, // includes the arguments and parentheses
    pub parser_type: ParserType, // of the function call, not the return type
    pub method: Method,
    pub arguments: Vec<ExpressionId>,
    pub definition: DefinitionId,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    // Builtin
    Get,
    Put,
    Fires,
    Create,
    Length,
    Assert,
    UserFunction,
    UserComponent,
}

#[derive(Debug, Clone)]
pub struct MethodSymbolic {
    pub(crate) parser_type: ParserType,
    pub(crate) definition: DefinitionId
}

#[derive(Debug, Clone)]
pub struct LiteralExpression {
    pub this: LiteralExpressionId,
    // Parsing
    pub span: InputSpan,
    pub value: Literal,
    // Validator/Linker
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}

#[derive(Debug, Clone)]
pub enum Literal {
    Null, // message
    True,
    False,
    Character(char),
    String(StringRef<'static>),
    Integer(LiteralInteger),
    Struct(LiteralStruct),
    Enum(LiteralEnum),
    Union(LiteralUnion),
    Array(Vec<ExpressionId>),
}

impl Literal {
    pub(crate) fn as_struct(&self) -> &LiteralStruct {
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
pub struct LiteralInteger {
    pub(crate) unsigned_value: u64,
    pub(crate) negated: bool, // for constant expression evaluation, TODO: @Int
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
    pub(crate) parser_type: ParserType,
    pub(crate) fields: Vec<LiteralStructField>,
    pub(crate) definition: DefinitionId,
}

#[derive(Debug, Clone)]
pub struct LiteralEnum {
    // Phase 1: parser
    pub(crate) parser_type: ParserType,
    pub(crate) variant: Identifier,
    pub(crate) definition: DefinitionId,
    // Phase 2: linker
    pub(crate) variant_idx: usize, // as present in the type table
}

#[derive(Debug, Clone)]
pub struct LiteralUnion {
    // Phase 1: parser
    pub(crate) parser_type: ParserType,
    pub(crate) variant: Identifier,
    pub(crate) values: Vec<ExpressionId>,
    pub(crate) definition: DefinitionId,
    // Phase 2: linker
    pub(crate) variant_idx: usize, // as present in type table
}

#[derive(Debug, Clone)]
pub struct VariableExpression {
    pub this: VariableExpressionId,
    // Parsing
    pub identifier: Identifier,
    // Validator/Linker
    pub declaration: Option<VariableId>,
    pub used_as_binding_target: bool,
    pub parent: ExpressionParent,
    pub unique_id_in_definition: i32,
}
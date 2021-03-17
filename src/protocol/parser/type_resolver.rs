use std::collections::{HashMap, HashSet, VecDeque};

use crate::protocol::ast::*;
use crate::protocol::inputsource::*;
use super::type_table::*;
use super::symbol_table::*;
use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx,
    Visitor2,
    VisitorResult
};

#[derive(Clone, Eq, PartialEq)]
pub(crate) enum InferredPart {
    // Unknown type, yet to be inferred
    Unknown,
    // Special cases
    Void, // For builtin functions without a return type. Result of a "call to a component"
    IntegerLike, // For integer literals without a concrete type
    ArrayLike, // Array or Slice
    // No subtypes
    Message,
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // One subtype
    Array,
    Slice,
    Input,
    Output,
    // One or more subtypes
    Instance(DefinitionId, usize),
}

impl InferredPart {
    fn is_concrete_int(&self) -> bool {
        use InferredPart as IP;
        match self {
            IP::Byte | IP::Short | IP::Int | IP::Long => true,
            _ => false
        }
    }

    fn is_concrete_arraylike(&self) -> bool {
        use InferredPart as IP;
        match self {
            IP::Slice | IP::Array => true,
            _ => false
        }
    }
}

impl From<ConcreteTypeVariant> for InferredPart {
    fn from(v: ConcreteTypeVariant) -> Self {
        use ConcreteTypeVariant as CTV;
        use InferredPart as IP;

        match v {
            CTV::Message => IP::Message,
            CTV::Bool => IP::Bool,
            CTV::Byte => IP::Byte,
            CTV::Short => IP::Short,
            CTV::Int => IP::Int,
            CTV::Long => IP::Long,
            CTV::String => IP::String,
            CTV::Array => IP::Array,
            CTV::Slice => IP::Slice,
            CTV::Input => IP::Input,
            CTV::Output => IP::Output,
            CTV::Instance(definition_id, num_sub) => IP::Instance(definition_id, num_sub),
        }
    }
}

pub(crate) struct InferenceType {
    done: bool,
    parts: Vec<InferredPart>,
}

const BOOL_TEMPLATE: InferenceType = InferenceType{ done: true, parts: vec![InferredPart::Bool] };
const INTEGERLIKE_TEMPLATE: InferenceType = InferenceType{ done: false, parts: vec![InferredPart::IntegerLike] };
const ARRAYLIKE_TEMPLATE: InferenceType = InferenceType{ done: false, parts: vec![InferredPart::ArrayLike, InferredPart::Unknown] };
const SLICE_TEMPLATE: InferenceType = InferenceType{ done: false, parts: vec![InferredPart::Slice, InferredPart::Unknown] };

impl InferenceType {
    fn new(done: bool, inferred: Vec<InferredPart>) -> Self {
        Self{ done, parts: inferred }
    }

    fn find_subtree_end_idx(&self, mut idx: usize) -> usize {
        use InferredPart as IP;

        let mut depth = 1;
        loop {
            match self.parts[idx] {
                IP::Unknown | IP::Void | IP::IntegerLike |
                IP::Message | IP::Bool |
                IP::Byte | IP::Short | IP::Int | IP::Long |
                IP::String => {
                    depth -= 1;
                },
                IP::ArrayLike | IP::Array | IP::Slice | IP::Input | IP::Output => {
                    // depth remains unaltered
                },
                IP::Instance(_, num_sub) => {
                    depth += (num_sub as i32) - 1
                }
            }

            idx += 1;
            if depth == 0 {
                return idx
            }
        }
    }

    // TODO: @float
    fn might_be_numeric(&self) -> bool {
        use InferredPart as IP;

        debug_assert!(!self.parts.is_empty());
        if self.parts.len() != 1 { return false; }
        match self.parts[0] {
            IP::Unknown | IP::IntegerLike | IP::Byte | IP::Short | IP::Int | IP::Long => true,
            _ => false
        }
    }

    fn might_be_integer(&self) -> bool {
        // TODO: @float Once floats are implemented this is no longer true
        self.might_be_numeric()
    }

    fn might_be_boolean(&self) -> bool {
        use InferredPart as IP;
        debug_assert!(!self.parts.is_empty());
        if self.parts.len() != 1 { return false; }
        match self.parts[0] {
            IP::Unknown | IP::Bool => true,
            _ =>
                false
        }
    }

    fn display_name(&self, heap: &Heap) -> String {
        use InferredPart as IP;

        fn write_recursive(v: &mut String, t: &InferenceType, h: &Heap, idx: &mut usize) {
            match &t.parts[*idx] {
                IP::Unknown => v.push_str("?"),
                IP::Void => v.push_str("void"),
                IP::IntegerLike => v.push_str("int?"),
                IP::Message => v.push_str("msg"),
                IP::Bool => v.push_str("bool"),
                IP::Byte => v.push_str("byte"),
                IP::Short => v.push_str("short"),
                IP::Int => v.push_str("int"),
                IP::Long => v.push_str("long"),
                IP::String => v.push_str("str"),
                IP::ArrayLike => {
                    *idx += 1;
                    write_recursive(v, t, h, idx);
                    v.push_str("[?]");
                }
                IP::Array => {
                    *idx += 1;
                    write_recursive(v, t, h, idx);
                    v.push_str("[]");
                },
                IP::Slice => {
                    *idx += 1;
                    write_recursive(v, t, h, idx);
                    v.push_str("[..]")
                },
                IP::Input => {
                    *idx += 1;
                    v.push_str("in<");
                    write_recursive(v, t, h, idx);
                    v.push('>');
                },
                IP::Output => {
                    *idx += 1;
                    v.push_str("out<");
                    write_recursive(v, t, h, idx);
                    v.push('>');
                },
                IP::Instance(definition_id, num_sub) => {
                    let definition = &h[*definition_id];
                    v.push_str(&String::from_utf8_lossy(&definition.identifier().value));
                    if *num_sub > 0 {
                        v.push('<');
                        *idx += 1;
                        write_recursive(v, t, h, idx);
                        for sub_idx in 1..*num_sub {
                            *idx += 1;
                            v.push_str(", ");
                            write_recursive(v, t, h, idx);
                        }
                        v.push('>');
                    }
                },
            }
        }

        let mut buffer = String::with_capacity(self.parts.len() * 5);
        let mut idx = 0;
        write_recursive(&mut buffer, self, heap, &mut idx);

        buffer
    }
}

#[derive(PartialEq, Eq)]
enum InferenceResult {
    Neither,        // neither argument is clarified
    First,          // first argument is clarified using the second one
    Second,         // second argument is clarified using the first one
    Both,           // both arguments are clarified
    Incompatible,   // types are incompatible: programmer error
}

impl InferenceResult {
    fn modified_any(&self) -> bool {
        match self {
            InferenceResult::First | InferenceResult::Second | InferenceResult::Both => true,
            _ => false
        }
    }
    fn modified_lhs(&self) -> bool {
        match self {
            InferenceResult::First | InferenceResult::Both => true,
            _ => false
        }
    }
    fn modified_rhs(&self) -> bool {
        match self {
            InferenceResult::Second | InferenceResult::Both => true,
            _ => false
        }
    }
}

fn progress_inference_part_and_advance_idx(
    a: &mut InferenceType, b: &mut InferenceType, iter_idx: &mut usize
) -> Result<(), InferenceResult> {

}

// Attempts to infer types within two `InferenceType` instances. If they are
// compatible then the result indicates which of the arguments were modified.
// After successful inference the parts of the inferred type have equal length.
//
// Personal note: inference is not the copying of types: this algorithm must
// infer that `TypeOuter<TypeA, auto>` and `TypeOuter<auto, TypeB>` resolves to
// `TypeOuter<TypeA, TypeB>`.
unsafe fn progress_inference_types(a: *mut InferenceType, b: *mut InferenceType) -> InferenceResult {
    let a = &mut *a;
    let b = &mut *b;
    debug_assert!(!a.parts.is_empty());
    debug_assert!(!b.parts.is_empty());

    // Iterate over the elements of both types. Each (partially) known type
    // element must be compatible. Unknown types will be replaced by their
    // concrete counterpart if possible.
    let mut modified_a = false;
    let mut modified_b = false;
    let mut iter_idx = 0;

    while iter_idx < a.parts.len() {
        let a_part = &a.parts[iter_idx];
        let b_part = &b.parts[iter_idx];

        if a_part == b_part {
            // Exact match
            iter_idx += 1;
            continue;
        }

        // Not an exact match, so deal with edge cases
        // - inference of integerlike types to conrete integers
        // - inference of arraylike to array/slice
        if (*a_part == InferredPart::IntegerLike && b_part.is_concrete_int()) ||
            (*a_part == InferredPart::ArrayLike && b_part.is_concrete_arraylike()) {
            modified_a = true;
            a.parts[iter_idx] = b_part.clone();
            iter_idx += 1;
            continue;
        }
        if (*b_part == InferredPart::IntegerLike && a_part.is_concrete_int()) ||
            (*b_part == InferredPart::ArrayLike && a_part.is_concrete_arraylike()){
            modified_b = true;
            b.parts[iter_idx] = a_part.clone();
            iter_idx += 1;
            continue;
        }

        // - inference of unknown type
        if *a_part == InferredPart::Unknown {
            let end_idx = b.find_subtree_end_idx(iter_idx);
            a.parts[iter_idx] = b.parts[iter_idx].clone();
            for insert_idx in (iter_idx + 1)..end_idx {
                a.parts.insert(insert_idx, b.parts[insert_idx].clone());
            }

            modified_a = true;
            iter_idx = end_idx;
            continue;
        }

        if *b_part == InferredPart::Unknown {
            let end_idx = a.find_subtree_end_idx(iter_idx);
            b.parts[iter_idx] = a.parts[iter_idx].clone();
            for insert_idx in (iter_idx + 1)..end_idx {
                b.parts.insert(insert_idx, a.parts[insert_idx].clone());
            }

            modified_b = true;
            iter_idx = end_idx;
            continue;
        }

        // If here then we cannot match the two parts
        return InferenceResult::Incompatible;
    }

    // TODO: @performance, can be done in the loop above
    a.done = true;
    for part in &a.parts {
        if *part == InferredPart::Unknown || *part == InferredPart::IntegerLike {
            a.done = false;
            break
        }
    }

    b.done = true;
    for part in &b.parts {
        if *part == InferredPart::Unknown || *part == InferredPart::IntegerLike {
            b.done = false;
            break;
        }
    }

    // If the inference parts are correctly constructed (>0 elements, proper
    // trees) then being here means that both types are not only of compatible
    // types, but MUST have the same length.
    debug_assert_eq!(a.parts.len(), b.parts.len());
    match (modified_a, modified_b) {
        (true, true) => InferenceResult::Both,
        (true, false) => InferenceResult::First,
        (false, true) => InferenceResult::Second,
        (false, false) => InferenceResult::Neither
    }
}

enum InferenceTemplateResult {
    Unmodified,
    Modified,
    Incompatible,
}

fn progress_inference_with_template(t: &mut InferenceType, template: &InferenceType) -> InferenceTemplateResult {
    debug_assert!(!t.parts.is_empty());
    debug_assert!(!template.parts.is_empty());

    // Iterate over the elements of both types, enforce the template onto the
    // provided mutable inference type.
    let mut modified = false;
    let mut iter_idx = 0;

    while iter_idx < t.parts.len() {
        let part = &t.parts[iter_idx];
        let template_part = &template.parts[iter_idx];

        if part == template_part {
            iter_idx += 1;
            continue
        }

        // Check if we can infer anything from the template
        if (*part == InferredPart::IntegerLike && template_part.is_concrete_int()) ||
            (*part == InferredPart::ArrayLike && template_part.is_concrete_arraylike()) {
            modified = true;
            t.parts[iter_idx] = template_part.clone();
            iter_idx += 1;
            continue;
        }

        if *part == InferredPart::Unknown {
            let end_idx = template.find_subtree_end_idx(iter_idx);
            t.parts[iter_idx] = template.parts[iter_idx].clone();
            for insert_idx in (iter_idx + 1)..end_idx {
                t.parts.insert(insert_idx, template.parts[insert_idx].clone());
            }

            modified = true;
            iter_idx = end_idx;
            continue;
        }

        // Because we're dealing with a template some mismatched parts are still
        // valid
        if (part.is_concrete_arraylike() && *template_part == InferredPart::ArrayLike) ||
            (part.is_concrete_int() && *template_part == InferredPart::IntegerLike) {
            iter_idx += 1;
            continue;
        }

        return InferenceTemplateResult::Incompatible;
    }

    if modified {
        InferenceTemplateResult::Modified
    } else {
        InferenceTemplateResult::Unmodified
    }
}

enum DefinitionType{
    None,
    Component(ComponentId),
    Function(FunctionId),
}

// TODO: @cleanup I will do a very dirty implementation first, because I have no idea
//  what I am doing.
// Very rough idea:
//  - go through entire AST first, find all places where we have inferred types
//      (which may be embedded) and store them in some kind of map.
//  - go through entire AST and visit all expressions depth-first. We will
//      attempt to resolve the return type of each expression. If we can't then
//      we store them in another lookup map and link the dependency on an
//      inferred variable to that expression.
//  - keep iterating until we have completely resolved all variables.

/// This particular visitor will recurse depth-first into the AST and ensures
/// that all expressions have the appropriate types. At the moment this implies:
///
///     - Type checking arguments to unary and binary operators.
///     - Type checking assignment, indexing, slicing and select expressions.
///     - Checking arguments to functions and component instantiations.
///
/// This will be achieved by slowly descending into the AST. At any given
/// expression we may depend on
pub(crate) struct TypeResolvingVisitor {
    definition_type: DefinitionType,

    // Buffers for iteration over substatements and subexpressions
    stmt_buffer: Vec<StatementId>,
    expr_buffer: Vec<ExpressionId>,

    // If instantiating a monomorph of a polymorphic proctype, then we store the
    // values of the polymorphic values here. There should be as many, and in
    // the same order as, in the definition's polyargs.
    polyvars: Vec<ConcreteType>,
    // Mapping from parser type to inferred type. We attempt to continue to
    // specify these types until we're stuck or we've fully determined the type.
    infer_types: HashMap<VariableId, InferenceType>,
    expr_types: HashMap<ExpressionId, InferenceType>,
    expr_queued: HashSet<ExpressionId>,
}

impl TypeResolvingVisitor {
    pub(crate) fn new() -> Self {
        TypeResolvingVisitor{
            definition_type: DefinitionType::None,
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            polyvars: Vec::new(),
            infer_types: HashMap::new(),
            expr_types: HashMap::new(),
            expr_queued: HashSet::new(),
        }
    }

    fn reset(&mut self) {
        self.definition_type = DefinitionType::None;
        self.stmt_buffer.clear();
        self.expr_buffer.clear();
        self.polyvars.clear();
        self.infer_types.clear();
        self.expr_types.clear();
    }
}

impl Visitor2 for TypeResolvingVisitor {
    // Definitions

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        self.reset();
        self.definition_type = DefinitionType::Component(id);

        let comp_def = &ctx.heap[id];
        debug_assert_eq!(comp_def.poly_vars.len(), self.polyvars.len(), "component polyvars do not match imposed polyvars");

        for param_id in comp_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let infer_type = self.determine_inference_type_from_parser_type(ctx, param.parser_type);
            debug_assert!(infer_type.done, "expected component arguments to be concrete types");
            self.infer_types.insert(param_id.upcast(), infer_type);
        }

        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        self.reset();
        self.definition_type = DefinitionType::Function(id);

        let func_def = &ctx.heap[id];
        debug_assert_eq!(func_def.poly_vars.len(), self.polyvars.len(), "function polyvars do not match imposed polyvars");

        for param_id in func_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let infer_type = self.determine_inference_type_from_parser_type(ctx, param.parser_type);
            debug_assert!(infer_type.done, "expected function arguments to be concrete types");
            self.infer_types.insert(param_id.upcast(), infer_type);
        }

        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    // Statements

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        // Transfer statements for traversal
        let block = &ctx.heap[id];

        for stmt_id in block.statements.clone() {
            self.visit_stmt(ctx, stmt_id);
        }

        Ok(())
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        let memory_stmt = &ctx.heap[id];

        let local = &ctx.heap[memory_stmt.variable];
        let infer_type = self.determine_inference_type_from_parser_type(ctx, local.parser_type);
        self.infer_types.insert(memory_stmt.variable.upcast(), infer_type);

        let expr_id = memory_stmt.initial;
        self.visit_expr(ctx, expr_id)?;

        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        let channel_stmt = &ctx.heap[id];

        let from_local = &ctx.heap[channel_stmt.from];
        let from_infer_type = self.determine_inference_type_from_parser_type(ctx, from_local.parser_type);
        self.infer_types.insert(from_local.this.upcast(), from_infer_type);

        let to_local = &ctx.heap[channel_stmt.to];
        let to_infer_type = self.determine_inference_type_from_parser_type(ctx, to_local.parser_type);
        self.infer_types.insert(to_local.this.upcast(), to_infer_type);

        Ok(())
    }

    fn visit_labeled_stmt(&mut self, ctx: &mut Ctx, id: LabeledStatementId) -> VisitorResult {
        let labeled_stmt = &ctx.heap[id];
        let substmt_id = labeled_stmt.body;
        self.visit_stmt(ctx, substmt_id)
    }

    fn visit_if_stmt(&mut self, ctx: &mut Ctx, id: IfStatementId) -> VisitorResult {
        let if_stmt = &ctx.heap[id];

        let true_body_id = if_stmt.true_body;
        let false_body_id = if_stmt.false_body;
        let test_expr_id = if_stmt.test;

        self.visit_expr(ctx, test_expr_id)?;
        self.visit_stmt(ctx, true_body_id)?;
        self.visit_stmt(ctx, false_body_id)?;

        Ok(())
    }

    fn visit_while_stmt(&mut self, ctx: &mut Ctx, id: WhileStatementId) -> VisitorResult {
        let while_stmt = &ctx.heap[id];

        let body_id = while_stmt.body;
        let test_expr_id = while_stmt.test;

        self.visit_expr(ctx, test_expr_id)?;
        self.visit_stmt(ctx, body_id)?;

        Ok(())
    }

    fn visit_synchronous_stmt(&mut self, ctx: &mut Ctx, id: SynchronousStatementId) -> VisitorResult {
        let sync_stmt = &ctx.heap[id];
        let body_id = sync_stmt.body;

        self.visit_stmt(ctx, body_id)
    }

    fn visit_return_stmt(&mut self, ctx: &mut Ctx, id: ReturnStatementId) -> VisitorResult {
        let return_stmt = &ctx.heap[id];
        let expr_id = return_stmt.expression;

        self.visit_expr(ctx, expr_id)
    }

    fn visit_assert_stmt(&mut self, ctx: &mut Ctx, id: AssertStatementId) -> VisitorResult {
        let assert_stmt = &ctx.heap[id];
        let test_expr_id = assert_stmt.expression;

        self.visit_expr(ctx, test_expr_id)
    }

    fn visit_new_stmt(&mut self, ctx: &mut Ctx, id: NewStatementId) -> VisitorResult {
        let new_stmt = &ctx.heap[id];
        let call_expr_id = new_stmt.expression;

        self.visit_call_expr(ctx, call_expr_id)
    }

    fn visit_put_stmt(&mut self, ctx: &mut Ctx, id: PutStatementId) -> VisitorResult {
        let put_stmt = &ctx.heap[id];

        let port_expr_id = put_stmt.port;
        let msg_expr_id = put_stmt.message;
        // TODO: What what?

        self.visit_expr(ctx, port_expr_id)?;
        self.visit_expr(ctx, msg_expr_id)?;

        Ok(())
    }

    fn visit_expr_stmt(&mut self, ctx: &mut Ctx, id: ExpressionStatementId) -> VisitorResult {
        let expr_stmt = &ctx.heap[id];
        let subexpr_id = expr_stmt.expression;

        self.visit_expr(ctx, subexpr_id)
    }

    // Expressions

    fn visit_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id);

        let assign_expr = &ctx.heap[id];
        let left_expr_id = assign_expr.left;
        let right_expr_id = assign_expr.right;

        self.visit_expr(ctx, left_expr_id)?;
        self.visit_expr(ctx, right_expr_id)?;

        self.progress_assignment_expr(ctx, id)
    }

    fn visit_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id);

        let conditional_expr = &ctx.heap[id];
        let test_expr_id = conditional_expr.test;
        let true_expr_id = conditional_expr.true_expression;
        let false_expr_id = conditional_expr.false_expression;

        self.expr_types.insert(test_expr_id, InferenceType::new(true, vec![InferredPart::Bool]));
        self.visit_expr(ctx, test_expr_id)?;
        self.visit_expr(ctx, true_expr_id)?;
        self.visit_expr(ctx, false_expr_id)?;

        self.progress_conditional_expr(ctx, id)
    }

    fn visit_binary_expr(&mut self, ctx: &mut Ctx, id: BinaryExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id);

        let binary_expr = &ctx.heap[id];
        let lhs_expr_id = binary_expr.left;
        let rhs_expr_id = binary_expr.right;

        self.visit_expr(ctx, lhs_expr_id)?;
        self.visit_expr(ctx, rhs_expr_id)?;

        self.progress_binary_expr(ctx, id)
    }

    fn visit_unary_expr(&mut self, ctx: &mut Ctx, id: UnaryExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id);

        let unary_expr = &ctx.heap[id];
        let arg_expr_id = unary_expr.expression;

        self.visit_expr(ctx, arg_expr_id);

        self.progress_unary_expr(ctx, id)
    }
}

// TODO: @cleanup Decide to use this where appropriate or to make templates for
//  everything
enum TypeClass {
    Numeric, // int and float
    Integer, // only ints
    Boolean, // only boolean
}

impl std::fmt::Display for TypeClass {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", match self {
            TypeClass::Numeric => "numeric",
            TypeClass::Integer => "integer",
            TypeClass::Boolean => "boolean",
        })
    }
}

macro_rules! debug_assert_expr_ids_unique_and_known {
    // Base case for a single expression ID
    ($resolver:ident, $id:ident) => {
        if cfg!(debug_assertions) {
            $resolver.expr_types.contains_key(&$id);
        }
    };
    // Base case for two expression IDs
    ($resolver:ident, $id1:ident, $id2:ident) => {
        debug_assert_ne!($id1, $id2);
        debug_assert_expr_ids_unique_and_known!($resolver, $id1);
        debug_assert_expr_ids_unique_and_known!($resolver, $id2);
    };
    // Generic case
    ($resolver:ident, $id1:ident, $id2:ident, $($tail:ident),+) => {
        debug_assert_ne!($id1, $id2);
        debug_assert_expr_ids_unique_and_known!($resolver, $id1);
        debug_assert_expr_ids_unique_and_known!($resolver, $id2, $($tail),+);
    };
}

enum TypeConstraintResult {
    Progress, // Success: Made progress in applying constraints
    NoProgess, // Success: But did not make any progress in applying constraints
    ErrExprType, // Error: Expression type did not match the argument(s) of the expression type
    ErrArgType, // Error: Expression argument types did not match
}

impl TypeResolvingVisitor {
    fn progress_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> Result<(), ParseError2> {
        use AssignmentOperator as AO;

        // TODO: Assignable check
        let (type_class, arg1_expr_id, arg2_expr_id) = {
            let expr = &ctx.heap[id];
            let type_class = match expr.operation {
                AO::Set =>
                    None,
                AO::Multiplied | AO::Divided | AO::Added | AO::Subtracted =>
                    Some(TypeClass::Numeric),
                AO::Remained | AO::ShiftedLeft | AO::ShiftedRight |
                AO::BitwiseAnded | AO::BitwiseXored | AO::BitwiseOred =>
                    Some(TypeClass::Integer),
            };

            (type_class, expr.left, expr.right)
        };

        let upcast_id = id.upcast();
        let (progress_expr, progress_arg1, progress_arg2) = self.apply_equal3_constraint(
            ctx, upcast_id, arg1_expr_id, arg2_expr_id
        )?;

        if let Some(type_class) = type_class {
            self.expr_type_is_of_type_class(ctx, id.upcast(), type_class)?
        }

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg1 { self.queue_expr(arg1_expr_id); }
        if progress_arg2 { self.queue_expr(arg2_expr_id); }

        Ok(())
    }

    fn progress_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> Result<(), ParseError2> {
        // Note: test expression type is already enforced
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg1_expr_id = expr.true_expression;
        let arg2_expr_id = expr.false_expression;

        let (progress_expr, progress_arg1, progress_arg2) = self.apply_equal3_constraint(
            ctx, upcast_id, arg1_expr_id, arg2_expr_id
        )?;

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg1 { self.queue_expr(arg1_expr_id); }
        if progress_arg2 { self.queue_expr(arg2_expr_id); }

        Ok(())
    }

    fn progress_binary_expr(&mut self, ctx: &mut Ctx, id: BinaryExpressionId) -> Result<(), ParseError2> {
        // Note: our expression type might be fixed by our parent, but we still
        // need to make sure it matches the type associated with our operation.
        use BinaryOperator as BO;

        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg1_id = expr.left;
        let arg2_id = expr.right;

        let (progress_expr, progress_arg1, progress_arg2) = match expr.operation {
            BO::Concatenate => {
                // Equal3 with array-like appearance, two slice arguments result
                // in an array output
                todo!("implement concatenate typechecking");
                (false, false, false)
            },
            BO::LogicalOr | BO::LogicalAnd => {
                // Forced boolean on all
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg1 = self.apply_forced_constraint(ctx, arg1_id, &BOOL_TEMPLATE)?;
                let progress_arg2 = self.apply_forced_constraint(ctx, arg2_id, &BOOL_TEMPLATE)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::BitwiseOr | BO::BitwiseXor | BO::BitwiseAnd | BO::Remainder | BO::ShiftLeft | BO::ShiftRight => {
                let result = self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id)?;
                self.expr_type_is_of_type_class(ctx, upcast_id, TypeClass::Integer)?;
                result
            },
            BO::Equality | BO::Inequality | BO::LessThan | BO::GreaterThan | BO::LessThanEqual | BO::GreaterThanEqual => {
                // Equal2 on args, forced boolean output
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let (progress_arg1, progress_arg2) = self.apply_equal2_constraint(ctx, upcast_id, arg1_id, arg2_id);
                self.expr_type_is_of_type_class(ctx, arg1_id, TypeClass::Numeric)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::Add | BO::Subtract | BO::Multiply | BO::Divide => {
                let result = self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id)?;
                self.expr_type_is_of_type_class(ctx, upcast_id, TypeClass::Numeric)?;
                result
            },
        };

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg1 { self.queue_expr(arg1_id); }
        if progress_arg2 { self.queue_expr(arg2_id); }

        Ok(())
    }

    fn progress_unary_expr(&mut self, ctx: &mut Ctx, id: UnaryExpressionId) -> Result<(), ParseError2> {
        use UnaryOperation as UO;

        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg_id = expr.expression;

        let (progress_expr, progress_arg) = match expr.operation {
            UO::Positive | UO::Negative => {
                // Equal types of numeric class
                let progress = self.apply_equal2_constraint(ctx, upcast_id, upcast_id, arg_id)?;
                self.expr_type_is_of_type_class(ctx, upcast_id, TypeClass::Numeric)?;
                progress
            },
            UO::BitwiseNot | UO::PreIncrement | UO::PreDecrement | UO::PostIncrement | UO::PostDecrement => {
                // Equal types of integer class
                let progress = self.apply_equal2_constraint(ctx, upcast_id, upcast_id, arg_id)?;
                self.expr_type_is_of_type_class(ctx, upcast_id, TypeClass::Integer)?;
                progress
            },
            UO::LogicalNot => {
                // Both booleans
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                (progress_expr, progress_arg)
            }
        };

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg { self.queue_expr(arg_id); }

        Ok(())
    }

    fn progress_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> Result<(), ParseError2> {
        // TODO: Indexable check
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;
        let index_id = expr.index;

        let progress_subject = self.apply_forced_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
        let progress_index = self.apply_forced_constraint(ctx, index_id, &INTEGERLIKE_TEMPLATE)?;

    }

    fn queue_expr_parent(&mut self, ctx: &Ctx, expr_id: ExpressionId) {
        if let ExpressionParent::Expression(parent_expr_id, _) = &ctx.heap[expr_id].parent() {
            self.expr_queued.insert(*parent_expr_id)
        }
    }

    fn queue_expr(&mut self, expr_id: ExpressionId) {
        self.expr_queued.insert(expr_id);
    }

    /// Applies a forced type constraint: the type associated with the supplied
    /// expression will be molded into the provided "template". The template may
    /// be fully specified (e.g. a bool) or contain "inference" variables (e.g.
    /// an array of T)
    fn apply_forced_constraint(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId, template: &InferenceType
    ) -> Result<bool, ParseError2> {
        debug_assert_expr_ids_unique_and_known!(self, expr_id);
        let expr_type = self.expr_types.get_mut(&expr_id).unwrap();
        match progress_inference_with_template(expr_type, template) {
            InferenceTemplateResult::Modified => Ok(true),
            InferenceTemplateResult::Unmodified => Ok(false),
            InferenceTemplateResult::Incompatible => Err(
                self.construct_template_type_error(ctx, expr_id, template)
            )
        }
    }

    /// Applies a type constraint that expects the two provided types to be
    /// equal. We attempt to make progress in inferring the types. If the call
    /// is successful then the composition of all types are made equal.
    /// The "parent" `expr_id` is provided to construct errors.
    fn apply_equal2_constraint(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId, arg1_id: ExpressionId, arg2_id: ExpressionId
    ) -> Result<(bool, bool), ParseError2> {
        debug_assert_expr_ids_unique_and_known!(self, arg1_id, arg2_id);
        let arg1_type: *mut _ = self.expr_types.get_mut(&arg1_id).unwrap();
        let arg2_type: *mut _ = self.expr_types.get_mut(&arg2_id).unwrap();

        let infer_res = unsafe{ progress_inference_types(expr_idarg1_type, arg2_type) };
        if infer_res == InferenceResult::Incompatible {
            return Err(self.construct_arg_type_error(ctx, expr_id, arg1_id, arg2_id));
        }

        Ok((infer_res.modified_lhs(), infer_res.modified_rhs()))
    }

    /// Applies a type constraint that expects all three provided types to be
    /// equal. In case we can make progress in inferring the types then we
    /// attempt to do so. If the call is successful then the composition of all
    /// types is made equal.
    fn apply_equal3_constraint(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg2_id: ExpressionId
    ) -> Result<(bool, bool, bool), ParseError2> {
        // Safety: all expression IDs are always distinct, and we do not modify
        //  the container
        debug_assert_expr_ids_unique_and_known!(self, expr_id, arg1_id, arg2_id);
        let expr_type: *mut _ = self.expr_types.get_mut(&expr_id).unwrap();
        let arg1_type: *mut _ = self.expr_types.get_mut(&arg1_id).unwrap();
        let arg2_type: *mut _ = self.expr_types.get_mut(&arg2_id).unwrap();

        let expr_res = unsafe{ progress_inference_types(expr_type, arg1_type) };
        if expr_res == InferenceResult::Incompatible { 
            return Err(self.construct_expr_type_error(ctx, expr_id, arg1_id));
        }

        let args_res = unsafe{ progress_inference_types(arg1_type, arg2_type) };
        if args_res == InferenceResult::Incompatible { 
            return Err(self.construct_arg_type_error(ctx, expr_id, arg1_id, arg2_id));
        }

        // If all types are compatible, but the second call caused the arg1_type
        // to be expanded, then we must also assign this to expr_type.
        let mut progress_expr = expr_res.modified_lhs();
        let mut progress_arg1 = expr_res.modified_rhs();
        let mut progress_arg2 = args_res.modified_rhs();

        if args_res.modified_lhs() { 
            unsafe {
                (*expr_type).parts.clear();
                (*expr_type).parts.extend((*arg2_type).parts.iter());
            }
            progress_expr = true;
            progress_arg1 = true;
        }

        Ok((progress_expr, progress_arg1, progress_arg2))
    }

    /// Applies a typeclass constraint: checks if the type is of a particular
    /// class or not
    fn expr_type_is_of_type_class(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId, type_class: TypeClass
    ) -> Result<(), ParseError2> {
        debug_assert_expr_ids_unique_and_known!(self, expr_id);
        let expr_type = self.expr_types.get(&expr_id).unwrap();

        let is_ok = match type_class {
            TypeClass::Numeric => expr_type.might_be_numeric(),
            TypeClass::Integer => expr_type.might_be_integer(),
            TypeClass::Boolean => expr_type.might_be_boolean(),
        };

        if is_ok {
            Ok(())
        } else {
            Err(self.construct_type_class_error(ctx, expr_id, type_class))
        }
    }

    /// Determines the `InferenceType` for the expression based on the
    /// expression parent. Note that if the parent is another expression, we do
    /// not take special action, instead we let parent expressions fix the type
    /// of subexpressions before they have a chance to call this function.
    /// Hence: if the expression type is already set, this function doesn't do
    /// anything.
    fn insert_initial_expr_inference_type(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId
    ) {
        // TODO: @cleanup Concept of "parent expression" can be removed, the
        //  type inferer/checker can set this upon the initial pass
        use ExpressionParent as EP;
        if self.expr_types.contains_key(&expr_id) { return; }

        let expr = &ctx.heap[expr_id];
        let inference_type = match expr.parent() {
            EP::None =>
                // Should have been set by linker
                unreachable!(),
            EP::Memory(_) | EP::ExpressionStmt(_) | EP::Expression(_, _) =>
                // Determined during type inference
                InferenceType::new(false, vec![InferredPart::Unknown]),
            EP::If(_) | EP::While(_) | EP::Assert(_) =>
                // Must be a boolean
                InferenceType::new(true, vec![InferredPart::Bool]),
            EP::Return(_) =>
                // Must match the return type of the function
                if let DefinitionType::Function(func_id) = self.definition_type {
                    let return_parser_type_id = ctx.heap[func_id].return_type;
                    self.determine_inference_type_from_parser_type(ctx, return_parser_type_id)
                } else {
                    // Cannot happen: definition always set upon body traversal
                    // and "return" calls in components are illegal.
                    unreachable!();
                },
            EP::New(_) =>
                // Must be a component call, which we assign a "Void" return
                // type
                InferenceType::new(true, vec![InferredPart::Void]),
            EP::Put(_, 0) =>
                // TODO: Change put to be a builtin function
                // port of "put" call
                InferenceType::new(false, vec![InferredPart::Output, InferredPart::Unknown]),
            EP::Put(_, 1) =>
                // TODO: Change put to be a builtin function
                // message of "put" call
                InferenceType::new(true, vec![InferredPart::Message]),
            EP::Put(_, _) =>
                unreachable!()
        };

        self.expr_types.insert(expr_id, inference_type);
    }

    fn determine_inference_type_from_parser_type(
        &mut self, ctx: &mut Ctx, parser_type_id: ParserTypeId
    ) -> InferenceType {
        use ParserTypeVariant as PTV;
        use InferredPart as IP;

        let mut to_consider = VecDeque::with_capacity(16);
        to_consider.push_back(parser_type_id);

        let mut infer_type = Vec::new();
        let mut has_inferred = false;

        while !to_consider.is_empty() {
            let parser_type_id = to_consider.pop_front().unwrap();
            let parser_type = &ctx.heap[parser_type_id];
            match &parser_type.variant {
                PTV::Message => { infer_type.push(IP::Message); },
                PTV::Bool => { infer_type.push(IP::Bool); },
                PTV::Byte => { infer_type.push(IP::Byte); },
                PTV::Short => { infer_type.push(IP::Short); },
                PTV::Int => { infer_type.push(IP::Int); },
                PTV::Long => { infer_type.push(IP::Long); },
                PTV::String => { infer_type.push(IP::String); },
                PTV::IntegerLiteral => { unreachable!("integer literal type on variable type"); },
                PTV::Inferred => {
                    infer_type.push(IP::Unknown);
                    has_inferred = true;
                },
                PTV::Array(subtype_id) => {
                    infer_type.push(IP::Array);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Input(subtype_id) => {
                    infer_type.push(IP::Input);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Output(subtype_id) => {
                    infer_type.push(IP::Output);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Symbolic(symbolic) => {
                    debug_assert!(symbolic.variant.is_some(), "symbolic variant not yet determined");
                    match symbolic.variant.unwrap() {
                        SymbolicParserTypeVariant::PolyArg(_, arg_idx) => {
                            // Retrieve concrete type of argument and add it to
                            // the inference type.
                            debug_assert!(symbolic.poly_args.is_empty()); // TODO: @hkt
                            debug_assert!(arg_idx < self.polyvars.len());
                            for concrete_part in &self.polyvars[arg_idx].v {
                                infer_type.push(IP::from(*concrete_part));
                            }
                        },
                        SymbolicParserTypeVariant::Definition(definition_id) => {
                            // TODO: @cleanup
                            if cfg!(debug_assertions) {
                                let definition = &ctx.heap[definition_id];
                                debug_assert!(definition.is_struct() || definition.is_enum()); // TODO: @function_ptrs
                                let num_poly = match definition {
                                    Definition::Struct(v) => v.poly_vars.len(),
                                    Definition::Enum(v) => v.poly_vars.len(),
                                    _ => unreachable!(),
                                };
                                debug_assert_eq!(symbolic.poly_args.len(), num_poly);
                            }

                            infer_type.push(IP::Instance(definition_id, symbolic.poly_args.len()));
                            let mut poly_arg_idx = symbolic.poly_args.len();
                            while poly_arg_idx > 0 {
                                poly_arg_idx -= 1;
                                to_consider.push_front(symbolic.poly_args[poly_arg_idx]);
                            }
                        }
                    }
                }
            }
        }

        InferenceType::new(!has_inferred, infer_type)
    }

    /// Construct an error when an expression's type does not match. This
    /// happens if we infer the expression type from its arguments (e.g. the
    /// expression type of an addition operator is the type of the arguments)
    /// But the expression type was already set due to our parent (e.g. an
    /// "if statement" or a "logical not" always expecting a boolean)
    fn construct_expr_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, arg_id: ExpressionId
    ) -> ParseError2 {
        // TODO: Expand and provide more meaningful information for humans
        let expr = &ctx.heap[expr_id];
        let arg_expr = &ctx.heap[arg_id];
        let expr_type = self.expr_types.get(&expr_id).unwrap();
        let arg_type = self.expr_types.get(&arg_id).unwrap();

        return ParseError2::new_error(
            &ctx.module.source, expr.position(),
            &format!(
                "Incompatible types: this expression expected a '{}'", 
                expr_type.display_name(&ctx.heap)
            )
        ).with_postfixed_info(
            &ctx.module.source, arg_expr.position(),
            &format!(
                "But this expression yields a '{}'",
                arg_type.display_name(&ctx.heap)
            )
        )
    }

    fn construct_arg_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg2_id: ExpressionId
    ) -> ParseError2 {
        let expr = &ctx.heap[expr_id];
        let arg1 = &ctx.heap[arg1_id];
        let arg2 = &ctx.heap[arg2_id];

        let arg1_type = self.expr_types.get(&arg1_id).unwrap();
        let arg2_type = self.expr_types.get(&arg2_id).unwrap();

        return ParseError2::new_error(
            &ctx.module.source, expr.position(),
            "Incompatible types: cannot apply this expression"
        ).with_postfixed_info(
            &ctx.module.source, arg1.position(),
            &format!(
                "Because this expression has type '{}'",
                arg1_type.display_name(&ctx.heap)
            )
        ).with_postfixed_info(
            &ctx.module.source, arg2.position(),
            &format!(
                "But this expression has type '{}'",
                arg2_type.display_name(&ctx.heap)
            )
        )
    }

    fn construct_type_class_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, type_class: TypeClass
    ) -> ParseError2 {
        let expr = &ctx.heap[expr_id];
        let expr_type = self.expr_types.get(&expr_id).unwrap();

        return ParseError2::new_error(
            &ctx.module.source, expr.position(),
            &format!(
                "Incompatible types: got a '{}' but expected a {} type",
                expr_type.display_name(&ctx.heap), type_class
            )
        )
    }

    fn construct_template_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, template: &InferenceType
    ) -> ParseError2 {
        let expr = &ctx.heap[expr_id];
        let expr_type = self.expr_types.get(&expr_id).unwrap();

        return ParseError2::new_error(
            &ctx.module.source, expr.position(),
            &format!(
                "Incompatible types: got a '{}' but expected a '{}'",
                expr_type.display_name(&ctx.heap), template.display_name(&ctx.heap)
            )
        )
    }
}
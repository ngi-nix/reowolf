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

const BOOL_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::Bool ];
const NUMBERLIKE_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::NumberLike ];
const ARRAYLIKE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::ArrayLike, InferenceTypePart::Unknown ];

/// TODO: @performance Turn into PartialOrd+Ord to simplify checks
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum InferenceTypePart {
    // A marker with an identifier which we can use to seek subsections of the 
    // inferred type
    Marker(usize),
    // Completely unknown type, needs to be inferred
    Unknown,
    // Partially known type, may be inferred to to be the appropriate related 
    // type.
    // IndexLike,      // index into array/slice
    NumberLike,     // any kind of integer/float
    IntegerLike,    // any kind of integer
    ArrayLike,      // array or slice. Note that this must have a subtype
    // Special types that cannot be instantiated by the user
    Void, // For builtin functions that do not return anything
    // Concrete types without subtypes
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
    // A user-defined type with any number of subtypes
    Instance(DefinitionId, usize)
}

impl InferenceTypePart {
    fn is_marker(&self) -> bool {
        if let InferenceTypePart::Marker(_) = self { true } else { false }
    }

    /// Checks if the type is concrete, markers are interpreted as concrete
    /// types.
    fn is_concrete(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike | ITP::ArrayLike => false,
            _ => true
        }
    }

    fn is_concrete_number(&self) -> bool {
        // TODO: @float
        use InferenceTypePart as ITP;
        match self {
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long => true,
            _ => false,
        }
    }

    fn is_concrete_integer(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long => true,
            _ => false,
        }
    }

    fn is_concrete_array_or_slice(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Array | ITP::Slice => true,
            _ => false,
        }
    }

    /// Returns the change in "iteration depth" when traversing this particular
    /// part. The iteration depth is used to traverse the tree in a linear 
    /// fashion. It is basically `number_of_subtypes - 1`
    fn depth_change(&self) -> i32 {
        use InferenceTypePart as ITP;
        match &self {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike |
            ITP::Void | ITP::Message | ITP::Bool | 
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long | 
            ITP::String => {
                -1
            },
            ITP::Marker(_) | ITP::ArrayLike | ITP::Array | ITP::Slice | 
            ITP::Input | ITP::Output => {
                // One subtype, so do not modify depth
                0
            },
            ITP::Instance(_, num_args) => {
                (*num_args as i32) - 1
            }
        }
    }
}

struct InferenceType {
    has_marker: bool,
    is_done: bool,
    parts: Vec<InferenceTypePart>,
}

impl InferenceType {
    fn new(has_marker: bool, is_done: bool, parts: Vec<InferenceTypePart>) -> Self {
        if cfg!(debug_assertions) {
            debug_assert(!parts.is_empty());
            if !has_marker {
                debug_assert!(parts.iter().all(|v| !v.is_marker()));
            }
            if is_done {
                debug_assert!(parts.iter().all(|v| v.is_concrete()));
            }
        }
        Self{ has_marker, is_done, parts }
    }

    // TODO: @performance, might all be done inline in the type inference methods
    fn recompute_is_done(&mut self) {
        self.done = self.parts.iter().all(|v| v.is_concrete());
    }

    /// Checks if type is, or may be inferred as, a number
    // TODO: @float
    fn might_be_number(&self) -> bool {
        use InferenceTypePart as ITP;

        // TODO: @marker?
        if self.parts.len() != 1 { return false; }
        match self.parts[0] {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike |
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long =>
                true,
            _ =>
                false,
        }
    }

    /// Checks if type is, or may be inferred as, an integer
    fn might_be_integer(&self) -> bool {
        use InferenceTypePart as ITP;

        // TODO: @marker?
        if self.parts.len() != 1 { return false; }
        match self.parts[0] {
            ITP::Unknown | ITP::IntegerLike |
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long =>
                true,
            _ =>
                false,
        }
    }

    /// Checks if type is, or may be inferred as, a boolean
    fn might_be_boolean(&self) -> bool {
        use InferenceTypePart as ITP;

        // TODO: @marker?
        if self.parts.len() != 1 { return false; }
        match self.parts[0] {
            ITP::Unknown | ITP::Bool => true,
            _ => false
        }
    }

    /// Given that the `parts` are a depth-first serialized tree of types, this
    /// function finds the subtree anchored at a specific node. The returned 
    /// index is exclusive.
    fn find_subtree_end_idx(parts: &[InferenceTypePart], start_idx: usize) -> usize {
        let mut depth = 1;
        let mut idx = start_idx;

        while idx < parts.len() {
            depth += parts[idx].depth_change();
            if depth == 0 {
                return idx + 1;
            }
            idx += 1;
        }

        // If here, then the inference type is malformed
        unreachable!();
    }

    /// Call that attempts to infer the part at `to_infer.parts[to_infer_idx]` 
    /// using the subtree at `template.parts[template_idx]`. Will return 
    /// `Some(depth_change_due_to_traversal)` if type inference has been 
    /// applied. In this case the indices will also be modified to point to the 
    /// next part in both templates. If type inference has not (or: could not) 
    /// be applied then `None` will be returned. Note that this might mean that 
    /// the types are incompatible.
    ///
    /// As this is a helper functions, some assumptions: the parts are not 
    /// exactly equal, and neither of them contains a marker.
    fn infer_part_for_single_type(
        to_infer: &mut InferenceType, to_infer_idx: &mut usize,
        template_parts: &[InferenceTypePart], template_idx: &mut usize,
    ) -> Option<i32> {
        use InferenceTypePart as ITP;

        let to_infer_part = &to_infer.parts[*to_infer_idx];
        let template_part = &template_parts[*template_idx];

        // Check for programmer mistakes
        if cfg!(debug_assertions) {
            debug_assert_ne!(to_infer_part, template_part);
            if let ITP::Marker(_) = to_infer_part {
                debug_assert!(false, "marker encountered in 'infer part'");
            }
            if let ITP::Marker(_) = template_part {
                debug_assert!(false, "marker encountered in 'infer part'");
            }
        }

        // Inference of a somewhat-specified type
        if (*to_infer_part == ITP::IntegerLike && template_part.is_concrete_int()) ||
            (*to_infer_part == ITP::NumberLike && template_part.is_concrete_number())||
            (*to_infer_part == ITP::ArrayLike && template_part.is_concrete_array_or_slice())
        {
            let depth_change = to_infer_part.depth_change();
            debug_assert_eq!(depth_change, template_part.depth_change());
            to_infer.parts[*to_infer_idx] = template_part.clone();
            *to_infer_idx += 1;
            *template_idx += 1;
            return Some(depth_change);
        }

        // Inference of a completely unknown type
        if *to_infer_part == ITP::Unknown {
            // template part is different, so cannot be unknown, hence copy the
            // entire subtree
            let template_end_idx = Self::find_subtree_end_idx(template_parts, *template_idx);
            to_infer.parts[*to_infer_idx] = template_part.clone();
            *to_infer_idx += 1;
            for insert_idx in (*template_idx + 1)..template_end_idx {
                to_infer.parts.insert(*to_infer_idx, template_parts[insert_idx].clone());
                *to_infer_idx += 1;
            }
            *template_idx = template_end_idx;

            // Note: by definition the LHS was Unknown and the RHS traversed a 
            // full subtree.
            return Some(-1);
        }

        None
    }

    /// Attempts to infer types between two `InferenceType` instances. This 
    /// function is unsafe as it accepts pointers to work around Rust's 
    /// borrowing rules. The caller must ensure that the pointers are distinct.
    unsafe fn infer_subtrees_for_both_types(
        type_a: *mut InferenceType, start_idx_a: usize,
        type_b: *mut InferenceType, start_idx_b: usize
    ) -> DualInferenceResult {
        use InferenceTypePart as ITP;

        debug_assert!(!std::ptr::eq(type_a, type_b), "same inference types");
        let type_a = &mut *type_a;
        let type_b = &mut *type_b;

        let mut modified_a = false;
        let mut modified_b = false;
        let mut idx_a = start_idx_a;
        let mut idx_b = start_idx_b;
        let mut depth = 1;

        while depth > 0 {
            // Advance indices if we encounter markers or equal parts
            let part_a = &type_a.parts[idx_a];
            let part_b = &type_b.parts[idx_b];
            
            if part_a == part_b {
                depth += part_a.depth_change();
                debug_assert_eq!(depth, part_b.depth_change());
                idx_a += 1;
                idx_b += 1;
                continue;
            }
            if let ITP::Marker(_) = part_a { idx_a += 1; continue; }
            if let ITP::Marker(_) = part_b { idx_b += 1; continue; }

            // Types are not equal and are both not markers
            if let Some(depth_change) = Self::infer_part_for_single_type(type_a, &mut idx_a, &type_b.parts, &mut idx_b) {
                depth += depth_change;
                modified_a = true;
                continue;
            }
            if let Some(depth_change) = Self::infer_part_for_single_type(type_b, &mut idx_b, &type_a.parts, &mut idx_a) {
                depth += depth_change;
                modified_b = true;
                continue;
            }

            // And can also not be inferred in any way: types must be incompatible
            return DualInferenceResult::Incompatible;
        }

        if modified_a { type_a.recompute_is_done(); }
        if modified_b { type_b.recompute_is_done(); }

        // If here then we completely inferred the subtrees.
        match (modified_a, modified_b) {
            (false, false) => DualInferenceResult::Neither,
            (false, true) => DualInferenceResult::Second,
            (true, false) => DualInferenceResult::First,
            (true, true) => DualInferenceResult::Both
        }
    }

    /// Attempts to infer the first subtree based on the template. Like
    /// `infer_subtrees_for_both_types`, but now only applying inference to
    /// `to_infer` based on the type information in `template`.
    /// Secondary use is to make sure that a type follows a certain template.
    fn infer_subtree_for_single_type(
        to_infer: &mut InferenceType, mut to_infer_idx: usize,
        template: &[InferenceTypePart], mut template_idx: usize,
    ) -> SingleInferenceResult {
        use InferenceTypePart as ITP;

        let mut modified = false;
        let mut depth = 1;

        while depth > 0 {
            let to_infer_part = &to_infer.parts[to_infer_idx];
            let template_part = &template.parts[template_idx];

            if part_a == part_b {
                depth += to_infer_part.depth_change();
                debug_assert!(depth, template_part.depth_change());
                to_infer_idx += 1;
                template_idx += 1;
                continue;
            }
            if let ITP::Marker(_) = to_infer_part { to_infer_idx += 1; continue; }
            if let ITP::Marker(_) = template_part { to_infer_idx += 1; continue; }

            // Types are not equal and not markers
            if let Some(depth_change) = Self::infer_part_for_single_type(
                to_infer, &mut to_infer_idx, template, &mut template_idx
            ) {
                depth += depth_change;
                modified = true;
                continue;
            }

            return SingleInferenceResult::Incompatible
        }

        return if modified {
            to_infer.recompute_is_done();
            SingleInferenceResult::Modified
        } else {
            SingleInferenceResult::Unmodified
        }
    }

    /// Returns a human-readable version of the type. Only use for debugging
    /// or returning errors (since it allocates a string).
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
                        for _sub_idx in 1..*num_sub {
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

/// Iterator over the subtrees that follow a marker in an `InferenceType`
/// instance
struct InferenceTypeMarkerIter<'a> {
    parts: &'a [InferenceTypePart],
    idx: usize,
}

impl<'a> Iterator for InferenceTypeMarkerIter<'a> {
    type Item = (usize, &'a [InferenceTypePart]);

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate until we find a marker
        while self.idx < self.parts.len() {
            if let InferenceTypePart::Marker(marker) = self.parts[self.idx] {
                // Found a marker, find the subtree end
                let start_idx = self.idx + 1;
                let end_idx = InferenceType::find_subtree_end_idx(self.parts, start_idx);

                // Modify internal index, then return items
                self.idx = end_idx;
                return Some((marker, &self.parts[start_idx..end_idx]))
            }

            self.idx += 1;
        }

        None
    }
}

/// Extra data needed to fully resolve polymorphic types. Each argument contains
/// "markers" with an index corresponding to the polymorphic variable. Hence if
/// we advance any of the inference types with markers then we need to compare
/// them against the polymorph type. If the polymorph type is then progressed
/// then we need to apply that to all arguments that contain that polymorphic
/// type.
struct PolymorphInferenceType {
    definition: DefinitionId,
    poly_vars: Vec<InferenceType>,
    arguments: Vec<InferenceType>,
    return_type: InferenceType,
}

#[derive(PartialEq, Eq)]
enum DualInferenceResult {
    Neither,        // neither argument is clarified
    First,          // first argument is clarified using the second one
    Second,         // second argument is clarified using the first one
    Both,           // both arguments are clarified
    Incompatible,   // types are incompatible: programmer error
}

impl DualInferenceResult {
    fn modified_any(&self) -> bool {
        match self {
            DualInferenceResult::First | DualInferenceResult::Second | DualInferenceResult::Both => true,
            _ => false
        }
    }
    fn modified_lhs(&self) -> bool {
        match self {
            DualInferenceResult::First | DualInferenceResult::Both => true,
            _ => false
        }
    }
    fn modified_rhs(&self) -> bool {
        match self {
            DualInferenceResult::Second | DualInferenceResult::Both => true,
            _ => false
        }
    }
}

#[derive(PartialEq, Eq)]
enum SingleInferenceResult {
    Unmodified,
    Modified,
    Incompatible
}

enum DefinitionType{
    None,
    Component(ComponentId),
    Function(FunctionId),
}

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

        self.expr_types.insert(test_expr_id, InferenceType::new(false, true, vec![InferenceTypePart::Bool]));
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

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id);

        let call_expr = &ctx.heap[id];
        // TODO: @performance
        for arg_expr_id in call_expr.arguments.clone() {
            self.visit_expr(ctx, arg_expr_id)?;
        }

        self.progress_call_expr(ctx, id)
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
                // Arguments may be arrays/slices with the same subtype. Output
                // is always an array with that subtype
                (false, false, false)
            },
            BO::LogicalOr | BO::LogicalAnd => {
                // Forced boolean on all
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, BOOL_TEMPLATE.as_slice())?;
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
                let (progress_arg1, progress_arg2) = self.apply_equal2_constraint(ctx, upcast_id, arg1_id, arg2_id)?;
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

        // TODO: Finish this
        Ok(())
    }

    fn progress_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> Result<(), ParseError2> {
        let
            upcast_id = id.upcast();
        let expr = &ctx.heap[id];


    }

    fn queue_expr_parent(&mut self, ctx: &Ctx, expr_id: ExpressionId) {
        if let ExpressionParent::Expression(parent_expr_id, _) = &ctx.heap[expr_id].parent() {
            self.expr_queued.insert(*parent_expr_id);
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
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId, template: &[InferenceTypePart]
    ) -> Result<bool, ParseError2> {
        debug_assert_expr_ids_unique_and_known!(self, expr_id);
        let expr_type = self.expr_types.get_mut(&expr_id).unwrap();
        match InferenceType::infer_subtree_for_single_type(expr_type, 0, template, 0) {
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

        let infer_res = unsafe{ InferenceType::infer_subtrees_for_both_types(arg1_type, 0, arg2_type, 0) };
        if infer_res == DualInferenceResult::Incompatible {
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

        let expr_res = unsafe{ InferenceType::infer_subtrees_for_both_types(expr_type, 0, arg1_type, 0) };
        if expr_res == DualInferenceResult::Incompatible {
            return Err(self.construct_expr_type_error(ctx, expr_id, arg1_id));
        }

        let args_res = unsafe{ InferenceType::infer_subtrees_for_both_types(arg1_type, 0, arg2_type, 0) };
        if args_res == DualInferenceResult::Incompatible {
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
                InferenceType::new(false, false, vec![InferredPart::Unknown]),
            EP::If(_) | EP::While(_) | EP::Assert(_) =>
                // Must be a boolean
                InferenceType::new(false, true, vec![InferredPart::Bool]),
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
                InferenceType::new(false, true, vec![InferredPart::Void]),
            EP::Put(_, 0) =>
                // TODO: Change put to be a builtin function
                // port of "put" call
                InferenceType::new(false, false, vec![InferredPart::Output, InferredPart::Unknown]),
            EP::Put(_, 1) =>
                // TODO: Change put to be a builtin function
                // message of "put" call
                InferenceType::new(false, true, vec![InferredPart::Message]),
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

        InferenceType::new(false, !has_inferred, infer_type)
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
        &self, ctx: &Ctx, expr_id: ExpressionId, template: &[InferenceType]
    ) -> ParseError2 {
        // TODO: @cleanup
        let fake = InferenceType::new(false, false, Vec::from(template));
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
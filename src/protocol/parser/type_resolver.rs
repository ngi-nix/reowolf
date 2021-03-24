/// type_resolver.rs
///
/// Performs type inference and type checking. Type inference is implemented by
/// applying constraints on (sub)trees of types. During this process the
/// resolver takes the `ParserType` structs (the representation of the types
/// written by the programmer), converts them to `InferenceType` structs (the
/// temporary data structure used during type inference) and attempts to arrive
/// at `ConcreteType` structs (the representation of a fully checked and
/// validated type).
///
/// The resolver will visit every statement and expression relevant to the
/// procedure and insert and determine its initial type based on context (e.g. a
/// return statement's expression must match the function's return type, an
/// if statement's test expression must evaluate to a boolean). When all are
/// visited we attempt to make progress in evaluating the types. Whenever a type
/// is progressed we queue the related expressions for further type progression.
/// Once no more expressions are in the queue the algorithm is finished. At this
/// point either all types are inferred (or can be trivially implicitly
/// determined), or we have incomplete types. In the latter casee we return an
/// error.
///
/// Inference may be applied on non-polymorphic procedures and on polymorphic
/// procedures. When dealing with a non-polymorphic procedure we apply the type
/// resolver and annotate the AST with the `ConcreteType`s. When dealing with
/// polymorphic procedures we will only annotate the AST once, preserving
/// references to polymorphic variables. Any later pass will perform just the
/// type checking.
///
/// TODO: Needs an optimization pass
/// TODO: Needs a cleanup pass
/// TODO: Disallow `Void` types in various expressions (and other future types)
/// TODO: Maybe remove msg type?

macro_rules! enabled_debug_print {
    (false, $name:literal, $format:literal) => {};
    (false, $name:literal, $format:literal, $($args:expr),*) => {};
    (true, $name:literal, $format:literal) => {
        println!("[{}] {}", $name, $format)
    };
    (true, $name:literal, $format:literal, $($args:expr),*) => {
        println!("[{}] {}", $name, format!($format, $($args),*))
    };
}

macro_rules! debug_log {
    ($format:literal) => {
        enabled_debug_print!(true, "types", $format);
    };
    ($format:literal, $($args:expr),*) => {
        enabled_debug_print!(true, "types", $format, $($args),*);
    };
}

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
use std::collections::hash_map::Entry;
use crate::protocol::parser::type_resolver::InferenceTypePart::IntegerLike;

const MESSAGE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::Message, InferenceTypePart::Byte ];
const BOOL_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::Bool ];
const NUMBERLIKE_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::NumberLike ];
const INTEGERLIKE_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::IntegerLike ];
const ARRAY_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::Array, InferenceTypePart::Unknown ];
const ARRAYLIKE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::ArrayLike, InferenceTypePart::Unknown ];
const PORTLIKE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::PortLike, InferenceTypePart::Unknown ];

/// TODO: @performance Turn into PartialOrd+Ord to simplify checks
/// TODO: @types Remove the Message -> Byte hack at some point...
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum InferenceTypePart {
    // A marker with an identifier which we can use to retrieve the type subtree
    // that follows the marker. This is used to perform type inference on
    // polymorphs: an expression may determine the polymorphs type, after we
    // need to apply that information to all other places where the polymorph is
    // used.
    MarkerDefinition(usize), // marker for polymorph types on a procedure's definition
    MarkerBody(usize), // marker for polymorph types within a procedure body
    // Completely unknown type, needs to be inferred
    Unknown,
    // Partially known type, may be inferred to to be the appropriate related 
    // type.
    // IndexLike,      // index into array/slice
    NumberLike,     // any kind of integer/float
    IntegerLike,    // any kind of integer
    ArrayLike,      // array or slice. Note that this must have a subtype
    PortLike,       // input or output port
    // Special types that cannot be instantiated by the user
    Void, // For builtin functions that do not return anything
    // Concrete types without subtypes
    Bool,
    Byte,
    Short,
    Int,
    Long,
    String,
    // One subtype
    Message,
    Array,
    Slice,
    Input,
    Output,
    // A user-defined type with any number of subtypes
    Instance(DefinitionId, usize)
}

impl InferenceTypePart {
    fn is_marker(&self) -> bool {
        use InferenceTypePart as ITP;

        match self {
            ITP::MarkerDefinition(_) | ITP::MarkerBody(_) => true,
            _ => false,
        }
    }

    /// Checks if the type is concrete, markers are interpreted as concrete
    /// types.
    fn is_concrete(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike | 
            ITP::ArrayLike | ITP::PortLike => false,
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

    fn is_concrete_msg_array_or_slice(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Array | ITP::Slice | ITP::Message => true,
            _ => false,
        }
    }

    fn is_concrete_port(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Input | ITP::Output => true,
            _ => false,
        }
    }

    /// Checks if a part is less specific than the argument. Only checks for 
    /// single-part inference (i.e. not the replacement of an `Unknown` variant 
    /// with the argument)
    fn may_be_inferred_from(&self, arg: &InferenceTypePart) -> bool {
        use InferenceTypePart as ITP;

        (*self == ITP::IntegerLike && arg.is_concrete_integer()) ||
        (*self == ITP::NumberLike && (arg.is_concrete_number() || *arg == ITP::IntegerLike)) ||
        (*self == ITP::ArrayLike && arg.is_concrete_msg_array_or_slice()) ||
        (*self == ITP::PortLike && arg.is_concrete_port())
    }

    /// Returns the change in "iteration depth" when traversing this particular
    /// part. The iteration depth is used to traverse the tree in a linear 
    /// fashion. It is basically `number_of_subtypes - 1`
    fn depth_change(&self) -> i32 {
        use InferenceTypePart as ITP;
        match &self {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike |
            ITP::Void | ITP::Bool |
            ITP::Byte | ITP::Short | ITP::Int | ITP::Long | 
            ITP::String => {
                -1
            },
            ITP::MarkerDefinition(_) | ITP::MarkerBody(_) |
            ITP::ArrayLike | ITP::Message | ITP::Array | ITP::Slice |
            ITP::PortLike | ITP::Input | ITP::Output => {
                // One subtype, so do not modify depth
                0
            },
            ITP::Instance(_, num_args) => {
                (*num_args as i32) - 1
            }
        }
    }
}

impl From<ConcreteTypePart> for InferenceTypePart {
    fn from(v: ConcreteTypePart) -> InferenceTypePart {
        use ConcreteTypePart as CTP;
        use InferenceTypePart as ITP;

        match v {
            CTP::Marker(_) => {
                unreachable!("encountered marker while converting concrete type to inferred type");
            }
            CTP::Void => ITP::Void,
            CTP::Message => ITP::Message,
            CTP::Bool => ITP::Bool,
            CTP::Byte => ITP::Byte,
            CTP::Short => ITP::Short,
            CTP::Int => ITP::Int,
            CTP::Long => ITP::Long,
            CTP::String => ITP::String,
            CTP::Array => ITP::Array,
            CTP::Slice => ITP::Slice,
            CTP::Input => ITP::Input,
            CTP::Output => ITP::Output,
            CTP::Instance(id, num) => ITP::Instance(id, num),
        }
    }
}

#[derive(Debug)]
struct InferenceType {
    has_body_marker: bool,
    is_done: bool,
    parts: Vec<InferenceTypePart>,
}

impl InferenceType {
    fn new(has_body_marker: bool, is_done: bool, parts: Vec<InferenceTypePart>) -> Self {
        if cfg!(debug_assertions) {
            debug_assert!(!parts.is_empty());
            if !has_body_marker {
                debug_assert!(parts.iter().all(|v| {
                    if let InferenceTypePart::MarkerBody(_) = v { false } else { true }
                }));
            }
            if is_done {
                debug_assert!(parts.iter().all(|v| v.is_concrete()));
            }
        }
        Self{ has_body_marker: has_body_marker, is_done, parts }
    }

    fn replace_subtree(&mut self, start_idx: usize, with: &[InferenceTypePart]) {
         let end_idx = Self::find_subtree_end_idx(&self.parts, start_idx);
        debug_assert_eq!(with.len(), Self::find_subtree_end_idx(with, 0));
        self.parts.splice(start_idx..end_idx, with.iter().cloned());
        self.recompute_is_done();
    }

    // TODO: @performance, might all be done inline in the type inference methods
    fn recompute_is_done(&mut self) {
        self.is_done = self.parts.iter().all(|v| v.is_concrete());
    }

    /// Returns an iterator over all body markers and the partial type tree that
    /// follows those markers.
    fn body_marker_iter(&self) -> InferenceTypeMarkerIter {
        InferenceTypeMarkerIter::new(&self.parts)
    }

    /// Attempts to find a specific type part appearing at or after the
    /// specified index. If found then the partial type tree's bounding indices
    /// that follow that marker are returned.
    fn find_subtree_idx_for_part(&self, part: InferenceTypePart, mut idx: usize) -> Option<(usize, usize)> {
        debug_assert!(part.depth_change() >= 0, "cannot find subtree for leaf part");
        while idx < self.parts.len() {
            if part == self.parts[idx] {
                // Found the specified part
                let start_idx = idx + 1;
                let end_idx = Self::find_subtree_end_idx(&self.parts, start_idx);
                return Some((start_idx, end_idx))
            }

            idx += 1;
        }

        None
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
    /// exactly equal, and neither of them contains a marker. Also: only the
    /// `to_infer` parts are checked for inference. It might be that this 
    /// function returns `None`, but that that `template` is still compatible
    /// with `to_infer`, e.g. when `template` has an `Unknown` part.
    fn infer_part_for_single_type(
        to_infer: &mut InferenceType, to_infer_idx: &mut usize,
        template_parts: &[InferenceTypePart], template_idx: &mut usize,
    ) -> Option<i32> {
        use InferenceTypePart as ITP;

        let to_infer_part = &to_infer.parts[*to_infer_idx];
        let template_part = &template_parts[*template_idx];

        // TODO: Maybe do this differently?
        let mut template_definition_marker = None;
        if *template_idx > 0 {
            if let ITP::MarkerDefinition(marker) = &template_parts[*template_idx - 1] {
                template_definition_marker = Some(*marker)
            }
        }

        // Check for programmer mistakes
        debug_assert_ne!(to_infer_part, template_part);
        debug_assert!(!to_infer_part.is_marker(), "marker encountered in 'infer part'");
        debug_assert!(!template_part.is_marker(), "marker encountered in 'template part'");

        // Inference of a somewhat-specified type
        if to_infer_part.may_be_inferred_from(template_part) {
            let depth_change = to_infer_part.depth_change();
            debug_assert_eq!(depth_change, template_part.depth_change());

            if let Some(marker) = template_definition_marker {
                to_infer.parts.insert(*to_infer_idx, ITP::MarkerDefinition(marker));
                *to_infer_idx += 1;
            }

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
            let erase_offset = if let Some(marker) = template_definition_marker {
                to_infer.parts[*to_infer_idx] = ITP::MarkerDefinition(marker);
                *to_infer_idx += 1;
                0
            } else {
                1
            };

            to_infer.parts.splice(
                *to_infer_idx..*to_infer_idx + erase_offset,
                template_parts[*template_idx..template_end_idx].iter().cloned()
            );
            *to_infer_idx += (template_end_idx - *template_idx);
            *template_idx = template_end_idx;

            // Note: by definition the LHS was Unknown and the RHS traversed a 
            // full subtree.
            return Some(-1);
        }

        None
    }

    /// Call that checks if the `to_check` part is compatible with the `infer`
    /// part. This is essentially a copy of `infer_part_for_single_type`, but
    /// without actually copying the type parts.
    fn check_part_for_single_type(
        to_check_parts: &[InferenceTypePart], to_check_idx: &mut usize,
        template_parts: &[InferenceTypePart], template_idx: &mut usize
    ) -> Option<i32> {
        use InferenceTypePart as ITP;

        let to_check_part = &to_check_parts[*to_check_idx];
        let template_part = &template_parts[*template_idx];

        // Checking programmer errors
        debug_assert_ne!(to_check_part, template_part);
        debug_assert!(!to_check_part.is_marker(), "marker encountered in 'to_check part'");
        debug_assert!(!template_part.is_marker(), "marker encountered in 'template part'");

        if to_check_part.may_be_inferred_from(template_part) {
            let depth_change = to_check_part.depth_change();
            debug_assert_eq!(depth_change, template_part.depth_change());
            *to_check_idx += 1;
            *template_idx += 1;
            return Some(depth_change);
        }

        if *to_check_part == ITP::Unknown {
            *to_check_idx += 1;
            *template_idx = Self::find_subtree_end_idx(template_parts, *template_idx);

            // By definition LHS and RHS had depth change of -1
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

        debug_assert!(!std::ptr::eq(type_a, type_b), "encountered pointers to the same inference type");
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
                let depth_change = part_a.depth_change();
                depth += depth_change;
                debug_assert_eq!(depth_change, part_b.depth_change());
                idx_a += 1;
                idx_b += 1;
                continue;
            }
            if part_a.is_marker() { idx_a += 1; continue; }
            if part_b.is_marker() { idx_b += 1; continue; }

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
        let mut modified = false;
        let mut depth = 1;

        while depth > 0 {
            let to_infer_part = &to_infer.parts[to_infer_idx];
            let template_part = &template[template_idx];

            if to_infer_part == template_part {
                let depth_change = to_infer_part.depth_change();
                depth += depth_change;
                debug_assert_eq!(depth_change, template_part.depth_change());
                to_infer_idx += 1;
                template_idx += 1;
                continue;
            }
            if to_infer_part.is_marker() { to_infer_idx += 1; continue; }
            if template_part.is_marker() { template_idx += 1; continue; }

            // Types are not equal and not markers. So check if we can infer 
            // anything
            if let Some(depth_change) = Self::infer_part_for_single_type(
                to_infer, &mut to_infer_idx, template, &mut template_idx
            ) {
                depth += depth_change;
                modified = true;
                continue;
            }

            // We cannot infer anything, but the template may still be 
            // compatible with the type we're inferring
            if let Some(depth_change) = Self::check_part_for_single_type(
                template, &mut template_idx, &to_infer.parts, &mut to_infer_idx
            ) {
                depth += depth_change;
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

    /// Checks if both types are compatible, doesn't perform any inference
    fn check_subtrees(
        type_parts_a: &[InferenceTypePart], start_idx_a: usize,
        type_parts_b: &[InferenceTypePart], start_idx_b: usize
    ) -> bool {
        let mut depth = 1;
        let mut idx_a = start_idx_a;
        let mut idx_b = start_idx_b;

        while depth > 0 {
            let part_a = &type_parts_a[idx_a];
            let part_b = &type_parts_b[idx_b];

            if part_a == part_b {
                let depth_change = part_a.depth_change();
                depth += depth_change;
                debug_assert_eq!(depth_change, part_b.depth_change());
                idx_a += 1;
                idx_b += 1;
                continue;
            }
            
            if part_a.is_marker() { idx_a += 1; continue; }
            if part_b.is_marker() { idx_b += 1; continue; }

            if let Some(depth_change) = Self::check_part_for_single_type(
                type_parts_a, &mut idx_a, type_parts_b, &mut idx_b
            ) {
                depth += depth_change;
                continue;
            }
            if let Some(depth_change) = Self::check_part_for_single_type(
                type_parts_b, &mut idx_b, type_parts_a, &mut idx_a
            ) {
                depth += depth_change;
                continue;
            }

            return false;
        }

        true
    }

    /// Performs the conversion of the inference type into a concrete type.
    /// By calling this function you must make sure that no unspecified types
    /// (e.g. Unknown or IntegerLike) exist in the type.
    fn write_concrete_type(&self, concrete_type: &mut ConcreteType) {
        use InferenceTypePart as ITP;
        use ConcreteTypePart as CTP;

        // Make sure inference type is specified but concrete type is not yet specified
        debug_assert!(!self.parts.is_empty());
        debug_assert!(concrete_type.parts.is_empty());
        concrete_type.parts.reserve(self.parts.len());

        let mut idx = 0;
        while idx < self.parts.len() {
            let part = &self.parts[idx];
            let converted_part = match part {
                ITP::MarkerDefinition(marker) => {
                    // Outer markers are converted to regular markers, we
                    // completely remove the type subtree that follows it
                    idx = InferenceType::find_subtree_end_idx(&self.parts, idx + 1);
                    concrete_type.parts.push(CTP::Marker(*marker));
                    continue;
                },
                ITP::MarkerBody(_) => {
                    // Inner markers are removed when writing to the concrete
                    // type.
                    idx += 1;
                    continue;
                },
                ITP::Unknown | ITP::NumberLike | ITP::IntegerLike | ITP::ArrayLike | ITP::PortLike => {
                    unreachable!("Attempted to convert inference type part {:?} into concrete type", part);
                },
                ITP::Void => CTP::Void,
                ITP::Message => CTP::Message,
                ITP::Bool => CTP::Bool,
                ITP::Byte => CTP::Byte,
                ITP::Short => CTP::Short,
                ITP::Int => CTP::Int,
                ITP::Long => CTP::Long,
                ITP::String => CTP::String,
                ITP::Array => CTP::Array,
                ITP::Slice => CTP::Slice,
                ITP::Input => CTP::Input,
                ITP::Output => CTP::Output,
                ITP::Instance(id, num) => CTP::Instance(*id, *num),
            };

            concrete_type.parts.push(converted_part);
            idx += 1;
        }
    }

    /// Writes a human-readable version of the type to a string. Mostly a
    /// function for interior use.
    fn write_display_name(
        buffer: &mut String, heap: &Heap, parts: &[InferenceTypePart], mut idx: usize
    ) -> usize {
        use InferenceTypePart as ITP;

        match &parts[idx] {
            ITP::MarkerDefinition(_) | ITP::MarkerBody(_) => {
                idx = Self::write_display_name(buffer, heap, parts, idx + 1)
            },
            ITP::Unknown => buffer.push_str("?"),
            ITP::NumberLike => buffer.push_str("num?"),
            ITP::IntegerLike => buffer.push_str("int?"),
            ITP::ArrayLike => {
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push_str("[?]");
            },
            ITP::PortLike => {
                buffer.push_str("port?<");
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            }
            ITP::Void => buffer.push_str("void"),
            ITP::Bool => buffer.push_str("bool"),
            ITP::Byte => buffer.push_str("byte"),
            ITP::Short => buffer.push_str("short"),
            ITP::Int => buffer.push_str("int"),
            ITP::Long => buffer.push_str("long"),
            ITP::String => buffer.push_str("str"),
            ITP::Message => {
                buffer.push_str("msg<");
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            },
            ITP::Array => {
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push_str("[]");
            },
            ITP::Slice => {
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push_str("[..]");
            },
            ITP::Input => {
                buffer.push_str("in<");
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            },
            ITP::Output => {
                buffer.push_str("out<");
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            },
            ITP::Instance(definition_id, num_sub) => {
                let definition = &heap[*definition_id];
                buffer.push_str(&String::from_utf8_lossy(&definition.identifier().value));
                if *num_sub > 0 {
                    buffer.push('<');
                    idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                    for _sub_idx in 1..*num_sub {
                        buffer.push_str(", ");
                        idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                    }
                    buffer.push('>');
                }
            },
        }

        idx
    }

    /// Returns the display name of a (part of) the type tree. Will allocate a
    /// string.
    fn partial_display_name(heap: &Heap, parts: &[InferenceTypePart]) -> String {
        let mut buffer = String::with_capacity(parts.len() * 6);
        Self::write_display_name(&mut buffer, heap, parts, 0);
        buffer
    }

    /// Returns the display name of the full type tree. Will allocate a string.
    fn display_name(&self, heap: &Heap) -> String {
        Self::partial_display_name(heap, &self.parts)
    }
}

/// Iterator over the subtrees that follow a marker in an `InferenceType`
/// instance. Returns immutable slices over the internal parts
struct InferenceTypeMarkerIter<'a> {
    parts: &'a [InferenceTypePart],
    idx: usize,
}

impl<'a> InferenceTypeMarkerIter<'a> {
    fn new(parts: &'a [InferenceTypePart]) -> Self {
        Self{ parts, idx: 0 }
    }
}

impl<'a> Iterator for InferenceTypeMarkerIter<'a> {
    type Item = (usize, &'a [InferenceTypePart]);

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate until we find a marker
        while self.idx < self.parts.len() {
            if let InferenceTypePart::MarkerBody(marker) = self.parts[self.idx] {
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

#[derive(Debug, PartialEq, Eq)]
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

#[derive(Debug, PartialEq, Eq)]
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

pub(crate) struct ResolveQueueElement {
    pub(crate) root_id: RootId,
    pub(crate) definition_id: DefinitionId,
    pub(crate) monomorph_types: Vec<ConcreteType>,
}

pub(crate) type ResolveQueue = Vec<ResolveQueueElement>;

/// This particular visitor will recurse depth-first into the AST and ensures
/// that all expressions have the appropriate types.
pub(crate) struct TypeResolvingVisitor {
    // Current definition we're typechecking.
    definition_type: DefinitionType,
    poly_vars: Vec<ConcreteType>,

    // Buffers for iteration over substatements and subexpressions
    stmt_buffer: Vec<StatementId>,
    expr_buffer: Vec<ExpressionId>,

    // Mapping from parser type to inferred type. We attempt to continue to
    // specify these types until we're stuck or we've fully determined the type.
    var_types: HashMap<VariableId, VarData>,      // types of variables
    expr_types: HashMap<ExpressionId, InferenceType>,   // types of expressions
    extra_data: HashMap<ExpressionId, ExtraData>,       // data for function call inference
    // Keeping track of which expressions need to be reinferred because the
    // expressions they're linked to made progression on an associated type
    expr_queued: HashSet<ExpressionId>,
}

// TODO: @rename used for calls and struct literals, maybe union literals?
struct ExtraData {
    /// Progression of polymorphic variables (if any)
    poly_vars: Vec<InferenceType>,
    /// Progression of types of call arguments or struct members
    embedded: Vec<InferenceType>,
    returned: InferenceType,
}

struct VarData {
    /// Type of the variable
    var_type: InferenceType,
    /// VariableExpressions that use the variable
    used_at: Vec<ExpressionId>,
    /// For channel statements we link to the other variable such that when one
    /// channel's interior type is resolved, we can also resolve the other one.
    linked_var: Option<VariableId>,
}

impl VarData {
    fn new_channel(var_type: InferenceType, other_port: VariableId) -> Self {
        Self{ var_type, used_at: Vec::new(), linked_var: Some(other_port) }
    }
    fn new_local(var_type: InferenceType) -> Self {
        Self{ var_type, used_at: Vec::new(), linked_var: None }
    }
}

impl TypeResolvingVisitor {
    pub(crate) fn new() -> Self {
        TypeResolvingVisitor{
            definition_type: DefinitionType::None,
            poly_vars: Vec::new(),
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            var_types: HashMap::new(),
            expr_types: HashMap::new(),
            extra_data: HashMap::new(),
            expr_queued: HashSet::new(),
        }
    }

    // TODO: @cleanup Unsure about this, maybe a pattern will arise after
    //  a while.
    pub(crate) fn queue_module_definitions(ctx: &Ctx, queue: &mut ResolveQueue) {
        let root_id = ctx.module.root_id;
        let root = &ctx.heap.protocol_descriptions[root_id];
        for definition_id in &root.definitions {
            let definition = &ctx.heap[*definition_id];
            match definition {
                Definition::Function(definition) => {
                    if definition.poly_vars.is_empty() {
                        queue.push(ResolveQueueElement{
                            root_id,
                            definition_id: *definition_id,
                            monomorph_types: Vec::new(),
                        })
                    }
                },
                Definition::Component(definition) => {
                    if definition.poly_vars.is_empty() {
                        queue.push(ResolveQueueElement{
                            root_id,
                            definition_id: *definition_id,
                            monomorph_types: Vec::new(),
                        })
                    }
                },
                Definition::Enum(_) | Definition::Struct(_) => {},
            }
        }
    }

    pub(crate) fn handle_module_definition(
        &mut self, ctx: &mut Ctx, queue: &mut ResolveQueue, element: ResolveQueueElement
    ) -> VisitorResult {
        // Visit the definition
        debug_assert_eq!(ctx.module.root_id, element.root_id);
        self.reset();
        self.poly_vars.clear();
        self.poly_vars.extend(element.monomorph_types.iter().cloned());
        self.visit_definition(ctx, element.definition_id)?;

        // Keep resolving types
        self.resolve_types(ctx, queue)?;
        Ok(())
    }

    fn reset(&mut self) {
        self.definition_type = DefinitionType::None;
        self.poly_vars.clear();
        self.stmt_buffer.clear();
        self.expr_buffer.clear();
        self.var_types.clear();
        self.expr_types.clear();
        self.extra_data.clear();
        self.expr_queued.clear();
    }
}

impl Visitor2 for TypeResolvingVisitor {
    // Definitions

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentId) -> VisitorResult {
        self.definition_type = DefinitionType::Component(id);

        let comp_def = &ctx.heap[id];
        debug_assert_eq!(comp_def.poly_vars.len(), self.poly_vars.len(), "component polyvars do not match imposed polyvars");

        debug_log!("{}", "-".repeat(50));
        debug_log!("Visiting component '{}': {}", &String::from_utf8_lossy(&comp_def.identifier.value), id.0.index);
        debug_log!("{}", "-".repeat(50));

        for param_id in comp_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let var_type = self.determine_inference_type_from_parser_type(ctx, param.parser_type, true);
            debug_assert!(var_type.is_done, "expected component arguments to be concrete types");
            self.var_types.insert(param_id.upcast(), VarData::new_local(var_type));
        }

        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionId) -> VisitorResult {
        self.definition_type = DefinitionType::Function(id);

        let func_def = &ctx.heap[id];
        debug_assert_eq!(func_def.poly_vars.len(), self.poly_vars.len(), "function polyvars do not match imposed polyvars");

        debug_log!("{}", "-".repeat(50));
        debug_log!("Visiting function '{}': {}", &String::from_utf8_lossy(&func_def.identifier.value), id.0.index);
        debug_log!("{}", "-".repeat(50));

        for param_id in func_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let var_type = self.determine_inference_type_from_parser_type(ctx, param.parser_type, true);
            debug_assert!(var_type.is_done, "expected function arguments to be concrete types");
            self.var_types.insert(param_id.upcast(), VarData::new_local(var_type));
        }

        let body_stmt_id = ctx.heap[id].body;
        self.visit_stmt(ctx, body_stmt_id)
    }

    // Statements

    fn visit_block_stmt(&mut self, ctx: &mut Ctx, id: BlockStatementId) -> VisitorResult {
        // Transfer statements for traversal
        let block = &ctx.heap[id];

        for stmt_id in block.statements.clone() {
            self.visit_stmt(ctx, stmt_id)?;
        }

        Ok(())
    }

    fn visit_local_memory_stmt(&mut self, ctx: &mut Ctx, id: MemoryStatementId) -> VisitorResult {
        let memory_stmt = &ctx.heap[id];

        let local = &ctx.heap[memory_stmt.variable];
        let var_type = self.determine_inference_type_from_parser_type(ctx, local.parser_type, true);
        self.var_types.insert(memory_stmt.variable.upcast(), VarData::new_local(var_type));

        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        let channel_stmt = &ctx.heap[id];

        let from_local = &ctx.heap[channel_stmt.from];
        let from_var_type = self.determine_inference_type_from_parser_type(ctx, from_local.parser_type, true);
        self.var_types.insert(from_local.this.upcast(), VarData::new_channel(from_var_type, channel_stmt.to.upcast()));

        let to_local = &ctx.heap[channel_stmt.to];
        let to_var_type = self.determine_inference_type_from_parser_type(ctx, to_local.parser_type, true);
        self.var_types.insert(to_local.this.upcast(), VarData::new_channel(to_var_type, channel_stmt.from.upcast()));

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

    fn visit_expr_stmt(&mut self, ctx: &mut Ctx, id: ExpressionStatementId) -> VisitorResult {
        let expr_stmt = &ctx.heap[id];
        let subexpr_id = expr_stmt.expression;

        self.visit_expr(ctx, subexpr_id)
    }

    // Expressions

    fn visit_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let assign_expr = &ctx.heap[id];
        let left_expr_id = assign_expr.left;
        let right_expr_id = assign_expr.right;

        self.visit_expr(ctx, left_expr_id)?;
        self.visit_expr(ctx, right_expr_id)?;

        self.progress_assignment_expr(ctx, id)
    }

    fn visit_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

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
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let binary_expr = &ctx.heap[id];
        let lhs_expr_id = binary_expr.left;
        let rhs_expr_id = binary_expr.right;

        self.visit_expr(ctx, lhs_expr_id)?;
        self.visit_expr(ctx, rhs_expr_id)?;

        self.progress_binary_expr(ctx, id)
    }

    fn visit_unary_expr(&mut self, ctx: &mut Ctx, id: UnaryExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let unary_expr = &ctx.heap[id];
        let arg_expr_id = unary_expr.expression;

        self.visit_expr(ctx, arg_expr_id)?;

        self.progress_unary_expr(ctx, id)
    }

    fn visit_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let indexing_expr = &ctx.heap[id];
        let subject_expr_id = indexing_expr.subject;
        let index_expr_id = indexing_expr.index;

        self.visit_expr(ctx, subject_expr_id)?;
        self.visit_expr(ctx, index_expr_id)?;

        self.progress_indexing_expr(ctx, id)
    }

    fn visit_slicing_expr(&mut self, ctx: &mut Ctx, id: SlicingExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let slicing_expr = &ctx.heap[id];
        let subject_expr_id = slicing_expr.subject;
        let from_expr_id = slicing_expr.from_index;
        let to_expr_id = slicing_expr.to_index;

        self.visit_expr(ctx, subject_expr_id)?;
        self.visit_expr(ctx, from_expr_id)?;
        self.visit_expr(ctx, to_expr_id)?;

        self.progress_slicing_expr(ctx, id)
    }

    fn visit_select_expr(&mut self, ctx: &mut Ctx, id: SelectExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let select_expr = &ctx.heap[id];
        let subject_expr_id = select_expr.subject;

        self.visit_expr(ctx, subject_expr_id)?;

        self.progress_select_expr(ctx, id)
    }

    fn visit_array_expr(&mut self, ctx: &mut Ctx, id: ArrayExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let array_expr = &ctx.heap[id];
        // TODO: @performance
        for element_id in array_expr.elements.clone().into_iter() {
            self.visit_expr(ctx, element_id)?;
        }

        self.progress_array_expr(ctx, id)
    }

    fn visit_literal_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;
        self.progress_constant_expr(ctx, id)
    }

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;
        self.insert_initial_call_polymorph_data(ctx, id);

        // TODO: @performance
        let call_expr = &ctx.heap[id];
        for arg_expr_id in call_expr.arguments.clone() {
            self.visit_expr(ctx, arg_expr_id)?;
        }

        self.progress_call_expr(ctx, id)
    }

    fn visit_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let var_expr = &ctx.heap[id];
        debug_assert!(var_expr.declaration.is_some());
        let var_data = self.var_types.get_mut(var_expr.declaration.as_ref().unwrap()).unwrap();
        var_data.used_at.push(upcast_id);

        self.progress_variable_expr(ctx, id)
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

macro_rules! debug_assert_ptrs_distinct {
    // Base case
    ($ptr1:ident, $ptr2:ident) => {
        debug_assert!(!std::ptr::eq($ptr1, $ptr2));
    };
    // Generic case
    ($ptr1:ident, $ptr2:ident, $($tail:ident),+) => {
        debug_assert_ptrs_distinct!($ptr1, $ptr2);
        debug_assert_ptrs_distinct!($ptr2, $($tail),+);
    };
}

impl TypeResolvingVisitor {
    fn resolve_types(&mut self, ctx: &mut Ctx, queue: &mut ResolveQueue) -> Result<(), ParseError2> {
        // Keep inferring until we can no longer make any progress
        while let Some(next_expr_id) = self.expr_queued.iter().next() {
            let next_expr_id = *next_expr_id;
            self.expr_queued.remove(&next_expr_id);
            self.progress_expr(ctx, next_expr_id)?;
        }

        // Should have inferred everything. Check for this and optionally
        // auto-infer the remaining types
        for (expr_id, expr_type) in self.expr_types.iter_mut() {
            if !expr_type.is_done {
                // Auto-infer numberlike/integerlike types to a regular int
                if expr_type.parts.len() == 1 && expr_type.parts[0] == InferenceTypePart::IntegerLike {
                    expr_type.parts[0] = InferenceTypePart::Int;
                } else {
                    let expr = &ctx.heap[*expr_id];
                    return Err(ParseError2::new_error(
                        &ctx.module.source, expr.position(),
                        &format!(
                            "Could not fully infer the type of this expression (got '{}')",
                            expr_type.display_name(&ctx.heap)
                        )
                    ))
                }
            }

            let concrete_type = ctx.heap[*expr_id].get_type_mut();
            expr_type.write_concrete_type(concrete_type);
        }

        // Check all things we need to monomorphize
        // TODO: Struct/enum/union monomorphization
        for (call_expr_id, extra_data) in self.extra_data.iter() {
            if extra_data.poly_vars.is_empty() { continue; }

            // Retrieve polymorph variable specification
            let mut monomorph_types = Vec::with_capacity(extra_data.poly_vars.len());
            for (poly_idx, poly_type) in extra_data.poly_vars.iter().enumerate() {
                if !poly_type.is_done {
                    // TODO: Single clean function for function signatures and polyvars.
                    // TODO: Better error message
                    let expr = &ctx.heap[*call_expr_id];
                    return Err(ParseError2::new_error(
                        &ctx.module.source, expr.position(),
                        &format!(
                            "Could not fully infer the type of polymorphic variable {} of this expression (got '{}')",
                            poly_idx, poly_type.display_name(&ctx.heap)
                        )
                    ))
                }

                let mut concrete_type = ConcreteType::default();
                poly_type.write_concrete_type(&mut concrete_type);
                monomorph_types.insert(poly_idx, concrete_type);
            }

            // Resolve to call expression's definition
            let call_expr = if let Expression::Call(call_expr) = &ctx.heap[*call_expr_id] {
                call_expr
            } else {
                todo!("implement different kinds of polymorph expressions");
            };

            // Add to type table if not yet typechecked
            if let Method::Symbolic(symbolic) = &call_expr.method {
                let definition_id = symbolic.definition.unwrap();
                if !ctx.types.has_monomorph(&definition_id, &monomorph_types) {
                    let root_id = ctx.types
                        .get_base_definition(&definition_id)
                        .unwrap()
                        .ast_root;

                    // Pre-emptively add the monomorph to the type table, but
                    // we still need to perform typechecking on it
                    ctx.types.add_monomorph(&definition_id, monomorph_types.clone());
                    queue.push(ResolveQueueElement {
                        root_id,
                        definition_id,
                        monomorph_types,
                    })
                }
            }
        }

        Ok(())
    }

    fn progress_expr(&mut self, ctx: &mut Ctx, id: ExpressionId) -> Result<(), ParseError2> {
        match &ctx.heap[id] {
            Expression::Assignment(expr) => {
                let id = expr.this;
                self.progress_assignment_expr(ctx, id)
            },
            Expression::Conditional(expr) => {
                let id = expr.this;
                self.progress_conditional_expr(ctx, id)
            },
            Expression::Binary(expr) => {
                let id = expr.this;
                self.progress_binary_expr(ctx, id)
            },
            Expression::Unary(expr) => {
                let id = expr.this;
                self.progress_unary_expr(ctx, id)
            },
            Expression::Indexing(expr) => {
                let id = expr.this;
                self.progress_indexing_expr(ctx, id)
            },
            Expression::Slicing(expr) => {
                let id = expr.this;
                self.progress_slicing_expr(ctx, id)
            },
            Expression::Select(expr) => {
                let id = expr.this;
                self.progress_select_expr(ctx, id)
            },
            Expression::Array(expr) => {
                let id = expr.this;
                self.progress_array_expr(ctx, id)
            },
            Expression::Literal(expr) => {
                let id = expr.this;
                self.progress_constant_expr(ctx, id)
            },
            Expression::Call(expr) => {
                let id = expr.this;
                self.progress_call_expr(ctx, id)
            },
            Expression::Variable(expr) => {
                let id = expr.this;
                self.progress_variable_expr(ctx, id)
            }
        }
    }

    fn progress_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> Result<(), ParseError2> {
        use AssignmentOperator as AO;

        // TODO: Assignable check
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg1_expr_id = expr.left;
        let arg2_expr_id = expr.right;

        debug_log!("Assignment expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.expr_types.get(&arg1_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type: {}", self.expr_types.get(&arg2_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        let progress_base = match expr.operation {
            AO::Set =>
                false,
            AO::Multiplied | AO::Divided | AO::Added | AO::Subtracted =>
                self.apply_forced_constraint(ctx, upcast_id, &NUMBERLIKE_TEMPLATE)?,
            AO::Remained | AO::ShiftedLeft | AO::ShiftedRight |
            AO::BitwiseAnded | AO::BitwiseXored | AO::BitwiseOred =>
                self.apply_forced_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?,
        };

        let (progress_expr, progress_arg1, progress_arg2) = self.apply_equal3_constraint(
            ctx, upcast_id, arg1_expr_id, arg2_expr_id, 0
        )?;

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_arg1, self.expr_types.get(&arg1_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.expr_types.get(&arg2_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type [{}]: {}", progress_base || progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));


        if progress_base || progress_expr { self.queue_expr_parent(ctx, upcast_id); }
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

        debug_log!("Conditional expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.expr_types.get(&arg1_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type: {}", self.expr_types.get(&arg2_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        let (progress_expr, progress_arg1, progress_arg2) = self.apply_equal3_constraint(
            ctx, upcast_id, arg1_expr_id, arg2_expr_id, 0
        )?;

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_arg1, self.expr_types.get(&arg1_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.expr_types.get(&arg2_expr_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

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

        debug_log!("Binary expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.expr_types.get(&arg1_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type: {}", self.expr_types.get(&arg2_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        let (progress_expr, progress_arg1, progress_arg2) = match expr.operation {
            BO::Concatenate => {
                // Arguments may be arrays/slices, output is always an array
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &ARRAY_TEMPLATE)?;
                let progress_arg1 = self.apply_forced_constraint(ctx, arg1_id, &ARRAYLIKE_TEMPLATE)?;
                let progress_arg2 = self.apply_forced_constraint(ctx, arg2_id, &ARRAYLIKE_TEMPLATE)?;

                // If they're all arraylike, then we want the subtype to match
                let (subtype_expr, subtype_arg1, subtype_arg2) =
                    self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 1)?;

                (progress_expr || subtype_expr, progress_arg1 || subtype_arg1, progress_arg2 || subtype_arg2)
            },
            BO::LogicalOr | BO::LogicalAnd => {
                // Forced boolean on all
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg1 = self.apply_forced_constraint(ctx, arg1_id, &BOOL_TEMPLATE)?;
                let progress_arg2 = self.apply_forced_constraint(ctx, arg2_id, &BOOL_TEMPLATE)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::BitwiseOr | BO::BitwiseXor | BO::BitwiseAnd | BO::Remainder | BO::ShiftLeft | BO::ShiftRight => {
                // All equal of integer type
                let progress_base = self.apply_forced_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg1, progress_arg2) =
                    self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg1, progress_base || progress_arg2)
            },
            BO::Equality | BO::Inequality => {
                // Equal2 on args, forced boolean output
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let (progress_arg1, progress_arg2) =
                    self.apply_equal2_constraint(ctx, upcast_id, arg1_id, 0, arg2_id, 0)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::LessThan | BO::GreaterThan | BO::LessThanEqual | BO::GreaterThanEqual => {
                // Equal2 on args with numberlike type, forced boolean output
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg_base = self.apply_forced_constraint(ctx, arg1_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_arg1, progress_arg2) =
                    self.apply_equal2_constraint(ctx, upcast_id, arg1_id, 0, arg2_id, 0)?;

                (progress_expr, progress_arg_base || progress_arg1, progress_arg_base || progress_arg2)
            },
            BO::Add | BO::Subtract | BO::Multiply | BO::Divide => {
                // All equal of number type
                let progress_base = self.apply_forced_constraint(ctx, upcast_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg1, progress_arg2) =
                    self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg1, progress_base || progress_arg2)
            },
        };

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_arg1, self.expr_types.get(&arg1_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.expr_types.get(&arg2_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

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

        debug_log!("Unary expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg  type: {}", self.expr_types.get(&arg_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        let (progress_expr, progress_arg) = match expr.operation {
            UO::Positive | UO::Negative => {
                // Equal types of numeric class
                let progress_base = self.apply_forced_constraint(ctx, upcast_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, arg_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg)
            },
            UO::BitwiseNot | UO::PreIncrement | UO::PreDecrement | UO::PostIncrement | UO::PostDecrement => {
                // Equal types of integer class
                let progress_base = self.apply_forced_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, arg_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg)
            },
            UO::LogicalNot => {
                // Both booleans
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                (progress_expr, progress_arg)
            }
        };

        debug_log!(" * After:");
        debug_log!("   - Arg  type [{}]: {}", progress_arg, self.expr_types.get(&arg_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg { self.queue_expr(arg_id); }

        Ok(())
    }

    fn progress_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;
        let index_id = expr.index;

        debug_log!("Indexing expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Index   type: {}", self.expr_types.get(&index_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        // Make sure subject is arraylike and index is integerlike
        let progress_subject_base = self.apply_forced_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
        let progress_index = self.apply_forced_constraint(ctx, index_id, &INTEGERLIKE_TEMPLATE)?;

        // Make sure if output is of T then subject is Array<T>
        let (progress_expr, progress_subject) =
            self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, subject_id, 1)?;

        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject_base || progress_subject, self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Index   type [{}]: {}", progress_index, self.expr_types.get(&index_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_subject_base || progress_subject { self.queue_expr(subject_id); }
        if progress_index { self.queue_expr(index_id); }

        Ok(())
    }

    fn progress_slicing_expr(&mut self, ctx: &mut Ctx, id: SlicingExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;
        let from_id = expr.from_index;
        let to_id = expr.to_index;

        debug_log!("Slicing expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - FromIdx type: {}", self.expr_types.get(&from_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - ToIdx   type: {}", self.expr_types.get(&to_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        // Make sure subject is arraylike and indices are of equal integerlike
        let progress_subject_base = self.apply_forced_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
        let progress_idx_base = self.apply_forced_constraint(ctx, from_id, &INTEGERLIKE_TEMPLATE)?;
        let (progress_from, progress_to) = self.apply_equal2_constraint(ctx, upcast_id, from_id, 0, to_id, 0)?;

        // Make sure if output is of T then subject is Array<T>
        let (progress_expr, progress_subject) =
            self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, subject_id, 1)?;


        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject_base || progress_subject, self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - FromIdx type [{}]: {}", progress_idx_base || progress_from, self.expr_types.get(&from_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - ToIdx   type [{}]: {}", progress_idx_base || progress_to, self.expr_types.get(&to_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_subject_base || progress_subject { self.queue_expr(subject_id); }
        if progress_idx_base || progress_from { self.queue_expr(from_id); }
        if progress_idx_base || progress_to { self.queue_expr(to_id); }

        Ok(())
    }

    fn progress_select_expr(&mut self, ctx: &mut Ctx, id: SelectExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;

        debug_log!("Select expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        let (progress_subject, progress_expr) = match &expr.field {
            Field::Length => {
                let progress_subject = self.apply_forced_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?;
                (progress_subject, progress_expr)
            },
            Field::Symbolic(_field) => {
                todo!("implement select expr for symbolic fields");
            }
        };

        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject, self.expr_types.get(&subject_id).unwrap().display_name(&ctx.heap));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        if progress_subject { self.queue_expr(subject_id); }
        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }

        Ok(())
    }

    fn progress_array_expr(&mut self, ctx: &mut Ctx, id: ArrayExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let expr_elements = expr.elements.clone(); // TODO: @performance

        debug_log!("Array expr ({} elements): {}", expr_elements.len(), upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        // All elements should have an equal type
        let progress = self.apply_equal_n_constraint(ctx, upcast_id, &expr_elements)?;
        let mut any_progress = false;
        for (progress_arg, arg_id) in progress.iter().zip(expr_elements.iter()) {
            if *progress_arg {
                any_progress = true;
                self.queue_expr(*arg_id);
            }
        }

        // And the output should be an array of the element types
        let mut expr_progress = self.apply_forced_constraint(ctx, upcast_id, &ARRAY_TEMPLATE)?;
        if !expr_elements.is_empty() {
            let first_arg_id = expr_elements[0];
            let (inner_expr_progress, arg_progress) = self.apply_equal2_constraint(
                ctx, upcast_id, upcast_id, 1, first_arg_id, 0
            )?;

            expr_progress = expr_progress || inner_expr_progress;

            // Note that if the array type progressed the type of the arguments,
            // then we should enqueue this progression function again
            // TODO: @fix Make apply_equal_n accept a start idx as well
            if arg_progress { self.queue_expr(upcast_id); }
        }

        debug_log!(" * After:");
        debug_log!("   - Expr type [{}]: {}", expr_progress, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        if expr_progress { self.queue_expr_parent(ctx, upcast_id); }

        Ok(())
    }

    fn progress_constant_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let template = match &expr.value {
            Literal::Null => &MESSAGE_TEMPLATE[..],
            Literal::Integer(_) => &INTEGERLIKE_TEMPLATE[..],
            Literal::True | Literal::False => &BOOL_TEMPLATE[..],
            Literal::Character(_) => todo!("character literals")
        };

        let progress = self.apply_forced_constraint(ctx, upcast_id, template)?;
        if progress { self.queue_expr_parent(ctx, upcast_id); }

        Ok(())
    }

    // TODO: @cleanup, see how this can be cleaned up once I implement
    //  polymorphic struct/enum/union literals. These likely follow the same
    //  pattern as here.
    fn progress_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let extra = self.extra_data.get_mut(&upcast_id).unwrap();

        debug_log!("Call expr '{}': {}", match &expr.method {
            Method::Create => String::from("create"),
            Method::Fires => String::from("fires"),
            Method::Get => String::from("get"),
            Method::Put => String::from("put"),
            Method::Symbolic(method) => String::from_utf8_lossy(&method.identifier.value).to_string()
        },upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));
        debug_log!(" * During (inferring types from arguments and return type):");

        // Check if we can make progress using the arguments and/or return types
        // while keeping track of the polyvars we've extended
        let mut poly_progress = HashSet::new();
        debug_assert_eq!(extra.embedded.len(), expr.arguments.len());
        let mut poly_infer_error = false;

        for (arg_idx, arg_id) in expr.arguments.clone().into_iter().enumerate() {
            let signature_type = &mut extra.embedded[arg_idx];
            let argument_type: *mut _ = self.expr_types.get_mut(&arg_id).unwrap();
            let (progress_sig, progress_arg) = Self::apply_equal2_signature_constraint(
                ctx, upcast_id, Some(arg_id), signature_type, 0, argument_type, 0
            )?;

            debug_log!("   - Arg {} type | sig: {}, arg: {}", arg_idx, signature_type.display_name(&ctx.heap), unsafe{&*argument_type}.display_name(&ctx.heap));

            if progress_sig {
                // Progressed signature, so also apply inference to the 
                // polymorph types using the markers 
                debug_assert!(signature_type.has_body_marker, "progress on signature argument type without markers");
                for (poly_idx, poly_section) in signature_type.body_marker_iter() {
                    let polymorph_type = &mut extra.poly_vars[poly_idx];
                    match Self::apply_forced_constraint_types(
                        polymorph_type, 0, poly_section, 0
                    ) {
                        Ok(true) => { poly_progress.insert(poly_idx); },
                        Ok(false) => {},
                        Err(()) => { poly_infer_error = true; }
                    }

                    debug_log!("   - Poly {} type | sig: {}, arg: {}", poly_idx, polymorph_type.display_name(&ctx.heap), InferenceType::partial_display_name(&ctx.heap, poly_section));
                }
            }
            if progress_arg {
                // Progressed argument expression
                self.expr_queued.insert(arg_id);
            }
        }

        // Do the same for the return type
        let signature_type = &mut extra.returned;
        let expr_type: *mut _ = self.expr_types.get_mut(&upcast_id).unwrap();
        let (progress_sig, progress_expr) = Self::apply_equal2_signature_constraint(
            ctx, upcast_id, None, signature_type, 0, expr_type, 0
        )?;

        debug_log!("   - Ret type | sig: {}, arg: {}", signature_type.display_name(&ctx.heap), unsafe{&*expr_type}.display_name(&ctx.heap));

        if progress_sig {
            // As above: apply inference to polyargs as well
            debug_assert!(signature_type.has_body_marker, "progress on signature return type without markers");
            for (poly_idx, poly_section) in signature_type.body_marker_iter() {
                let polymorph_type = &mut extra.poly_vars[poly_idx];
                match Self::apply_forced_constraint_types(
                    polymorph_type, 0, poly_section, 0
                ) {
                    Ok(true) => { poly_progress.insert(poly_idx); },
                    Ok(false) => {},
                    Err(()) => { poly_infer_error = true; }
                }
                debug_log!("   - Poly {} type | sig: {}, arg: {}", poly_idx, polymorph_type.display_name(&ctx.heap), InferenceType::partial_display_name(&ctx.heap, poly_section));
            }
        }
        if progress_expr {
            if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                self.expr_queued.insert(parent_id);
            }
        }

        // If we had an error in the polymorphic variable's inference, then we
        // need to provide a human readable error: find a pair of inference
        // types in the arguments/return type that do not agree on the
        // polymorphic variable's type
        if poly_infer_error { return Err(self.construct_poly_arg_error(ctx, id)) }

        // If we did not have an error in the polymorph inference above, then
        // reapplying the polymorph type to each argument type and the return
        // type should always succeed.
        debug_log!(" * During (reinferring from progress polyvars):");
        // TODO: @performance If the algorithm is changed to be more "on demand
        //  argument re-evaluation", instead of "all-argument re-evaluation",
        //  then this is no longer true
        for poly_idx in poly_progress.into_iter() {
            // For each polymorphic argument: first extend the signature type,
            // then reapply the equal2 constraint to the expressions
            let poly_type = &extra.poly_vars[poly_idx];
            for (arg_idx, sig_type) in extra.embedded.iter_mut().enumerate() {
                let mut seek_idx = 0;
                let mut modified_sig = false;
                while let Some((start_idx, end_idx)) = sig_type.find_subtree_idx_for_part(
                    InferenceTypePart::MarkerBody(poly_idx), seek_idx
                ) {
                    let modified_at_marker = Self::apply_forced_constraint_types(
                        sig_type, start_idx, &poly_type.parts, 0
                    ).unwrap();
                    modified_sig = modified_sig || modified_at_marker;
                    seek_idx = end_idx;
                }

                if !modified_sig {
                    debug_log!("   - Poly {} | Arg {} type | signature has not changed", poly_idx, arg_idx);
                    continue;
                }

                // Part of signature was modified, so update expression used as
                // argument as well
                let arg_expr_id = expr.arguments[arg_idx];
                let arg_type: *mut _ = self.expr_types.get_mut(&arg_expr_id).unwrap();
                let (_, progress_arg) = Self::apply_equal2_signature_constraint(
                    ctx, arg_expr_id, Some(arg_expr_id), sig_type, 0, arg_type, 0
                ).expect("no inference error at argument type");
                if progress_arg { self.expr_queued.insert(arg_expr_id); }
                debug_log!("   - Poly {} | Arg {} type | sig: {}, arg: {}", poly_idx, arg_idx, sig_type.display_name(&ctx.heap), unsafe{&*arg_type}.display_name(&ctx.heap));
            }

            // Again: do the same for the return type
            let sig_type = &mut extra.returned;
            let mut seek_idx = 0;
            let mut modified_sig = false;
            while let Some((start_idx, end_idx)) = sig_type.find_subtree_idx_for_part(
                InferenceTypePart::MarkerBody(poly_idx), seek_idx
            ) {
                let modified_at_marker = Self::apply_forced_constraint_types(
                    sig_type, start_idx, &poly_type.parts, 0
                ).unwrap();
                modified_sig = modified_sig || modified_at_marker;
                seek_idx = end_idx;
            }

            if modified_sig {
                let ret_type = self.expr_types.get_mut(&upcast_id).unwrap();
                let (_, progress_ret) = Self::apply_equal2_signature_constraint(
                    ctx, upcast_id, None, sig_type, 0, ret_type, 0
                ).expect("no inference error at return type");
                if progress_ret {
                    if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                        self.expr_queued.insert(parent_id);
                    }
                }
                debug_log!("   - Poly {} | Ret type | sig: {}, arg: {}", poly_idx, sig_type.display_name(&ctx.heap), ret_type.display_name(&ctx.heap));
            } else {
                debug_log!("   - Poly {} | Ret type | signature has not changed", poly_idx);
            }
        }

        debug_log!(" * After:");
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        Ok(())
    }

    fn progress_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> Result<(), ParseError2> {
        let upcast_id = id.upcast();
        let var_expr = &ctx.heap[id];
        let var_id = var_expr.declaration.unwrap();

        debug_log!("Variable expr '{}': {}", &String::from_utf8_lossy(&ctx.heap[var_id].identifier().value), upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Var  type: {}", self.var_types.get(&var_id).unwrap().var_type.display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));

        // Retrieve shared variable type and expression type and apply inference
        let var_data = self.var_types.get_mut(&var_id).unwrap();
        let expr_type = self.expr_types.get_mut(&upcast_id).unwrap();

        let infer_res = unsafe{ InferenceType::infer_subtrees_for_both_types(
            &mut var_data.var_type as *mut _, 0, expr_type, 0
        ) };
        if infer_res == DualInferenceResult::Incompatible {
            let var_decl = &ctx.heap[var_id];
            return Err(ParseError2::new_error(
                &ctx.module.source, var_decl.position(),
                &format!(
                    "Conflicting types for this variable, previously assigned the type '{}'",
                    var_data.var_type.display_name(&ctx.heap)
                )
            ).with_postfixed_info(
                &ctx.module.source, var_expr.position,
                &format!(
                    "But inferred to have incompatible type '{}' here",
                    expr_type.display_name(&ctx.heap)
                )
            ))
        }

        let progress_var = infer_res.modified_lhs();
        let progress_expr = infer_res.modified_rhs();

        if progress_var {
            // Let other variable expressions using this type progress as well
            for other_expr in var_data.used_at.iter() {
                if *other_expr != upcast_id {
                    self.expr_queued.insert(*other_expr);
                }
            }

            // Let a linked port know that our type has updated
            if let Some(linked_id) = var_data.linked_var {
                // Only perform one-way inference to prevent updating our type, this
                // would lead to an inconsistency
                let var_type: *mut _ = &mut var_data.var_type;
                let mut link_data = self.var_types.get_mut(&linked_id).unwrap();

                debug_assert!(
                    unsafe{&*var_type}.parts[0] == InferenceTypePart::Input ||
                    unsafe{&*var_type}.parts[0] == InferenceTypePart::Output
                );
                debug_assert!(
                    link_data.var_type.parts[0] == InferenceTypePart::Input ||
                    link_data.var_type.parts[0] == InferenceTypePart::Output
                );
                match InferenceType::infer_subtree_for_single_type(&mut link_data.var_type, 1, &unsafe{&*var_type}.parts, 1) {
                    SingleInferenceResult::Modified => {
                        for other_expr in &link_data.used_at {
                            self.expr_queued.insert(*other_expr);
                        }
                    },
                    SingleInferenceResult::Unmodified => {},
                    SingleInferenceResult::Incompatible => {
                        let var_data = self.var_types.get(&var_id).unwrap();
                        let link_data = self.var_types.get(&linked_id).unwrap();
                        let var_decl = &ctx.heap[var_id];
                        let link_decl = &ctx.heap[linked_id];

                        return Err(ParseError2::new_error(
                            &ctx.module.source, var_decl.position(),
                            &format!(
                                "Conflicting types for this variable, assigned the type '{}'",
                                var_data.var_type.display_name(&ctx.heap)
                            )
                        ).with_postfixed_info(
                            &ctx.module.source, link_decl.position(),
                            &format!(
                                "Because it is incompatible with this variable, assigned the type '{}'",
                                link_data.var_type.display_name(&ctx.heap)
                            )
                        ));
                    }
                }
            }
        }
        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }

        debug_log!(" * After:");
        debug_log!("   - Var  type [{}]: {}", progress_var, self.var_types.get(&var_id).unwrap().var_type.display_name(&ctx.heap));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.expr_types.get(&upcast_id).unwrap().display_name(&ctx.heap));


        Ok(())
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
            SingleInferenceResult::Modified => Ok(true),
            SingleInferenceResult::Unmodified => Ok(false),
            SingleInferenceResult::Incompatible => Err(
                self.construct_template_type_error(ctx, expr_id, template)
            )
        }
    }

    fn apply_forced_constraint_types(
        to_infer: *mut InferenceType, to_infer_start_idx: usize,
        template: &[InferenceTypePart], template_start_idx: usize
    ) -> Result<bool, ()> {
        match InferenceType::infer_subtree_for_single_type(
            unsafe{ &mut *to_infer }, to_infer_start_idx,
            template, template_start_idx
        ) {
            SingleInferenceResult::Modified => Ok(true),
            SingleInferenceResult::Unmodified => Ok(false),
            SingleInferenceResult::Incompatible => Err(()),
        }
    }

    /// Applies a type constraint that expects the two provided types to be
    /// equal. We attempt to make progress in inferring the types. If the call
    /// is successful then the composition of all types are made equal.
    /// The "parent" `expr_id` is provided to construct errors.
    fn apply_equal2_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg1_start_idx: usize,
        arg2_id: ExpressionId, arg2_start_idx: usize
    ) -> Result<(bool, bool), ParseError2> {
        debug_assert_expr_ids_unique_and_known!(self, arg1_id, arg2_id);
        let arg1_type: *mut _ = self.expr_types.get_mut(&arg1_id).unwrap();
        let arg2_type: *mut _ = self.expr_types.get_mut(&arg2_id).unwrap();

        let infer_res = unsafe{ InferenceType::infer_subtrees_for_both_types(
            arg1_type, arg1_start_idx,
            arg2_type, arg2_start_idx
        ) };
        if infer_res == DualInferenceResult::Incompatible {
            return Err(self.construct_arg_type_error(ctx, expr_id, arg1_id, arg2_id));
        }

        Ok((infer_res.modified_lhs(), infer_res.modified_rhs()))
    }

    fn apply_equal2_signature_constraint(
        ctx: &Ctx, outer_expr_id: ExpressionId, expr_id: Option<ExpressionId>,
        signature_type: *mut InferenceType, signature_start_idx: usize,
        expression_type: *mut InferenceType, expression_start_idx: usize
    ) -> Result<(bool, bool), ParseError2> {
        debug_assert_ptrs_distinct!(signature_type, expression_type);
        let infer_res = unsafe { 
            InferenceType::infer_subtrees_for_both_types(
                signature_type, signature_start_idx,
                expression_type, expression_start_idx
            ) 
        };

        if infer_res == DualInferenceResult::Incompatible {
            // TODO: Check if I still need to use this
            let outer_position = ctx.heap[outer_expr_id].position();
            let (position_name, position) = match expr_id {
                Some(expr_id) => ("argument's", ctx.heap[expr_id].position()),
                None => ("return type's", outer_position)
            };
            let (signature_display_type, expression_display_type) = unsafe { (
                (&*signature_type).display_name(&ctx.heap),
                (&*expression_type).display_name(&ctx.heap)
            ) };

            return Err(ParseError2::new_error(
                &ctx.module.source, outer_position,
                "Failed to fully resolve the types of this expression"
            ).with_postfixed_info(
                &ctx.module.source, position,
                &format!(
                    "Because the {} signature has been resolved to '{}', but the expression has been resolved to '{}'",
                    position_name, signature_display_type, expression_display_type
                )
            ));
        }

        Ok((infer_res.modified_lhs(), infer_res.modified_rhs()))
    }

    /// Applies a type constraint that expects all three provided types to be
    /// equal. In case we can make progress in inferring the types then we
    /// attempt to do so. If the call is successful then the composition of all
    /// types is made equal.
    fn apply_equal3_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg2_id: ExpressionId,
        start_idx: usize
    ) -> Result<(bool, bool, bool), ParseError2> {
        // Safety: all expression IDs are always distinct, and we do not modify
        //  the container
        debug_assert_expr_ids_unique_and_known!(self, expr_id, arg1_id, arg2_id);
        let expr_type: *mut _ = self.expr_types.get_mut(&expr_id).unwrap();
        let arg1_type: *mut _ = self.expr_types.get_mut(&arg1_id).unwrap();
        let arg2_type: *mut _ = self.expr_types.get_mut(&arg2_id).unwrap();

        let expr_res = unsafe{
            InferenceType::infer_subtrees_for_both_types(expr_type, start_idx, arg1_type, start_idx)
        };
        if expr_res == DualInferenceResult::Incompatible {
            return Err(self.construct_expr_type_error(ctx, expr_id, arg1_id));
        }

        let args_res = unsafe{
            InferenceType::infer_subtrees_for_both_types(arg1_type, start_idx, arg2_type, start_idx) };
        if args_res == DualInferenceResult::Incompatible {
            return Err(self.construct_arg_type_error(ctx, expr_id, arg1_id, arg2_id));
        }

        // If all types are compatible, but the second call caused the arg1_type
        // to be expanded, then we must also assign this to expr_type.
        let mut progress_expr = expr_res.modified_lhs();
        let mut progress_arg1 = expr_res.modified_rhs();
        let progress_arg2 = args_res.modified_rhs();

        if args_res.modified_lhs() { 
            unsafe {
                let end_idx = InferenceType::find_subtree_end_idx(&(*arg2_type).parts, start_idx);
                let subtree = &((*arg2_type).parts[start_idx..end_idx]);
                (*expr_type).replace_subtree(start_idx, subtree);
            }
            progress_expr = true;
            progress_arg1 = true;
        }

        Ok((progress_expr, progress_arg1, progress_arg2))
    }

    // TODO: @optimize Since we only deal with a single type this might be done
    //  a lot more efficiently, methinks (disregarding the allocations here)
    fn apply_equal_n_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId, args: &[ExpressionId],
    ) -> Result<Vec<bool>, ParseError2> {
        // Early exit
        match args.len() {
            0 => return Ok(vec!()),         // nothing to progress
            1 => return Ok(vec![false]),    // only one type, so nothing to infer
            _ => {}
        }

        let mut progress = Vec::new();
        progress.resize(args.len(), false);

        // Do pairwise inference, keep track of the last entry we made progress
        // on. Once done we need to update everything to the most-inferred type.
        let mut arg_iter = args.iter();
        let mut last_arg_id = *arg_iter.next().unwrap();
        let mut last_lhs_progressed = 0;
        let mut lhs_arg_idx = 0;

        while let Some(next_arg_id) = arg_iter.next() {
            let arg1_type: *mut _ = self.expr_types.get_mut(&last_arg_id).unwrap();
            let arg2_type: *mut _ = self.expr_types.get_mut(next_arg_id).unwrap();

            let res = unsafe {
                InferenceType::infer_subtrees_for_both_types(arg1_type, 0, arg2_type, 0)
            };

            if res == DualInferenceResult::Incompatible {
                return Err(self.construct_arg_type_error(ctx, expr_id, last_arg_id, *next_arg_id));
            }

            if res.modified_lhs() {
                // We re-inferred something on the left hand side, so everything
                // up until now should be re-inferred.
                progress[lhs_arg_idx] = true;
                last_lhs_progressed = lhs_arg_idx;
            }
            progress[lhs_arg_idx + 1] = res.modified_rhs();

            last_arg_id = *next_arg_id;
            lhs_arg_idx += 1;
        }

        // Re-infer everything. Note that we do not need to re-infer the type
        // exactly at `last_lhs_progressed`, but only everything up to it.
        let last_type: *mut _ = self.expr_types.get_mut(args.last().unwrap()).unwrap();
        for arg_idx in 0..last_lhs_progressed {
            let arg_type: *mut _ = self.expr_types.get_mut(&args[arg_idx]).unwrap();
            unsafe{
                (*arg_type).replace_subtree(0, &(*last_type).parts);
            }
            progress[arg_idx] = true;
        }

        Ok(progress)
    }

    /// Determines the `InferenceType` for the expression based on the
    /// expression parent. Note that if the parent is another expression, we do
    /// not take special action, instead we let parent expressions fix the type
    /// of subexpressions before they have a chance to call this function.
    /// Hence: if the expression type is already set, this function doesn't do
    /// anything.
    fn insert_initial_expr_inference_type(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId
    ) -> Result<(), ParseError2> {
        use ExpressionParent as EP;
        use InferenceTypePart as ITP;

        let expr = &ctx.heap[expr_id];
        let inference_type = match expr.parent() {
            EP::None =>
                // Should have been set by linker
                unreachable!(),
            EP::ExpressionStmt(_) | EP::Expression(_, _) =>
                // Determined during type inference
                InferenceType::new(false, false, vec![ITP::Unknown]),
            EP::If(_) | EP::While(_) | EP::Assert(_) =>
                // Must be a boolean
                InferenceType::new(false, true, vec![ITP::Bool]),
            EP::Return(_) =>
                // Must match the return type of the function
                if let DefinitionType::Function(func_id) = self.definition_type {
                    let return_parser_type_id = ctx.heap[func_id].return_type;
                    self.determine_inference_type_from_parser_type(ctx, return_parser_type_id, true)
                } else {
                    // Cannot happen: definition always set upon body traversal
                    // and "return" calls in components are illegal.
                    unreachable!();
                },
            EP::New(_) =>
                // Must be a component call, which we assign a "Void" return
                // type
                InferenceType::new(false, true, vec![ITP::Void]),
        };

        match self.expr_types.entry(expr_id) {
            Entry::Vacant(vacant) => {
                vacant.insert(inference_type);
            },
            Entry::Occupied(mut preexisting) => {
                // We already have an entry, this happens if our parent fixed
                // our type (e.g. we're used in a conditional expression's test)
                // but we have a different type.
                // TODO: Is this ever called? Seems like it can't
                debug_assert!(false, "I am actually called, my ID is {}", expr_id.index);
                let old_type = preexisting.get_mut();
                if let SingleInferenceResult::Incompatible = InferenceType::infer_subtree_for_single_type(
                    old_type, 0, &inference_type.parts, 0
                ) {
                    return Err(self.construct_expr_type_error(ctx, expr_id, expr_id))
                }
            }
        }

        Ok(())
    }

    fn insert_initial_call_polymorph_data(
        &mut self, ctx: &mut Ctx, call_id: CallExpressionId
    ) {
        use InferenceTypePart as ITP;

        // Note: the polymorph variables may be partially specified and may
        // contain references to the wrapping definition's (i.e. the proctype
        // we are currently visiting) polymorphic arguments.
        //
        // The arguments of the call may refer to polymorphic variables in the
        // definition of the function we're calling, not of the wrapping
        // definition. We insert markers in these inferred types to be able to
        // map them back and forth to the polymorphic arguments of the function
        // we are calling.
        let call = &ctx.heap[call_id];

        // Handle the polymorphic variables themselves
        let mut poly_vars = Vec::with_capacity(call.poly_args.len());
        for poly_arg_type_id in call.poly_args.clone() { // TODO: @performance
            poly_vars.push(self.determine_inference_type_from_parser_type(ctx, poly_arg_type_id, true));
        }

        // Handle the arguments
        // TODO: @cleanup: Maybe factor this out for reuse in the validator/linker, should also
        //  make the code slightly more robust.
        let (embedded_types, return_type) = match &call.method {
            Method::Create => {
                // Not polymorphic
                (
                    vec![InferenceType::new(false, true, vec![ITP::Int])],
                    InferenceType::new(false, true, vec![ITP::Message, ITP::Byte])
                )
            },
            Method::Fires => {
                // bool fires<T>(PortLike<T> arg)
                (
                    vec![InferenceType::new(true, false, vec![ITP::PortLike, ITP::MarkerBody(0), ITP::Unknown])],
                    InferenceType::new(false, true, vec![ITP::Bool])
                )
            },
            Method::Get => {
                // T get<T>(input<T> arg)
                (
                    vec![InferenceType::new(true, false, vec![ITP::Input, ITP::MarkerBody(0), ITP::Unknown])],
                    InferenceType::new(true, false, vec![ITP::MarkerBody(0), ITP::Unknown])
                )
            },
            Method::Put => {
                // void Put<T>(output<T> port, T msg)
                (
                    vec![
                        InferenceType::new(true, false, vec![ITP::Output, ITP::MarkerBody(0), ITP::Unknown]),
                        InferenceType::new(true, false, vec![ITP::MarkerBody(0), ITP::Unknown])
                    ],
                    InferenceType::new(false, true, vec![ITP::Void])
                )
            }
            Method::Symbolic(symbolic) => {
                let definition = &ctx.heap[symbolic.definition.unwrap()];

                match definition {
                    Definition::Component(definition) => {
                        let mut parameter_types = Vec::with_capacity(definition.parameters.len());
                        for param_id in definition.parameters.clone() {
                            let param = &ctx.heap[param_id];
                            let param_parser_type_id = param.parser_type;
                            parameter_types.push(self.determine_inference_type_from_parser_type(ctx, param_parser_type_id, false));
                        }

                        (parameter_types, InferenceType::new(false, true, vec![InferenceTypePart::Void]))
                    },
                    Definition::Function(definition) => {
                        let mut parameter_types = Vec::with_capacity(definition.parameters.len());
                        for param_id in definition.parameters.clone() {
                            let param = &ctx.heap[param_id];
                            let param_parser_type_id = param.parser_type;
                            parameter_types.push(self.determine_inference_type_from_parser_type(ctx, param_parser_type_id, false));
                        }

                        let return_type = self.determine_inference_type_from_parser_type(ctx, definition.return_type, false);
                        (parameter_types, return_type)
                    },
                    Definition::Struct(_) | Definition::Enum(_) => {
                        unreachable!("insert initial polymorph data for struct/enum");
                    }
                }
            }
        };

        self.extra_data.insert(call_id.upcast(), ExtraData {
            poly_vars,
            embedded: embedded_types,
            returned: return_type
        });
    }

    /// Determines the initial InferenceType from the provided ParserType. This
    /// may be called with two kinds of intentions:
    /// 1. To resolve a ParserType within the body of a function, or on
    ///     polymorphic arguments to calls/instantiations within that body. This
    ///     means that the polymorphic variables are known and can be replaced
    ///     with the monomorph we're instantiating.
    /// 2. To resolve a ParserType on a called function's definition or on
    ///     an instantiated datatype's members. This means that the polymorphic
    ///     arguments inside those ParserTypes refer to the polymorphic
    ///     variables in the called/instantiated type's definition.
    /// In the second case we place InferenceTypePart::Marker instances such
    /// that we can perform type inference on the polymorphic variables.
    fn determine_inference_type_from_parser_type(
        &mut self, ctx: &Ctx, parser_type_id: ParserTypeId,
        parser_type_in_body: bool
    ) -> InferenceType {
        use ParserTypeVariant as PTV;
        use InferenceTypePart as ITP;

        let mut to_consider = VecDeque::with_capacity(16);
        to_consider.push_back(parser_type_id);

        let mut infer_type = Vec::new();
        let mut has_inferred = false;
        let mut has_markers = false;

        while !to_consider.is_empty() {
            let parser_type_id = to_consider.pop_front().unwrap();
            let parser_type = &ctx.heap[parser_type_id];
            match &parser_type.variant {
                PTV::Message => {
                    // TODO: @types Remove the Message -> Byte hack at some point...
                    infer_type.push(ITP::Message);
                    infer_type.push(ITP::Byte);
                },
                PTV::Bool => { infer_type.push(ITP::Bool); },
                PTV::Byte => { infer_type.push(ITP::Byte); },
                PTV::Short => { infer_type.push(ITP::Short); },
                PTV::Int => { infer_type.push(ITP::Int); },
                PTV::Long => { infer_type.push(ITP::Long); },
                PTV::String => { infer_type.push(ITP::String); },
                PTV::IntegerLiteral => { unreachable!("integer literal type on variable type"); },
                PTV::Inferred => {
                    infer_type.push(ITP::Unknown);
                    has_inferred = true;
                },
                PTV::Array(subtype_id) => {
                    infer_type.push(ITP::Array);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Input(subtype_id) => {
                    infer_type.push(ITP::Input);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Output(subtype_id) => {
                    infer_type.push(ITP::Output);
                    to_consider.push_front(*subtype_id);
                },
                PTV::Symbolic(symbolic) => {
                    debug_assert!(symbolic.variant.is_some(), "symbolic variant not yet determined");
                    match symbolic.variant.as_ref().unwrap() {
                        SymbolicParserTypeVariant::PolyArg(_, arg_idx) => {
                            let arg_idx = *arg_idx;
                            debug_assert!(symbolic.poly_args.is_empty()); // TODO: @hkt

                            if parser_type_in_body {
                                // Polymorphic argument refers to definition's
                                // polymorphic variables
                                debug_assert!(arg_idx < self.poly_vars.len());
                                debug_assert!(!self.poly_vars[arg_idx].has_marker());
                                infer_type.push(ITP::MarkerDefinition(arg_idx));
                                for concrete_part in &self.poly_vars[arg_idx].parts {
                                    infer_type.push(ITP::from(*concrete_part));
                                }
                            } else {
                                // Polymorphic argument has to be inferred
                                has_markers = true;
                                has_inferred = true;
                                infer_type.push(ITP::MarkerBody(arg_idx));
                                infer_type.push(ITP::Unknown);
                            }
                        },
                        SymbolicParserTypeVariant::Definition(definition_id) => {
                            // TODO: @cleanup
                            if cfg!(debug_assertions) {
                                let definition = &ctx.heap[*definition_id];
                                debug_assert!(definition.is_struct() || definition.is_enum()); // TODO: @function_ptrs
                                let num_poly = match definition {
                                    Definition::Struct(v) => v.poly_vars.len(),
                                    Definition::Enum(v) => v.poly_vars.len(),
                                    _ => unreachable!(),
                                };
                                debug_assert_eq!(symbolic.poly_args.len(), num_poly);
                            }

                            infer_type.push(ITP::Instance(*definition_id, symbolic.poly_args.len()));
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

        InferenceType::new(has_markers, !has_inferred, infer_type)
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

    fn construct_template_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, template: &[InferenceTypePart]
    ) -> ParseError2 {
        let expr = &ctx.heap[expr_id];
        let expr_type = self.expr_types.get(&expr_id).unwrap();

        return ParseError2::new_error(
            &ctx.module.source, expr.position(),
            &format!(
                "Incompatible types: got a '{}' but expected a '{}'",
                expr_type.display_name(&ctx.heap), 
                InferenceType::partial_display_name(&ctx.heap, template)
            )
        )
    }

    /// Constructs a human interpretable error in the case that type inference
    /// on a polymorphic variable to a function call failed. This may only be
    /// caused by a pair of inference types (which may come from arguments or
    /// the return type) having two different inferred values for that
    /// polymorphic variable.
    ///
    /// So we find this pair (which may be a argument type or return type
    /// conflicting with itself) and construct the error using it.
    fn construct_poly_arg_error(
        &self, ctx: &Ctx, call_id: CallExpressionId
    ) -> ParseError2 {
        // Helper function to check for polymorph mismatch between two inference
        // types.
        fn has_poly_mismatch<'a>(type_a: &'a InferenceType, type_b: &'a InferenceType) -> Option<(usize, &'a [InferenceTypePart], &'a [InferenceTypePart])> {
            if !type_a.has_body_marker || !type_b.has_body_marker {
                return None
            }

            for (marker_a, section_a) in type_a.body_marker_iter() {
                for (marker_b, section_b) in type_b.body_marker_iter() {
                    if marker_a != marker_b {
                        // Not the same polymorphic variable
                        continue;
                    }

                    if !InferenceType::check_subtrees(section_a, 0, section_b, 0) {
                        // Not compatible
                        return Some((marker_a, section_a, section_b))
                    }
                }
            }

            None
        }

        // Helpers function to retrieve polyvar name and function name
        fn get_poly_var_and_func_name(ctx: &Ctx, poly_var_idx: usize, expr: &CallExpression) -> (String, String) {
            match &expr.method {
                Method::Create => unreachable!(),
                Method::Fires => (String::from('T'), String::from("fires")),
                Method::Get => (String::from('T'), String::from("get")),
                Method::Put => (String::from('T'), String::from("put")),
                Method::Symbolic(symbolic) => {
                    let definition = &ctx.heap[symbolic.definition.unwrap()];
                    let poly_var = match definition {
                        Definition::Struct(_) | Definition::Enum(_) => unreachable!(),
                        Definition::Function(definition) => {
                            String::from_utf8_lossy(&definition.poly_vars[poly_var_idx].value).to_string()
                        },
                        Definition::Component(definition) => {
                            String::from_utf8_lossy(&definition.poly_vars[poly_var_idx].value).to_string()
                        }
                    };
                    let func_name = String::from_utf8_lossy(&symbolic.identifier.value).to_string();
                    (poly_var, func_name)
                }
            }
        }

        // Helper function to construct initial error
        fn construct_main_error(ctx: &Ctx, poly_var_idx: usize, expr: &CallExpression) -> ParseError2 {
            let (poly_var, func_name) = get_poly_var_and_func_name(ctx, poly_var_idx, expr);
            return ParseError2::new_error(
                &ctx.module.source, expr.position(),
                &format!(
                    "Conflicting type for polymorphic variable '{}' of '{}'",
                    poly_var, func_name
                )
            )
        }

        // Actual checking
        let extra = self.extra_data.get(&call_id.upcast()).unwrap();
        let expr = &ctx.heap[call_id];

        // - check return type with itself
        if let Some((poly_idx, section_a, section_b)) = has_poly_mismatch(&extra.returned, &extra.returned) {
            return construct_main_error(ctx, poly_idx, expr)
                .with_postfixed_info(
                    &ctx.module.source, expr.position(),
                    &format!(
                        "The return type inferred the conflicting types '{}' and '{}'",
                        InferenceType::partial_display_name(&ctx.heap, section_a),
                        InferenceType::partial_display_name(&ctx.heap, section_b)
                    )
                )
        }

        // - check arguments with each other argument and with return type
        for (arg_a_idx, arg_a) in extra.embedded.iter().enumerate() {
            for (arg_b_idx, arg_b) in extra.embedded.iter().enumerate() {
                if arg_b_idx > arg_a_idx {
                    break;
                }

                if let Some((poly_idx, section_a, section_b)) = has_poly_mismatch(&arg_a, &arg_b) {
                    let error = construct_main_error(ctx, poly_idx, expr);
                    if arg_a_idx == arg_b_idx {
                        // Same argument
                        let arg = &ctx.heap[expr.arguments[arg_a_idx]];
                        return error.with_postfixed_info(
                            &ctx.module.source, arg.position(),
                            &format!(
                                "This argument inferred the conflicting types '{}' and '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_a),
                                InferenceType::partial_display_name(&ctx.heap, section_b)
                            )
                        )
                    } else {
                        let arg_a = &ctx.heap[expr.arguments[arg_a_idx]];
                        let arg_b = &ctx.heap[expr.arguments[arg_b_idx]];
                        return error.with_postfixed_info(
                            &ctx.module.source, arg_a.position(),
                            &format!(
                                "This argument inferred it to '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_a)
                            )
                        ).with_postfixed_info(
                            &ctx.module.source, arg_b.position(),
                            &format!(
                                "While this argument inferred it to '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_b)
                            )
                        )
                    }
                }
            }

            // Check with return type
            if let Some((poly_idx, section_arg, section_ret)) = has_poly_mismatch(arg_a, &extra.returned) {
                let arg = &ctx.heap[expr.arguments[arg_a_idx]];
                return construct_main_error(ctx, poly_idx, expr)
                    .with_postfixed_info(
                        &ctx.module.source, arg.position(),
                        &format!(
                            "This argument inferred it to '{}'",
                            InferenceType::partial_display_name(&ctx.heap, section_arg)
                        )
                    )
                    .with_postfixed_info(
                        &ctx.module.source, expr.position,
                        &format!(
                            "While the return type inferred it to '{}'",
                            InferenceType::partial_display_name(&ctx.heap, section_ret)
                        )
                    )
            }
        }

        unreachable!("construct_poly_arg_error without actual error found?")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::arena::Id;
    use InferenceTypePart as ITP;
    use InferenceType as IT;

    #[test]
    fn test_single_part_inference() {
        // lhs argument inferred from rhs
        let pairs = [
            (ITP::NumberLike, ITP::Byte),
            (ITP::IntegerLike, ITP::Int),
            (ITP::Unknown, ITP::Long),
            (ITP::Unknown, ITP::String)
        ];
        for (lhs, rhs) in pairs.iter() {
            // Using infer-both
            let mut lhs_type = IT::new(false, false, vec![lhs.clone()]);
            let mut rhs_type = IT::new(false, true, vec![rhs.clone()]);
            let result = unsafe{ IT::infer_subtrees_for_both_types(
                &mut lhs_type, 0, &mut rhs_type, 0
            ) };
            assert_eq!(DualInferenceResult::First, result);
            assert_eq!(lhs_type.parts, rhs_type.parts);

            // Using infer-single
            let mut lhs_type = IT::new(false, false, vec![lhs.clone()]);
            let rhs_type = IT::new(false, true, vec![rhs.clone()]);
            let result = IT::infer_subtree_for_single_type(
                &mut lhs_type, 0, &rhs_type.parts, 0
            );
            assert_eq!(SingleInferenceResult::Modified, result);
            assert_eq!(lhs_type.parts, rhs_type.parts);
        }
    }

    #[test]
    fn test_multi_part_inference() {
        let pairs = [
            (vec![ITP::ArrayLike, ITP::NumberLike], vec![ITP::Slice, ITP::Byte]),
            (vec![ITP::Unknown], vec![ITP::Input, ITP::Array, ITP::String]),
            (vec![ITP::PortLike, ITP::Int], vec![ITP::Input, ITP::Int]),
            (vec![ITP::Unknown], vec![ITP::Output, ITP::Int]),
            (
                vec![ITP::Instance(Id::new(0), 2), ITP::Input, ITP::Unknown, ITP::Output, ITP::Unknown],
                vec![ITP::Instance(Id::new(0), 2), ITP::Input, ITP::Array, ITP::Int, ITP::Output, ITP::Int]
            )
        ];

        for (lhs, rhs) in pairs.iter() {
            let mut lhs_type = IT::new(false, false, lhs.clone());
            let mut rhs_type = IT::new(false, false, rhs.clone());
            let result = unsafe{ IT::infer_subtrees_for_both_types(
                &mut lhs_type, 0, &mut rhs_type, 0
            ) };
            assert_eq!(DualInferenceResult::First, result);
            assert_eq!(lhs_type.parts, rhs_type.parts);

            let mut lhs_type = IT::new(false, false, lhs.clone());
            let rhs_type = IT::new(false, false, rhs.clone());
            let result = IT::infer_subtree_for_single_type(
                &mut lhs_type, 0, &rhs_type.parts, 0
            );
            assert_eq!(SingleInferenceResult::Modified, result);
            assert_eq!(lhs_type.parts, rhs_type.parts)
        }
    }
}
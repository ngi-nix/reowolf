/// pass_typing
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
/// determined), or we have incomplete types. In the latter case we return an
/// error.
///
/// TODO: Needs a thorough rewrite:
///  0. polymorph_progress is intentionally broken at the moment. Make it work
///     again and use a normal VecSomething.
///  1. The foundation for doing all of the work with predetermined indices
///     instead of with HashMaps is there, but it is not really used because of
///     time constraints. When time is available, rewrite the system such that
///     AST IDs are not needed, and only indices into arrays are used.
///  2. We're doing a lot of extra work. It seems better to apply the initial
///     type based on expression parents, and immediately apply forced
///     constraints (arg to a fires() call must be port-like). All of the \
///     progress_xxx calls should then only be concerned with "transmitting"
///     type inference across their parent/child expressions.
///  3. Remove the `msg` type?
///  4. Disallow certain types in certain operations (e.g. `Void`).

macro_rules! debug_log_enabled {
    () => { false };
}

macro_rules! debug_log {
    ($format:literal) => {
        enabled_debug_print!(false, "types", $format);
    };
    ($format:literal, $($args:expr),*) => {
        enabled_debug_print!(false, "types", $format, $($args),*);
    };
}

use std::collections::{HashMap, HashSet};

use crate::collections::DequeSet;
use crate::protocol::ast::*;
use crate::protocol::input_source::ParseError;
use crate::protocol::parser::ModuleCompilationPhase;
use crate::protocol::parser::type_table::*;
use crate::protocol::parser::token_parsing::*;
use super::visitor::{
    STMT_BUFFER_INIT_CAPACITY,
    EXPR_BUFFER_INIT_CAPACITY,
    Ctx,
    Visitor,
    VisitorResult
};

const VOID_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::Void ];
const MESSAGE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::Message, InferenceTypePart::UInt8 ];
const BOOL_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::Bool ];
const CHARACTER_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::Character ];
const STRING_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::String, InferenceTypePart::Character ];
const NUMBERLIKE_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::NumberLike ];
const INTEGERLIKE_TEMPLATE: [InferenceTypePart; 1] = [ InferenceTypePart::IntegerLike ];
const ARRAY_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::Array, InferenceTypePart::Unknown ];
const SLICE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::Slice, InferenceTypePart::Unknown ];
const ARRAYLIKE_TEMPLATE: [InferenceTypePart; 2] = [ InferenceTypePart::ArrayLike, InferenceTypePart::Unknown ];

/// TODO: @performance Turn into PartialOrd+Ord to simplify checks
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum InferenceTypePart {
    // When we infer types of AST elements that support polymorphic arguments,
    // then we might have the case that multiple embedded types depend on the
    // polymorphic type (e.g. func bla(T a, T[] b) -> T[][]). If we can infer
    // the type in one place (e.g. argument a), then we may propagate this
    // information to other types (e.g. argument b and the return type). For
    // this reason we place markers in the `InferenceType` instances such that
    // we know which part of the type was originally a polymorphic argument.
    Marker(u32),
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
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    SInt8,
    SInt16,
    SInt32,
    SInt64,
    Character,
    String,
    // One subtype
    Message,
    Array,
    Slice,
    Input,
    Output,
    // A user-defined type with any number of subtypes
    Instance(DefinitionId, u32)
}

impl InferenceTypePart {
    fn is_marker(&self) -> bool {
        match self {
            InferenceTypePart::Marker(_) => true,
            _ => false,
        }
    }

    /// Checks if the type is concrete, markers are interpreted as concrete
    /// types.
    fn is_concrete(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Unknown | ITP::NumberLike |
            ITP::IntegerLike | ITP::ArrayLike | ITP::PortLike => false,
            _ => true
        }
    }

    fn is_concrete_number(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::UInt8 | ITP::UInt16 | ITP::UInt32 | ITP::UInt64 |
            ITP::SInt8 | ITP::SInt16 | ITP::SInt32 | ITP::SInt64 => true,
            _ => false,
        }
    }

    fn is_concrete_integer(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::UInt8 | ITP::UInt16 | ITP::UInt32 | ITP::UInt64 |
            ITP::SInt8 | ITP::SInt16 | ITP::SInt32 | ITP::SInt64 => true,
            _ => false,
        }
    }

    fn is_concrete_arraylike(&self) -> bool {
        use InferenceTypePart as ITP;
        match self {
            ITP::Array | ITP::Slice | ITP::String | ITP::Message => true,
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
        (*self == ITP::ArrayLike && arg.is_concrete_arraylike()) ||
        (*self == ITP::PortLike && arg.is_concrete_port())
    }

    /// Checks if a part is more specific

    /// Returns the change in "iteration depth" when traversing this particular
    /// part. The iteration depth is used to traverse the tree in a linear 
    /// fashion. It is basically `number_of_subtypes - 1`
    fn depth_change(&self) -> i32 {
        use InferenceTypePart as ITP;
        match &self {
            ITP::Unknown | ITP::NumberLike | ITP::IntegerLike |
            ITP::Void | ITP::Bool |
            ITP::UInt8 | ITP::UInt16 | ITP::UInt32 | ITP::UInt64 |
            ITP::SInt8 | ITP::SInt16 | ITP::SInt32 | ITP::SInt64 |
            ITP::Character => {
                -1
            },
            ITP::Marker(_) |
            ITP::ArrayLike | ITP::Message | ITP::Array | ITP::Slice |
            ITP::PortLike | ITP::Input | ITP::Output | ITP::String => {
                // One subtype, so do not modify depth
                0
            },
            ITP::Instance(_, num_args) => {
                (*num_args as i32) - 1
            }
        }
    }
}

#[derive(Debug, Clone)]
struct InferenceType {
    has_marker: bool,
    is_done: bool,
    parts: Vec<InferenceTypePart>,
}

impl InferenceType {
    /// Generates a new InferenceType. The two boolean flags will be checked in
    /// debug mode.
    fn new(has_marker: bool, is_done: bool, parts: Vec<InferenceTypePart>) -> Self {
        if cfg!(debug_assertions) {
            debug_assert!(!parts.is_empty());
            let parts_body_marker = parts.iter().any(|v| v.is_marker());
            debug_assert_eq!(has_marker, parts_body_marker);
            let parts_done = parts.iter().all(|v| v.is_concrete());
            debug_assert_eq!(is_done, parts_done, "{:?}", parts);
        }
        Self{ has_marker, is_done, parts }
    }

    /// Replaces a type subtree with the provided subtree. The caller must make
    /// sure the the replacement is a well formed type subtree.
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

    /// Seeks a body marker starting at the specified position. If a marker is
    /// found then its value and the index of the type subtree that follows it
    /// is returned.
    fn find_marker(&self, mut start_idx: usize) -> Option<(u32, usize)> {
        while start_idx < self.parts.len() {
            if let InferenceTypePart::Marker(marker) = &self.parts[start_idx] {
                return Some((*marker, start_idx + 1))
            }

            start_idx += 1;
        }

        None
    }

    /// Returns an iterator over all body markers and the partial type tree that
    /// follows those markers. If it is a problem that `InferenceType` is 
    /// borrowed by the iterator, then use `find_body_marker`.
    fn marker_iter(&self) -> InferenceTypeMarkerIter {
        InferenceTypeMarkerIter::new(&self.parts)
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
        unreachable!("Malformed type: {:?}", parts);
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

        // Check for programmer mistakes
        debug_assert_ne!(to_infer_part, template_part);
        debug_assert!(!to_infer_part.is_marker(), "marker encountered in 'infer part'");
        debug_assert!(!template_part.is_marker(), "marker encountered in 'template part'");

        // Inference of a somewhat-specified type
        if to_infer_part.may_be_inferred_from(template_part) {
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
            // entire subtree. Make sure not to copy markers.
            let template_end_idx = Self::find_subtree_end_idx(template_parts, *template_idx);
            to_infer.parts[*to_infer_idx] = template_parts[*template_idx].clone(); // first element

            *to_infer_idx += 1;
            for template_idx in *template_idx + 1..template_end_idx {
                let template_part = &template_parts[template_idx];
                if !template_part.is_marker() {
                    to_infer.parts.insert(*to_infer_idx, template_part.clone());
                    *to_infer_idx += 1;
                }
            }
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

            // Types can not be inferred in any way: types must be incompatible
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
    ///
    /// The `forced_template` flag controls whether `to_infer` is considered
    /// valid if it is more specific then the template. When `forced_template`
    /// is false, then as long as the `to_infer` and `template` types are
    /// compatible the inference will succeed. If `forced_template` is true,
    /// then `to_infer` MUST be less specific than `template` (e.g.
    /// `IntegerLike` is less specific than `UInt32`)
    fn infer_subtree_for_single_type(
        to_infer: &mut InferenceType, mut to_infer_idx: usize,
        template: &[InferenceTypePart], mut template_idx: usize,
        forced_template: bool,
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

            if !forced_template {
                // We cannot infer anything, but the template may still be
                // compatible with the type we're inferring
                if let Some(depth_change) = Self::check_part_for_single_type(
                    template, &mut template_idx, &to_infer.parts, &mut to_infer_idx
                ) {
                    depth += depth_change;
                    continue;
                }
            }

            return SingleInferenceResult::Incompatible
        }

        if modified {
            to_infer.recompute_is_done();
            return SingleInferenceResult::Modified;
        } else {
            return SingleInferenceResult::Unmodified;
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
    /// (e.g. Unknown or IntegerLike) exist in the type. Will not clear or check
    /// if the supplied `ConcreteType` is empty, will simply append to the parts
    /// vector.
    fn write_concrete_type(&self, concrete_type: &mut ConcreteType) {
        use InferenceTypePart as ITP;
        use ConcreteTypePart as CTP;

        // Make sure inference type is specified but concrete type is not yet specified
        debug_assert!(!self.parts.is_empty());
        concrete_type.parts.reserve(self.parts.len());

        let mut idx = 0;
        while idx < self.parts.len() {
            let part = &self.parts[idx];
            let converted_part = match part {
                ITP::Marker(_) => {
                    // Markers are removed when writing to the concrete type.
                    idx += 1;
                    continue;
                },
                ITP::Unknown | ITP::NumberLike |
                ITP::IntegerLike | ITP::ArrayLike | ITP::PortLike => {
                    // Should not happen if type inferencing works correctly: we
                    // should have returned a programmer-readable error or have
                    // inferred all types.
                    unreachable!("attempted to convert inference type part {:?} into concrete type", part);
                },
                ITP::Void => CTP::Void,
                ITP::Message => CTP::Message,
                ITP::Bool => CTP::Bool,
                ITP::UInt8 => CTP::UInt8,
                ITP::UInt16 => CTP::UInt16,
                ITP::UInt32 => CTP::UInt32,
                ITP::UInt64 => CTP::UInt64,
                ITP::SInt8 => CTP::SInt8,
                ITP::SInt16 => CTP::SInt16,
                ITP::SInt32 => CTP::SInt32,
                ITP::SInt64 => CTP::SInt64,
                ITP::Character => CTP::Character,
                ITP::String => {
                    // Inferred type has a 'char' subtype to simplify array
                    // checking, we remove it here.
                    debug_assert_eq!(self.parts[idx + 1], InferenceTypePart::Character);
                    idx += 1;
                    CTP::String
                },
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

    /// Writes a human-readable version of the type to a string. This is used
    /// to display error messages
    fn write_display_name(
        buffer: &mut String, heap: &Heap, parts: &[InferenceTypePart], mut idx: usize
    ) -> usize {
        use InferenceTypePart as ITP;

        match &parts[idx] {
            ITP::Marker(_marker_idx) => {
                if debug_log_enabled!() {
                    buffer.push_str(&format!("{{Marker:{}}}", *_marker_idx));
                }
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
            },
            ITP::Unknown => buffer.push_str("?"),
            ITP::NumberLike => buffer.push_str("numberlike"),
            ITP::IntegerLike => buffer.push_str("integerlike"),
            ITP::ArrayLike => {
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push_str("[?]");
            },
            ITP::PortLike => {
                buffer.push_str("portlike<");
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            }
            ITP::Void => buffer.push_str("void"),
            ITP::Bool => buffer.push_str(KW_TYPE_BOOL_STR),
            ITP::UInt8 => buffer.push_str(KW_TYPE_UINT8_STR),
            ITP::UInt16 => buffer.push_str(KW_TYPE_UINT16_STR),
            ITP::UInt32 => buffer.push_str(KW_TYPE_UINT32_STR),
            ITP::UInt64 => buffer.push_str(KW_TYPE_UINT64_STR),
            ITP::SInt8 => buffer.push_str(KW_TYPE_SINT8_STR),
            ITP::SInt16 => buffer.push_str(KW_TYPE_SINT16_STR),
            ITP::SInt32 => buffer.push_str(KW_TYPE_SINT32_STR),
            ITP::SInt64 => buffer.push_str(KW_TYPE_SINT64_STR),
            ITP::Character => buffer.push_str(KW_TYPE_CHAR_STR),
            ITP::String => {
                buffer.push_str(KW_TYPE_STRING_STR);
                idx += 1; // skip the 'char' subtype
            },
            ITP::Message => {
                buffer.push_str(KW_TYPE_MESSAGE_STR);
                buffer.push('<');
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
                buffer.push_str(KW_TYPE_IN_PORT_STR);
                buffer.push('<');
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            },
            ITP::Output => {
                buffer.push_str(KW_TYPE_OUT_PORT_STR);
                buffer.push('<');
                idx = Self::write_display_name(buffer, heap, parts, idx + 1);
                buffer.push('>');
            },
            ITP::Instance(definition_id, num_sub) => {
                let definition = &heap[*definition_id];
                buffer.push_str(definition.identifier().value.as_str());
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

impl Default for InferenceType {
    fn default() -> Self {
        Self{
            has_marker: false,
            is_done: false,
            parts: Vec::new(),
        }
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
    type Item = (u32, &'a [InferenceTypePart]);

    fn next(&mut self) -> Option<Self::Item> {
        // Iterate until we find a marker
        while self.idx < self.parts.len() {
            if let InferenceTypePart::Marker(marker) = self.parts[self.idx] {
                // Found a marker, find the subtree end
                let start_idx = self.idx + 1;
                let end_idx = InferenceType::find_subtree_end_idx(self.parts, start_idx);

                // Modify internal index, then return items
                self.idx = end_idx;
                return Some((marker, &self.parts[start_idx..end_idx]));
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
    Component(ComponentDefinitionId),
    Function(FunctionDefinitionId),
}

impl DefinitionType {
    fn definition_id(&self) -> DefinitionId {
        match self {
            DefinitionType::Component(v) => v.upcast(),
            DefinitionType::Function(v) => v.upcast(),
        }
    }
}

pub(crate) struct ResolveQueueElement {
    // Note that using the `definition_id` and the `monomorph_idx` one may
    // query the type table for the full procedure type, thereby retrieving
    // the polymorphic arguments to the procedure.
    pub(crate) root_id: RootId,
    pub(crate) definition_id: DefinitionId,
    pub(crate) reserved_monomorph_idx: i32,
}

pub(crate) type ResolveQueue = Vec<ResolveQueueElement>;

#[derive(Clone)]
struct InferenceExpression {
    expr_type: InferenceType,       // result type from expression
    expr_id: ExpressionId,          // expression that is evaluated
    field_or_monomorph_idx: i32,    // index of field, of index of monomorph array in type table
    extra_data_idx: i32,     // index of extra data needed for inference
}

impl Default for InferenceExpression {
    fn default() -> Self {
        Self{
            expr_type: InferenceType::default(),
            expr_id: ExpressionId::new_invalid(),
            field_or_monomorph_idx: -1,
            extra_data_idx: -1,
        }
    }
}

/// This particular visitor will recurse depth-first into the AST and ensures
/// that all expressions have the appropriate types.
pub(crate) struct PassTyping {
    // Current definition we're typechecking.
    reserved_idx: i32,
    definition_type: DefinitionType,
    poly_vars: Vec<ConcreteType>,

    // Buffers for iteration over substatements and subexpressions
    stmt_buffer: Vec<StatementId>,
    expr_buffer: Vec<ExpressionId>,

    // Mapping from parser type to inferred type. We attempt to continue to
    // specify these types until we're stuck or we've fully determined the type.
    var_types: HashMap<VariableId, VarData>,            // types of variables
    expr_types: Vec<InferenceExpression>,                     // will be transferred to type table at end
    extra_data: Vec<ExtraData>,       // data for polymorph inference
    // Keeping track of which expressions need to be reinferred because the
    // expressions they're linked to made progression on an associated type
    expr_queued: DequeSet<i32>,
}

// TODO: @Rename, this is used for a lot of type inferencing. It seems like
//  there is a different underlying architecture waiting to surface.
struct ExtraData {
    expr_id: ExpressionId, // the expression with which this data is associated
    definition_id: DefinitionId, // the definition, only used for user feedback
    /// Progression of polymorphic variables (if any)
    poly_vars: Vec<InferenceType>,
    /// Progression of types of call arguments or struct members
    embedded: Vec<InferenceType>,
    returned: InferenceType,
}

impl Default for ExtraData {
    fn default() -> Self {
        Self{
            expr_id: ExpressionId::new_invalid(),
            definition_id: DefinitionId::new_invalid(),
            poly_vars: Vec::new(),
            embedded: Vec::new(),
            returned: InferenceType::default(),
        }
    }
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

impl PassTyping {
    pub(crate) fn new() -> Self {
        PassTyping {
            reserved_idx: -1,
            definition_type: DefinitionType::Function(FunctionDefinitionId::new_invalid()),
            poly_vars: Vec::new(),
            stmt_buffer: Vec::with_capacity(STMT_BUFFER_INIT_CAPACITY),
            expr_buffer: Vec::with_capacity(EXPR_BUFFER_INIT_CAPACITY),
            var_types: HashMap::new(),
            expr_types: Vec::new(),
            extra_data: Vec::new(),
            expr_queued: DequeSet::new(),
        }
    }

    // TODO: @cleanup Unsure about this, maybe a pattern will arise after
    //  a while.
    pub(crate) fn queue_module_definitions(ctx: &mut Ctx, queue: &mut ResolveQueue) {
        debug_assert_eq!(ctx.module().phase, ModuleCompilationPhase::ValidatedAndLinked);
        let root_id = ctx.module().root_id;
        let root = &ctx.heap.protocol_descriptions[root_id];
        for definition_id in &root.definitions {
            let definition = &ctx.heap[*definition_id];

            let first_concrete_part = match definition {
                Definition::Function(definition) => {
                    if definition.poly_vars.is_empty() {
                        Some(ConcreteTypePart::Function(*definition_id, 0))
                    } else {
                        None
                    }
                }
                Definition::Component(definition) => {
                    if definition.poly_vars.is_empty() {
                        Some(ConcreteTypePart::Component(*definition_id, 0))
                    } else {
                        None
                    }
                },
                Definition::Enum(_) | Definition::Struct(_) | Definition::Union(_) => None,
            };

            if let Some(first_concrete_part) = first_concrete_part {
                let concrete_type = ConcreteType{ parts: vec![first_concrete_part] };
                let reserved_idx = ctx.types.reserve_procedure_monomorph_index(definition_id, concrete_type);
                queue.push(ResolveQueueElement{
                    root_id,
                    definition_id: *definition_id,
                    reserved_monomorph_idx: reserved_idx,
                })
            }
        }
    }

    pub(crate) fn handle_module_definition(
        &mut self, ctx: &mut Ctx, queue: &mut ResolveQueue, element: ResolveQueueElement
    ) -> VisitorResult {
        self.reset();
        debug_assert_eq!(ctx.module().root_id, element.root_id);
        debug_assert!(self.poly_vars.is_empty());

        // Prepare for visiting the definition
        self.reserved_idx = element.reserved_monomorph_idx;

        let proc_base = ctx.types.get_base_definition(&element.definition_id).unwrap();
        if proc_base.is_polymorph {
            let proc_monos = proc_base.definition.procedure_monomorphs();
            let proc_mono = &(*proc_monos)[element.reserved_monomorph_idx as usize];

            for poly_arg in proc_mono.concrete_type.embedded_iter(0) {
                self.poly_vars.push(ConcreteType{ parts: Vec::from(poly_arg) });
            }
        }

        // Visit the definition, setting up the type resolving process, then
        // (attempt to) resolve all types
        self.visit_definition(ctx, element.definition_id)?;
        self.resolve_types(ctx, queue)?;
        Ok(())
    }

    fn reset(&mut self) {
        self.reserved_idx = -1;
        self.definition_type = DefinitionType::Function(FunctionDefinitionId::new_invalid());
        self.poly_vars.clear();
        self.stmt_buffer.clear();
        self.expr_buffer.clear();
        self.var_types.clear();
        self.expr_types.clear();
        self.extra_data.clear();
        self.expr_queued.clear();
    }
}

impl Visitor for PassTyping {
    // Definitions

    fn visit_component_definition(&mut self, ctx: &mut Ctx, id: ComponentDefinitionId) -> VisitorResult {
        self.definition_type = DefinitionType::Component(id);

        let comp_def = &ctx.heap[id];
        debug_assert_eq!(comp_def.poly_vars.len(), self.poly_vars.len(), "component polyvars do not match imposed polyvars");

        debug_log!("{}", "-".repeat(50));
        debug_log!("Visiting component '{}': {}", comp_def.identifier.value.as_str(), id.0.index);
        debug_log!("{}", "-".repeat(50));

        // Reserve data for expression types
        debug_assert!(self.expr_types.is_empty());
        self.expr_types.resize(comp_def.num_expressions_in_body as usize, Default::default());

        // Visit parameters
        for param_id in comp_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let var_type = self.determine_inference_type_from_parser_type_elements(&param.parser_type.elements, true);
            debug_assert!(var_type.is_done, "expected component arguments to be concrete types");
            self.var_types.insert(param_id, VarData::new_local(var_type));
        }

        // Visit the body and all of its expressions
        let body_stmt_id = ctx.heap[id].body;
        self.visit_block_stmt(ctx, body_stmt_id)
    }

    fn visit_function_definition(&mut self, ctx: &mut Ctx, id: FunctionDefinitionId) -> VisitorResult {
        self.definition_type = DefinitionType::Function(id);

        let func_def = &ctx.heap[id];
        debug_assert_eq!(func_def.poly_vars.len(), self.poly_vars.len(), "function polyvars do not match imposed polyvars");

        debug_log!("{}", "-".repeat(50));
        debug_log!("Visiting function '{}': {}", func_def.identifier.value.as_str(), id.0.index);
        if debug_log_enabled!() {
            debug_log!("Polymorphic variables:");
            for (_idx, poly_var) in self.poly_vars.iter().enumerate() {
                let mut infer_type_parts = Vec::new();
                Self::determine_inference_type_from_concrete_type(
                    &mut infer_type_parts, &poly_var.parts
                );
                let _infer_type = InferenceType::new(false, true, infer_type_parts);
                debug_log!(" - [{:03}] {:?}", _idx, _infer_type.display_name(&ctx.heap));
            }
        }
        debug_log!("{}", "-".repeat(50));

        // Reserve data for expression types
        debug_assert!(self.expr_types.is_empty());
        self.expr_types.resize(func_def.num_expressions_in_body as usize, Default::default());

        // Visit parameters
        for param_id in func_def.parameters.clone() {
            let param = &ctx.heap[param_id];
            let var_type = self.determine_inference_type_from_parser_type_elements(&param.parser_type.elements, true);
            debug_assert!(var_type.is_done, "expected function arguments to be concrete types");
            self.var_types.insert(param_id, VarData::new_local(var_type));
        }

        // Visit all of the expressions within the body
        let body_stmt_id = ctx.heap[id].body;
        self.visit_block_stmt(ctx, body_stmt_id)
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
        let var_type = self.determine_inference_type_from_parser_type_elements(&local.parser_type.elements, true);
        self.var_types.insert(memory_stmt.variable, VarData::new_local(var_type));

        Ok(())
    }

    fn visit_local_channel_stmt(&mut self, ctx: &mut Ctx, id: ChannelStatementId) -> VisitorResult {
        let channel_stmt = &ctx.heap[id];

        let from_local = &ctx.heap[channel_stmt.from];
        let from_var_type = self.determine_inference_type_from_parser_type_elements(&from_local.parser_type.elements, true);
        self.var_types.insert(from_local.this, VarData::new_channel(from_var_type, channel_stmt.to));

        let to_local = &ctx.heap[channel_stmt.to];
        let to_var_type = self.determine_inference_type_from_parser_type_elements(&to_local.parser_type.elements, true);
        self.var_types.insert(to_local.this, VarData::new_channel(to_var_type, channel_stmt.from));

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
        self.visit_block_stmt(ctx, true_body_id)?;
        if let Some(false_body_id) = false_body_id {
            self.visit_block_stmt(ctx, false_body_id)?;
        }

        Ok(())
    }

    fn visit_while_stmt(&mut self, ctx: &mut Ctx, id: WhileStatementId) -> VisitorResult {
        let while_stmt = &ctx.heap[id];

        let body_id = while_stmt.body;
        let test_expr_id = while_stmt.test;

        self.visit_expr(ctx, test_expr_id)?;
        self.visit_block_stmt(ctx, body_id)?;

        Ok(())
    }

    fn visit_synchronous_stmt(&mut self, ctx: &mut Ctx, id: SynchronousStatementId) -> VisitorResult {
        let sync_stmt = &ctx.heap[id];
        let body_id = sync_stmt.body;

        self.visit_block_stmt(ctx, body_id)
    }

    fn visit_return_stmt(&mut self, ctx: &mut Ctx, id: ReturnStatementId) -> VisitorResult {
        let return_stmt = &ctx.heap[id];
        debug_assert_eq!(return_stmt.expressions.len(), 1);
        let expr_id = return_stmt.expressions[0];

        self.visit_expr(ctx, expr_id)
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

    fn visit_binding_expr(&mut self, ctx: &mut Ctx, id: BindingExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let binding_expr = &ctx.heap[id];
        let bound_to_id = binding_expr.bound_to;
        let bound_from_id = binding_expr.bound_from;

        self.visit_expr(ctx, bound_to_id)?;
        self.visit_expr(ctx, bound_from_id)?;

        self.progress_binding_expr(ctx, id)
    }

    fn visit_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let conditional_expr = &ctx.heap[id];
        let test_expr_id = conditional_expr.test;
        let true_expr_id = conditional_expr.true_expression;
        let false_expr_id = conditional_expr.false_expression;

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

    fn visit_literal_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let literal_expr = &ctx.heap[id];
        match &literal_expr.value {
            Literal::Null | Literal::False | Literal::True |
            Literal::Integer(_) | Literal::Character(_) | Literal::String(_) => {
                // No subexpressions
            },
            Literal::Struct(literal) => {
                // TODO: @performance
                let expr_ids: Vec<_> = literal.fields
                    .iter()
                    .map(|f| f.value)
                    .collect();

                self.insert_initial_struct_polymorph_data(ctx, id);

                for expr_id in expr_ids {
                    self.visit_expr(ctx, expr_id)?;
                }
            },
            Literal::Enum(_) => {
                // Enumerations do not carry any subexpressions, but may still
                // have a user-defined polymorphic marker variable. For this 
                // reason we may still have to apply inference to this 
                // polymorphic variable
                self.insert_initial_enum_polymorph_data(ctx, id);
            },
            Literal::Union(literal) => {
                // May carry subexpressions and polymorphic arguments
                // TODO: @performance
                let expr_ids = literal.values.clone();
                self.insert_initial_union_polymorph_data(ctx, id);

                for expr_id in expr_ids {
                    self.visit_expr(ctx, expr_id)?;
                }
            },
            Literal::Array(expressions) => {
                // TODO: @performance
                let expr_ids = expressions.clone();
                for expr_id in expr_ids {
                    self.visit_expr(ctx, expr_id)?;
                }
            }
        }

        self.progress_literal_expr(ctx, id)
    }

    fn visit_cast_expr(&mut self, ctx: &mut Ctx, id: CastExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let cast_expr = &ctx.heap[id];
        let subject_expr_id = cast_expr.subject;

        self.visit_expr(ctx, subject_expr_id)?;

        self.progress_cast_expr(ctx, id)
    }

    fn visit_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;
        self.insert_initial_call_polymorph_data(ctx, id);

        // By default we set the polymorph idx for calls to 0. If the call ends
        // up not being a polymorphic one, then we will select the default
        // expression types in the type table
        let call_expr = &ctx.heap[id];
        self.expr_types[call_expr.unique_id_in_definition as usize].field_or_monomorph_idx = 0;

        // Visit all arguments
        for arg_expr_id in call_expr.arguments.clone() { // TODO: @Performance
            self.visit_expr(ctx, arg_expr_id)?;
        }

        self.progress_call_expr(ctx, id)
    }

    fn visit_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> VisitorResult {
        let upcast_id = id.upcast();
        self.insert_initial_expr_inference_type(ctx, upcast_id)?;

        let var_expr = &ctx.heap[id];
        debug_assert!(var_expr.declaration.is_some());

        // Not pretty: if a binding expression, then this is the first time we
        // encounter the variable, so we still need to insert the variable data.
        let declaration = &ctx.heap[var_expr.declaration.unwrap()];
        if !self.var_types.contains_key(&declaration.this)  {
            debug_assert!(declaration.kind == VariableKind::Binding);
            let var_type = self.determine_inference_type_from_parser_type_elements(
                &declaration.parser_type.elements, true
            );
            self.var_types.insert(declaration.this, VarData{
                var_type,
                used_at: vec![upcast_id],
                linked_var: None
            });
        } else {
            let var_data = self.var_types.get_mut(&declaration.this).unwrap();
            var_data.used_at.push(upcast_id);
        }

        self.progress_variable_expr(ctx, id)
    }
}

impl PassTyping {
    #[allow(dead_code)] // used when debug flag at the top of this file is true.
    fn debug_get_display_name(&self, ctx: &Ctx, expr_id: ExpressionId) -> String {
        let expr_idx = ctx.heap[expr_id].get_unique_id_in_definition();
        let expr_type = &self.expr_types[expr_idx as usize].expr_type;
        expr_type.display_name(&ctx.heap)
    }

    fn resolve_types(&mut self, ctx: &mut Ctx, queue: &mut ResolveQueue) -> Result<(), ParseError> {
        // Keep inferring until we can no longer make any progress
        while !self.expr_queued.is_empty() {
            let next_expr_idx = self.expr_queued.pop_front().unwrap();
            self.progress_expr(ctx, next_expr_idx)?;
        }

        // Helper for transferring polymorphic variables to concrete types and
        // checking if they're completely specified
        fn inference_type_to_concrete_type(
            ctx: &Ctx, expr_id: ExpressionId, inference: &Vec<InferenceType>,
            first_concrete_part: ConcreteTypePart,
        ) -> Result<ConcreteType, ParseError> {
            // Prepare storage vector
            let mut num_inference_parts = 0;
            for inference_type in inference {
                num_inference_parts += inference_type.parts.len();
            }

            let mut concrete_type = ConcreteType{
                parts: Vec::with_capacity(1 + num_inference_parts),
            };
            concrete_type.parts.push(first_concrete_part);

            // Go through all polymorphic arguments and add them to the concrete
            // types.
            for (poly_idx, poly_type) in inference.iter().enumerate() {
                if !poly_type.is_done {
                    let expr = &ctx.heap[expr_id];
                    let definition = match expr {
                        Expression::Call(expr) => expr.definition,
                        Expression::Literal(expr) => match &expr.value {
                            Literal::Enum(lit) => lit.definition,
                            Literal::Union(lit) => lit.definition,
                            Literal::Struct(lit) => lit.definition,
                            _ => unreachable!()
                        },
                        _ => unreachable!(),
                    };
                    let poly_vars = ctx.heap[definition].poly_vars();
                    return Err(ParseError::new_error_at_span(
                        &ctx.module().source, expr.operation_span(), format!(
                            "could not fully infer the type of polymorphic variable '{}' of this expression (got '{}')",
                            poly_vars[poly_idx].value.as_str(), poly_type.display_name(&ctx.heap)
                        )
                    ));
                }

                poly_type.write_concrete_type(&mut concrete_type);
            }

            Ok(concrete_type)
        }

        // Inference is now done. But we may still have uninferred types. So we
        // check for these.
        for (infer_expr_idx, infer_expr) in self.expr_types.iter_mut().enumerate() {
            let expr_type = &mut infer_expr.expr_type;
            if !expr_type.is_done {
                // Auto-infer numberlike/integerlike types to a regular int
                if expr_type.parts.len() == 1 && expr_type.parts[0] == InferenceTypePart::IntegerLike {
                    expr_type.parts[0] = InferenceTypePart::SInt32;
                    self.expr_queued.push_back(infer_expr_idx as i32);
                } else {
                    let expr = &ctx.heap[infer_expr.expr_id];
                    return Err(ParseError::new_error_at_span(
                        &ctx.module().source, expr.full_span(), format!(
                            "could not fully infer the type of this expression (got '{}')",
                            expr_type.display_name(&ctx.heap)
                        )
                    ));
                }
            }

            // Expression is fine, check if any extra data is attached
            if infer_expr.extra_data_idx < 0 { continue; }

            // Extra data is attached, perform typechecking and transfer
            // resolved information to the expression
            let extra_data = &self.extra_data[infer_expr.extra_data_idx as usize];
            if extra_data.poly_vars.is_empty() { continue; }

            // Note that only call and literal expressions need full inference.
            // Select expressions also use `extra_data`, but only for temporary
            // storage of the struct type whose field it is selecting.
            match &ctx.heap[extra_data.expr_id] {
                Expression::Call(expr) => {
                    // Check if it is not a builtin function. If not, then
                    // construct the first part of the concrete type.
                    let first_concrete_part = if expr.method == Method::UserFunction {
                        ConcreteTypePart::Function(expr.definition, extra_data.poly_vars.len() as u32)
                    } else if expr.method == Method::UserComponent {
                        ConcreteTypePart::Component(expr.definition, extra_data.poly_vars.len() as u32)
                    } else {
                        // Builtin function
                        continue;
                    };

                    let definition_id = expr.definition;
                    let concrete_type = inference_type_to_concrete_type(
                        ctx, extra_data.expr_id, &extra_data.poly_vars, first_concrete_part
                    )?;

                    match ctx.types.get_procedure_monomorph_index(&definition_id, &concrete_type) {
                        Some(reserved_idx) => {
                            // Already typechecked, or already put into the resolve queue
                            infer_expr.field_or_monomorph_idx = reserved_idx;
                        },
                        None => {
                            // Not typechecked yet, so add an entry in the queue
                            let reserved_idx = ctx.types.reserve_procedure_monomorph_index(&definition_id, concrete_type);
                            infer_expr.field_or_monomorph_idx = reserved_idx;
                            queue.push(ResolveQueueElement{
                                root_id: ctx.heap[definition_id].defined_in(),
                                definition_id,
                                reserved_monomorph_idx: reserved_idx,
                            });
                        }
                    }
                },
                Expression::Literal(expr) => {
                    let definition_id = match &expr.value {
                        Literal::Enum(lit) => lit.definition,
                        Literal::Union(lit) => lit.definition,
                        Literal::Struct(lit) => lit.definition,
                        _ => unreachable!(),
                    };
                    let first_concrete_part = ConcreteTypePart::Instance(definition_id, extra_data.poly_vars.len() as u32);
                    let concrete_type = inference_type_to_concrete_type(
                        ctx, extra_data.expr_id, &extra_data.poly_vars, first_concrete_part
                    )?;
                    let mono_index = ctx.types.add_data_monomorph(ctx.modules, ctx.heap, ctx.arch, definition_id, concrete_type)?;
                    infer_expr.field_or_monomorph_idx = mono_index;
                },
                Expression::Select(_) => {
                    debug_assert!(infer_expr.field_or_monomorph_idx >= 0);
                },
                _ => {
                    unreachable!("handling extra data for expression {:?}", &ctx.heap[extra_data.expr_id]);
                }
            }
        }

        // If we did any implicit type forcing, then our queue isn't empty
        // anymore
        while !self.expr_queued.is_empty() {
            let expr_idx = self.expr_queued.pop_back().unwrap();
            self.progress_expr(ctx, expr_idx)?;
        }

        // Every expression checked, and new monomorphs are queued. Transfer the
        // expression information to the type table.
        let definition_id = match &self.definition_type {
            DefinitionType::Component(id) => id.upcast(),
            DefinitionType::Function(id) => id.upcast(),
        };

        let target = ctx.types.get_procedure_expression_data_mut(&definition_id, self.reserved_idx);
        debug_assert!(target.expr_data.is_empty()); // makes sure we never queue something twice

        target.expr_data.reserve(self.expr_types.len());
        for infer_expr in self.expr_types.iter() {
            let mut concrete = ConcreteType::default();
            infer_expr.expr_type.write_concrete_type(&mut concrete);
            target.expr_data.push(MonomorphExpression{
                expr_type: concrete,
                field_or_monomorph_idx: infer_expr.field_or_monomorph_idx
            });
        }

        Ok(())
    }

    fn progress_expr(&mut self, ctx: &mut Ctx, idx: i32) -> Result<(), ParseError> {
        let id = self.expr_types[idx as usize].expr_id; // TODO: @Temp
        match &ctx.heap[id] {
            Expression::Assignment(expr) => {
                let id = expr.this;
                self.progress_assignment_expr(ctx, id)
            },
            Expression::Binding(expr) => {
                let id = expr.this;
                self.progress_binding_expr(ctx, id)
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
            Expression::Literal(expr) => {
                let id = expr.this;
                self.progress_literal_expr(ctx, id)
            },
            Expression::Cast(expr) => {
                let id = expr.this;
                self.progress_cast_expr(ctx, id)
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

    fn progress_assignment_expr(&mut self, ctx: &mut Ctx, id: AssignmentExpressionId) -> Result<(), ParseError> {
        use AssignmentOperator as AO;

        let upcast_id = id.upcast();

        let expr = &ctx.heap[id];
        let arg1_expr_id = expr.left;
        let arg2_expr_id = expr.right;

        debug_log!("Assignment expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.debug_get_display_name(ctx, arg1_expr_id));
        debug_log!("   - Arg2 type: {}", self.debug_get_display_name(ctx, arg2_expr_id));
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        // Assignment does not return anything (it operates like a statement)
        let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &VOID_TEMPLATE)?;

        // Apply forced constraint to LHS value
        let progress_forced = match expr.operation {
            AO::Set =>
                false,
            AO::Concatenated =>
                self.apply_template_constraint(ctx, arg1_expr_id, &ARRAYLIKE_TEMPLATE)?,
            AO::Multiplied | AO::Divided | AO::Added | AO::Subtracted =>
                self.apply_template_constraint(ctx, arg1_expr_id, &NUMBERLIKE_TEMPLATE)?,
            AO::Remained | AO::ShiftedLeft | AO::ShiftedRight |
            AO::BitwiseAnded | AO::BitwiseXored | AO::BitwiseOred =>
                self.apply_template_constraint(ctx, arg1_expr_id, &INTEGERLIKE_TEMPLATE)?,
        };

        let (progress_arg1, progress_arg2) = self.apply_equal2_constraint(
            ctx, upcast_id, arg1_expr_id, 0, arg2_expr_id, 0
        )?;

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_forced || progress_arg1, self.debug_get_display_name(ctx, arg1_expr_id));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.debug_get_display_name(ctx, arg2_expr_id));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));


        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_forced || progress_arg1 { self.queue_expr(ctx, arg1_expr_id); }
        if progress_arg2 { self.queue_expr(ctx, arg2_expr_id); }

        Ok(())
    }

    fn progress_binding_expr(&mut self, ctx: &mut Ctx, id: BindingExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let binding_expr = &ctx.heap[id];
        let bound_from_id = binding_expr.bound_from;
        let bound_to_id = binding_expr.bound_to;

        // Output is always a boolean. The two arguments should be of equal
        // type.
        let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
        let (progress_from, progress_to) = self.apply_equal2_constraint(ctx, upcast_id, bound_from_id, 0, bound_to_id, 0)?;

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_from { self.queue_expr(ctx, bound_from_id); }
        if progress_to { self.queue_expr(ctx, bound_to_id); }

        Ok(())
    }

    fn progress_conditional_expr(&mut self, ctx: &mut Ctx, id: ConditionalExpressionId) -> Result<(), ParseError> {
        // Note: test expression type is already enforced
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg1_expr_id = expr.true_expression;
        let arg2_expr_id = expr.false_expression;

        debug_log!("Conditional expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.debug_get_display_name(ctx, arg1_expr_id));
        debug_log!("   - Arg2 type: {}", self.debug_get_display_name(ctx, arg2_expr_id));
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        // I keep confusing myself: this applies equality of types between the
        // condition branches' types, and the result from the conditional
        // expression, because the result from the conditional is one of the
        // branches.
        let (progress_expr, progress_arg1, progress_arg2) = self.apply_equal3_constraint(
            ctx, upcast_id, arg1_expr_id, arg2_expr_id, 0
        )?;

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_arg1, self.debug_get_display_name(ctx, arg1_expr_id));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.debug_get_display_name(ctx, arg2_expr_id));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg1 { self.queue_expr(ctx, arg1_expr_id); }
        if progress_arg2 { self.queue_expr(ctx, arg2_expr_id); }

        Ok(())
    }

    fn progress_binary_expr(&mut self, ctx: &mut Ctx, id: BinaryExpressionId) -> Result<(), ParseError> {
        // Note: our expression type might be fixed by our parent, but we still
        // need to make sure it matches the type associated with our operation.
        use BinaryOperator as BO;

        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg1_id = expr.left;
        let arg2_id = expr.right;

        debug_log!("Binary expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg1 type: {}", self.debug_get_display_name(ctx, arg1_id));
        debug_log!("   - Arg2 type: {}", self.debug_get_display_name(ctx, arg2_id));
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        let (progress_expr, progress_arg1, progress_arg2) = match expr.operation {
            BO::Concatenate => {
                // Two cases: if one of the arguments or the output type is a
                // string, then all must be strings. Otherwise the arguments
                // must be arraylike and the output will be a array.
                let (expr_is_str, expr_is_not_str) = self.type_is_certainly_or_certainly_not_string(ctx, upcast_id);
                let (arg1_is_str, arg1_is_not_str) = self.type_is_certainly_or_certainly_not_string(ctx, arg1_id);
                let (arg2_is_str, arg2_is_not_str) = self.type_is_certainly_or_certainly_not_string(ctx, arg2_id);

                let someone_is_str = expr_is_str || arg1_is_str || arg2_is_str;
                let someone_is_not_str = expr_is_not_str || arg1_is_not_str || arg2_is_not_str;

                // Note: this statement is an expression returning the progression bools
                if someone_is_str {
                    // One of the arguments is a string, then all must be strings
                    self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 0)?
                } else {
                    let progress_expr = if someone_is_not_str {
                        // Output must be a normal array
                        self.apply_template_constraint(ctx, upcast_id, &ARRAY_TEMPLATE)?
                    } else {
                        // Output may still be anything
                        self.apply_template_constraint(ctx, upcast_id, &ARRAYLIKE_TEMPLATE)?
                    };

                    let progress_arg1 = self.apply_template_constraint(ctx, arg1_id, &ARRAYLIKE_TEMPLATE)?;
                    let progress_arg2 = self.apply_template_constraint(ctx, arg2_id, &ARRAYLIKE_TEMPLATE)?;

                    // If they're all arraylike, then we want the subtype to match
                    let (subtype_expr, subtype_arg1, subtype_arg2) =
                        self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 1)?;

                    (progress_expr || subtype_expr, progress_arg1 || subtype_arg1, progress_arg2 || subtype_arg2)
                }
            },
            BO::LogicalAnd => {
                // Forced boolean on all
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg1 = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg2 = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::LogicalOr => {
                // Forced boolean on all
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg1 = self.apply_forced_constraint(ctx, arg1_id, &BOOL_TEMPLATE)?;
                let progress_arg2 = self.apply_forced_constraint(ctx, arg2_id, &BOOL_TEMPLATE)?;

                (progress_expr, progress_arg1, progress_arg2)
            },
            BO::BitwiseOr | BO::BitwiseXor | BO::BitwiseAnd | BO::Remainder | BO::ShiftLeft | BO::ShiftRight => {
                // All equal of integer type
                let progress_base = self.apply_template_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?;
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
                let progress_arg_base = self.apply_template_constraint(ctx, arg1_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_arg1, progress_arg2) =
                    self.apply_equal2_constraint(ctx, upcast_id, arg1_id, 0, arg2_id, 0)?;

                (progress_expr, progress_arg_base || progress_arg1, progress_arg_base || progress_arg2)
            },
            BO::Add | BO::Subtract | BO::Multiply | BO::Divide => {
                // All equal of number type
                let progress_base = self.apply_template_constraint(ctx, upcast_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg1, progress_arg2) =
                    self.apply_equal3_constraint(ctx, upcast_id, arg1_id, arg2_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg1, progress_base || progress_arg2)
            },
        };

        debug_log!(" * After:");
        debug_log!("   - Arg1 type [{}]: {}", progress_arg1, self.debug_get_display_name(ctx, arg1_id));
        debug_log!("   - Arg2 type [{}]: {}", progress_arg2, self.debug_get_display_name(ctx, arg2_id));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg1 { self.queue_expr(ctx, arg1_id); }
        if progress_arg2 { self.queue_expr(ctx, arg2_id); }

        Ok(())
    }

    fn progress_unary_expr(&mut self, ctx: &mut Ctx, id: UnaryExpressionId) -> Result<(), ParseError> {
        use UnaryOperator as UO;

        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let arg_id = expr.expression;

        debug_log!("Unary expr '{:?}': {}", expr.operation, upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Arg  type: {}", self.debug_get_display_name(ctx, arg_id));
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        let (progress_expr, progress_arg) = match expr.operation {
            UO::Positive | UO::Negative => {
                // Equal types of numeric class
                let progress_base = self.apply_template_constraint(ctx, upcast_id, &NUMBERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, arg_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg)
            },
            UO::BitwiseNot => {
                // Equal types of integer class
                let progress_base = self.apply_template_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?;
                let (progress_expr, progress_arg) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, arg_id, 0)?;

                (progress_base || progress_expr, progress_base || progress_arg)
            },
            UO::LogicalNot => {
                // Both bools
                let progress_expr = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                let progress_arg = self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?;
                (progress_expr, progress_arg)
            }
        };

        debug_log!(" * After:");
        debug_log!("   - Arg  type [{}]: {}", progress_arg, self.debug_get_display_name(ctx, arg_id));
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_arg { self.queue_expr(ctx, arg_id); }

        Ok(())
    }

    fn progress_indexing_expr(&mut self, ctx: &mut Ctx, id: IndexingExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;
        let index_id = expr.index;

        debug_log!("Indexing expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.debug_get_display_name(ctx, subject_id));
        debug_log!("   - Index   type: {}", self.debug_get_display_name(ctx, index_id));
        debug_log!("   - Expr    type: {}", self.debug_get_display_name(ctx, upcast_id));

        // Make sure subject is arraylike and index is integerlike
        let progress_subject_base = self.apply_template_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
        let progress_index = self.apply_template_constraint(ctx, index_id, &INTEGERLIKE_TEMPLATE)?;

        // Make sure if output is of T then subject is Array<T>
        let (progress_expr, progress_subject) =
            self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 0, subject_id, 1)?;

        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject_base || progress_subject, self.debug_get_display_name(ctx, subject_id));
        debug_log!("   - Index   type [{}]: {}", progress_index, self.debug_get_display_name(ctx, index_id));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_subject_base || progress_subject { self.queue_expr(ctx, subject_id); }
        if progress_index { self.queue_expr(ctx, index_id); }

        Ok(())
    }

    fn progress_slicing_expr(&mut self, ctx: &mut Ctx, id: SlicingExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let subject_id = expr.subject;
        let from_id = expr.from_index;
        let to_id = expr.to_index;

        debug_log!("Slicing expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.debug_get_display_name(ctx, subject_id));
        debug_log!("   - FromIdx type: {}", self.debug_get_display_name(ctx, from_id));
        debug_log!("   - ToIdx   type: {}", self.debug_get_display_name(ctx, to_id));
        debug_log!("   - Expr    type: {}", self.debug_get_display_name(ctx, upcast_id));

        // Make sure subject is arraylike and indices are of equal integerlike
        let progress_subject_base = self.apply_template_constraint(ctx, subject_id, &ARRAYLIKE_TEMPLATE)?;
        let progress_idx_base = self.apply_template_constraint(ctx, from_id, &INTEGERLIKE_TEMPLATE)?;
        let (progress_from, progress_to) = self.apply_equal2_constraint(ctx, upcast_id, from_id, 0, to_id, 0)?;

        let (progress_expr, progress_subject) = match self.type_is_certainly_or_certainly_not_string(ctx, subject_id) {
            (true, _) => {
                // Certainly a string
                (self.apply_forced_constraint(ctx, upcast_id, &STRING_TEMPLATE)?, false)
            },
            (_, true) => {
                // Certainly not a string
                let progress_expr_base = self.apply_template_constraint(ctx, upcast_id, &SLICE_TEMPLATE)?;
                let (progress_expr, progress_subject) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 1, subject_id, 1)?;

                (progress_expr_base || progress_expr, progress_subject)
            },
            _ => {
                // Could be anything, at least attempt to progress subtype
                let progress_expr_base = self.apply_template_constraint(ctx, upcast_id, &ARRAYLIKE_TEMPLATE)?;
                let (progress_expr, progress_subject) =
                    self.apply_equal2_constraint(ctx, upcast_id, upcast_id, 1, subject_id, 1)?;

                (progress_expr_base || progress_expr, progress_subject)
            }
        };

        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject_base || progress_subject, self.debug_get_display_name(ctx, subject_id));
        debug_log!("   - FromIdx type [{}]: {}", progress_idx_base || progress_from, self.debug_get_display_name(ctx, from_id));
        debug_log!("   - ToIdx   type [{}]: {}", progress_idx_base || progress_to, self.debug_get_display_name(ctx, to_id));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }
        if progress_subject_base || progress_subject { self.queue_expr(ctx, subject_id); }
        if progress_idx_base || progress_from { self.queue_expr(ctx, from_id); }
        if progress_idx_base || progress_to { self.queue_expr(ctx, to_id); }

        Ok(())
    }

    fn progress_select_expr(&mut self, ctx: &mut Ctx, id: SelectExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        
        debug_log!("Select expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Subject type: {}", self.debug_get_display_name(ctx, ctx.heap[id].subject));
        debug_log!("   - Expr    type: {}", self.debug_get_display_name(ctx, upcast_id));

        let subject_id = ctx.heap[id].subject;
        let subject_expr_idx = ctx.heap[subject_id].get_unique_id_in_definition();
        let select_expr = &ctx.heap[id];
        let expr_idx = select_expr.unique_id_in_definition;

        let infer_expr = &self.expr_types[expr_idx as usize];
        let extra_idx = infer_expr.extra_data_idx;

        fn determine_inference_type_instance<'a>(types: &'a TypeTable, infer_type: &InferenceType) -> Result<Option<&'a DefinedType>, ()> {
            for part in &infer_type.parts {
                if part.is_marker() || !part.is_concrete() {
                    continue;
                }

                // Part is concrete, check if it is an instance of something
                if let InferenceTypePart::Instance(definition_id, _num_sub) = part {
                    // Lookup type definition and ensure the specified field 
                    // name exists on the struct
                    let definition = types.get_base_definition(definition_id);
                    debug_assert!(definition.is_some());
                    let definition = definition.unwrap();

                    return Ok(Some(definition))
                } else {
                    // Expected an instance of something
                    return Err(())
                }
            }

            // Nothing is concrete yet
            Ok(None)
        }

        if infer_expr.field_or_monomorph_idx < 0 {
            // We don't know the field or the definition it is pointing to yet
            // Not yet known, check if we can determine it
            let subject_type = &self.expr_types[subject_expr_idx as usize].expr_type;
            let type_def = determine_inference_type_instance(&ctx.types, subject_type);

            match type_def {
                Ok(Some(type_def)) => {
                    // Subject type is known, check if it is a
                    // struct and the field exists on the struct
                    let struct_def = if let DefinedTypeVariant::Struct(struct_def) = &type_def.definition {
                        struct_def
                    } else {
                        return Err(ParseError::new_error_at_span(
                            &ctx.module().source, select_expr.field_name.span, format!(
                                "Can only apply field access to structs, got a subject of type '{}'",
                                subject_type.display_name(&ctx.heap)
                            )
                        ));
                    };

                    let mut struct_def_id = None;

                    for (field_def_idx, field_def) in struct_def.fields.iter().enumerate() {
                        if field_def.identifier == select_expr.field_name {
                            // Set field definition and index
                            let infer_expr = &mut self.expr_types[expr_idx as usize];
                            infer_expr.field_or_monomorph_idx = field_def_idx as i32;
                            struct_def_id = Some(type_def.ast_definition);
                            break;
                        }
                    }

                    if struct_def_id.is_none() {
                        let ast_struct_def = ctx.heap[type_def.ast_definition].as_struct();
                        return Err(ParseError::new_error_at_span(
                            &ctx.module().source, select_expr.field_name.span, format!(
                                "this field does not exist on the struct '{}'",
                                ast_struct_def.identifier.value.as_str()
                            )
                        ))
                    }

                    // Encountered definition and field index for the
                    // first time
                    self.insert_initial_select_polymorph_data(ctx, id, struct_def_id.unwrap());
                },
                Ok(None) => {
                    // Type of subject is not yet known, so we
                    // cannot make any progress yet
                    return Ok(())
                },
                Err(()) => {
                    return Err(ParseError::new_error_at_span(
                        &ctx.module().source, select_expr.field_name.span, format!(
                            "Can only apply field access to structs, got a subject of type '{}'",
                            subject_type.display_name(&ctx.heap)
                        )
                    ));
                }
            }
        }

        // If here then field index is known, and the referenced struct type
        // information is inserted into `extra_data`. Check to see if we can
        // do some mutual inference.
        let poly_data = &mut self.extra_data[extra_idx as usize];
        let mut poly_progress = HashSet::new();

        // Apply to struct's type
        let signature_type: *mut _ = &mut poly_data.embedded[0];
        let subject_type: *mut _ = &mut self.expr_types[subject_expr_idx as usize].expr_type;

        let (_, progress_subject) = Self::apply_equal2_signature_constraint(
            ctx, upcast_id, Some(subject_id), poly_data, &mut poly_progress,
            signature_type, 0, subject_type, 0
        )?;

        if progress_subject {
            self.expr_queued.push_back(subject_expr_idx);
        }

        // Apply to field's type
        let signature_type: *mut _ = &mut poly_data.returned;
        let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;

        let (_, progress_expr) = Self::apply_equal2_signature_constraint(
            ctx, upcast_id, None, poly_data, &mut poly_progress,
            signature_type, 0, expr_type, 0
        )?;

        if progress_expr {
            if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                let parent_idx = ctx.heap[parent_id].get_unique_id_in_definition();
                self.expr_queued.push_back(parent_idx);
            }
        }

        // Reapply progress in polymorphic variables to struct's type
        let signature_type: *mut _ = &mut poly_data.embedded[0];
        let subject_type: *mut _ = &mut self.expr_types[subject_expr_idx as usize].expr_type;

        let progress_subject = Self::apply_equal2_polyvar_constraint(
            poly_data, &poly_progress, signature_type, subject_type
        );

        let signature_type: *mut _ = &mut poly_data.returned;
        let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;

        let progress_expr = Self::apply_equal2_polyvar_constraint(
            poly_data, &poly_progress, signature_type, expr_type
        );

        if progress_subject { self.queue_expr(ctx, subject_id); }
        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }

        debug_log!(" * After:");
        debug_log!("   - Subject type [{}]: {}", progress_subject, self.debug_get_display_name(ctx, subject_id));
        debug_log!("   - Expr    type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

        Ok(())
    }

    fn progress_literal_expr(&mut self, ctx: &mut Ctx, id: LiteralExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let expr_idx = expr.unique_id_in_definition;
        let extra_idx = self.expr_types[expr_idx as usize].extra_data_idx;

        debug_log!("Literal expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        let progress_expr = match &expr.value {
            Literal::Null => {
                self.apply_template_constraint(ctx, upcast_id, &MESSAGE_TEMPLATE)?
            },
            Literal::Integer(_) => {
                self.apply_template_constraint(ctx, upcast_id, &INTEGERLIKE_TEMPLATE)?
            },
            Literal::True | Literal::False => {
                self.apply_forced_constraint(ctx, upcast_id, &BOOL_TEMPLATE)?
            },
            Literal::Character(_) => {
                self.apply_forced_constraint(ctx, upcast_id, &CHARACTER_TEMPLATE)?
            },
            Literal::String(_) => {
                self.apply_forced_constraint(ctx, upcast_id, &STRING_TEMPLATE)?
            },
            Literal::Struct(data) => {
                let extra = &mut self.extra_data[extra_idx as usize];
                for _poly in &extra.poly_vars {
                    debug_log!(" * Poly: {}", _poly.display_name(&ctx.heap));
                }
                let mut poly_progress = HashSet::new();
                debug_assert_eq!(extra.embedded.len(), data.fields.len());

                debug_log!(" * During (inferring types from fields and struct type):");

                // Mutually infer field signature/expression types
                for (field_idx, field) in data.fields.iter().enumerate() {
                    let field_expr_id = field.value;
                    let field_expr_idx = ctx.heap[field_expr_id].get_unique_id_in_definition();
                    let signature_type: *mut _ = &mut extra.embedded[field_idx];
                    let field_type: *mut _ = &mut self.expr_types[field_expr_idx as usize].expr_type;
                    let (_, progress_arg) = Self::apply_equal2_signature_constraint(
                        ctx, upcast_id, Some(field_expr_id), extra, &mut poly_progress,
                        signature_type, 0, field_type, 0
                    )?;

                    debug_log!(
                        "   - Field {} type | sig: {}, field: {}", field_idx,
                        unsafe{&*signature_type}.display_name(&ctx.heap),
                        unsafe{&*field_type}.display_name(&ctx.heap)
                    );

                    if progress_arg {
                        self.expr_queued.push_back(field_expr_idx);
                    }
                }

                debug_log!("   - Field poly progress | {:?}", poly_progress);

                // Same for the type of the struct itself
                let signature_type: *mut _ = &mut extra.returned;
                let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;
                let (_, progress_expr) = Self::apply_equal2_signature_constraint(
                    ctx, upcast_id, None, extra, &mut poly_progress,
                    signature_type, 0, expr_type, 0
                )?;

                debug_log!(
                    "   - Ret type | sig: {}, expr: {}",
                    unsafe{&*signature_type}.display_name(&ctx.heap),
                    unsafe{&*expr_type}.display_name(&ctx.heap)
                );
                debug_log!("   - Ret poly progress | {:?}", poly_progress);

                if progress_expr {
                    // TODO: @cleanup, cannot call utility self.queue_parent thingo
                    if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                        let parent_idx = ctx.heap[parent_id].get_unique_id_in_definition();
                        self.expr_queued.push_back(parent_idx);
                    }
                }

                // Check which expressions use the polymorphic arguments. If the
                // polymorphic variables have been progressed then we try to 
                // progress them inside the expression as well.
                debug_log!(" * During (reinferring from progressed polyvars):");

                // For all field expressions
                for field_idx in 0..extra.embedded.len() {
                    // Note: fields in extra.embedded are in the same order as
                    // they are specified in the literal. Whereas
                    // `data.fields[...].field_idx` points to the field in the
                    // struct definition.
                    let signature_type: *mut _ = &mut extra.embedded[field_idx];
                    let field_expr_id = data.fields[field_idx].value;
                    let field_expr_idx = ctx.heap[field_expr_id].get_unique_id_in_definition();
                    let field_type: *mut _ = &mut self.expr_types[field_expr_idx as usize].expr_type;

                    let progress_arg = Self::apply_equal2_polyvar_constraint(
                        extra, &poly_progress, signature_type, field_type
                    );

                    debug_log!(
                        "   - Field {} type | sig: {}, field: {}", field_idx,
                        unsafe{&*signature_type}.display_name(&ctx.heap),
                        unsafe{&*field_type}.display_name(&ctx.heap)
                    );
                    if progress_arg {
                        self.expr_queued.push_back(field_expr_idx);
                    }
                }
                
                // For the return type
                let signature_type: *mut _ = &mut extra.returned;
                let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;

                let progress_expr = Self::apply_equal2_polyvar_constraint(
                    extra, &poly_progress, signature_type, expr_type
                );

                progress_expr
            },
            Literal::Enum(_) => {
                let extra = &mut self.extra_data[extra_idx as usize];
                for _poly in &extra.poly_vars {
                    debug_log!(" * Poly: {}", _poly.display_name(&ctx.heap));
                }
                let mut poly_progress = HashSet::new();
                
                debug_log!(" * During (inferring types from return type)");

                let signature_type: *mut _ = &mut extra.returned;
                let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;
                let (_, progress_expr) = Self::apply_equal2_signature_constraint(
                    ctx, upcast_id, None, extra, &mut poly_progress,
                    signature_type, 0, expr_type, 0
                )?;

                debug_log!(
                    "   - Ret type | sig: {}, expr: {}",
                    unsafe{&*signature_type}.display_name(&ctx.heap),
                    unsafe{&*expr_type}.display_name(&ctx.heap)
                );

                if progress_expr {
                    // TODO: @cleanup
                    if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                        let parent_idx = ctx.heap[parent_id].get_unique_id_in_definition();
                        self.expr_queued.push_back(parent_idx);
                    }
                }

                debug_log!(" * During (reinferring from progress polyvars):");
                let progress_expr = Self::apply_equal2_polyvar_constraint(
                    extra, &poly_progress, signature_type, expr_type
                );

                progress_expr
            },
            Literal::Union(data) => {
                let extra = &mut self.extra_data[extra_idx as usize];
                for _poly in &extra.poly_vars {
                    debug_log!(" * Poly: {}", _poly.display_name(&ctx.heap));
                }
                let mut poly_progress = HashSet::new();
                debug_assert_eq!(extra.embedded.len(), data.values.len());

                debug_log!(" * During (inferring types from variant values and union type):");

                // Mutually infer union variant values
                for (value_idx, value_expr_id) in data.values.iter().enumerate() {
                    let value_expr_id = *value_expr_id;
                    let value_expr_idx = ctx.heap[value_expr_id].get_unique_id_in_definition();
                    let signature_type: *mut _ = &mut extra.embedded[value_idx];
                    let value_type: *mut _ = &mut self.expr_types[value_expr_idx as usize].expr_type;
                    let (_, progress_arg) = Self::apply_equal2_signature_constraint(
                        ctx, upcast_id, Some(value_expr_id), extra, &mut poly_progress,
                        signature_type, 0, value_type, 0 
                    )?;

                    debug_log!(
                        "   - Value {} type | sig: {}, field: {}", value_idx,
                        unsafe{&*signature_type}.display_name(&ctx.heap),
                        unsafe{&*value_type}.display_name(&ctx.heap)
                    );

                    if progress_arg {
                        self.expr_queued.push_back(value_expr_idx);
                    }
                }

                debug_log!("   - Field poly progress | {:?}", poly_progress);

                // Infer type of union itself
                let signature_type: *mut _ = &mut extra.returned;
                let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;
                let (_, progress_expr) = Self::apply_equal2_signature_constraint(
                    ctx, upcast_id, None, extra, &mut poly_progress,
                    signature_type, 0, expr_type, 0
                )?;

                debug_log!(
                    "   - Ret type | sig: {}, expr: {}",
                    unsafe{&*signature_type}.display_name(&ctx.heap),
                    unsafe{&*expr_type}.display_name(&ctx.heap)
                );
                debug_log!("   - Ret poly progress | {:?}", poly_progress);

                if progress_expr {
                    // TODO: @cleanup, borrowing rules
                    if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                        let parent_idx = ctx.heap[parent_id].get_unique_id_in_definition();
                        self.expr_queued.push_back(parent_idx);
                    }
                }

                debug_log!(" * During (reinferring from progress polyvars):");
            
                // For all embedded values of the union variant
                for value_idx in 0..extra.embedded.len() {
                    let signature_type: *mut _ = &mut extra.embedded[value_idx];
                    let value_expr_id = data.values[value_idx];
                    let value_expr_idx = ctx.heap[value_expr_id].get_unique_id_in_definition();
                    let value_type: *mut _ = &mut self.expr_types[value_expr_idx as usize].expr_type;
                    
                    let progress_arg = Self::apply_equal2_polyvar_constraint(
                        extra, &poly_progress, signature_type, value_type
                    );

                    debug_log!(
                        "   - Value {} type | sig: {}, value: {}", value_idx,
                        unsafe{&*signature_type}.display_name(&ctx.heap),
                        unsafe{&*value_type}.display_name(&ctx.heap)
                    );
                    if progress_arg {
                        self.expr_queued.push_back(value_expr_idx);
                    }
                }

                // And for the union type itself
                let signature_type: *mut _ = &mut extra.returned;
                let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;

                let progress_expr = Self::apply_equal2_polyvar_constraint(
                    extra, &poly_progress, signature_type, expr_type
                );

                progress_expr
            },
            Literal::Array(data) => {
                let expr_elements = data.clone(); // TODO: @performance
                debug_log!("Array expr ({} elements): {}", expr_elements.len(), upcast_id.index);
                debug_log!(" * Before:");
                debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

                // All elements should have an equal type
                let progress = self.apply_equal_n_constraint(ctx, upcast_id, &expr_elements)?;
                for (progress_arg, arg_id) in progress.iter().zip(expr_elements.iter()) {
                    if *progress_arg {
                        self.queue_expr(ctx, *arg_id);
                    }
                }

                // And the output should be an array of the element types
                let mut progress_expr = self.apply_template_constraint(ctx, upcast_id, &ARRAY_TEMPLATE)?;
                if !expr_elements.is_empty() {
                    let first_arg_id = expr_elements[0];
                    let (inner_expr_progress, arg_progress) = self.apply_equal2_constraint(
                        ctx, upcast_id, upcast_id, 1, first_arg_id, 0
                    )?;

                    progress_expr = progress_expr || inner_expr_progress;

                    // Note that if the array type progressed the type of the arguments,
                    // then we should enqueue this progression function again
                    // TODO: @fix Make apply_equal_n accept a start idx as well
                    if arg_progress { self.queue_expr(ctx, upcast_id); }
                }

                debug_log!(" * After:");
                debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));

                progress_expr
            },
        };

        debug_log!(" * After:");
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        if progress_expr { self.queue_expr_parent(ctx, upcast_id); }

        Ok(())
    }

    fn progress_cast_expr(&mut self, ctx: &mut Ctx, id: CastExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let expr_idx = expr.unique_id_in_definition;

        debug_log!("Casting expr: {}", upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Expr type:    {}", self.debug_get_display_name(ctx, upcast_id));
        debug_log!("   - Subject type: {}", self.debug_get_display_name(ctx, expr.subject));

        // The cast expression might have its output type fixed by the
        // programmer, so apply that type to the output. Apart from that casting
        // acts like a blocker for two-way inference. So we'll just have to wait
        // until we know if the cast is valid.
        // TODO: Another thing that has to be updated the moment the type
        //  inferencer is fully index/job-based
        let infer_type = self.determine_inference_type_from_parser_type_elements(&expr.to_type.elements, true);
        let expr_progress = self.apply_template_constraint(ctx, upcast_id, &infer_type.parts)?;

        if expr_progress {
            self.queue_expr_parent(ctx, upcast_id);
        }

        // Check if the two types are compatible
        debug_log!(" * After:");
        debug_log!("   - Expr type [{}]: {}", expr_progress, self.debug_get_display_name(ctx, upcast_id));
        debug_log!("   - Note that the subject type can never be inferred");
        debug_log!(" * Decision:");

        let subject_idx = ctx.heap[expr.subject].get_unique_id_in_definition();
        let expr_type = &self.expr_types[expr_idx as usize].expr_type;
        let subject_type = &self.expr_types[subject_idx as usize].expr_type;
        if !expr_type.is_done || !subject_type.is_done {
            // Not yet done
            debug_log!("   - Casting is valid: unknown as the types are not yet complete");
            return Ok(())
        }

        // Valid casts: (bool, integer, character) can always be cast to one
        // another. A cast from a type to itself is also valid.
        fn is_bool_int_or_char(parts: &[InferenceTypePart]) -> bool {
            return parts.len() == 1 && (
                parts[0] == InferenceTypePart::Bool ||
                parts[0] == InferenceTypePart::Character ||
                parts[0].is_concrete_integer()
            );
        }

        let is_valid = if is_bool_int_or_char(&expr_type.parts) && is_bool_int_or_char(&subject_type.parts) {
            true
        } else if expr_type.parts == subject_type.parts {
            true
        } else {
            false
        };

        debug_log!("   - Casting is valid: {}", is_valid);

        if !is_valid {
            let cast_expr = &ctx.heap[id];
            let subject_expr = &ctx.heap[cast_expr.subject];
            return Err(ParseError::new_error_str_at_span(
                &ctx.module().source, cast_expr.full_span, "invalid casting operation"
            ).with_info_at_span(
                &ctx.module().source, subject_expr.full_span(), format!(
                    "cannot cast the argument type '{}' to the cast type '{}'",
                    subject_type.display_name(&ctx.heap),
                    expr_type.display_name(&ctx.heap)
                )
            ));
        }

        Ok(())
    }

    // TODO: @cleanup, see how this can be cleaned up once I implement
    //  polymorphic struct/enum/union literals. These likely follow the same
    //  pattern as here.
    fn progress_call_expr(&mut self, ctx: &mut Ctx, id: CallExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let expr = &ctx.heap[id];
        let expr_idx = expr.unique_id_in_definition;
        let extra_idx = self.expr_types[expr_idx as usize].extra_data_idx;

        debug_log!("Call expr '{}': {}", ctx.heap[expr.definition].identifier().value.as_str(), upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));
        debug_log!(" * During (inferring types from arguments and return type):");

        let extra = &mut self.extra_data[extra_idx as usize];

        // Check if we can make progress using the arguments and/or return types
        // while keeping track of the polyvars we've extended
        let mut poly_progress = HashSet::new();
        debug_assert_eq!(extra.embedded.len(), expr.arguments.len());

        for (call_arg_idx, arg_id) in expr.arguments.clone().into_iter().enumerate() {
            let arg_expr_idx = ctx.heap[arg_id].get_unique_id_in_definition();
            let signature_type: *mut _ = &mut extra.embedded[call_arg_idx];
            let argument_type: *mut _ = &mut self.expr_types[arg_expr_idx as usize].expr_type;
            let (_, progress_arg) = Self::apply_equal2_signature_constraint(
                ctx, upcast_id, Some(arg_id), extra, &mut poly_progress,
                signature_type, 0, argument_type, 0
            )?;

            debug_log!(
                "   - Arg {} type | sig: {}, arg: {}", call_arg_idx,
                unsafe{&*signature_type}.display_name(&ctx.heap), 
                unsafe{&*argument_type}.display_name(&ctx.heap));

            if progress_arg {
                // Progressed argument expression
                self.expr_queued.push_back(arg_expr_idx);
            }
        }

        // Do the same for the return type
        let signature_type: *mut _ = &mut extra.returned;
        let expr_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;
        let (_, progress_expr) = Self::apply_equal2_signature_constraint(
            ctx, upcast_id, None, extra, &mut poly_progress,
            signature_type, 0, expr_type, 0
        )?;

        debug_log!(
            "   - Ret type | sig: {}, expr: {}", 
            unsafe{&*signature_type}.display_name(&ctx.heap), 
            unsafe{&*expr_type}.display_name(&ctx.heap)
        );

        if progress_expr {
            // TODO: @cleanup, cannot call utility self.queue_parent thingo
            if let Some(parent_id) = ctx.heap[upcast_id].parent_expr_id() {
                let parent_idx = ctx.heap[parent_id].get_unique_id_in_definition();
                self.expr_queued.push_back(parent_idx);
            }
        }

        // If we did not have an error in the polymorph inference above, then
        // reapplying the polymorph type to each argument type and the return
        // type should always succeed.
        debug_log!(" * During (reinferring from progressed polyvars):");
        for (_poly_idx, _poly_var) in extra.poly_vars.iter().enumerate() {
            debug_log!("   - Poly {} | sig: {}", _poly_idx, _poly_var.display_name(&ctx.heap));
        }
        // TODO: @performance If the algorithm is changed to be more "on demand
        //  argument re-evaluation", instead of "all-argument re-evaluation",
        //  then this is no longer true
        for arg_idx in 0..extra.embedded.len() {
            let signature_type: *mut _ = &mut extra.embedded[arg_idx];
            let arg_expr_id = expr.arguments[arg_idx];
            let arg_expr_idx = ctx.heap[arg_expr_id].get_unique_id_in_definition();
            let arg_type: *mut _ = &mut self.expr_types[arg_expr_idx as usize].expr_type;
            
            let progress_arg = Self::apply_equal2_polyvar_constraint(
                extra, &poly_progress,
                signature_type, arg_type
            );
            
            debug_log!(
                "   - Arg {} type | sig: {}, arg: {}", arg_idx, 
                unsafe{&*signature_type}.display_name(&ctx.heap), 
                unsafe{&*arg_type}.display_name(&ctx.heap)
            );
            if progress_arg {
                self.expr_queued.push_back(arg_expr_idx);
            }
        }

        // Once more for the return type
        let signature_type: *mut _ = &mut extra.returned;
        let ret_type: *mut _ = &mut self.expr_types[expr_idx as usize].expr_type;

        let progress_ret = Self::apply_equal2_polyvar_constraint(
            extra, &poly_progress, signature_type, ret_type
        );
        debug_log!(
            "   - Ret type | sig: {}, arg: {}", 
            unsafe{&*signature_type}.display_name(&ctx.heap), 
            unsafe{&*ret_type}.display_name(&ctx.heap)
        );
        if progress_ret {
            self.queue_expr_parent(ctx, upcast_id);
        }

        debug_log!(" * After:");
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        Ok(())
    }

    fn progress_variable_expr(&mut self, ctx: &mut Ctx, id: VariableExpressionId) -> Result<(), ParseError> {
        let upcast_id = id.upcast();
        let var_expr = &ctx.heap[id];
        let var_expr_idx = var_expr.unique_id_in_definition;
        let var_id = var_expr.declaration.unwrap();

        debug_log!("Variable expr '{}': {}", ctx.heap[var_id].identifier.value.as_str(), upcast_id.index);
        debug_log!(" * Before:");
        debug_log!("   - Var  type: {}", self.var_types.get(&var_id).unwrap().var_type.display_name(&ctx.heap));
        debug_log!("   - Expr type: {}", self.debug_get_display_name(ctx, upcast_id));

        // Retrieve shared variable type and expression type and apply inference
        let var_data = self.var_types.get_mut(&var_id).unwrap();
        let expr_type = &mut self.expr_types[var_expr_idx as usize].expr_type;

        let infer_res = unsafe{ InferenceType::infer_subtrees_for_both_types(
            &mut var_data.var_type as *mut _, 0, expr_type, 0
        ) };
        if infer_res == DualInferenceResult::Incompatible {
            let var_decl = &ctx.heap[var_id];
            return Err(ParseError::new_error_at_span(
                &ctx.module().source, var_decl.identifier.span, format!(
                    "Conflicting types for this variable, previously assigned the type '{}'",
                    var_data.var_type.display_name(&ctx.heap)
                )
            ).with_info_at_span(
                &ctx.module().source, var_expr.identifier.span, format!(
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
                    let other_expr_idx = ctx.heap[*other_expr].get_unique_id_in_definition();
                    self.expr_queued.push_back(other_expr_idx);
                }
            }

            // Let a linked port know that our type has updated
            if let Some(linked_id) = var_data.linked_var {
                // Only perform one-way inference to prevent updating our type,
                // this would lead to an inconsistency in the type inference
                // algorithm otherwise.
                let var_type: *mut _ = &mut var_data.var_type;
                let link_data = self.var_types.get_mut(&linked_id).unwrap();

                debug_assert!(
                    unsafe{&*var_type}.parts[0] == InferenceTypePart::Input ||
                    unsafe{&*var_type}.parts[0] == InferenceTypePart::Output
                );
                debug_assert!(
                    link_data.var_type.parts[0] == InferenceTypePart::Input ||
                    link_data.var_type.parts[0] == InferenceTypePart::Output
                );
                match InferenceType::infer_subtree_for_single_type(&mut link_data.var_type, 1, &unsafe{&*var_type}.parts, 1, false) {
                    SingleInferenceResult::Modified => {
                        for other_expr in &link_data.used_at {
                            let other_expr_idx = ctx.heap[*other_expr].get_unique_id_in_definition();
                            self.expr_queued.push_back(other_expr_idx);
                        }
                    },
                    SingleInferenceResult::Unmodified => {},
                    SingleInferenceResult::Incompatible => {
                        let var_data = self.var_types.get(&var_id).unwrap();
                        let link_data = self.var_types.get(&linked_id).unwrap();
                        let var_decl = &ctx.heap[var_id];
                        let link_decl = &ctx.heap[linked_id];

                        return Err(ParseError::new_error_at_span(
                            &ctx.module().source, var_decl.identifier.span, format!(
                                "Conflicting types for this variable, assigned the type '{}'",
                                var_data.var_type.display_name(&ctx.heap)
                            )
                        ).with_info_at_span(
                            &ctx.module().source, link_decl.identifier.span, format!(
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
        debug_log!("   - Expr type [{}]: {}", progress_expr, self.debug_get_display_name(ctx, upcast_id));


        Ok(())
    }

    fn queue_expr_parent(&mut self, ctx: &Ctx, expr_id: ExpressionId) {
        if let ExpressionParent::Expression(parent_expr_id, _) = &ctx.heap[expr_id].parent() {
            let expr_idx = ctx.heap[*parent_expr_id].get_unique_id_in_definition();
            self.expr_queued.push_back(expr_idx);
        }
    }

    fn queue_expr(&mut self, ctx: &Ctx, expr_id: ExpressionId) {
        let expr_idx = ctx.heap[expr_id].get_unique_id_in_definition();
        self.expr_queued.push_back(expr_idx);
    }


    // first returned is certainly string, second is certainly not
    fn type_is_certainly_or_certainly_not_string(&self, ctx: &Ctx, expr_id: ExpressionId) -> (bool, bool) {
        let expr_idx = ctx.heap[expr_id].get_unique_id_in_definition();
        let expr_type = &self.expr_types[expr_idx as usize].expr_type;
        if expr_type.is_done {
            if expr_type.parts[0] == InferenceTypePart::String {
                return (true, false);
            } else {
                return (false, true);
            }
        }

        (false, false)
    }

    /// Applies a template type constraint: the type associated with the
    /// supplied expression will be molded into the provided `template`. But
    /// will be considered valid if the template could've been molded into the
    /// expression type as well. Hence the template may be fully specified (e.g.
    /// a bool) or contain "inference" variables (e.g. an array of T)
    fn apply_template_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId, template: &[InferenceTypePart]
    ) -> Result<bool, ParseError> {
        let expr_idx = ctx.heap[expr_id].get_unique_id_in_definition(); // TODO: @Temp
        let expr_type = &mut self.expr_types[expr_idx as usize].expr_type;
        match InferenceType::infer_subtree_for_single_type(expr_type, 0, template, 0, false) {
            SingleInferenceResult::Modified => Ok(true),
            SingleInferenceResult::Unmodified => Ok(false),
            SingleInferenceResult::Incompatible => Err(
                self.construct_template_type_error(ctx, expr_id, template)
            )
        }
    }

    fn apply_template_constraint_to_types(
        to_infer: *mut InferenceType, to_infer_start_idx: usize,
        template: &[InferenceTypePart], template_start_idx: usize
    ) -> Result<bool, ()> {
        match InferenceType::infer_subtree_for_single_type(
            unsafe{ &mut *to_infer }, to_infer_start_idx,
            template, template_start_idx, false
        ) {
            SingleInferenceResult::Modified => Ok(true),
            SingleInferenceResult::Unmodified => Ok(false),
            SingleInferenceResult::Incompatible => Err(()),
        }
    }

    /// Applies a forced constraint: the supplied expression's type MUST be
    /// inferred from the template, the other way around is considered invalid.
    fn apply_forced_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId, template: &[InferenceTypePart]
    ) -> Result<bool, ParseError> {
        let expr_idx = ctx.heap[expr_id].get_unique_id_in_definition();
        let expr_type = &mut self.expr_types[expr_idx as usize].expr_type;
        match InferenceType::infer_subtree_for_single_type(expr_type, 0, template, 0, true) {
            SingleInferenceResult::Modified => Ok(true),
            SingleInferenceResult::Unmodified => Ok(false),
            SingleInferenceResult::Incompatible => Err(
                self.construct_template_type_error(ctx, expr_id, template)
            )
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
    ) -> Result<(bool, bool), ParseError> {
        let arg1_expr_idx = ctx.heap[arg1_id].get_unique_id_in_definition(); // TODO: @Temp
        let arg2_expr_idx = ctx.heap[arg2_id].get_unique_id_in_definition();
        let arg1_type: *mut _ = &mut self.expr_types[arg1_expr_idx as usize].expr_type;
        let arg2_type: *mut _ = &mut self.expr_types[arg2_expr_idx as usize].expr_type;

        let infer_res = unsafe{ InferenceType::infer_subtrees_for_both_types(
            arg1_type, arg1_start_idx,
            arg2_type, arg2_start_idx
        ) };
        if infer_res == DualInferenceResult::Incompatible {
            return Err(self.construct_arg_type_error(ctx, expr_id, arg1_id, arg2_id));
        }

        Ok((infer_res.modified_lhs(), infer_res.modified_rhs()))
    }

    /// Applies an equal2 constraint between a signature type (e.g. a function
    /// argument or struct field) and an expression whose type should match that
    /// expression. If we make progress on the signature, then we try to see if
    /// any of the embedded polymorphic types can be progressed.
    ///
    /// `outer_expr_id` is the main expression we're progressing (e.g. a 
    /// function call), while `expr_id` is the embedded expression we're 
    /// matching against the signature. `expression_type` and 
    /// `expression_start_idx` belong to `expr_id`.
    fn apply_equal2_signature_constraint(
        ctx: &Ctx, outer_expr_id: ExpressionId, expr_id: Option<ExpressionId>,
        polymorph_data: &mut ExtraData, polymorph_progress: &mut HashSet<u32>,
        signature_type: *mut InferenceType, signature_start_idx: usize,
        expression_type: *mut InferenceType, expression_start_idx: usize
    ) -> Result<(bool, bool), ParseError> {
        // Safety: all pointers distinct

        // Infer the signature and expression type
        let infer_res = unsafe { 
            InferenceType::infer_subtrees_for_both_types(
                signature_type, signature_start_idx,
                expression_type, expression_start_idx
            ) 
        };

        if infer_res == DualInferenceResult::Incompatible {
            // TODO: Check if I still need to use this
            let outer_span = ctx.heap[outer_expr_id].full_span();
            let (span_name, span) = match expr_id {
                Some(expr_id) => ("argument's", ctx.heap[expr_id].full_span()),
                None => ("type's", outer_span)
            };
            let (signature_display_type, expression_display_type) = unsafe { (
                (&*signature_type).display_name(&ctx.heap),
                (&*expression_type).display_name(&ctx.heap)
            ) };

            return Err(ParseError::new_error_str_at_span(
                &ctx.module().source, outer_span,
                "failed to fully resolve the types of this expression"
            ).with_info_at_span(
                &ctx.module().source, span, format!(
                    "because the {} signature has been resolved to '{}', but the expression has been resolved to '{}'",
                    span_name, signature_display_type, expression_display_type
                )
            ));
        }

        // Try to see if we can progress any of the polymorphic variables
        let progress_sig = infer_res.modified_lhs();
        let progress_expr = infer_res.modified_rhs();

        if progress_sig {
            let signature_type = unsafe{&mut *signature_type};
            debug_assert!(
                signature_type.has_marker,
                "made progress on signature type, but it doesn't have a marker"
            );
            for (poly_idx, poly_section) in signature_type.marker_iter() {
                let polymorph_type = &mut polymorph_data.poly_vars[poly_idx as usize];
                match Self::apply_template_constraint_to_types(
                    polymorph_type, 0, poly_section, 0
                ) {
                    Ok(true) => { polymorph_progress.insert(poly_idx); },
                    Ok(false) => {},
                    Err(()) => { return Err(Self::construct_poly_arg_error(ctx, polymorph_data, outer_expr_id))}
                }
            }
        }
        Ok((progress_sig, progress_expr))
    }

    /// Applies equal2 constraints on the signature type for each of the 
    /// polymorphic variables. If the signature type is progressed then we 
    /// progress the expression type as well.
    ///
    /// This function assumes that the polymorphic variables have already been
    /// progressed as far as possible by calling 
    /// `apply_equal2_signature_constraint`. As such, we expect to not encounter
    /// any errors.
    ///
    /// This function returns true if the expression's type has been progressed
    fn apply_equal2_polyvar_constraint(
        polymorph_data: &ExtraData, _polymorph_progress: &HashSet<u32>,
        signature_type: *mut InferenceType, expr_type: *mut InferenceType
    ) -> bool {
        // Safety: all pointers should be distinct
        //         polymorph_data containers may not be modified
        let signature_type = unsafe{&mut *signature_type};
        let expr_type = unsafe{&mut *expr_type};

        // Iterate through markers in signature type to try and make progress
        // on the polymorphic variable        
        let mut seek_idx = 0;
        let mut modified_sig = false;
        
        while let Some((poly_idx, start_idx)) = signature_type.find_marker(seek_idx) {
            let end_idx = InferenceType::find_subtree_end_idx(&signature_type.parts, start_idx);
            // if polymorph_progress.contains(&poly_idx) {
                // Need to match subtrees
                let polymorph_type = &polymorph_data.poly_vars[poly_idx as usize];
                let modified_at_marker = Self::apply_template_constraint_to_types(
                    signature_type, start_idx, 
                    &polymorph_type.parts, 0
                ).expect("no failure when applying polyvar constraints");

                modified_sig = modified_sig || modified_at_marker;
            // }

            seek_idx = end_idx;
        }

        // If we made any progress on the signature's type, then we also need to
        // apply it to the expression that is supposed to match the signature.
        if modified_sig {
            match InferenceType::infer_subtree_for_single_type(
                expr_type, 0, &signature_type.parts, 0, true
            ) {
                SingleInferenceResult::Modified => true,
                SingleInferenceResult::Unmodified => false,
                SingleInferenceResult::Incompatible =>
                    unreachable!("encountered failure while reapplying modified signature to expression after polyvar inference")
            }
        } else {
            false
        }
    }

    /// Applies a type constraint that expects all three provided types to be
    /// equal. In case we can make progress in inferring the types then we
    /// attempt to do so. If the call is successful then the composition of all
    /// types is made equal.
    fn apply_equal3_constraint(
        &mut self, ctx: &Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg2_id: ExpressionId,
        start_idx: usize
    ) -> Result<(bool, bool, bool), ParseError> {
        // Safety: all points are unique
        //         containers may not be modified
        let expr_expr_idx = ctx.heap[expr_id].get_unique_id_in_definition(); // TODO: @Temp
        let arg1_expr_idx = ctx.heap[arg1_id].get_unique_id_in_definition();
        let arg2_expr_idx = ctx.heap[arg2_id].get_unique_id_in_definition();

        let expr_type: *mut _ = &mut self.expr_types[expr_expr_idx as usize].expr_type;
        let arg1_type: *mut _ = &mut self.expr_types[arg1_expr_idx as usize].expr_type;
        let arg2_type: *mut _ = &mut self.expr_types[arg2_expr_idx as usize].expr_type;

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
    ) -> Result<Vec<bool>, ParseError> {
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
            let last_expr_idx = ctx.heap[last_arg_id].get_unique_id_in_definition(); // TODO: @Temp
            let next_expr_idx = ctx.heap[*next_arg_id].get_unique_id_in_definition();
            let last_type: *mut _ = &mut self.expr_types[last_expr_idx as usize].expr_type;
            let next_type: *mut _ = &mut self.expr_types[next_expr_idx as usize].expr_type;

            let res = unsafe {
                InferenceType::infer_subtrees_for_both_types(last_type, 0, next_type, 0)
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
        let last_arg_expr_idx = ctx.heap[*args.last().unwrap()].get_unique_id_in_definition();
        let last_type: *mut _ = &mut self.expr_types[last_arg_expr_idx as usize].expr_type;
        for arg_idx in 0..last_lhs_progressed {
            let other_arg_expr_idx = ctx.heap[args[arg_idx]].get_unique_id_in_definition();
            let arg_type: *mut _ = &mut self.expr_types[other_arg_expr_idx as usize].expr_type;
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
    fn insert_initial_expr_inference_type(
        &mut self, ctx: &mut Ctx, expr_id: ExpressionId
    ) -> Result<(), ParseError> {
        use ExpressionParent as EP;
        use InferenceTypePart as ITP;

        let expr = &ctx.heap[expr_id];
        let inference_type = match expr.parent() {
            EP::None =>
                // Should have been set by linker
                unreachable!(),
            EP::ExpressionStmt(_) =>
                // Determined during type inference
                InferenceType::new(false, false, vec![ITP::Unknown]),
            EP::Expression(parent_id, idx_in_parent) => {
                // If we are the test expression of a conditional expression,
                // then we must resolve to a boolean
                let is_conditional = if let Expression::Conditional(_) = &ctx.heap[*parent_id] {
                    true
                } else {
                    false
                };

                if is_conditional && *idx_in_parent == 0 {
                    InferenceType::new(false, true, vec![ITP::Bool])
                } else {
                    InferenceType::new(false, false, vec![ITP::Unknown])
                }
            },
            EP::If(_) | EP::While(_) =>
                // Must be a boolean
                InferenceType::new(false, true, vec![ITP::Bool]),
            EP::Return(_) =>
                // Must match the return type of the function
                if let DefinitionType::Function(func_id) = self.definition_type {
                    debug_assert_eq!(ctx.heap[func_id].return_types.len(), 1);
                    let returned = &ctx.heap[func_id].return_types[0];
                    self.determine_inference_type_from_parser_type_elements(&returned.elements, true)
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

        let infer_expr = &mut self.expr_types[expr.get_unique_id_in_definition() as usize];
        let needs_extra_data = match expr {
            Expression::Call(_) => true,
            Expression::Literal(expr) => match expr.value {
                Literal::Enum(_) | Literal::Union(_) | Literal::Struct(_) => true,
                _ => false,
            },
            Expression::Select(_) => true,
            _ => false,
        };

        if infer_expr.expr_id.is_invalid() {
            // Nothing is set yet
            infer_expr.expr_type = inference_type;
            infer_expr.expr_id = expr_id;
            if needs_extra_data {
                let extra_idx = self.extra_data.len() as i32;
                self.extra_data.push(ExtraData::default());
                infer_expr.extra_data_idx = extra_idx;
            }
        } else {
            // We already have an entry
            debug_assert!(false, "does this ever happen?");
            if let SingleInferenceResult::Incompatible = InferenceType::infer_subtree_for_single_type(
                &mut infer_expr.expr_type, 0, &inference_type.parts, 0, false
            ) {
                return Err(self.construct_expr_type_error(ctx, expr_id, expr_id));
            }

            debug_assert!((infer_expr.extra_data_idx != -1) == needs_extra_data);
        }

        Ok(())
    }

    fn insert_initial_call_polymorph_data(
        &mut self, ctx: &mut Ctx, call_id: CallExpressionId
    ) {
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
        let extra_data_idx = self.expr_types[call.unique_id_in_definition as usize].extra_data_idx; // TODO: @Temp
        debug_assert!(extra_data_idx != -1, "insert initial call polymorph data, no preallocated ExtraData");

        // Handle the polymorphic arguments (if there are any)
        let num_poly_args = call.parser_type.elements[0].variant.num_embedded();
        let mut poly_args = Vec::with_capacity(num_poly_args);
        for embedded_elements in call.parser_type.iter_embedded(0) {
            poly_args.push(self.determine_inference_type_from_parser_type_elements(embedded_elements, true));
        }

        // Handle the arguments and return types
        let definition = &ctx.heap[call.definition];
        let (parameters, returned) = match definition {
            Definition::Component(definition) => {
                debug_assert_eq!(poly_args.len(), definition.poly_vars.len());
                (&definition.parameters, None)
            },
            Definition::Function(definition) => {
                debug_assert_eq!(poly_args.len(), definition.poly_vars.len());
                (&definition.parameters, Some(&definition.return_types))
            },
            Definition::Struct(_) | Definition::Enum(_) | Definition::Union(_) => {
                unreachable!("insert_initial_call_polymorph data for non-procedure type");
            },
        };

        let mut parameter_types = Vec::with_capacity(parameters.len());
        for parameter_id in parameters.clone().into_iter() { // TODO: @Performance @Now
            let param = &ctx.heap[parameter_id];
            parameter_types.push(self.determine_inference_type_from_parser_type_elements(&param.parser_type.elements, false));
        }

        let return_type = match returned {
            None => {
                // Component, so returns a "Void"
                InferenceType::new(false, true, vec![InferenceTypePart::Void])
            },
            Some(returned) => {
                debug_assert_eq!(returned.len(), 1); // TODO: @ReturnTypes
                let returned = &returned[0];
                self.determine_inference_type_from_parser_type_elements(&returned.elements, false)
            }
        };

        self.extra_data[extra_data_idx as usize] = ExtraData{
            expr_id: call_id.upcast(),
            definition_id: call.definition,
            poly_vars: poly_args,
            embedded: parameter_types,
            returned: return_type
        };
    }

    fn insert_initial_struct_polymorph_data(
        &mut self, ctx: &mut Ctx, lit_id: LiteralExpressionId,
    ) {
        use InferenceTypePart as ITP;
        let literal = &ctx.heap[lit_id];
        let extra_data_idx = self.expr_types[literal.unique_id_in_definition as usize].extra_data_idx; // TODO: @Temp
        debug_assert!(extra_data_idx != -1, "initial struct polymorph data, but no preallocated ExtraData");
        let literal = ctx.heap[lit_id].value.as_struct();

        // Handle polymorphic arguments
        let num_embedded = literal.parser_type.elements[0].variant.num_embedded();
        let mut total_num_poly_parts = 0;
        let mut poly_args = Vec::with_capacity(num_embedded);

        for embedded_elements in literal.parser_type.iter_embedded(0) {
            let poly_type = self.determine_inference_type_from_parser_type_elements(embedded_elements, true);
            total_num_poly_parts += poly_type.parts.len();
            poly_args.push(poly_type);
        }

        // Handle parser types on struct definition
        let defined_type = ctx.types.get_base_definition(&literal.definition).unwrap();
        let struct_type = defined_type.definition.as_struct();
        debug_assert_eq!(poly_args.len(), defined_type.poly_vars.len());

        // Note: programmer is capable of specifying fields in a struct literal
        // in a different order than on the definition. We take the literal-
        // specified order to be leading.
        let mut embedded_types = Vec::with_capacity(struct_type.fields.len());
        for lit_field in literal.fields.iter() {
            let def_field = &struct_type.fields[lit_field.field_idx];
            let inference_type = self.determine_inference_type_from_parser_type_elements(&def_field.parser_type.elements, false);
            embedded_types.push(inference_type);
        }

        // Return type is the struct type itself, with the appropriate 
        // polymorphic variables. So:
        // - 1 part for definition
        // - N_poly_arg marker parts for each polymorphic argument
        // - all the parts for the currently known polymorphic arguments 
        let parts_reserved = 1 + poly_args.len() + total_num_poly_parts;
        let mut parts = Vec::with_capacity(parts_reserved);
        parts.push(ITP::Instance(literal.definition, poly_args.len() as u32));
        let mut return_type_done = true;
        for (poly_var_idx, poly_var) in poly_args.iter().enumerate() {
            if !poly_var.is_done { return_type_done = false; }

            parts.push(ITP::Marker(poly_var_idx as u32));
            parts.extend(poly_var.parts.iter().cloned());
        }

        debug_assert_eq!(parts.len(), parts_reserved);
        let return_type = InferenceType::new(!poly_args.is_empty(), return_type_done, parts);

        self.extra_data[extra_data_idx as usize] = ExtraData{
            expr_id: lit_id.upcast(),
            definition_id: literal.definition,
            poly_vars: poly_args,
            embedded: embedded_types,
            returned: return_type,
        };
    }

    /// Inserts the extra polymorphic data struct for enum expressions. These
    /// can never be determined from the enum itself, but may be inferred from
    /// the use of the enum.
    fn insert_initial_enum_polymorph_data(
        &mut self, ctx: &Ctx, lit_id: LiteralExpressionId
    ) {
        use InferenceTypePart as ITP;
        let literal = &ctx.heap[lit_id];
        let extra_data_idx = self.expr_types[literal.unique_id_in_definition as usize].extra_data_idx; // TODO: @Temp
        debug_assert!(extra_data_idx != -1, "initial enum polymorph data, but no preallocated ExtraData");
        let literal = ctx.heap[lit_id].value.as_enum();

        // Handle polymorphic arguments to the enum
        let num_poly_args = literal.parser_type.elements[0].variant.num_embedded();
        let mut total_num_poly_parts = 0;
        let mut poly_args = Vec::with_capacity(num_poly_args);

        for embedded_elements in literal.parser_type.iter_embedded(0) {
            let poly_type = self.determine_inference_type_from_parser_type_elements(embedded_elements, true);
            total_num_poly_parts += poly_type.parts.len();
            poly_args.push(poly_type);
        }

        // Handle enum type itself
        let parts_reserved = 1 + poly_args.len() + total_num_poly_parts;
        let mut parts = Vec::with_capacity(parts_reserved);
        parts.push(ITP::Instance(literal.definition, poly_args.len() as u32));
        let mut enum_type_done = true;
        for (poly_var_idx, poly_var) in poly_args.iter().enumerate() {
            if !poly_var.is_done { enum_type_done = false; }

            parts.push(ITP::Marker(poly_var_idx as u32));
            parts.extend(poly_var.parts.iter().cloned());
        }

        debug_assert_eq!(parts.len(), parts_reserved);
        let enum_type = InferenceType::new(!poly_args.is_empty(), enum_type_done, parts);

        self.extra_data[extra_data_idx as usize] = ExtraData{
            expr_id: lit_id.upcast(),
            definition_id: literal.definition,
            poly_vars: poly_args,
            embedded: Vec::new(),
            returned: enum_type,
        };
    }

    /// Inserts the extra polymorphic data struct for unions. The polymorphic
    /// arguments may be partially determined from embedded values in the union.
    fn insert_initial_union_polymorph_data(
        &mut self, ctx: &Ctx, lit_id: LiteralExpressionId
    ) {
        use InferenceTypePart as ITP;
        let literal = &ctx.heap[lit_id];
        let extra_data_idx = self.expr_types[literal.unique_id_in_definition as usize].extra_data_idx; // TODO: @Temp
        debug_assert!(extra_data_idx != -1, "initial union polymorph data, but no preallocated ExtraData");
        let literal = ctx.heap[lit_id].value.as_union();

        // Construct the polymorphic variables
        let num_poly_args = literal.parser_type.elements[0].variant.num_embedded();
        let mut total_num_poly_parts = 0;
        let mut poly_args = Vec::with_capacity(num_poly_args);

        for embedded_elements in literal.parser_type.iter_embedded(0) {
            let poly_type = self.determine_inference_type_from_parser_type_elements(embedded_elements, true);
            total_num_poly_parts += poly_type.parts.len();
            poly_args.push(poly_type);
        }

        // Handle any of the embedded values in the variant, if specified
        let definition_id = literal.definition;
        let type_definition = ctx.types.get_base_definition(&definition_id).unwrap();
        let union_definition = type_definition.definition.as_union();
        debug_assert_eq!(poly_args.len(), type_definition.poly_vars.len());

        let variant_definition = &union_definition.variants[literal.variant_idx];
        debug_assert_eq!(variant_definition.embedded.len(), literal.values.len());

        let mut embedded = Vec::with_capacity(variant_definition.embedded.len());
        for embedded_parser_type in &variant_definition.embedded {
            let inference_type = self.determine_inference_type_from_parser_type_elements(&embedded_parser_type.elements, false);
            embedded.push(inference_type);
        }

        // Handle the type of the union itself
        let parts_reserved = 1 + poly_args.len() + total_num_poly_parts;
        let mut parts = Vec::with_capacity(parts_reserved);
        parts.push(ITP::Instance(definition_id, poly_args.len() as u32));
        let mut union_type_done = true;
        for (poly_var_idx, poly_var) in poly_args.iter().enumerate() {
            if !poly_var.is_done { union_type_done = false; }

            parts.push(ITP::Marker(poly_var_idx as u32));
            parts.extend(poly_var.parts.iter().cloned());
        }

        debug_assert_eq!(parts_reserved, parts.len());
        let union_type = InferenceType::new(!poly_args.is_empty(), union_type_done, parts);

        self.extra_data[extra_data_idx as usize] = ExtraData{
            expr_id: lit_id.upcast(),
            definition_id: literal.definition,
            poly_vars: poly_args,
            embedded,
            returned: union_type
        };
    }

    /// Inserts the extra polymorphic data struct. Assumes that the select
    /// expression's referenced (definition_id, field_idx) has been resolved.
    fn insert_initial_select_polymorph_data(
        &mut self, ctx: &Ctx, select_id: SelectExpressionId, struct_def_id: DefinitionId
    ) {
        use InferenceTypePart as ITP;

        // Retrieve relevant data
        let expr = &ctx.heap[select_id];
        let expr_type = &self.expr_types[expr.unique_id_in_definition as usize];
        let field_idx = expr_type.field_or_monomorph_idx as usize;
        let extra_data_idx = expr_type.extra_data_idx; // TODO: @Temp
        debug_assert!(extra_data_idx != -1, "initial select polymorph data, but no preallocated ExtraData");

        let definition = ctx.heap[struct_def_id].as_struct();

        // Generate initial polyvar types and struct type
        // TODO: @Performance: we can immediately set the polyvars of the subject's struct type
        let num_poly_vars = definition.poly_vars.len();
        let mut poly_vars = Vec::with_capacity(num_poly_vars);
        let struct_parts_reserved = 1 + 2 * num_poly_vars;
        let mut struct_parts = Vec::with_capacity(struct_parts_reserved);
        struct_parts.push(ITP::Instance(struct_def_id, num_poly_vars as u32));

        for poly_idx in 0..num_poly_vars {
            poly_vars.push(InferenceType::new(true, false, vec![
                ITP::Marker(poly_idx as u32), ITP::Unknown,
            ]));
            struct_parts.push(ITP::Marker(poly_idx as u32));
            struct_parts.push(ITP::Unknown);
        }
        debug_assert_eq!(struct_parts.len(), struct_parts_reserved);

        // Generate initial field type
        let field_type = self.determine_inference_type_from_parser_type_elements(&definition.fields[field_idx].parser_type.elements, false);
        self.extra_data[extra_data_idx as usize] = ExtraData{
            expr_id: select_id.upcast(),
            definition_id: struct_def_id,
            poly_vars,
            embedded: vec![InferenceType::new(num_poly_vars != 0, num_poly_vars == 0, struct_parts)],
            returned: field_type
        };
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
    fn determine_inference_type_from_parser_type_elements(
        &mut self, elements: &[ParserTypeElement],
        use_definitions_known_poly_args: bool
    ) -> InferenceType {
        use ParserTypeVariant as PTV;
        use InferenceTypePart as ITP;

        let mut infer_type = Vec::with_capacity(elements.len());
        let mut has_inferred = false;
        let mut has_markers = false;

        for element in elements {
            match &element.variant {
                // Compiler-only types
                PTV::Void => { infer_type.push(ITP::Void); },
                PTV::InputOrOutput => { infer_type.push(ITP::PortLike); has_inferred = true },
                PTV::ArrayLike => { infer_type.push(ITP::ArrayLike); has_inferred = true },
                PTV::IntegerLike => { infer_type.push(ITP::IntegerLike); has_inferred = true },
                // Builtins
                PTV::Message => {
                    // TODO: @types Remove the Message -> Byte hack at some point...
                    infer_type.push(ITP::Message);
                    infer_type.push(ITP::UInt8);
                },
                PTV::Bool => { infer_type.push(ITP::Bool); },
                PTV::UInt8 => { infer_type.push(ITP::UInt8); },
                PTV::UInt16 => { infer_type.push(ITP::UInt16); },
                PTV::UInt32 => { infer_type.push(ITP::UInt32); },
                PTV::UInt64 => { infer_type.push(ITP::UInt64); },
                PTV::SInt8 => { infer_type.push(ITP::SInt8); },
                PTV::SInt16 => { infer_type.push(ITP::SInt16); },
                PTV::SInt32 => { infer_type.push(ITP::SInt32); },
                PTV::SInt64 => { infer_type.push(ITP::SInt64); },
                PTV::Character => { infer_type.push(ITP::Character); },
                PTV::String => {
                    infer_type.push(ITP::String);
                    infer_type.push(ITP::Character);
                },
                // Special markers
                PTV::IntegerLiteral => { unreachable!("integer literal type on variable type"); },
                PTV::Inferred => {
                    infer_type.push(ITP::Unknown);
                    has_inferred = true;
                },
                // With nested types
                PTV::Array => { infer_type.push(ITP::Array); },
                PTV::Input => { infer_type.push(ITP::Input); },
                PTV::Output => { infer_type.push(ITP::Output); },
                PTV::PolymorphicArgument(belongs_to_definition, poly_arg_idx) => {
                    let poly_arg_idx = *poly_arg_idx;
                    if use_definitions_known_poly_args {
                        // Refers to polymorphic argument on procedure we're currently processing.
                        // This argument is already known.
                        debug_assert_eq!(*belongs_to_definition, self.definition_type.definition_id());
                        debug_assert!((poly_arg_idx as usize) < self.poly_vars.len());

                        Self::determine_inference_type_from_concrete_type(
                            &mut infer_type, &self.poly_vars[poly_arg_idx as usize].parts
                        );
                    } else {
                        // Polymorphic argument has to be inferred
                        has_markers = true;
                        has_inferred = true;
                        infer_type.push(ITP::Marker(poly_arg_idx));
                        infer_type.push(ITP::Unknown)
                    }
                },
                PTV::Definition(definition_id, num_embedded) => {
                    infer_type.push(ITP::Instance(*definition_id, *num_embedded));
                }
            }
        }

        InferenceType::new(has_markers, !has_inferred, infer_type)
    }

    /// Determines the inference type from an already concrete type. Applies the
    /// various type "hacks" inside the type inferencer.
    fn determine_inference_type_from_concrete_type(parser_type: &mut Vec<InferenceTypePart>, concrete_type: &[ConcreteTypePart]) {
        use InferenceTypePart as ITP;
        use ConcreteTypePart as CTP;

        for concrete_part in concrete_type {
            match concrete_part {
                CTP::Void => parser_type.push(ITP::Void),
                CTP::Message => parser_type.push(ITP::Message),
                CTP::Bool => parser_type.push(ITP::Bool),
                CTP::UInt8 => parser_type.push(ITP::UInt8),
                CTP::UInt16 => parser_type.push(ITP::UInt16),
                CTP::UInt32 => parser_type.push(ITP::UInt32),
                CTP::UInt64 => parser_type.push(ITP::UInt64),
                CTP::SInt8 => parser_type.push(ITP::SInt8),
                CTP::SInt16 => parser_type.push(ITP::SInt16),
                CTP::SInt32 => parser_type.push(ITP::SInt32),
                CTP::SInt64 => parser_type.push(ITP::SInt64),
                CTP::Character => parser_type.push(ITP::Character),
                CTP::String => {
                    parser_type.push(ITP::String);
                    parser_type.push(ITP::Character)
                },
                CTP::Array => parser_type.push(ITP::Array),
                CTP::Slice => parser_type.push(ITP::Slice),
                CTP::Input => parser_type.push(ITP::Input),
                CTP::Output => parser_type.push(ITP::Output),
                CTP::Instance(id, num) => parser_type.push(ITP::Instance(*id, *num)),
                CTP::Function(_, _) => unreachable!("function type during concrete to inference type conversion"),
                CTP::Component(_, _) => unreachable!("component type during concrete to inference type conversion"),
            }
        }
    }

    /// Construct an error when an expression's type does not match. This
    /// happens if we infer the expression type from its arguments (e.g. the
    /// expression type of an addition operator is the type of the arguments)
    /// But the expression type was already set due to our parent (e.g. an
    /// "if statement" or a "logical not" always expecting a boolean)
    fn construct_expr_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, arg_id: ExpressionId
    ) -> ParseError {
        // TODO: Expand and provide more meaningful information for humans
        let expr = &ctx.heap[expr_id];
        let arg_expr = &ctx.heap[arg_id];
        let expr_idx = expr.get_unique_id_in_definition();
        let arg_expr_idx = arg_expr.get_unique_id_in_definition();
        let expr_type = &self.expr_types[expr_idx as usize].expr_type;
        let arg_type = &self.expr_types[arg_expr_idx as usize].expr_type;

        return ParseError::new_error_at_span(
            &ctx.module().source, expr.operation_span(), format!(
                "incompatible types: this expression expected a '{}'",
                expr_type.display_name(&ctx.heap)
            )
        ).with_info_at_span(
            &ctx.module().source, arg_expr.full_span(), format!(
                "but this expression yields a '{}'",
                arg_type.display_name(&ctx.heap)
            )
        )
    }

    fn construct_arg_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId,
        arg1_id: ExpressionId, arg2_id: ExpressionId
    ) -> ParseError {
        let expr = &ctx.heap[expr_id];
        let arg1 = &ctx.heap[arg1_id];
        let arg2 = &ctx.heap[arg2_id];

        let arg1_idx = arg1.get_unique_id_in_definition();
        let arg1_type = &self.expr_types[arg1_idx as usize].expr_type;
        let arg2_idx = arg2.get_unique_id_in_definition();
        let arg2_type = &self.expr_types[arg2_idx as usize].expr_type;

        return ParseError::new_error_str_at_span(
            &ctx.module().source, expr.operation_span(),
            "incompatible types: cannot apply this expression"
        ).with_info_at_span(
            &ctx.module().source, arg1.full_span(), format!(
                "Because this expression has type '{}'",
                arg1_type.display_name(&ctx.heap)
            )
        ).with_info_at_span(
            &ctx.module().source, arg2.full_span(), format!(
                "But this expression has type '{}'",
                arg2_type.display_name(&ctx.heap)
            )
        )
    }

    fn construct_template_type_error(
        &self, ctx: &Ctx, expr_id: ExpressionId, template: &[InferenceTypePart]
    ) -> ParseError {
        let expr = &ctx.heap[expr_id];
        let expr_idx = expr.get_unique_id_in_definition();
        let expr_type = &self.expr_types[expr_idx as usize].expr_type;

        return ParseError::new_error_at_span(
            &ctx.module().source, expr.full_span(), format!(
                "incompatible types: got a '{}' but expected a '{}'",
                expr_type.display_name(&ctx.heap), 
                InferenceType::partial_display_name(&ctx.heap, template)
            )
        )
    }

    /// Constructs a human interpretable error in the case that type inference
    /// on a polymorphic variable to a function call or literal construction 
    /// failed. This may only be caused by a pair of inference types (which may 
    /// come from arguments or the return type) having two different inferred 
    /// values for that polymorphic variable.
    ///
    /// So we find this pair and construct the error using it.
    ///
    /// We assume that the expression is a function call or a struct literal,
    /// and that an actual error has occurred.
    fn construct_poly_arg_error(
        ctx: &Ctx, poly_data: &ExtraData, expr_id: ExpressionId
    ) -> ParseError {
        // Helper function to check for polymorph mismatch between two inference
        // types.
        fn has_poly_mismatch<'a>(type_a: &'a InferenceType, type_b: &'a InferenceType) -> Option<(u32, &'a [InferenceTypePart], &'a [InferenceTypePart])> {
            if !type_a.has_marker || !type_b.has_marker {
                return None
            }

            for (marker_a, section_a) in type_a.marker_iter() {
                for (marker_b, section_b) in type_b.marker_iter() {
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

        // Helper function to check for polymorph mismatch between an inference
        // type and the polymorphic variables in the poly_data struct.
        fn has_explicit_poly_mismatch<'a>(
            poly_vars: &'a [InferenceType], arg: &'a InferenceType
        ) -> Option<(u32, &'a [InferenceTypePart], &'a [InferenceTypePart])> {
            for (marker, section) in arg.marker_iter() {
                debug_assert!((marker as usize) < poly_vars.len());
                let poly_section = &poly_vars[marker as usize].parts;
                if !InferenceType::check_subtrees(poly_section, 0, section, 0) {
                    return Some((marker, poly_section, section))
                }
            }

            None
        }

        // Helpers function to retrieve polyvar name and definition name
        fn get_poly_var_and_definition_name<'a>(ctx: &'a Ctx, poly_var_idx: u32, definition_id: DefinitionId) -> (&'a str, &'a str) {
            let definition = &ctx.heap[definition_id];
            let poly_var = definition.poly_vars()[poly_var_idx as usize].value.as_str();
            let func_name = definition.identifier().value.as_str();

            (poly_var, func_name)
        }

        // Helper function to construct initial error
        fn construct_main_error(ctx: &Ctx, poly_data: &ExtraData, poly_var_idx: u32, expr: &Expression) -> ParseError {
            match expr {
                Expression::Call(expr) => {
                    let (poly_var, func_name) = get_poly_var_and_definition_name(ctx, poly_var_idx, poly_data.definition_id);
                    return ParseError::new_error_at_span(
                        &ctx.module().source, expr.func_span, format!(
                            "Conflicting type for polymorphic variable '{}' of '{}'",
                            poly_var, func_name
                        )
                    )
                },
                Expression::Literal(expr) => {
                    let (poly_var, type_name) = get_poly_var_and_definition_name(ctx, poly_var_idx, poly_data.definition_id);
                    return ParseError::new_error_at_span(
                        &ctx.module().source, expr.span, format!(
                            "Conflicting type for polymorphic variable '{}' of instantiation of '{}'",
                            poly_var, type_name
                        )
                    );
                },
                Expression::Select(expr) => {
                    let (poly_var, struct_name) = get_poly_var_and_definition_name(ctx, poly_var_idx, poly_data.definition_id);
                    return ParseError::new_error_at_span(
                        &ctx.module().source, expr.full_span, format!(
                            "Conflicting type for polymorphic variable '{}' while accessing field '{}' of '{}'",
                            poly_var, expr.field_name.value.as_str(), struct_name
                        )
                    )
                }
                _ => unreachable!("called construct_poly_arg_error without an expected expression, got: {:?}", expr)
            }
        }

        // Actual checking
        let expr = &ctx.heap[expr_id];
        let (expr_args, expr_return_name) = match expr {
            Expression::Call(expr) => 
                (
                    expr.arguments.clone(),
                    "return type"
                ),
            Expression::Literal(expr) => {
                let expressions = match &expr.value {
                    Literal::Struct(v) => v.fields.iter()
                        .map(|f| f.value)
                        .collect(),
                    Literal::Enum(_) => Vec::new(),
                    Literal::Union(v) => v.values.clone(),
                    _ => unreachable!()
                };

                ( expressions, "literal" )
            },
            Expression::Select(expr) =>
                // Select expression uses the polymorphic variables of the 
                // struct it is accessing, so get the subject expression.
                (
                    vec![expr.subject],
                    "selected field"
                ),
            _ => unreachable!(),
        };

        // - check return type with itself
        if let Some((poly_idx, section_a, section_b)) = has_poly_mismatch(
            &poly_data.returned, &poly_data.returned
        ) {
            return construct_main_error(ctx, poly_data, poly_idx, expr)
                .with_info_at_span(
                    &ctx.module().source, expr.full_span(), format!(
                        "The {} inferred the conflicting types '{}' and '{}'",
                        expr_return_name,
                        InferenceType::partial_display_name(&ctx.heap, section_a),
                        InferenceType::partial_display_name(&ctx.heap, section_b)
                    )
                );
        }

        // - check arguments with each other argument and with return type
        for (arg_a_idx, arg_a) in poly_data.embedded.iter().enumerate() {
            for (arg_b_idx, arg_b) in poly_data.embedded.iter().enumerate() {
                if arg_b_idx > arg_a_idx {
                    break;
                }

                if let Some((poly_idx, section_a, section_b)) = has_poly_mismatch(&arg_a, &arg_b) {
                    let error = construct_main_error(ctx, poly_data, poly_idx, expr);
                    if arg_a_idx == arg_b_idx {
                        // Same argument
                        let arg = &ctx.heap[expr_args[arg_a_idx]];
                        return error.with_info_at_span(
                            &ctx.module().source, arg.full_span(), format!(
                                "This argument inferred the conflicting types '{}' and '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_a),
                                InferenceType::partial_display_name(&ctx.heap, section_b)
                            )
                        );
                    } else {
                        let arg_a = &ctx.heap[expr_args[arg_a_idx]];
                        let arg_b = &ctx.heap[expr_args[arg_b_idx]];
                        return error.with_info_at_span(
                            &ctx.module().source, arg_a.full_span(), format!(
                                "This argument inferred it to '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_a)
                            )
                        ).with_info_at_span(
                            &ctx.module().source, arg_b.full_span(), format!(
                                "While this argument inferred it to '{}'",
                                InferenceType::partial_display_name(&ctx.heap, section_b)
                            )
                        )
                    }
                }
            }

            // Check with return type
            if let Some((poly_idx, section_arg, section_ret)) = has_poly_mismatch(arg_a, &poly_data.returned) {
                let arg = &ctx.heap[expr_args[arg_a_idx]];
                return construct_main_error(ctx, poly_data, poly_idx, expr)
                    .with_info_at_span(
                        &ctx.module().source, arg.full_span(), format!(
                            "This argument inferred it to '{}'",
                            InferenceType::partial_display_name(&ctx.heap, section_arg)
                        )
                    )
                    .with_info_at_span(
                        &ctx.module().source, expr.full_span(), format!(
                            "While the {} inferred it to '{}'",
                            expr_return_name,
                            InferenceType::partial_display_name(&ctx.heap, section_ret)
                        )
                    );
            }
        }

        // Now check against the explicitly specified polymorphic variables (if
        // any).
        for (arg_idx, arg) in poly_data.embedded.iter().enumerate() {
            if let Some((poly_idx, poly_section, arg_section)) = has_explicit_poly_mismatch(&poly_data.poly_vars, arg) {
                let arg = &ctx.heap[expr_args[arg_idx]];
                return construct_main_error(ctx, poly_data, poly_idx, expr)
                    .with_info_at_span(
                        &ctx.module().source, arg.full_span(), format!(
                            "The polymorphic variable has type '{}' (which might have been partially inferred) while the argument inferred it to '{}'",
                            InferenceType::partial_display_name(&ctx.heap, poly_section),
                            InferenceType::partial_display_name(&ctx.heap, arg_section)
                        )
                    );
            }
        }

        if let Some((poly_idx, poly_section, ret_section)) = has_explicit_poly_mismatch(&poly_data.poly_vars, &poly_data.returned) {
            return construct_main_error(ctx, poly_data, poly_idx, expr)
                .with_info_at_span(
                    &ctx.module().source, expr.full_span(), format!(
                        "The polymorphic variable has type '{}' (which might have been partially inferred) while the {} inferred it to '{}'",
                        InferenceType::partial_display_name(&ctx.heap, poly_section),
                        expr_return_name,
                        InferenceType::partial_display_name(&ctx.heap, ret_section)
                    )
                )
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
            (ITP::NumberLike, ITP::UInt8),
            (ITP::IntegerLike, ITP::SInt32),
            (ITP::Unknown, ITP::UInt64),
            (ITP::Unknown, ITP::Bool)
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
                &mut lhs_type, 0, &rhs_type.parts, 0, false
            );
            assert_eq!(SingleInferenceResult::Modified, result);
            assert_eq!(lhs_type.parts, rhs_type.parts);
        }
    }

    #[test]
    fn test_multi_part_inference() {
        let pairs = [
            (vec![ITP::ArrayLike, ITP::NumberLike], vec![ITP::Slice, ITP::SInt8]),
            (vec![ITP::Unknown], vec![ITP::Input, ITP::Array, ITP::String, ITP::Character]),
            (vec![ITP::PortLike, ITP::SInt32], vec![ITP::Input, ITP::SInt32]),
            (vec![ITP::Unknown], vec![ITP::Output, ITP::SInt32]),
            (
                vec![ITP::Instance(Id::new(0), 2), ITP::Input, ITP::Unknown, ITP::Output, ITP::Unknown],
                vec![ITP::Instance(Id::new(0), 2), ITP::Input, ITP::Array, ITP::SInt32, ITP::Output, ITP::SInt32]
            )
        ];

        for (lhs, rhs) in pairs.iter() {
            let mut lhs_type = IT::new(false, false, lhs.clone());
            let mut rhs_type = IT::new(false, true, rhs.clone());
            let result = unsafe{ IT::infer_subtrees_for_both_types(
                &mut lhs_type, 0, &mut rhs_type, 0
            ) };
            assert_eq!(DualInferenceResult::First, result);
            assert_eq!(lhs_type.parts, rhs_type.parts);

            let mut lhs_type = IT::new(false, false, lhs.clone());
            let rhs_type = IT::new(false, true, rhs.clone());
            let result = IT::infer_subtree_for_single_type(
                &mut lhs_type, 0, &rhs_type.parts, 0, false
            );
            assert_eq!(SingleInferenceResult::Modified, result);
            assert_eq!(lhs_type.parts, rhs_type.parts)
        }
    }
}
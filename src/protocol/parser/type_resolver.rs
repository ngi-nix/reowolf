use super::visitor::{Visitor2, VisitorResult};

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

}

impl Visitor2 for TypeResolvingVisitor {

}
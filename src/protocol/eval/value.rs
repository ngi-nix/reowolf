
use crate::PortId;
use crate::protocol::ast::{
    AssignmentOperator,
    BinaryOperator,
    UnaryOperator,
};
use crate::protocol::eval::Store;

pub type StackPos = u32;
pub type HeapPos = u32;

#[derive(Copy, Clone)]
pub enum ValueId {
    Stack(StackPos), // place on stack
    Heap(HeapPos, u32), // allocated region + values within that region
}

#[derive(Debug, Clone)]
pub enum Value {
    // Special types, never encountered during evaluation if the compiler works correctly
    Unassigned,                 // Marker when variables are first declared, immediately followed by assignment
    PrevStackBoundary(isize),   // Marker for stack frame beginning, so we can pop stack values
    Ref(ValueId),               // Reference to a value, used by expressions producing references
    // Builtin types
    Input(PortId),
    Output(PortId),
    Message(HeapPos),
    Null,
    Bool(bool),
    Char(char),
    String(HeapPos),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    SInt8(i8),
    SInt16(i16),
    SInt32(i32),
    SInt64(i64),
    Array(HeapPos),
    // Instances of user-defined types
    Enum(i64),
    Union(i64, HeapPos),
    Struct(HeapPos),
}

macro_rules! impl_union_unpack_as_value {
    ($func_name:ident, $variant_name:path, $return_type:ty) => {
        impl Value {
            pub(crate) fn $func_name(&self) -> $return_type {
                match self {
                    $variant_name(v) => *v,
                    _ => panic!(concat!("called ", stringify!($func_name()), " on {:?}"), self),
                }
            }
        }
    }
}

impl_union_unpack_as_value!(as_stack_boundary, Value::PrevStackBoundary, isize);
impl_union_unpack_as_value!(as_ref,     Value::Ref,     ValueId);
impl_union_unpack_as_value!(as_input,   Value::Input,   PortId);
impl_union_unpack_as_value!(as_output,  Value::Output,  PortId);
impl_union_unpack_as_value!(as_message, Value::Message, HeapPos);
impl_union_unpack_as_value!(as_bool,    Value::Bool,    bool);
impl_union_unpack_as_value!(as_char,    Value::Char,    char);
impl_union_unpack_as_value!(as_string,  Value::String,  HeapPos);
impl_union_unpack_as_value!(as_uint8,   Value::UInt8,   u8);
impl_union_unpack_as_value!(as_uint16,  Value::UInt16,  u16);
impl_union_unpack_as_value!(as_uint32,  Value::UInt32,  u32);
impl_union_unpack_as_value!(as_uint64,  Value::UInt64,  u64);
impl_union_unpack_as_value!(as_sint8,   Value::SInt8,   i8);
impl_union_unpack_as_value!(as_sint16,  Value::SInt16,  i16);
impl_union_unpack_as_value!(as_sint32,  Value::SInt32,  i32);
impl_union_unpack_as_value!(as_sint64,  Value::SInt64,  i64);
impl_union_unpack_as_value!(as_array,   Value::Array,   HeapPos);
impl_union_unpack_as_value!(as_enum,    Value::Enum,    i64);
impl_union_unpack_as_value!(as_struct,  Value::Struct,  HeapPos);

impl Value {
    pub(crate) fn as_union(&self) -> (i64, HeapPos) {
        match self {
            Value::Union(tag, v) => (*tag, *v),
            _ => panic!("called as_union on {:?}", self),
        }
    }

    pub(crate) fn is_integer(&self) -> bool {
        match self {
            Value::UInt8(_) | Value::UInt16(_) | Value::UInt32(_) | Value::UInt64(_) |
            Value::SInt8(_) | Value::SInt16(_) | Value::SInt32(_) | Value::SInt64(_) => true,
            _ => false
        }
    }

    pub(crate) fn is_unsigned_integer(&self) -> bool {
        match self {
            Value::UInt8(_) | Value::UInt16(_) | Value::UInt32(_) | Value::UInt64(_) => true,
            _ => false
        }
    }

    pub(crate) fn is_signed_integer(&self) -> bool {
        match self {
            Value::SInt8(_) | Value::SInt16(_) | Value::SInt32(_) | Value::SInt64(_) => true,
            _ => false
        }
    }

    pub(crate) fn as_unsigned_integer(&self) -> u64 {
        match self {
            Value::UInt8(v)  => *v as u64,
            Value::UInt16(v) => *v as u64,
            Value::UInt32(v) => *v as u64,
            Value::UInt64(v) => *v as u64,
            _ => unreachable!("called as_unsigned_integer on {:?}", self),
        }
    }

    pub(crate) fn as_signed_integer(&self) -> i64 {
        match self {
            Value::SInt8(v)  => *v as i64,
            Value::SInt16(v) => *v as i64,
            Value::SInt32(v) => *v as i64,
            Value::SInt64(v) => *v as i64,
            _ => unreachable!("called as_signed_integer on {:?}", self)
        }
    }

    /// Returns the heap position associated with the value. If the value
    /// doesn't store anything in the heap then we return `None`.
    pub(crate) fn get_heap_pos(&self) -> Option<HeapPos> {
        match self {
            Value::Message(v) => Some(*v),
            Value::Array(v) => Some(*v),
            Value::Union(_, v) => Some(*v),
            Value::Struct(v) => Some(*v),
            _ => None
        }
    }
}

/// Applies the assignment operator. If a heap position is returned then that
/// heap position should be cleared
pub(crate) fn apply_assignment_operator(store: &mut Store, lhs: ValueRef, op: AssignmentOperator, rhs: Value) {
    use AssignmentOperator as AO;

    macro_rules! apply_int_op {
        ($lhs:ident, $assignment_tokens:tt, $operator:ident, $rhs:ident) => {
            match $lhs {
                Value::UInt8(v)  => { *v $assignment_tokens $rhs.as_uint8();  },
                Value::UInt16(v) => { *v $assignment_tokens $rhs.as_uint16(); },
                Value::UInt32(v) => { *v $assignment_tokens $rhs.as_uint32(); },
                Value::UInt64(v) => { *v $assignment_tokens $rhs.as_uint64(); },
                Value::SInt8(v)  => { *v $assignment_tokens $rhs.as_sint8();  },
                Value::SInt16(v) => { *v $assignment_tokens $rhs.as_sint16(); },
                Value::SInt32(v) => { *v $assignment_tokens $rhs.as_sint32(); },
                Value::SInt64(v) => { *v $assignment_tokens $rhs.as_sint64(); },
                _ => unreachable!("apply_assignment_operator {:?} on lhs {:?} and rhs {:?}", $operator, $lhs, $rhs),
            }
        }
    }

    let lhs = store.read_mut_ref(lhs);

    let mut to_dealloc = None;
    match AO {
        AO::Set => {
            match lhs {
                Value::Unassigned => { *lhs = rhs; },
                Value::Input(v)  => { *v = rhs.as_input(); },
                Value::Output(v) => { *v = rhs.as_output(); },
                Value::Message(v)  => { to_dealloc = Some(*v); *v = rhs.as_message(); },
                Value::Bool(v)    => { *v = rhs.as_bool(); },
                Value::Char(v) => { *v = rhs.as_char(); },
                Value::String(v) => { *v = rhs.as_string().clone(); },
                Value::UInt8(v) => { *v = rhs.as_uint8(); },
                Value::UInt16(v) => { *v = rhs.as_uint16(); },
                Value::UInt32(v) => { *v = rhs.as_uint32(); },
                Value::UInt64(v) => { *v = rhs.as_uint64(); },
                Value::SInt8(v) => { *v = rhs.as_sint8(); },
                Value::SInt16(v) => { *v = rhs.as_sint16(); },
                Value::SInt32(v) => { *v = rhs.as_sint32(); },
                Value::SInt64(v) => { *v = rhs.as_sint64(); },
                Value::Array(v) => { to_dealloc = Some(*v); *v = rhs.as_array(); },
                Value::Enum(v) => { *v = rhs.as_enum(); },
                Value::Union(lhs_tag, lhs_heap_pos) => {
                    to_dealloc = Some(*lhs_heap_pos);
                    let (rhs_tag, rhs_heap_pos) = rhs.as_union();
                    *lhs_tag = rhs_tag;
                    *lhs_heap_pos = rhs_heap_pos;
                }
                Value::Struct(v) => { to_dealloc = Some(*v); *v = rhs.as_struct(); },
                _ => unreachable!("apply_assignment_operator {:?} on lhs {:?} and rhs {:?}", op, lhs, rhs),
            }
        },
        AO::Multiplied =>   { apply_int_op!(lhs, *=,  op, rhs) },
        AO::Divided =>      { apply_int_op!(lhs, /=,  op, rhs) },
        AO::Remained =>     { apply_int_op!(lhs, %=,  op, rhs) },
        AO::Added =>        { apply_int_op!(lhs, +=,  op, rhs) },
        AO::Subtracted =>   { apply_int_op!(lhs, -=,  op, rhs) },
        AO::ShiftedLeft =>  { apply_int_op!(lhs, <<=, op, rhs) },
        AO::ShiftedRight => { apply_int_op!(lhs, >>=, op, rhs) },
        AO::BitwiseAnded => { apply_int_op!(lhs, &=,  op, rhs) },
        AO::BitwiseXored => { apply_int_op!(lhs, ^=,  op, rhs) },
        AO::BitwiseOred =>  { apply_int_op!(lhs, |=,  op, rhs) },
    }

    if let Some(heap_pos) = to_dealloc {
        store.drop_heap_pos(heap_pos);
    }
}

pub(crate) fn apply_binary_operator(store: &mut Store, lhs: &Value, op: BinaryOperator, rhs: &Value) -> Value {
    use BinaryOperator as BO;

    macro_rules! apply_int_op_and_return {
        ($lhs:ident, $operator_tokens:tt, $operator:ident, $rhs:ident) => {
            return match $lhs {
                Value::UInt8(v)  => { Value::UInt8( *v $operator_tokens $rhs.as_uint8() ); },
                Value::UInt16(v) => { Value::UInt16(*v $operator_tokens $rhs.as_uint16()); },
                Value::UInt32(v) => { Value::UInt32(*v $operator_tokens $rhs.as_uint32()); },
                Value::UInt64(v) => { Value::UInt64(*v $operator_tokens $rhs.as_uint64()); },
                Value::SInt8(v)  => { Value::SInt8( *v $operator_tokens $rhs.as_sint8() ); },
                Value::SInt16(v) => { Value::SInt16(*v $operator_tokens $rhs.as_sint16()); },
                Value::SInt32(v) => { Value::SInt32(*v $operator_tokens $rhs.as_sint32()); },
                Value::SInt64(v) => { Value::SInt64(*v $operator_tokens $rhs.as_sint64()); },
                _ => unreachable!("apply_binary_operator {:?} on lhs {:?} and rhs {:?}", $operator, $lhs, $rhs)
            };
        }
    }

    match op {
        BO::Concatenate => {
            let lhs_heap_pos;
            let rhs_heap_pos;
            let construct_fn;
            match lhs {
                Value::Message(lhs_pos) => {
                    lhs_heap_pos = *lhs_pos;
                    rhs_heap_pos = rhs.as_message();
                    construct_fn = |pos: HeapPos| Value::Message(pos);
                },
                Value::String(lhs_pos) => {
                    lhs_heap_pos = *lhs_pos;
                    rhs_heap_pos = rhs.as_string();
                    construct_fn = |pos: HeapPos| Value::String(pos);
                },
                Value::Array(lhs_pos) => {
                    lhs_heap_pos = *lhs_pos;
                    rhs_heap_pos = *rhs.as_array();
                    construct_fn = |pos: HeapPos| Value::Array(pos);
                },
                _ => unreachable!("apply_binary_operator {:?} on lhs {:?} and rhs {:?}", op, lhs, rhs)
            }

            let target_heap_pos = store.alloc_heap();
            let target = &mut store.heap_regions[target_heap_pos as usize].values;
            target.extend(&store.heap_regions[lhs_heap_pos as usize].values);
            target.extend(&store.heap_regions[rhs_heap_pos as usize].values);
            return construct_fn(target_heap_pos);
        },
        BO::LogicalOr => {
            return Value::Bool(lhs.as_bool() || rhs.as_bool());
        },
        BO::LogicalAnd => {
            return Value::Bool(lhs.as_bool() && rhs.as_bool());
        },
        BO::BitwiseOr        => { apply_int_op_and_return!(lhs, |,  op, rhs); },
        BO::BitwiseXor       => { apply_int_op_and_return!(lhs, ^,  op, rhs); },
        BO::BitwiseAnd       => { apply_int_op_and_return!(lhs, &,  op, rhs); },
        BO::Equality => { todo!("implement") },
        BO::Inequality =>  { todo!("implement") },
        BO::LessThan         => { apply_int_op_and_return!(lhs, <,  op, rhs); },
        BO::GreaterThan      => { apply_int_op_and_return!(lhs, >,  op, rhs); },
        BO::LessThanEqual    => { apply_int_op_and_return!(lhs, <=, op, rhs); },
        BO::GreaterThanEqual => { apply_int_op_and_return!(lhs, >=, op, rhs); },
        BO::ShiftLeft        => { apply_int_op_and_return!(lhs, <<, op, rhs); },
        BO::ShiftRight       => { apply_int_op_and_return!(lhs, >>, op, rhs); },
        BO::Add              => { apply_int_op_and_return!(lhs, +,  op, rhs); },
        BO::Subtract         => { apply_int_op_and_return!(lhs, -,  op, rhs); },
        BO::Multiply         => { apply_int_op_and_return!(lhs, *,  op, rhs); },
        BO::Divide           => { apply_int_op_and_return!(lhs, /,  op, rhs); },
        BO::Remainder        => { apply_int_op_and_return!(lhs, %,  op, rhs); }
    }
}

pub(crate) fn apply_unary_operator(store: &mut Store, op: UnaryOperator, value: &Value) -> Value {
    use UnaryOperator as UO;

    macro_rules! apply_int_expr_and_return {
        ($value:ident, $apply:expr, $op:ident) => {
            return match $value {
                Value::UInt8(v)  => Value::UInt8($apply),
                Value::UInt16(v) => Value::UInt16($apply),
                Value::UInt32(v) => Value::UInt32($apply),
                Value::UInt64(v) => Value::UInt64($apply),
                Value::SInt8(v)  => Value::SInt8($apply),
                Value::SInt16(v) => Value::SInt16($apply),
                Value::SInt32(v) => Value::SInt32($apply),
                Value::SInt64(v) => Value::SInt64($apply),
                _ => unreachable!("apply_unary_operator {:?} on value {:?}", $op, $value),
            };
        }
    }

    match op {
        UO::Positive   => { apply_int_expr_and_return!(value, *v, op) },
        UO::Negative   => { apply_int_expr_and_return!(value, *v, op) },
        UO::BitwiseNot => { apply_int_expr_and_return!(value, *v, op)},
        UO::LogicalNot => { return Value::Bool(!value.as_bool()); },
        UO::PreIncrement => { todo!("implement") },
        UO::PreDecrement => { todo!("implement") },
        UO::PostIncrement => { todo!("implement") },
        UO::PostDecrement => { todo!("implement") },
    }
}
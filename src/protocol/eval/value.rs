
use super::store::*;
use crate::PortId;
use crate::protocol::ast::{
    AssignmentOperator,
    BinaryOperator,
    UnaryOperator,
};

pub type StackPos = u32;
pub type HeapPos = u32;

#[derive(Copy, Clone)]
pub enum ValueId {
    Stack(StackPos), // place on stack
    Heap(HeapPos, u32), // allocated region + values within that region
}

/// Represents a value stored on the stack or on the heap. Some values contain
/// a `HeapPos`, implying that they're stored in the store's `Heap`. Clearing
/// a `Value` with a `HeapPos` from a stack must also clear the associated
/// region from the `Heap`.
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

/// When providing arguments to a new component, or when transferring values
/// from one component's store to a newly instantiated component, one has to
/// transfer stack and heap values. This `ValueGroup` represents such a
/// temporary group of values with potential heap allocations.
///
/// Constructing such a ValueGroup manually requires some extra care to make
/// sure all elements of `values` point to valid elements of `regions`.
///
/// Again: this is a temporary thing, hopefully removed once we move to a
/// bytecode interpreter.
pub struct ValueGroup {
    pub(crate) values: Vec<Value>,
    pub(crate) regions: Vec<Vec<Value>>
}

impl ValueGroup {
    pub(crate) fn new_stack(values: Vec<Value>) -> Self {
        debug_assert!(values.iter().all(|v| v.get_heap_pos().is_none()));
        Self{
            values,
            regions: Vec::new(),
        }
    }
    pub(crate) fn from_store(store: &Store, values: &[Value]) -> Self {
        let mut group = ValueGroup{
            values: Vec::with_capacity(values.len()),
            regions: Vec::with_capacity(values.len()), // estimation
        };

        for value in values {
            let transferred = group.retrieve_value(value, store);
            group.values.push(transferred);
        }

        group
    }

    /// Transfers a provided value from a store into a local value with its
    /// heap allocations (if any) stored in the ValueGroup. Calling this
    /// function will not store the returned value in the `values` member.
    fn retrieve_value(&mut self, value: &Value, from_store: &Store) -> Value {
        if let Some(heap_pos) = value.get_heap_pos() {
            // Value points to a heap allocation, so transfer the heap values
            // internally.
            let from_region = &from_store.heap_regions[heap_pos as usize].values;
            let mut new_region = Vec::with_capacity(from_region.len());
            for value in from_region {
                let transferred = self.transfer_value(value, from_store);
                new_region.push(transferred);
            }

            // Region is constructed, store internally and return the new value.
            let new_region_idx = self.regions.len() as HeapPos;
            self.regions.push(new_region);

            return match value {
                Value::Message(_)    => Value::Message(new_region_idx),
                Value::String(_)     => Value::String(new_region_idx),
                Value::Array(_)      => Value::Array(new_region_idx),
                Value::Union(tag, _) => Value::Union(*tag, new_region_idx),
                Value::Struct(_)     => Value::Struct(new_region_idx),
                _ => unreachable!(),
            };
        } else {
            return value.clone();
        }
    }

    /// Transfers the heap values and the stack values into the store. Stack
    /// values are pushed onto the Store's stack in the order in which they
    /// appear in the value group.
    pub fn into_store(self, store: &mut Store) {
        for value in &self.values {
            let transferred = self.provide_value(value, store);
            store.stack.push(transferred);
        }
    }

    fn provide_value(&self, value: &Value, to_store: &mut Store) -> Value {
        if let Some(from_heap_pos) = value.get_heap_pos() {
            let from_heap_pos = from_heap_pos as usize;
            let to_heap_pos = to_store.alloc_heap();
            let to_heap_pos_usize = to_heap_pos as usize;
            to_store.heap_regions[to_heap_pos_usize].values.reserve(self.regions[from_heap_pos].len());

            for value in &self.regions[from_heap_pos as usize] {
                let transferred = self.provide_value(value, to_store);
                to_store.heap_regions[to_heap_pos_usize].values.push(transferred);
            }

            return match value {
                Value::Message(_)    => Value::Message(to_heap_pos),
                Value::String(_)     => Value::String(to_heap_pos),
                Value::Array(_)      => Value::Array(to_heap_pos),
                Value::Union(tag, _) => Value::Union(*tag, to_heap_pos),
                Value::Struct(_)     => Value::Struct(to_heap_pos),
                _ => unreachable!(),
            };
        } else {
            return value.clone();
        }
    }
}

impl Default for ValueGroup {
    /// Returns an empty ValueGroup
    fn default() -> Self {
        Self { values: Vec::new(), regions: Vec::new() }
    }
}

pub(crate) fn apply_assignment_operator(store: &mut Store, lhs: ValueId, op: AssignmentOperator, rhs: Value) {
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
    match op {
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
        ($value:ident, $apply:tt, $op:ident) => {
            return match $value {
                Value::UInt8(v)  => Value::UInt8($apply *v),
                Value::UInt16(v) => Value::UInt16($apply *v),
                Value::UInt32(v) => Value::UInt32($apply *v),
                Value::UInt64(v) => Value::UInt64($apply *v),
                Value::SInt8(v)  => Value::SInt8($apply *v),
                Value::SInt16(v) => Value::SInt16($apply *v),
                Value::SInt32(v) => Value::SInt32($apply *v),
                Value::SInt64(v) => Value::SInt64($apply *v),
                _ => unreachable!("apply_unary_operator {:?} on value {:?}", $op, $value),
            };
        }
    }

    match op {
        UO::Positive   => {
            debug_assert!(value.is_integer());
            return value.clone();
        },
        UO::Negative   => { apply_int_expr_and_return!(value, -, op) },
        UO::BitwiseNot => { apply_int_expr_and_return!(value, !, op)},
        UO::LogicalNot => { return Value::Bool(!value.as_bool()); },
        UO::PreIncrement => { todo!("implement") },
        UO::PreDecrement => { todo!("implement") },
        UO::PostIncrement => { todo!("implement") },
        UO::PostDecrement => { todo!("implement") },
    }
}
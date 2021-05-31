
use std::collections::VecDeque;

use super::value::{Value, ValueId, HeapPos};

#[derive(Debug, Clone)]
pub(crate) struct HeapAllocation {
    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub(crate) struct Store {
    // The stack where variables/parameters are stored. Note that this is a
    // non-shrinking stack. So it may be filled with garbage.
    pub(crate) stack: Vec<Value>,
    // Represents the place in the stack where we find the `PrevStackBoundary`
    // value containing the previous stack boundary. This is so we can pop from
    // the stack after function calls.
    pub(crate) cur_stack_boundary: usize,
    // A rather ridiculous simulated heap, but this allows us to "allocate"
    // things that occupy more then one stack slot.
    pub(crate) heap_regions: Vec<HeapAllocation>,
    pub(crate) free_regions: VecDeque<HeapPos>,
}

impl Store {
    pub(crate) fn new() -> Self {
        let mut store = Self{
            stack: Vec::with_capacity(64),
            cur_stack_boundary: 0,
            heap_regions: Vec::new(),
            free_regions: VecDeque::new(),
        };

        store.stack.push(Value::PrevStackBoundary(-1));
        store
    }

    /// Resizes(!) the stack to fit the required number of values. Any
    /// unallocated slots are initialized to `Unassigned`. The specified stack
    /// index is exclusive.
    pub(crate) fn reserve_stack(&mut self, unique_stack_idx: u32) {
        let new_size = self.cur_stack_boundary + unique_stack_idx as usize + 1;
        if new_size > self.stack.len() {
            self.stack.resize(new_size, Value::Unassigned);
        }
    }

    /// Clears values on the stack and removes their heap allocations when
    /// applicable. The specified index itself will also be cleared (so if you
    /// specify 0 all values in the frame will be destroyed)
    pub(crate) fn clear_stack(&mut self, unique_stack_idx: usize) {
        let new_size = self.cur_stack_boundary + unique_stack_idx + 1;
        for idx in new_size..self.stack.len() {
            self.drop_value(self.stack[idx].get_heap_pos());
            self.stack[idx] = Value::Unassigned;
        }
    }

    /// Reads a value and takes ownership. This is different from a move because
    /// the value might indirectly reference stack/heap values. For these kinds
    /// values we will actually return a cloned value.
    pub(crate) fn read_take_ownership(&mut self, value: Value) -> Value {
        match value {
            Value::Ref(ValueId::Stack(pos)) => {
                let abs_pos = self.cur_stack_boundary + 1 + pos as usize;
                return self.clone_value(self.stack[abs_pos].clone());
            },
            Value::Ref(ValueId::Heap(heap_pos, value_idx)) => {
                let heap_pos = heap_pos as usize;
                let value_idx = value_idx as usize;
                return self.clone_value(self.heap_regions[heap_pos].values[value_idx].clone());
            },
            _ => value
        }
    }

    /// Reads a value from a specific address. The value is always copied, hence
    /// if the value ends up not being written, one should call `drop_value` on
    /// it.
    pub(crate) fn read_copy(&mut self, address: ValueId) -> Value {
        match address {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                return self.clone_value(self.stack[cur_pos].clone());
            },
            ValueId::Heap(heap_pos, region_idx) => {
                return self.clone_value(self.heap_regions[heap_pos as usize].values[region_idx as usize].clone())
            }
        }
    }

    /// Potentially reads a reference value. The supplied `Value` might not
    /// actually live in the store's stack or heap, but live on the expression
    /// stack. Generally speaking you only want to call this if the value comes
    /// from the expression stack due to borrowing issues.
    pub(crate) fn maybe_read_ref<'a>(&'a self, value: &'a Value) -> &'a Value {
        match value {
            Value::Ref(value_id) => self.read_ref(*value_id),
            _ => value,
        }
    }

    /// Returns an immutable reference to the value pointed to by an address
    pub(crate) fn read_ref(&self, address: ValueId) -> &Value {
        match address {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                return &self.stack[cur_pos];
            },
            ValueId::Heap(heap_pos, region_idx) => {
                return &self.heap_regions[heap_pos as usize].values[region_idx as usize];
            }
        }
    }

    /// Returns a mutable reference to the value pointed to by an address
    pub(crate) fn read_mut_ref(&mut self, address: ValueId) -> &mut Value {
        match address {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                return &mut self.stack[cur_pos];
            },
            ValueId::Heap(heap_pos, region_idx) => {
                return &mut self.heap_regions[heap_pos as usize].values[region_idx as usize];
            }
        }
    }

    /// Writes a value
    pub(crate) fn write(&mut self, address: ValueId, value: Value) {
        match address {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                self.drop_value(self.stack[cur_pos].get_heap_pos());
                self.stack[cur_pos] = value;
            },
            ValueId::Heap(heap_pos, region_idx) => {
                let heap_pos = heap_pos as usize;
                let region_idx = region_idx as usize;
                self.drop_value(self.heap_regions[heap_pos].values[region_idx].get_heap_pos());
                self.heap_regions[heap_pos].values[region_idx] = value
            }
        }
    }

    /// This thing takes a cloned Value, because of borrowing issues (which is
    /// either a direct value, or might contain an index to a heap value), but
    /// should be treated by the programmer as a reference (i.e. don't call
    /// `drop_value(thing)` after calling `clone_value(thing.clone())`.
    pub(crate) fn clone_value(&mut self, value: Value) -> Value {
        // Quickly check if the value is not on the heap
        let source_heap_pos = value.get_heap_pos();
        if source_heap_pos.is_none() {
            // We can do a trivial copy, unless we're dealing with a value
            // reference
            return match value {
                Value::Ref(ValueId::Stack(stack_pos)) => {
                    let abs_stack_pos = self.cur_stack_boundary + stack_pos as usize + 1;
                    self.clone_value(self.stack[abs_stack_pos].clone())
                },
                Value::Ref(ValueId::Heap(heap_pos, val_idx)) => {
                    self.clone_value(self.heap_regions[heap_pos as usize].values[val_idx as usize].clone())
                },
                _ => value,
            };
        }

        // Value does live on heap, copy it
        let source_heap_pos = source_heap_pos.unwrap() as usize;
        let target_heap_pos = self.alloc_heap();
        let target_heap_pos_usize = target_heap_pos as usize;

        let num_values = self.heap_regions[source_heap_pos].values.len();
        for value_idx in 0..num_values {
            let cloned = self.clone_value(self.heap_regions[source_heap_pos].values[value_idx].clone());
            self.heap_regions[target_heap_pos_usize].values.push(cloned);
        }

        match value {
            Value::Message(_) => Value::Message(target_heap_pos),
            Value::String(_) => Value::String(target_heap_pos),
            Value::Array(_) => Value::Array(target_heap_pos),
            Value::Union(tag, _) => Value::Union(tag, target_heap_pos),
            Value::Struct(_) => Value::Struct(target_heap_pos),
            _ => unreachable!("performed clone_value on heap, but {:?} is not a heap value", value),
        }
    }

    pub(crate) fn drop_value(&mut self, value: Option<HeapPos>) {
        if let Some(heap_pos) = value {
            self.drop_heap_pos(heap_pos);
        }
    }

    pub(crate) fn drop_heap_pos(&mut self, heap_pos: HeapPos) {
        let num_values = self.heap_regions[heap_pos as usize].values.len();
        for value_idx in 0..num_values {
            if let Some(other_heap_pos) = self.heap_regions[heap_pos as usize].values[value_idx].get_heap_pos() {
                self.drop_heap_pos(other_heap_pos);
            }
        }

        self.heap_regions[heap_pos as usize].values.clear();
        self.free_regions.push_back(heap_pos);
    }

    pub(crate) fn alloc_heap(&mut self) -> HeapPos {
        if self.free_regions.is_empty() {
            let idx = self.heap_regions.len() as HeapPos;
            self.heap_regions.push(HeapAllocation{ values: Vec::new() });
            return idx;
        } else {
            let idx = self.free_regions.pop_back().unwrap();
            return idx;
        }
    }
}
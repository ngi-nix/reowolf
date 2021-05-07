/// eval
///
/// Evaluator of the generated AST. Note that we use some misappropriated terms
/// to describe where values live and what they do. This is a temporary
/// implementation of an evaluator until some kind of appropriate bytecode or
/// machine code is generated.
///
/// Code is always executed within a "frame". For Reowolf the first frame is
/// usually an executed component. All subsequent frames are function calls.
/// Simple values live on the "stack". Each variable/parameter has a place on
/// the stack where its values are stored. If the value is not a primitive, then
/// its value will be stored in the "heap". Expressions are treated differently
/// and use a separate "stack" for their evaluation.
///
/// Since this is a value-based language, most values are copied. One has to be
/// careful with values that reside in the "heap" and make sure that copies are
/// properly removed from the heap..
///
/// Just to reiterate: this is a temporary implementation. A proper
/// implementation would fully fill out the type table with alignment/size/
/// offset information and lay out bytecode.

mod value;

use std::collections::VecDeque;

use super::value::*;
use crate::PortId;
use crate::protocol::ast::*;
use crate::protocol::*;

struct HeapAllocation {
    values: Vec<Value>,
}

struct Store {
    // The stack where variables/parameters are stored. Note that this is a
    // non-shrinking stack. So it may be filled with garbage.
    stack: Vec<Value>,
    // Represents the place in the stack where we find the `PrevStackBoundary`
    // value containing the previous stack boundary. This is so we can pop from
    // the stack after function calls.
    cur_stack_boundary: usize,
    // A rather ridiculous simulated heap, but this allows us to "allocate"
    // things that occupy more then one stack slot.
    heap_regions: Vec<HeapAllocation>,
    free_regions: VecDeque<HeapPos>,
}

impl Store {
    fn new() -> Self {
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
    fn reserve_stack(&mut self, unique_stack_idx: usize) {
        let new_size = self.cur_stack_boundary + unique_stack_idx + 1;
        if new_size > self.stack.len() {
            self.stack.resize(new_size, Value::Unassigned);
        }
    }

    /// Clears values on the stack and removes their heap allocations when
    /// applicable. The specified index itself will also be cleared (so if you
    /// specify 0 all values in the frame will be destroyed)
    fn clear_stack(&mut self, unique_stack_idx: usize) {
        let new_size = self.cur_stack_boundary + unique_stack_idx + 1;
        for idx in new_size..self.stack.len() {
            self.drop_value(&self.stack[idx]);
            self.stack[idx] = Value::Unassigned;
        }
    }

    /// Reads a value from a specific address. The value is always copied, hence
    /// if the value ends up not being written, one should call `drop_value` on
    /// it.
    fn read_copy(&mut self, address: ValueId) -> Value {
        match value {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                return self.clone_value(&self.stack[cur_pos]);
            },
            ValueId::Heap(heap_pos, region_idx) => {
                return self.clone_value(&self.heap_regions[heap_pos as usize].values[region_idx as usize])
            }
        }
    }

    /// Returns an immutable reference to the value pointed to by an address
    fn read_ref(&mut self, address: ValueId) -> &Value {
        match value {
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
    fn read_mut_ref(&mut self, address: ValueId) -> &mut Value {
        match value {
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
    fn write(&mut self, address: ValueId, value: Value) {
        match address {
            ValueId::Stack(pos) => {
                let cur_pos = self.cur_stack_boundary + 1 + pos as usize;
                self.drop_value(&self.stack[cur_pos]);
                self.stack[cur_pos] = value;
            },
            ValueId::Heap(heap_pos, region_idx) => {
                let heap_pos = heap_pos as usize;
                let region_idx = region_idx as usize;
                self.drop_value(&self.heap_regions[heap_pos].values[region_idx]);
                self.heap_regions[heap_pos].values[region_idx] = value
            }
        }
    }

    fn clone_value(&mut self, value: &Value) -> Value {
        // Quickly check if the value is not on the heap
        let source_heap_pos = value.get_heap_pos();
        if source_heap_pos.is_none() {
            // We can do a trivial copy
            return value.clone();
        }

        // Value does live on heap, copy it
        let source_heap_pos = source_heap_pos.unwrap() as usize;
        let target_heap_pos = self.alloc_heap();
        let target_heap_pos_usize = target_heap_pos as usize;

        let num_values = self.heap_regions[source_heap_pos].values.len();
        for value_idx in 0..num_values {
            let cloned = self.clone_value(&self.heap_regions[source_heap_pos].values[value_idx]);
            self.heap_regions[target_heap_pos_usize].values.push(cloned);
        }

        match value {
            Value::Message(_) => Value::Message(target_heap_pos),
            Value::Array(_) => Value::Array(target_heap_pos),
            Value::Union(tag, _) => Value::Union(*tag, target_heap_pos),
            Value::Struct(_) => Value::Struct(target_heap_pos),
            _ => unreachable!("performed clone_value on heap, but {:?} is not a heap value", value),
        }
    }

    fn drop_value(&mut self, value: &Value) {
        if let Some(heap_pos) = value.get_heap_pos() {
            self.drop_heap_pos(heap_pos);
        }
    }

    fn drop_heap_pos(&mut self, heap_pos: HeapPos) {
        let num_values = self.heap_regions[heap_pos as usize].values.len();
        for value_idx in 0..num_values {
            if let Some(other_heap_pos) = self.heap_regions[heap_pos as usize].values[value_idx].get_heap_pos() {
                self.drop_heap_pos(other_heap_pos);
            }
        }

        self.heap_regions[heap_pos as usize].values.clear();
        self.free_regions.push_back(heap_pos);
    }

    fn alloc_heap(&mut self) -> HeapPos {
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

enum ExprInstruction {
    EvalExpr(ExpressionId),
    PushValToFront,
}

struct Frame {
    definition: DefinitionId,
    position: StatementId,
    expr_stack: VecDeque<ExprInstruction>, // hack for expression evaluation, evaluated by popping from back
    expr_values: VecDeque<Value>, // hack for expression results, evaluated by popping from front/back
}

impl Frame {
    /// Creates a new execution frame. Does not modify the stack in any way.
    pub fn new(heap: &Heap, definition_id: DefinitionId) -> Self {
        let definition = &heap[definition_id];
        let first_statement = match definition {
            Definition::Component(definition) => definition.body,
            Definition::Function(definition) => definition.body,
            _ => unreachable!("initializing frame with {:?} instead of a function/component", definition),
        };

        Frame{
            definition: definition_id,
            position: first_statement.upcast(),
            expr_stack: VecDeque::with_capacity(128),
            expr_values: VecDeque::with_capacity(128),
        }
    }

    /// Prepares a single expression for execution. This involves walking the
    /// expression tree and putting them in the `expr_stack` such that
    /// continuously popping from its back will evaluate the expression. The
    /// results of each expression will be stored by pushing onto `expr_values`.
    pub fn prepare_single_expression(&mut self, heap: &Heap, expr_id: ExpressionId) {
        debug_assert!(self.expr_stack.is_empty());
        self.expr_values.clear(); // May not be empty if last expression result(s) were discarded

        self.serialize_expression(heap, expr_id);
    }

    /// Prepares multiple expressions for execution (i.e. evaluating all
    /// function arguments or all elements of an array/union literal). Per
    /// expression this works the same as `prepare_single_expression`. However
    /// after each expression is evaluated we insert a `PushValToFront`
    /// instruction
    pub fn prepare_multiple_expressions(&mut self, heap: &Heap, expr_ids: &[ExpressionId]) {
        debug_assert!(self.expr_stack.is_empty());
        self.expr_values.clear();

        for expr_id in expr_ids {
            self.expr_stack.push_back(ExprInstruction::PushValToFront);
            self.serialize_expression(heap, *expr_id);
        }
    }

    /// Performs depth-first serialization of expression tree. Let's not care
    /// about performance for a temporary runtime implementation
    fn serialize_expression(&mut self, heap: &Heap, id: ExpressionId) {
        self.expr_stack.push_back(ExprInstruction::EvalExpr(id));

        match &heap[id] {
            Expression::Assignment(expr) => {
                self.serialize_expression(heap, expr.left);
                self.serialize_expression(heap, expr.right);
            },
            Expression::Binding(expr) => {
                todo!("implement binding expression");
            },
            Expression::Conditional(expr) => {
                self.serialize_expression(heap, expr.test);
            },
            Expression::Binary(expr) => {
                self.serialize_expression(heap, expr.left);
                self.serialize_expression(heap, expr.right);
            },
            Expression::Unary(expr) => {
                self.serialize_expression(heap, expr.expression);
            },
            Expression::Indexing(expr) => {
                self.serialize_expression(heap, expr.index);
                self.serialize_expression(heap, expr.subject);
            },
            Expression::Slicing(expr) => {
                self.serialize_expression(heap, expr.from_index);
                self.serialize_expression(heap, expr.to_index);
                self.serialize_expression(heap, expr.subject);
            },
            Expression::Select(expr) => {
                self.serialize_expression(heap, expr.subject);
            },
            Expression::Literal(expr) => {
                // Here we only care about literals that have subexpressions
                match &expr.value {
                    Literal::Null | Literal::True | Literal::False |
                    Literal::Character(_) | Literal::String(_) |
                    Literal::Integer(_) | Literal::Enum(_) => {
                        // No subexpressions
                    },
                    Literal::Struct(literal) => {
                        for field in &literal.fields {
                            self.expr_stack.push_back(ExprInstruction::PushValToFront);
                            self.serialize_expression(heap, field.value);
                        }
                    },
                    Literal::Union(literal) => {
                        for value_expr_id in &literal.values {
                            self.expr_stack.push_back(ExprInstruction::PushValToFront);
                            self.serialize_expression(heap, *value_expr_id);
                        }
                    },
                    Literal::Array(value_expr_ids) => {
                        for value_expr_id in value_expr_ids {
                            self.expr_stack.push_back(ExprInstruction::PushValToFront);
                            self.serialize_expression(heap, *value_expr_id);
                        }
                    }
                }
            },
            Expression::Call(expr) => {
                for arg_expr_id in &expr.arguments {
                    self.expr_stack.push_back(ExprInstruction::PushValToFront);
                    self.serialize_expression(heap, *arg_expr_id);
                }
            },
            Expression::Variable(expr) => {
                // No subexpressions
            }
        }
    }
}

type EvalResult = Result<(), EvalContinuation>;
pub enum EvalContinuation {
    Stepping,
    Inconsistent,
    Terminal,
    SyncBlockStart,
    SyncBlockEnd,
    NewComponent(DefinitionId, Vec<Value>),
    BlockFires(Value),
    BlockGet(Value),
    Put(Value, Value),
}

pub struct Prompt {
    frames: Vec<Frame>,
    store: Store,
}

impl Prompt {
    pub fn new(heap: &Heap, def: DefinitionId) -> Self {
        let mut prompt = Self{
            frames: Vec::new(),
            store: Store::new(),
        };

        prompt.frames.push(Frame::new(heap, def));
        prompt
    }

    pub fn step(&mut self, heap: &Heap, ctx: &mut EvalContext) -> EvalResult {
        let cur_frame = self.frames.last_mut().unwrap();
        if cur_frame.position.is_invalid() {
            if heap[cur_frame.definition].is_function() {
                todo!("End of function without return, return an evaluation error");
            }
            return Err(EvalContinuation::Terminal);
        }

        while !cur_frame.expr_stack.is_empty() {
            let next = cur_frame.expr_stack.pop_back().unwrap();
            match next {
                ExprInstruction::PushValToFront => {
                    cur_frame.expr_values.rotate_right(1);
                },
                ExprInstruction::EvalExpr(expr_id) => {
                    let expr = &heap[expr_id];
                    match expr {
                        Expression::Assignment(expr) => {
                            let to = cur_frame.expr_values.pop_back().unwrap().as_ref();
                            let rhs = cur_frame.expr_values.pop_back().unwrap();
                            apply_assignment_operator(&mut self.store, to, expr.operation, rhs);
                            cur_frame.expr_values.push_back(self.store.read_copy(to));
                            self.store.drop_value(&rhs);
                        },
                        Expression::Binding(_expr) => {
                            todo!("Binding expression");
                        },
                        Expression::Conditional(expr) => {
                            // Evaluate testing expression, then extend the
                            // expression stack with the appropriate expression
                            let test_result = cur_frame.expr_values.pop_back().unwrap().as_bool();
                            if test_result {
                                cur_frame.serialize_expression(heap, expr.true_expression);
                            } else {
                                cur_frame.serialize_expression(heap, expr.false_expression);
                            }
                        },
                        Expression::Binary(expr) => {
                            let lhs = cur_frame.expr_values.pop_back().unwrap();
                            let rhs = cur_frame.expr_values.pop_back().unwrap();
                            let result = apply_binary_operator(&mut self.store, &lhs, expr.operation, &rhs);
                            cur_frame.expr_values.push_back(result);
                            self.store.drop_value(&lhs);
                            self.store.drop_value(&rhs);
                        },
                        Expression::Unary(expr) => {
                            let val = cur_frame.expr_values.pop_back().unwrap();
                            let result = apply_unary_operator(&mut self.store, expr.operation, &val);
                            cur_frame.expr_values.push_back(result);
                            self.store.drop_value(&val);
                        },
                        Expression::Indexing(expr) => {
                            // TODO: Out of bounds checking
                            // Evaluate index. Never heap allocated so we do
                            // not have to drop it.
                            let index = cur_frame.expr_values.pop_back().unwrap();
                            let index = match &index {
                                Value::Ref(value_ref) => self.store.read_ref(*value_ref),
                                index => index,
                            };
                            debug_assert!(deref.is_integer());
                            let index = if index.is_signed_integer() {
                                index.as_signed_integer() as u32
                            } else {
                                index.as_unsigned_integer() as u32
                            };

                            let subject = cur_frame.expr_values.pop_back().unwrap();
                            let heap_pos = match val {
                                Value::Ref(value_ref) => self.store.read_ref(value_ref).as_array(),
                                val => val.as_array(),
                            };

                            cur_frame.expr_values.push_back(Value::Ref(ValueId::Heap(heap_pos, index)));
                            self.store.drop_value(&subject);
                        },
                        Expression::Slicing(expr) => {
                            // TODO: Out of bounds checking
                            todo!("implement slicing")
                        },
                        Expression::Select(expr) => {
                            let subject= cur_frame.expr_values.pop_back().unwrap();
                            let heap_pos = match &subject {
                                Value::Ref(value_ref) => self.store.read_ref(*value_ref).as_struct(),
                                subject => subject.as_struct(),
                            };

                            cur_frame.expr_values.push_back(Value::Ref(ValueId::Heap(heap_pos, expr.field.as_symbolic().field_idx as u32)));
                            self.store.drop_value(&subject);
                        },
                        Expression::Literal(expr) => {
                            let value = match &expr.value {
                                Literal::Null => Value::Null,
                                Literal::True => Value::Bool(true),
                                Literal::False => Value::Bool(false),
                                Literal::Character(lit_value) => Value::Char(*lit_value),
                                Literal::String(lit_value) => {
                                    let heap_pos = self.store.alloc_heap();
                                    let values = &mut self.store.heap_regions[heap_pos as usize].values;
                                    let value = lit_value.as_str();
                                    debug_assert!(values.is_empty());
                                    values.reserve(value.len());
                                    for character in value.as_bytes() {
                                        debug_assert!(character.is_ascii());
                                        values.push(Value::Char(*character as char));
                                    }
                                    Value::String(heap_pos)
                                }
                                Literal::Integer(lit_value) => {
                                    use ConcreteTypePart as CTP;
                                    debug_assert_eq!(expr.concrete_type.parts.len(), 1);
                                    match expr.concrete_type.parts[0] {
                                        CTP::UInt8  => Value::UInt8(lit_value.unsigned_value as u8),
                                        CTP::UInt16 => Value::UInt16(lit_value.unsigned_value as u16),
                                        CTP::UInt32 => Value::UInt32(lit_value.unsigned_value as u32),
                                        CTP::UInt64 => Value::UInt64(lit_value.unsigned_value as u64),
                                        CTP::SInt8  => Value::SInt8(lit_value.unsigned_value as i8),
                                        CTP::SInt16 => Value::SInt16(lit_value.unsigned_value as i16),
                                        CTP::SInt32 => Value::SInt32(lit_value.unsigned_value as i32),
                                        CTP::SInt64 => Value::SInt64(lit_value.unsigned_value as i64),
                                        _ => unreachable!(),
                                    }
                                }
                                Literal::Struct(lit_value) => {
                                    let heap_pos = self.store.alloc_heap();
                                    let num_fields = lit_value.fields.len();
                                    let values = &mut self.store.heap_regions[heap_pos as usize].values;
                                    debug_assert!(values.is_empty());
                                    values.reserve(num_fields);
                                    for _ in 0..num_fields {
                                        values.push(cur_frame.expr_values.pop_front().unwrap());
                                    }
                                    Value::Struct(heap_pos)
                                }
                                Literal::Enum(lit_value) => {
                                    Value::Enum(lit_value.variant_idx as i64);
                                }
                                Literal::Union(lit_value) => {
                                    let heap_pos = self.store.alloc_heap();
                                    let num_values = lit_value.values.len();
                                    let values = &mut self.store.heap_regions[heap_pos as usize].values;
                                    debug_assert!(values.is_empty());
                                    values.reserve(num_values);
                                    for _ in 0..num_values {
                                        values.push(cur_frame.expr_values.pop_front().unwrap());
                                    }
                                    Value::Union(lit_value.variant_idx as i64, heap_pos)
                                }
                                Literal::Array(lit_value) => {
                                    let heap_pos = self.store.alloc_heap();
                                    let num_values = lit_value.len();
                                    let values = &mut self.store.heap_regions[heap_pos as usize].values;
                                    debug_assert!(values.is_empty());
                                    values.reserve(num_values);
                                    for _ in 0..num_values {
                                        values.push(cur_frame.expr_values.pop_front().unwrap())
                                    }
                                }
                            };

                            cur_frame.expr_values.push_back(value);
                        },
                        Expression::Call(expr) => {
                            // Push a new frame. Note that all expressions have
                            // been pushed to the front, so they're in the order
                            // of the definition.
                            let num_args = expr.arguments.len();

                            // Prepare stack for a new frame
                            let cur_stack_boundary = self.store.cur_stack_boundary;
                            self.store.cur_stack_boundary = self.store.stack.len();
                            self.store.stack.push(Value::PrevStackBoundary(cur_stack_boundary as isize));
                            for _ in 0..num_args {
                                self.store.stack.push(cur_frame.expr_values.pop_front().unwrap());
                            }

                            // Push the new frame
                            self.frames.push(Frame::new(heap, expr.definition));

                            // To simplify the logic a little bit we will now
                            // return and ask our caller to call us again
                            return Err(EvalContinuation::Stepping);
                        },
                        Expression::Variable(expr) => {
                            let variable = &heap[expr.declaration.unwrap()];
                            cur_frame.expr_values.push_back(Value::Ref(ValueId::Stack(variable.unique_id_in_scope as StackPos)));
                        }
                    }
                }
            }
        }

        // No (more) expressions to evaluate. So evaluate statement (that may
        // depend on the result on the last evaluated expression(s))
        let stmt = &heap[cur_frame.position];
        let return_value = match stmt {
            Statement::Block(stmt) => {
                // Reserve space on stack, but also make sure excess stack space
                // is cleared
                self.store.clear_stack(stmt.first_unique_id_in_scope as usize);
                self.store.reserve_stack(stmt.next_unique_id_in_scope as usize);
                cur_frame.position = stmt.statements[0];

                Ok(())
            },
            Statement::Local(stmt) => {
                match stmt {
                    LocalStatement::Memory(stmt) => {
                        let variable = &heap[stmt.variable];
                        self.store.write(ValueId::Stack(variable.unique_id_in_scope as u32), Value::Unassigned);

                        cur_frame.position = stmt.next;
                    },
                    LocalStatement::Channel(stmt) => {
                        let [from_value, to_value] = ctx.new_channel();
                        self.store.write(ValueId::Stack(heap[stmt.from].unique_id_in_scope as u32), from_value);
                        self.store.write(ValueId::Stack(heap[stmt.to].unique_id_in_scope as u32), to_value);

                        cur_frame.position = stmt.next;
                    }
                }

                Ok(())
            },
            Statement::Labeled(stmt) => {
                cur_frame.position = stmt.body;

                Ok(())
            },
            Statement::If(stmt) => {
                debug_assert_eq!(cur_frame.expr_values.len(), 1, "expected one expr value for if statement");
                let test_value = cur_frame.expr_values.pop_back().unwrap().as_bool();
                if test_value {
                    cur_frame.position = stmt.true_body.upcast();
                } else if let Some(false_body) = stmt.false_body {
                    cur_frame.position = false_body.upcast();
                } else {
                    // Not true, and no false body
                    cur_frame.position = stmt.end_if.unwrap().upcast();
                }

                Ok(())
            },
            Statement::EndIf(stmt) => {
                cur_frame.position = stmt.next;
                Ok(())
            },
            Statement::While(stmt) => {
                debug_assert_eq!(cur_frame.expr_values.len(), 1, "expected one expr value for while statement");
                let test_value = cur_frame.expr_values.pop_back().unwrap().as_bool();
                if test_value {
                    cur_frame.position = stmt.body.upcast();
                } else {
                    cur_frame.position = stmt.end_while.unwrap().upcast();
                }

                Ok(())
            },
            Statement::EndWhile(stmt) => {
                cur_frame.position = stmt.next;

                Ok(())
            },
            Statement::Break(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(())
            },
            Statement::Continue(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(())
            },
            Statement::Synchronous(stmt) => {
                cur_frame.position = stmt.body.upcast();

                Err(EvalContinuation::SyncBlockStart)
            },
            Statement::EndSynchronous(stmt) => {
                cur_frame.position = stmt.next;

                Err(EvalContinuation::SyncBlockEnd)
            },
            Statement::Return(stmt) => {
                debug_assert!(heap[cur_frame.definition].is_function());
                debug_assert_eq!(cur_frame.expr_values.len(), 1, "expected one expr value for return statement");

                // Clear any values in the current stack frame
                self.store.clear_stack(0);

                // The preceding frame has executed a call, so is expecting the
                // return expression on its expression value stack.
                let return_value = cur_frame.expr_values.pop_back().unwrap();
                let prev_stack_idx = self.store.stack[self.store.cur_stack_boundary].as_stack_boundary();
                debug_assert!(prev_stack_idx >= 0);
                self.store.cur_stack_boundary = prev_stack_idx as usize;
                self.frames.pop();
                let cur_frame = self.frames.last_mut().unwrap();
                cur_frame.expr_values.push_back(return_value);

                // Immediately return, we don't care about the current frame
                // anymore and there is nothing left to evaluate
                return Ok(());
            },
            Statement::Goto(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(())
            },
            Statement::New(stmt) => {
                let call_expr = &heap[stmt.expression];
                debug_assert!(heap[call_expr.definition].is_component());
                debug_assert_eq!(
                    cur_frame.expr_values.len(), heap[call_expr.definition].parameters().len(),
                    "mismatch in expr stack size and number of arguments for new statement"
                );

                // Note that due to expression value evaluation they exist in
                // reverse order on the stack.
                // TODO: Revise this code, keep it as is to be compatible with current runtime
                let mut args = Vec::new();
                while let Some(value) = cur_frame.expr_values.pop_front() {
                    args.push(value);
                }

                cur_frame.position = stmt.next;

                todo!("Make sure this is handled correctly, transfer 'heap' values to another Prompt");
                Err(EvalContinuation::NewComponent(call_expr.definition, args))
            },
            Statement::Expression(stmt) => {
                // The expression has just been completely evaluated. Some
                // values might have remained on the expression value stack.
                cur_frame.expr_values.clear();
                cur_frame.position = stmt.next;

                Ok(())
            },
        };

        // If the next statement requires evaluating expressions then we push
        // these onto the expression stack. This way we will evaluate this
        // stack in the next loop, then evaluate the statement using the result
        // from the expression evaluation.
        if !cur_frame.position.is_invalid() {
            let stmt = &heap[cur_frame.position];

            match stmt {
                Statement::If(stmt) => cur_frame.prepare_single_expression(heap, stmt.test),
                Statement::While(stmt) => cur_frame.prepare_single_expression(heap, stmt.test),
                Statement::Return(stmt) => {
                    debug_assert_eq!(stmt.expressions.len(), 1); // TODO: @ReturnValues
                    cur_frame.prepare_single_expression(heap, stmt.expressions[0]);
                },
                Statement::New(stmt) => {
                    // Note that we will end up not evaluating the call itself.
                    // Rather we will evaluate its expressions and then
                    // instantiate the component upon reaching the "new" stmt.
                    let call_expr = &heap[stmt.expression];
                    cur_frame.prepare_multiple_expressions(heap, &call_expr.arguments);
                },
                Statement::Expression(stmt) => {
                    cur_frame.prepare_single_expression(heap, stmt.expression);
                }
                _ => {},
            }
        }

        return_value
    }
}
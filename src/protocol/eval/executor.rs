
use std::collections::VecDeque;

use super::value::*;
use super::store::*;
use super::error::*;
use crate::protocol::*;
use crate::protocol::ast::*;
use crate::protocol::type_table::*;

macro_rules! debug_enabled { () => { false }; }
macro_rules! debug_log {
    ($format:literal) => {
        enabled_debug_print!(false, "exec", $format);
    };
    ($format:literal, $($args:expr),*) => {
        enabled_debug_print!(false, "exec", $format, $($args),*);
    };
}

#[derive(Debug, Clone)]
pub(crate) enum ExprInstruction {
    EvalExpr(ExpressionId),
    PushValToFront,
}

#[derive(Debug, Clone)]
pub(crate) struct Frame {
    pub(crate) definition: DefinitionId,
    pub(crate) monomorph_idx: i32,
    pub(crate) position: StatementId,
    pub(crate) expr_stack: VecDeque<ExprInstruction>, // hack for expression evaluation, evaluated by popping from back
    pub(crate) expr_values: VecDeque<Value>, // hack for expression results, evaluated by popping from front/back
}

impl Frame {
    /// Creates a new execution frame. Does not modify the stack in any way.
    pub fn new(heap: &Heap, definition_id: DefinitionId, monomorph_idx: i32) -> Self {
        let definition = &heap[definition_id];
        let first_statement = match definition {
            Definition::Component(definition) => definition.body,
            Definition::Function(definition) => definition.body,
            _ => unreachable!("initializing frame with {:?} instead of a function/component", definition),
        };

        Frame{
            definition: definition_id,
            monomorph_idx,
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
                        // Note: fields expressions are evaluated in programmer-
                        // specified order. But struct construction expects them
                        // in type-defined order. I might want to come back to
                        // this.
                        let mut _num_pushed = 0;
                        for want_field_idx in 0..literal.fields.len() {
                            for field in &literal.fields {
                                if field.field_idx == want_field_idx {
                                    _num_pushed += 1;
                                    self.expr_stack.push_back(ExprInstruction::PushValToFront);
                                    self.serialize_expression(heap, field.value);
                                }
                            }
                        }
                        debug_assert_eq!(_num_pushed, literal.fields.len())
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

type EvalResult = Result<EvalContinuation, EvalError>;

pub enum EvalContinuation {
    Stepping,
    Inconsistent,
    Terminal,
    SyncBlockStart,
    SyncBlockEnd,
    NewComponent(DefinitionId, i32, ValueGroup),
    BlockFires(Value),
    BlockGet(Value),
    Put(Value, Value),
}

// Note: cloning is fine, methinks. cloning all values and the heap regions then
// we end up with valid "pointers" to heap regions.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub(crate) frames: Vec<Frame>,
    pub(crate) store: Store,
}

impl Prompt {
    pub fn new(_types: &TypeTable, heap: &Heap, def: DefinitionId, monomorph_idx: i32, args: ValueGroup) -> Self {
        let mut prompt = Self{
            frames: Vec::new(),
            store: Store::new(),
        };

        // Maybe do typechecking in the future?
        debug_assert!((monomorph_idx as usize) < _types.get_base_definition(&def).unwrap().definition.procedure_monomorphs().len());
        prompt.frames.push(Frame::new(heap, def, monomorph_idx));
        args.into_store(&mut prompt.store);

        prompt
    }

    pub(crate) fn step(&mut self, types: &TypeTable, heap: &Heap, modules: &[Module], ctx: &mut EvalContext) -> EvalResult {
        // Helper function to transfer multiple values from the expression value
        // array into a heap region (e.g. constructing arrays or structs).
        fn transfer_expression_values_front_into_heap(cur_frame: &mut Frame, store: &mut Store, num_values: usize) -> HeapPos {
            let heap_pos = store.alloc_heap();

            // Do the transformation first (because Rust...)
            for val_idx in 0..num_values {
                cur_frame.expr_values[val_idx] = store.read_take_ownership(cur_frame.expr_values[val_idx].clone());
            }

            // And now transfer to the heap region
            let values = &mut store.heap_regions[heap_pos as usize].values;
            debug_assert!(values.is_empty());
            values.reserve(num_values);
            for _ in 0..num_values {
                values.push(cur_frame.expr_values.pop_front().unwrap());
            }

            heap_pos
        }

        // Helper function to make sure that an index into an aray is valid.
        fn array_inclusive_index_is_invalid(store: &Store, array_heap_pos: u32, idx: i64) -> bool {
            let array_len = store.heap_regions[array_heap_pos as usize].values.len();
            return idx < 0 || idx >= array_len as i64;
        }

        fn array_exclusive_index_is_invalid(store: &Store, array_heap_pos: u32, idx: i64) -> bool {
            let array_len = store.heap_regions[array_heap_pos as usize].values.len();
            return idx < 0 || idx > array_len as i64;
        }

        fn construct_array_error(prompt: &Prompt, modules: &[Module], heap: &Heap, expr_id: ExpressionId, heap_pos: u32, idx: i64) -> EvalError {
            let array_len = prompt.store.heap_regions[heap_pos as usize].values.len();
            return EvalError::new_error_at_expr(
                prompt, modules, heap, expr_id,
                format!("index {} is out of bounds: array length is {}", idx, array_len)
            )
        }

        // Checking if we're at the end of execution
        let cur_frame = self.frames.last_mut().unwrap();
        if cur_frame.position.is_invalid() {
            if heap[cur_frame.definition].is_function() {
                todo!("End of function without return, return an evaluation error");
            }
            return Ok(EvalContinuation::Terminal);
        }

        debug_log!("Taking step in '{}'", heap[cur_frame.definition].identifier().value.as_str());

        // Execute all pending expressions
        while !cur_frame.expr_stack.is_empty() {
            let next = cur_frame.expr_stack.pop_back().unwrap();
            debug_log!("Expr stack: {:?}", next);
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

                            // Note: although not pretty, the assignment operator takes ownership
                            // of the right-hand side value when possible. So we do not drop the
                            // rhs's optionally owned heap data.
                            let rhs = self.store.read_take_ownership(rhs);
                            apply_assignment_operator(&mut self.store, to, expr.operation, rhs);
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
                            self.store.drop_value(lhs.get_heap_pos());
                            self.store.drop_value(rhs.get_heap_pos());
                        },
                        Expression::Unary(expr) => {
                            let val = cur_frame.expr_values.pop_back().unwrap();
                            let result = apply_unary_operator(&mut self.store, expr.operation, &val);
                            cur_frame.expr_values.push_back(result);
                            self.store.drop_value(val.get_heap_pos());
                        },
                        Expression::Indexing(expr) => {
                            // Evaluate index. Never heap allocated so we do
                            // not have to drop it.
                            let index = cur_frame.expr_values.pop_back().unwrap();
                            let index = self.store.maybe_read_ref(&index);

                            debug_assert!(index.is_integer());
                            let index = if index.is_signed_integer() {
                                index.as_signed_integer() as i64
                            } else {
                                index.as_unsigned_integer() as i64
                            };

                            let subject = cur_frame.expr_values.pop_back().unwrap();

                            let (deallocate_heap_pos, value_to_push, subject_heap_pos) = match subject {
                                Value::Ref(value_ref) => {
                                    // Our expression stack value is a reference to something that
                                    // exists in the normal stack/heap. We don't want to deallocate
                                    // this thing. Rather we want to return a reference to it.
                                    let subject = self.store.read_ref(value_ref);
                                    let subject_heap_pos = subject.as_array();

                                    if array_inclusive_index_is_invalid(&self.store, subject_heap_pos, index) {
                                        return Err(construct_array_error(self, modules, heap, expr_id, subject_heap_pos, index));
                                    }

                                    (None, Value::Ref(ValueId::Heap(subject_heap_pos, index as u32)), subject_heap_pos)
                                },
                                _ => {
                                    // Our value lives on the expression stack, hence we need to
                                    // clone whatever we're referring to. Then drop the subject.
                                    let subject_heap_pos = subject.as_array();

                                    if array_inclusive_index_is_invalid(&self.store, subject_heap_pos, index) {
                                        return Err(construct_array_error(self, modules, heap, expr_id, subject_heap_pos, index));
                                    }

                                    let subject_indexed = Value::Ref(ValueId::Heap(subject_heap_pos, index as u32));
                                    (Some(subject_heap_pos), self.store.clone_value(subject_indexed), subject_heap_pos)
                                },
                            };

                            cur_frame.expr_values.push_back(value_to_push);
                            self.store.drop_value(deallocate_heap_pos);
                        },
                        Expression::Slicing(expr) => {
                            // Evaluate indices
                            let from_index = cur_frame.expr_values.pop_back().unwrap();
                            let from_index = self.store.maybe_read_ref(&from_index);
                            let to_index = cur_frame.expr_values.pop_back().unwrap();
                            let to_index = self.store.maybe_read_ref(&to_index);

                            debug_assert!(from_index.is_integer() && to_index.is_integer());
                            let from_index = if from_index.is_signed_integer() {
                                from_index.as_signed_integer()
                            } else {
                                from_index.as_unsigned_integer() as i64
                            };
                            let to_index = if to_index.is_signed_integer() {
                                to_index.as_signed_integer()
                            } else {
                                to_index.as_unsigned_integer() as i64
                            };

                            // Dereference subject if needed
                            let subject = cur_frame.expr_values.pop_back().unwrap();
                            let deref_subject = self.store.maybe_read_ref(&subject);

                            // Slicing needs to produce a copy anyway (with the
                            // current evaluator implementation)
                            let array_heap_pos = deref_subject.as_array();
                            if array_inclusive_index_is_invalid(&self.store, array_heap_pos, from_index) {
                                return Err(construct_array_error(self, modules, heap, expr.from_index, array_heap_pos, from_index));
                            }
                            if array_exclusive_index_is_invalid(&self.store, array_heap_pos, to_index) {
                                return Err(construct_array_error(self, modules, heap, expr.to_index, array_heap_pos, to_index));
                            }

                            // Again: would love to push directly, but rust...
                            let new_heap_pos = self.store.alloc_heap();
                            debug_assert!(self.store.heap_regions[new_heap_pos as usize].values.is_empty());
                            if to_index > from_index {
                                let from_index = from_index as usize;
                                let to_index = to_index as usize;
                                let mut values = Vec::with_capacity(to_index - from_index);
                                for idx in from_index..to_index {
                                    let value = self.store.heap_regions[array_heap_pos as usize].values[idx].clone();
                                    values.push(self.store.clone_value(value));
                                }

                                self.store.heap_regions[new_heap_pos as usize].values = values;

                            } // else: empty range

                            cur_frame.expr_values.push_back(Value::Array(new_heap_pos));

                            // Dropping the original subject, because we don't
                            // want to drop something on the stack
                            self.store.drop_value(subject.get_heap_pos());
                        },
                        Expression::Select(expr) => {
                            let subject= cur_frame.expr_values.pop_back().unwrap();
                            let mono_data = types.get_procedure_expression_data(&cur_frame.definition, cur_frame.monomorph_idx);
                            let field_idx = mono_data.expr_data[expr.unique_id_in_definition as usize].field_or_monomorph_idx as u32;

                            // Note: same as above: clone if value lives on expr stack, simply
                            // refer to it if it already lives on the stack/heap.
                            let (deallocate_heap_pos, value_to_push) = match subject {
                                Value::Ref(value_ref) => {
                                    let subject = self.store.read_ref(value_ref);
                                    let subject_heap_pos = subject.as_struct();

                                    (None, Value::Ref(ValueId::Heap(subject_heap_pos, field_idx)))
                                },
                                _ => {
                                    let subject_heap_pos = subject.as_struct();
                                    let subject_indexed = Value::Ref(ValueId::Heap(subject_heap_pos, field_idx));
                                    (Some(subject_heap_pos), self.store.clone_value(subject_indexed))
                                },
                            };

                            cur_frame.expr_values.push_back(value_to_push);
                            self.store.drop_value(deallocate_heap_pos);
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
                                    let def_types = types.get_procedure_expression_data(&cur_frame.definition, cur_frame.monomorph_idx);
                                    let concrete_type = &def_types.expr_data[expr.unique_id_in_definition as usize].expr_type;

                                    debug_assert_eq!(concrete_type.parts.len(), 1);
                                    match concrete_type.parts[0] {
                                        CTP::UInt8  => Value::UInt8(lit_value.unsigned_value as u8),
                                        CTP::UInt16 => Value::UInt16(lit_value.unsigned_value as u16),
                                        CTP::UInt32 => Value::UInt32(lit_value.unsigned_value as u32),
                                        CTP::UInt64 => Value::UInt64(lit_value.unsigned_value as u64),
                                        CTP::SInt8  => Value::SInt8(lit_value.unsigned_value as i8),
                                        CTP::SInt16 => Value::SInt16(lit_value.unsigned_value as i16),
                                        CTP::SInt32 => Value::SInt32(lit_value.unsigned_value as i32),
                                        CTP::SInt64 => Value::SInt64(lit_value.unsigned_value as i64),
                                        _ => unreachable!("got concrete type {:?} for integer literal at expr {:?}", concrete_type, expr_id),
                                    }
                                }
                                Literal::Struct(lit_value) => {
                                    let heap_pos = transfer_expression_values_front_into_heap(
                                        cur_frame, &mut self.store, lit_value.fields.len()
                                    );
                                    Value::Struct(heap_pos)
                                }
                                Literal::Enum(lit_value) => {
                                    Value::Enum(lit_value.variant_idx as i64)
                                }
                                Literal::Union(lit_value) => {
                                    let heap_pos = transfer_expression_values_front_into_heap(
                                        cur_frame, &mut self.store, lit_value.values.len()
                                    );
                                    Value::Union(lit_value.variant_idx as i64, heap_pos)
                                }
                                Literal::Array(lit_value) => {
                                    let heap_pos = transfer_expression_values_front_into_heap(
                                        cur_frame, &mut self.store, lit_value.len()
                                    );
                                    Value::Array(heap_pos)
                                }
                            };

                            cur_frame.expr_values.push_back(value);
                        },
                        Expression::Call(expr) => {
                            // Push a new frame. Note that all expressions have
                            // been pushed to the front, so they're in the order
                            // of the definition.
                            let num_args = expr.arguments.len();

                            // Determine stack boundaries
                            let cur_stack_boundary = self.store.cur_stack_boundary;
                            let new_stack_boundary = self.store.stack.len();

                            // Push new boundary and function arguments for new frame
                            self.store.stack.push(Value::PrevStackBoundary(cur_stack_boundary as isize));
                            for _ in 0..num_args {
                                let argument = self.store.read_take_ownership(cur_frame.expr_values.pop_front().unwrap());
                                self.store.stack.push(argument);
                            }

                            // Determine the monomorph index of the function we're calling
                            let mono_data = types.get_procedure_expression_data(&cur_frame.definition, cur_frame.monomorph_idx);
                            let call_data = &mono_data.expr_data[expr.unique_id_in_definition as usize];

                            // Push the new frame
                            self.frames.push(Frame::new(heap, expr.definition, call_data.field_or_monomorph_idx));
                            self.store.cur_stack_boundary = new_stack_boundary;

                            // To simplify the logic a little bit we will now
                            // return and ask our caller to call us again
                            return Ok(EvalContinuation::Stepping);
                        },
                        Expression::Variable(expr) => {
                            let variable = &heap[expr.declaration.unwrap()];
                            cur_frame.expr_values.push_back(Value::Ref(ValueId::Stack(variable.unique_id_in_scope as StackPos)));
                        }
                    }
                }
            }
        }

        debug_log!("Frame [{:?}] at {:?}, stack size = {}", cur_frame.definition, cur_frame.position, self.store.stack.len());
        if debug_enabled!() {
            debug_log!("Stack:");
            for (stack_idx, stack_val) in self.store.stack.iter().enumerate() {
                debug_log!("  [{:03}] {:?}", stack_idx, stack_val);
            }

            debug_log!("Heap:");
            for (heap_idx, heap_region) in self.store.heap_regions.iter().enumerate() {
                let is_free = self.store.free_regions.iter().any(|idx| *idx as usize == heap_idx);
                debug_log!("  [{:03}] in_use: {}, len: {}, vals: {:?}", heap_idx, !is_free, heap_region.values.len(), &heap_region.values);
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

                Ok(EvalContinuation::Stepping)
            },
            Statement::EndBlock(stmt) => {
                let block = &heap[stmt.start_block];
                self.store.clear_stack(block.first_unique_id_in_scope as usize);
                cur_frame.position = stmt.next;

                Ok(EvalContinuation::Stepping)
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

                Ok(EvalContinuation::Stepping)
            },
            Statement::Labeled(stmt) => {
                cur_frame.position = stmt.body;

                Ok(EvalContinuation::Stepping)
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
                    cur_frame.position = stmt.end_if.upcast();
                }

                Ok(EvalContinuation::Stepping)
            },
            Statement::EndIf(stmt) => {
                cur_frame.position = stmt.next;
                Ok(EvalContinuation::Stepping)
            },
            Statement::While(stmt) => {
                debug_assert_eq!(cur_frame.expr_values.len(), 1, "expected one expr value for while statement");
                let test_value = cur_frame.expr_values.pop_back().unwrap().as_bool();
                if test_value {
                    cur_frame.position = stmt.body.upcast();
                } else {
                    cur_frame.position = stmt.end_while.upcast();
                }

                Ok(EvalContinuation::Stepping)
            },
            Statement::EndWhile(stmt) => {
                cur_frame.position = stmt.next;

                Ok(EvalContinuation::Stepping)
            },
            Statement::Break(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(EvalContinuation::Stepping)
            },
            Statement::Continue(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(EvalContinuation::Stepping)
            },
            Statement::Synchronous(stmt) => {
                cur_frame.position = stmt.body.upcast();

                Ok(EvalContinuation::SyncBlockStart)
            },
            Statement::EndSynchronous(stmt) => {
                cur_frame.position = stmt.next;

                Ok(EvalContinuation::SyncBlockEnd)
            },
            Statement::Return(stmt) => {
                debug_assert!(heap[cur_frame.definition].is_function());
                debug_assert_eq!(cur_frame.expr_values.len(), 1, "expected one expr value for return statement");

                // The preceding frame has executed a call, so is expecting the
                // return expression on its expression value stack. Note that
                // we may be returning a reference to something on our stack,
                // so we need to read that value and clone it.
                let return_value = cur_frame.expr_values.pop_back().unwrap();
                let return_value = match return_value {
                    Value::Ref(value_id) => self.store.read_copy(value_id),
                    _ => return_value,
                };

                // Pre-emptively pop our stack frame
                self.frames.pop();

                // Clean up our section of the stack
                self.store.clear_stack(0);
                let prev_stack_idx = self.store.stack.pop().unwrap().as_stack_boundary();

                // TODO: Temporary hack for testing, remove at some point
                if self.frames.is_empty() {
                    debug_assert!(prev_stack_idx == -1);
                    debug_assert!(self.store.stack.len() == 0);
                    self.store.stack.push(return_value);
                    return Ok(EvalContinuation::Terminal);
                }

                debug_assert!(prev_stack_idx >= 0);
                // Return to original state of stack frame
                self.store.cur_stack_boundary = prev_stack_idx as usize;
                let cur_frame = self.frames.last_mut().unwrap();
                cur_frame.expr_values.push_back(return_value);

                // We just returned to the previous frame, which might be in
                // the middle of evaluating expressions for a particular
                // statement. So we don't want to enter the code below.
                return Ok(EvalContinuation::Stepping);
            },
            Statement::Goto(stmt) => {
                cur_frame.position = stmt.target.unwrap().upcast();

                Ok(EvalContinuation::Stepping)
            },
            Statement::New(stmt) => {
                let call_expr = &heap[stmt.expression];
                debug_assert!(heap[call_expr.definition].is_component());
                debug_assert_eq!(
                    cur_frame.expr_values.len(), heap[call_expr.definition].parameters().len(),
                    "mismatch in expr stack size and number of arguments for new statement"
                );

                let mono_data = types.get_procedure_expression_data(&cur_frame.definition, cur_frame.monomorph_idx);
                let expr_data = &mono_data.expr_data[call_expr.unique_id_in_definition as usize];

                // Note that due to expression value evaluation they exist in
                // reverse order on the stack.
                // TODO: Revise this code, keep it as is to be compatible with current runtime
                let mut args = Vec::new();
                while let Some(value) = cur_frame.expr_values.pop_front() {
                    args.push(value);
                }

                // Construct argument group, thereby copying heap regions
                let argument_group = ValueGroup::from_store(&self.store, &args);

                // Clear any heap regions
                for arg in &args {
                    self.store.drop_value(arg.get_heap_pos());
                }

                cur_frame.position = stmt.next;

                todo!("Make sure this is handled correctly, transfer 'heap' values to another Prompt");
                Ok(EvalContinuation::NewComponent(call_expr.definition, expr_data.field_or_monomorph_idx, argument_group))
            },
            Statement::Expression(stmt) => {
                // The expression has just been completely evaluated. Some
                // values might have remained on the expression value stack.
                cur_frame.expr_values.clear();
                cur_frame.position = stmt.next;

                Ok(EvalContinuation::Stepping)
            },
        };

        assert!(
            cur_frame.expr_values.is_empty(),
            "This is a debugging assertion that will fail if you perform expressions without \
            assigning to anything. This should be completely valid, and this assertion should be \
            replaced by something that clears the expression values if needed, but I'll keep this \
            in for now for debugging purposes."
        );

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
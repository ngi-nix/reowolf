mod arena;
mod eval;
pub(crate) mod input_source;
mod parser;
#[cfg(test)] mod tests;

pub(crate) mod ast;
pub(crate) mod ast_printer;

use crate::common::*;
use crate::protocol::ast::*;
use crate::protocol::eval::*;
use crate::protocol::input_source::*;
use crate::protocol::parser::*;

/// Description of a protocol object, used to configure new connectors.
#[repr(C)]
pub struct ProtocolDescription {
    heap: Heap,
    source: InputSource,
    root: RootId,
}
#[derive(Debug, Clone)]
pub(crate) struct ComponentState {
    prompt: Prompt,
}
pub(crate) enum EvalContext<'a> {
    Nonsync(&'a mut NonsyncProtoContext<'a>),
    Sync(&'a mut SyncProtoContext<'a>),
    None,
}
//////////////////////////////////////////////

impl std::fmt::Debug for ProtocolDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "(An opaque protocol description)")
    }
}
impl ProtocolDescription {
    // TODO: Allow for multi-file compilation
    pub fn parse(buffer: &[u8]) -> Result<Self, String> {
        // TODO: @fixme, keep code compilable, but needs support for multiple
        //  input files.
        let source = InputSource::new(String::new(), Vec::from(buffer));
        let mut parser = Parser::new();
        parser.feed(source).expect("failed to feed source");
        
        if let Err(err) = parser.parse() {
            println!("ERROR:\n{}", err);
            return Err(format!("{}", err))
        }

        debug_assert_eq!(parser.modules.len(), 1, "only supporting one module here for now");
        let module = parser.modules.remove(0);
        let root = module.root_id;
        let source = module.source;
        return Ok(ProtocolDescription { heap: parser.heap, source, root });
    }
    pub(crate) fn component_polarities(
        &self,
        identifier: &[u8],
    ) -> Result<Vec<Polarity>, AddComponentError> {
        use AddComponentError::*;
        let h = &self.heap;
        let root = &h[self.root];
        let def = root.get_definition_ident(h, identifier);
        if def.is_none() {
            return Err(NoSuchComponent);
        }
        let def = &h[def.unwrap()];
        if !def.is_component() {
            return Err(NoSuchComponent);
        }
        for &param in def.parameters().iter() {
            let param = &h[param];
            let first_element = &param.parser_type.elements[0];

            match first_element.variant {
                ParserTypeVariant::Input | ParserTypeVariant::Output => continue,
                _ => {
                    return Err(NonPortTypeParameters);
                }
            }
        }
        let mut result = Vec::new();
        for &param in def.parameters().iter() {
            let param = &h[param];
            let first_element = &param.parser_type.elements[0];

            if first_element.variant == ParserTypeVariant::Input {
                result.push(Polarity::Getter)
            } else if first_element.variant == ParserTypeVariant::Output {
                result.push(Polarity::Putter)
            } else {
                unreachable!()
            }
        }
        Ok(result)
    }
    // expects port polarities to be correct
    pub(crate) fn new_component(&self, identifier: &[u8], ports: &[PortId]) -> ComponentState {
        let mut args = Vec::new();
        for (&x, y) in ports.iter().zip(self.component_polarities(identifier).unwrap()) {
            match y {
                Polarity::Getter => args.push(Value::Input(x)),
                Polarity::Putter => args.push(Value::Output(x)),
            }
        }
        let h = &self.heap;
        let root = &h[self.root];
        let def = root.get_definition_ident(h, identifier).unwrap();
        ComponentState { prompt: Prompt::new(h, def, ValueGroup::new_stack(args)) }
    }
}
impl ComponentState {
    pub(crate) fn nonsync_run<'a: 'b, 'b>(
        &'a mut self,
        context: &'b mut NonsyncProtoContext<'b>,
        pd: &'a ProtocolDescription,
    ) -> NonsyncBlocker {
        let mut context = EvalContext::Nonsync(context);
        loop {
            let result = self.prompt.step(&pd.heap, &mut context);
            match result {
                Err(_) => todo!("error handling"),
                Ok(cont) => match cont {
                    EvalContinuation::Stepping => continue,
                    EvalContinuation::Inconsistent => return NonsyncBlocker::Inconsistent,
                    EvalContinuation::Terminal => return NonsyncBlocker::ComponentExit,
                    EvalContinuation::SyncBlockStart => return NonsyncBlocker::SyncBlockStart,
                    // Not possible to end sync block if never entered one
                    EvalContinuation::SyncBlockEnd => unreachable!(),
                    EvalContinuation::NewComponent(definition_id, args) => {
                        // Look up definition (TODO for now, assume it is a definition)
                        let mut moved_ports = HashSet::new();
                        for arg in args.values.iter() {
                            match arg {
                                Value::Output(port) => {
                                    moved_ports.insert(*port);
                                }
                                Value::Input(port) => {
                                    moved_ports.insert(*port);
                                }
                                _ => {}
                            }
                        }
                        for region in args.regions.iter() {
                            for arg in region {
                                match arg {
                                    Value::Output(port) => { moved_ports.insert(*port); },
                                    Value::Input(port) => { moved_ports.insert(*port); },
                                    _ => {},
                                }
                            }
                        }
                        let h = &pd.heap;
                        let init_state = ComponentState { prompt: Prompt::new(h, definition_id, args) };
                        context.new_component(moved_ports, init_state);
                        // Continue stepping
                        continue;
                    }
                    // Outside synchronous blocks, no fires/get/put happens
                    EvalContinuation::BlockFires(_) => unreachable!(),
                    EvalContinuation::BlockGet(_) => unreachable!(),
                    EvalContinuation::Put(_, _) => unreachable!(),
                },
            }
        }
    }

    pub(crate) fn sync_run<'a: 'b, 'b>(
        &'a mut self,
        context: &'b mut SyncProtoContext<'b>,
        pd: &'a ProtocolDescription,
    ) -> SyncBlocker {
        let mut context = EvalContext::Sync(context);
        loop {
            let result = self.prompt.step(&pd.heap, &mut context);
            match result {
                Err(_) => todo!("error handling"),
                Ok(cont) => match cont {
                    EvalContinuation::Stepping => continue,
                    EvalContinuation::Inconsistent => return SyncBlocker::Inconsistent,
                    // First need to exit synchronous block before definition may end
                    EvalContinuation::Terminal => unreachable!(),
                    // No nested synchronous blocks
                    EvalContinuation::SyncBlockStart => unreachable!(),
                    EvalContinuation::SyncBlockEnd => return SyncBlocker::SyncBlockEnd,
                    // Not possible to create component in sync block
                    EvalContinuation::NewComponent(_, _) => unreachable!(),
                    EvalContinuation::BlockFires(port) => match port {
                        Value::Output(port) => {
                            return SyncBlocker::CouldntCheckFiring(port);
                        }
                        Value::Input(port) => {
                            return SyncBlocker::CouldntCheckFiring(port);
                        }
                        _ => unreachable!(),
                    },
                    EvalContinuation::BlockGet(port) => match port {
                        Value::Output(port) => {
                            return SyncBlocker::CouldntReadMsg(port);
                        }
                        Value::Input(port) => {
                            return SyncBlocker::CouldntReadMsg(port);
                        }
                        _ => unreachable!(),
                    },
                    EvalContinuation::Put(port, message) => {
                        let value;
                        match port {
                            Value::Output(port_value) => {
                                value = port_value;
                            }
                            Value::Input(port_value) => {
                                value = port_value;
                            }
                            _ => unreachable!(),
                        }
                        let payload;
                        match message {
                            Value::Null => {
                                return SyncBlocker::Inconsistent;
                            },
                            Value::Message(heap_pos) => {
                                // Create a copy of the payload
                                let values = &self.prompt.store.heap_regions[heap_pos as usize].values;
                                let mut bytes = Vec::with_capacity(values.len());
                                for value in values {
                                    bytes.push(value.as_uint8());
                                }
                                payload = Payload(Arc::new(bytes));
                            }
                            _ => unreachable!(),
                        }
                        return SyncBlocker::PutMsg(value, payload);
                    }
                },
            }
        }
    }
}
impl EvalContext<'_> {
    // fn random(&mut self) -> LongValue {
    //     match self {
    //         // EvalContext::None => unreachable!(),
    //         EvalContext::Nonsync(_context) => todo!(),
    //         EvalContext::Sync(_) => unreachable!(),
    //     }
    // }
    fn new_component(&mut self, moved_ports: HashSet<PortId>, init_state: ComponentState) -> () {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(context) => {
                context.new_component(moved_ports, init_state)
            }
            EvalContext::Sync(_) => unreachable!(),
        }
    }
    fn new_channel(&mut self) -> [Value; 2] {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(context) => {
                let [from, to] = context.new_port_pair();
                let from = Value::Output(from);
                let to = Value::Input(to);
                return [from, to];
            }
            EvalContext::Sync(_) => unreachable!(),
        }
    }
    fn fires(&mut self, port: Value) -> Option<Value> {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(_) => unreachable!(),
            EvalContext::Sync(context) => match port {
                Value::Output(port) => context.is_firing(port).map(Value::Bool),
                Value::Input(port) => context.is_firing(port).map(Value::Bool),
                _ => unreachable!(),
            },
        }
    }
    fn get(&mut self, port: Value, store: &mut Store) -> Option<Value> {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(_) => unreachable!(),
            EvalContext::Sync(context) => match port {
                Value::Output(port) => {
                    debug_assert!(false, "Getting from an output port? Am I mad?");
                    unreachable!();
                }
                Value::Input(port) => {
                    let heap_pos = store.alloc_heap();
                    let heap_pos_usize = heap_pos as usize;

                    let payload = context.read_msg(port);
                    if payload.is_none() { return None; }

                    let payload = payload.unwrap();
                    store.heap_regions[heap_pos_usize].values.reserve(payload.0.len());
                    for value in payload.0.iter() {
                        store.heap_regions[heap_pos_usize].values.push(Value::UInt8(*value));
                    }

                    return Some(Value::Message(heap_pos));
                }
                _ => unreachable!(),
            },
        }
    }
    fn did_put(&mut self, port: Value) -> bool {
        match self {
            EvalContext::None => unreachable!("did_put in None context"),
            EvalContext::Nonsync(_) => unreachable!("did_put in nonsync context"),
            EvalContext::Sync(context) => match port {
                Value::Output(port) => {
                    context.did_put_or_get(port)
                },
                Value::Input(_) => unreachable!("did_put on input port"),
                _ => unreachable!("did_put on non-port value")
            }
        }
    }
}

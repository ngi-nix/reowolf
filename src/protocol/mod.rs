mod arena;
mod eval;
pub(crate) mod input_source;
mod parser;
#[cfg(test)] mod tests;

pub(crate) mod ast;
pub(crate) mod ast_printer;

use std::sync::Mutex;

use crate::collections::{StringPool, StringRef};
use crate::common::*;
use crate::protocol::ast::*;
use crate::protocol::eval::*;
use crate::protocol::input_source::*;
use crate::protocol::parser::*;
use crate::protocol::type_table::*;

/// A protocol description module
pub struct Module {
    pub(crate) source: InputSource,
    pub(crate) root_id: RootId,
    pub(crate) name: Option<StringRef<'static>>,
}
/// Description of a protocol object, used to configure new connectors.
#[repr(C)]
pub struct ProtocolDescription {
    modules: Vec<Module>,
    heap: Heap,
    types: TypeTable,
    pool: Mutex<StringPool>,
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
        let modules: Vec<Module> = parser.modules.into_iter()
            .map(|module| Module{
                source: module.source,
                root_id: module.root_id,
                name: module.name.map(|(_, name)| name)
            })
            .collect();

        return Ok(ProtocolDescription {
            modules,
            heap: parser.heap,
            types: parser.type_table,
            pool: Mutex::new(parser.string_pool),
        });
    }
    pub(crate) fn component_polarities(
        &self,
        module_name: &[u8],
        identifier: &[u8],
    ) -> Result<Vec<Polarity>, AddComponentError> {
        use AddComponentError::*;

        let module_root = self.lookup_module_root(module_name);
        if module_root.is_none() {
            return Err(AddComponentError::NoSuchModule);
        }
        let module_root = module_root.unwrap();

        let root = &self.heap[module_root];
        let def = root.get_definition_ident(&self.heap, identifier);
        if def.is_none() {
            return Err(NoSuchComponent);
        }

        let def = &self.heap[def.unwrap()];
        if !def.is_component() {
            return Err(NoSuchComponent);
        }

        for &param in def.parameters().iter() {
            let param = &self.heap[param];
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
            let param = &self.heap[param];
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
    pub(crate) fn new_component(&self, module_name: &[u8], identifier: &[u8], ports: &[PortId]) -> ComponentState {
        let mut args = Vec::new();
        for (&x, y) in ports.iter().zip(self.component_polarities(module_name, identifier).unwrap()) {
            match y {
                Polarity::Getter => args.push(Value::Input(x)),
                Polarity::Putter => args.push(Value::Output(x)),
            }
        }

        let module_root = self.lookup_module_root(module_name).unwrap();
        let root = &self.heap[module_root];
        let def = root.get_definition_ident(&self.heap, identifier).unwrap();
        ComponentState { prompt: Prompt::new(&self.types, &self.heap, def, 0, ValueGroup::new_stack(args)) }
    }

    fn lookup_module_root(&self, module_name: &[u8]) -> Option<RootId> {
        for module in self.modules.iter() {
            match &module.name {
                Some(name) => if name.as_bytes() == module_name {
                    return Some(module.root_id);
                },
                None => if module_name.is_empty() {
                    return Some(module.root_id);
                }
            }
        }

        return None;
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
            let result = self.prompt.step(&pd.types, &pd.heap, &pd.modules, &mut context);
            match result {
                Err(err) => {
                    println!("Evaluation error:\n{}", err);
                    panic!("proper error handling when component fails");
                },
                Ok(cont) => match cont {
                    EvalContinuation::Stepping => continue,
                    EvalContinuation::Inconsistent => return NonsyncBlocker::Inconsistent,
                    EvalContinuation::Terminal => return NonsyncBlocker::ComponentExit,
                    EvalContinuation::SyncBlockStart => return NonsyncBlocker::SyncBlockStart,
                    // Not possible to end sync block if never entered one
                    EvalContinuation::SyncBlockEnd => unreachable!(),
                    EvalContinuation::NewComponent(definition_id, monomorph_idx, args) => {
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
                        let init_state = ComponentState { prompt: Prompt::new(&pd.types, &pd.heap, definition_id, monomorph_idx, args) };
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
            let result = self.prompt.step(&pd.types, &pd.heap, &pd.modules, &mut context);
            match result {
                Err(err) => {
                    println!("Evaluation error:\n{}", err);
                    panic!("proper error handling when component fails");
                },
                Ok(cont) => match cont {
                    EvalContinuation::Stepping => continue,
                    EvalContinuation::Inconsistent => return SyncBlocker::Inconsistent,
                    // First need to exit synchronous block before definition may end
                    EvalContinuation::Terminal => unreachable!(),
                    // No nested synchronous blocks
                    EvalContinuation::SyncBlockStart => unreachable!(),
                    EvalContinuation::SyncBlockEnd => return SyncBlocker::SyncBlockEnd,
                    // Not possible to create component in sync block
                    EvalContinuation::NewComponent(_, _, _) => unreachable!(),
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
                Value::Input(port) => {
                    let payload = context.read_msg(port);
                    if payload.is_none() { return None; }

                    let heap_pos = store.alloc_heap();
                    let heap_pos_usize = heap_pos as usize;
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
                _ => unreachable!("did_put on non-output port value")
            }
        }
    }
}

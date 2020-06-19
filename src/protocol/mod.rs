mod arena;
mod ast;
mod eval;
pub mod inputsource;
mod lexer;
mod library;
mod parser;

use crate::common::*;
use crate::protocol::ast::*;
use crate::protocol::eval::*;
use crate::protocol::inputsource::*;
use crate::protocol::parser::*;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProtocolDescription {
    heap: Heap,
    source: InputSource,
    root: RootId,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ComponentState {
    prompt: Prompt,
}
pub enum EvalContext<'a> {
    Nonsync(&'a mut NonsyncContext<'a>),
    Sync(&'a mut SyncContext<'a>),
    None,
}
//////////////////////////////////////////////

impl std::fmt::Debug for ProtocolDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Protocol")
    }
}
impl ProtocolDescription {
    pub fn parse(buffer: &[u8]) -> Result<Self, String> {
        let mut heap = Heap::new();
        let mut source = InputSource::from_buffer(buffer).unwrap();
        let mut parser = Parser::new(&mut source);
        match parser.parse(&mut heap) {
            Ok(root) => {
                return Ok(ProtocolDescription { heap, source, root });
            }
            Err(err) => {
                let mut vec: Vec<u8> = Vec::new();
                err.write(&source, &mut vec).unwrap();
                Err(String::from_utf8_lossy(&vec).to_string())
            }
        }
    }
    pub fn component_polarities(
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
            let type_annot = &h[param.type_annotation];
            if type_annot.the_type.array {
                return Err(NonPortTypeParameters);
            }
            match type_annot.the_type.primitive {
                PrimitiveType::Input | PrimitiveType::Output => continue,
                _ => {
                    return Err(NonPortTypeParameters);
                }
            }
        }
        let mut result = Vec::new();
        for &param in def.parameters().iter() {
            let param = &h[param];
            let type_annot = &h[param.type_annotation];
            let ptype = &type_annot.the_type.primitive;
            if ptype == &PrimitiveType::Input {
                result.push(Polarity::Getter)
            } else if ptype == &PrimitiveType::Output {
                result.push(Polarity::Putter)
            } else {
                unreachable!()
            }
        }
        Ok(result)
    }
    // expects port polarities to be correct
    pub fn new_main_component(&self, identifier: &[u8], ports: &[PortId]) -> ComponentState {
        let mut args = Vec::new();
        for (&x, y) in ports.iter().zip(self.component_polarities(identifier).unwrap()) {
            match y {
                Polarity::Getter => args.push(Value::Input(InputValue(x))),
                Polarity::Putter => args.push(Value::Output(OutputValue(x))),
            }
        }
        let h = &self.heap;
        let root = &h[self.root];
        let def = root.get_definition_ident(h, identifier).unwrap();
        ComponentState { prompt: Prompt::new(h, def, &args) }
    }
}
impl ComponentState {
    pub fn nonsync_run<'a: 'b, 'b>(
        &'a mut self,
        context: &'b mut NonsyncContext<'b>,
        pd: &'a ProtocolDescription,
    ) -> NonsyncBlocker {
        let mut context = EvalContext::Nonsync(context);
        loop {
            let result = self.prompt.step(&pd.heap, &mut context);
            match result {
                // In component definitions, there are no return statements
                Ok(_) => unreachable!(),
                Err(cont) => match cont {
                    EvalContinuation::Stepping => continue,
                    EvalContinuation::Inconsistent => return NonsyncBlocker::Inconsistent,
                    EvalContinuation::Terminal => return NonsyncBlocker::ComponentExit,
                    EvalContinuation::SyncBlockStart => return NonsyncBlocker::SyncBlockStart,
                    // Not possible to end sync block if never entered one
                    EvalContinuation::SyncBlockEnd => unreachable!(),
                    EvalContinuation::NewComponent(decl, args) => {
                        // Look up definition (TODO for now, assume it is a definition)
                        let h = &pd.heap;
                        let def = h[decl].as_defined().definition;
                        let init_state = ComponentState { prompt: Prompt::new(h, def, &args) };
                        context.new_component(&args, init_state);
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

    pub fn sync_run<'a: 'b, 'b>(
        &'a mut self,
        context: &'b mut SyncContext<'b>,
        pd: &'a ProtocolDescription,
    ) -> SyncBlocker {
        let mut context = EvalContext::Sync(context);
        loop {
            let result = self.prompt.step(&pd.heap, &mut context);
            match result {
                // Inside synchronous blocks, there are no return statements
                Ok(_) => unreachable!(),
                Err(cont) => match cont {
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
                        Value::Output(OutputValue(port)) => {
                            return SyncBlocker::CouldntCheckFiring(port);
                        }
                        Value::Input(InputValue(port)) => {
                            return SyncBlocker::CouldntCheckFiring(port);
                        }
                        _ => unreachable!(),
                    },
                    EvalContinuation::BlockGet(port) => match port {
                        Value::Output(OutputValue(port)) => {
                            return SyncBlocker::CouldntReadMsg(port);
                        }
                        Value::Input(InputValue(port)) => {
                            return SyncBlocker::CouldntReadMsg(port);
                        }
                        _ => unreachable!(),
                    },
                    EvalContinuation::Put(port, message) => {
                        let value;
                        match port {
                            Value::Output(OutputValue(port_value)) => {
                                value = port_value;
                            }
                            Value::Input(InputValue(port_value)) => {
                                value = port_value;
                            }
                            _ => unreachable!(),
                        }
                        let payload;
                        match message {
                            Value::Message(MessageValue(None)) => {
                                // Putting a null message is inconsistent
                                return SyncBlocker::Inconsistent;
                            }
                            Value::Message(MessageValue(Some(buffer))) => {
                                // Create a copy of the payload
                                payload = buffer.clone();
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
    //         EvalContext::None => unreachable!(),
    //         EvalContext::Nonsync(_context) => todo!(),
    //         EvalContext::Sync(_) => unreachable!(),
    //     }
    // }
    fn new_component(&mut self, args: &[Value], init_state: ComponentState) -> () {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(context) => {
                let mut moved_ports = HashSet::new();
                for arg in args.iter() {
                    match arg {
                        Value::Output(OutputValue(port)) => {
                            moved_ports.insert(*port);
                        }
                        Value::Input(InputValue(port)) => {
                            moved_ports.insert(*port);
                        }
                        _ => {}
                    }
                }
                context.new_component(moved_ports, init_state)
            }
            EvalContext::Sync(_) => unreachable!(),
        }
    }
    fn new_channel(&mut self) -> [Value; 2] {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(context) => {
                let [from, to] = context.new_channel();
                let from = Value::Output(OutputValue(from));
                let to = Value::Input(InputValue(to));
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
                Value::Output(OutputValue(port)) => context.is_firing(port).map(Value::from),
                Value::Input(InputValue(port)) => context.is_firing(port).map(Value::from),
                _ => unreachable!(),
            },
        }
    }
    fn get(&mut self, port: Value) -> Option<Value> {
        match self {
            EvalContext::None => unreachable!(),
            EvalContext::Nonsync(_) => unreachable!(),
            EvalContext::Sync(context) => match port {
                Value::Output(OutputValue(port)) => {
                    context.read_msg(port).map(Value::receive_message)
                }
                Value::Input(InputValue(port)) => {
                    context.read_msg(port).map(Value::receive_message)
                }
                _ => unreachable!(),
            },
        }
    }
}

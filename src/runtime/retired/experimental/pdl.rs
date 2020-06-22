use crate::common::Port;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Pdl {
    ops: Vec<Op>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum Op {
    JumpIfEq,  //
    SyncStart, //
    SyncEnd,   //
    Exit,      //
    Firing,    // pop port A. push bool B.
    Put,       // pop port A. pop payload B.
    Get,       // pop port A. push payload B.
    //
    PushConst(Value),
    Store, // pop unslong A. pop B. store[A] := B.
    Load,  // pop unslong A. push store[A].
    Dup,   // pop A. push A. push A.
    //
    Lt,  // pop A. pop B. push bool C.
    Eq,  // pop A. pop B. push bool C.
    Add, // pop integer A. pop integer B. push A+B
    Neg, // pop signed integer A. push -A.
    //
    Nand, // pop bool A. pop bool B. push nand(A,B)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct State {
    op_index: usize,
    store: Vec<Value>, // TODO multiple frames
    stack: Vec<Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum Value {
    Null,
    Int(i32),
    Bool(bool),
    UnsLong(u64),
    Payload(Vec<u8>),
    Port(Port),
}

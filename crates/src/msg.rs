use crate::runtime::NodeKey;

/// A subset of the Values used in the AST that are realtime safe
#[derive(Clone, Debug, PartialEq)]
pub enum RtValue {
    F32(f32),
    I32(i32),
    U32(u32),
    Bool(bool),
    Ident(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamPayload {
    pub param_name: &'static str,
    pub value: RtValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LegatoMsg {
    NodeMessage(NodeKey, NodeMessage),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeMessage {
    SetParam(ParamPayload),
}

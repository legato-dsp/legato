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

#[derive(Clone, Debug, PartialEq)]
pub struct StepPayload {
    pub index: usize,
    pub freq: Option<f32>,
    pub vel: Option<f32>,
    /// 0.0 = muted, 1.0 = active
    pub gate: Option<f32>,
    pub length: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeMessage {
    SetParam(ParamPayload),
    SetStep(StepPayload),
    Dummy(),
}

//! A set of common message values used to pass to nodes.
//! These should not require allocation, as we don't want to
//! affect realtime safety when something is dropped.

use crate::{
    batch::MessageBatch, node_state::NodeState, resources::params::ParamKey, runtime::NodeKey,
};

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
    /// Fade a node into a [`NodeState`]. A bypass the node can't do is ignored;
    /// [`crate::LegatoFrontend::set_state`] rejects it before sending.
    SetState(NodeKey, NodeState),
    /// Write a shared param on the audio thread, so it lands in the same
    /// block as the rest of a batch. Already clamped to the param's range.
    SetParam(ParamKey, f32),
    /// Messages applied together in one block; see [`crate::batch`].
    Batch(MessageBatch),
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

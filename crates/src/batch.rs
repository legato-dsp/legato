//! Batches: several messages that land in the same block.

use std::sync::Arc;

use crate::{
    FrontendError, LegatoFrontend,
    msg::{LegatoMsg, NodeMessage, ParamPayload, RtValue},
    node_state::NodeState,
};

/// Messages applied together in one block. Cloning only bumps a refcount, so
/// a preset built once can be sent again and again without allocating.
///
/// The audio thread hands every batch back to the frontend, so it is never
/// the last owner and never frees one.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch(Arc<[LegatoMsg]>);

impl Batch {
    pub fn messages(&self) -> &[LegatoMsg] {
        &self.0
    }
}

/// Builds a [`Batch`], checking names, bypasses and params on the control
/// thread. Messages apply in order, so the last state set for a node wins.
pub struct BatchBuilder<'a> {
    frontend: &'a LegatoFrontend,
    msgs: Vec<LegatoMsg>,
}

impl<'a> BatchBuilder<'a> {
    pub(crate) fn new(frontend: &'a LegatoFrontend) -> Self {
        Self {
            frontend,
            msgs: Vec::new(),
        }
    }

    /// Fade `node` into `state`.
    pub fn set_state(&mut self, node: &str, state: NodeState) -> Result<&mut Self, FrontendError> {
        let key = self.frontend.checked_state(node, state)?;
        self.msgs.push(LegatoMsg::SetState(key, state));
        Ok(self)
    }

    pub fn activate(&mut self, node: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(node, NodeState::Active)
    }

    pub fn mute(&mut self, node: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(node, NodeState::Muted)
    }

    pub fn bypass(&mut self, node: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(node, NodeState::Bypassed)
    }

    pub fn suspend(&mut self, node: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(node, NodeState::Suspended)
    }

    /// Set one of `node`'s own params, e.g. an svf's `cutoff`.
    pub fn set(
        &mut self,
        node: &str,
        param: &'static str,
        val: f32,
    ) -> Result<&mut Self, FrontendError> {
        self.node_msg(
            node,
            NodeMessage::SetParam(ParamPayload {
                param_name: param,
                value: RtValue::F32(val),
            }),
        )
    }

    pub fn node_msg(&mut self, node: &str, msg: NodeMessage) -> Result<&mut Self, FrontendError> {
        let key = self.frontend.node_key(node)?;
        self.msgs.push(LegatoMsg::NodeMessage(key, msg));
        Ok(self)
    }

    /// Set a shared param, such as a DSL `signal`. Clamped to its range here,
    /// written on the audio thread with the rest of the batch.
    pub fn set_param(&mut self, name: &str, val: f32) -> Result<&mut Self, FrontendError> {
        let (key, val) = self
            .frontend
            .clamped_param(name, val)
            .map_err(|e| FrontendError::Param(name.to_string(), e))?;
        self.msgs.push(LegatoMsg::SetParam(key, val));
        Ok(self)
    }

    pub fn finish(self) -> Batch {
        Batch(self.msgs.into())
    }
}

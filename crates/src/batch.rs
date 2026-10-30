//! [`MessageBatch] is used to provide a group of updates into
//! one message sent from the [`LegatoFrontend`] to the realtime
//! audio thread.
//!
//! This provides a more atomic way of updating a number of effects,
//! and can be used to compose presets, instruments, etc.
//!
//! We can do this by allocating the Arc<[T]> on the frontend,
//! and ensuring that this *only* drops on the frontend as well.

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
/// the last owner and never frees on the realtime thread.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageBatch(Arc<[LegatoMsg]>);

impl MessageBatch {
    pub fn messages(&self) -> &[LegatoMsg] {
        &self.0
    }
}

/// Builds a [`MessageBatch`], checking names, bypasses and params on the control
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

    /// Fade every node `nodes` selects into `state`, e.g. `voice(*).op4`; see
    /// [`crate::select`]. Every selected node is checked before any is added.
    pub fn set_state(&mut self, nodes: &str, state: NodeState) -> Result<&mut Self, FrontendError> {
        let selected = self.frontend.select(nodes)?;
        for &(node, key) in &selected {
            self.frontend.check_state(node, key, state)?;
        }
        self.msgs.extend(
            selected
                .iter()
                .map(|&(_, key)| LegatoMsg::SetState(key, state)),
        );
        Ok(self)
    }

    pub fn activate(&mut self, nodes: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(nodes, NodeState::Active)
    }

    pub fn mute(&mut self, nodes: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(nodes, NodeState::Muted)
    }

    pub fn bypass(&mut self, nodes: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(nodes, NodeState::Bypassed)
    }

    pub fn suspend(&mut self, nodes: &str) -> Result<&mut Self, FrontendError> {
        self.set_state(nodes, NodeState::Suspended)
    }

    /// Set a param on every node `nodes` selects, e.g. an svf's `cutoff`.
    pub fn set(
        &mut self,
        nodes: &str,
        param: &'static str,
        val: f32,
    ) -> Result<&mut Self, FrontendError> {
        self.node_msg(
            nodes,
            NodeMessage::SetParam(ParamPayload {
                param_name: param,
                value: RtValue::F32(val),
            }),
        )
    }

    /// Send `msg` to every node `nodes` selects, checking all of them first.
    pub fn node_msg(&mut self, nodes: &str, msg: NodeMessage) -> Result<&mut Self, FrontendError> {
        let selected = self.frontend.select(nodes)?;
        for &(node, key) in &selected {
            self.frontend.check_node_msg(node, key, &msg)?;
        }
        self.msgs.extend(
            selected
                .iter()
                .map(|&(_, key)| LegatoMsg::NodeMessage(key, msg.clone())),
        );
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

    pub fn finish(self) -> MessageBatch {
        MessageBatch(self.msgs.into())
    }
}

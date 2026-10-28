//! Executor-level node states.
//!
//! We hoist this up, so we can mute, bypass or suspend
//! nodes without the node knowing.

/// Fade length for every state change.
pub const FADE_MS: usize = 5;

/// Fade length in samples at `sample_rate`; at least one sample.
pub const fn fade_samples(sample_rate: usize) -> u32 {
    let n = sample_rate * FADE_MS / 1000;
    if n == 0 { 1 } else { n as u32 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NodeState {
    /// `process` runs as expected, with no post-processing
    #[default]
    Active,
    /// `process` runs, output faded to silence.
    Muted,
    /// `process` skipped, default inputs copied to default outputs.
    Bypassed,
    /// `process` skipped, output silent.
    Suspended,
}

/// What the executor does with a node for one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Run `process` and leave the output alone.
    Active,
    /// Run `process`, then zero the output.
    Muted,
    /// Skip `process`; copy dry input to output.
    Bypassed,
    /// Skip `process`; zero the output if it isn't already.
    Suspended { zero: bool },
    /// Run `process`, then mix with the per-sample gains from [`NodeRuntime::advance`].
    Fading,
}

/// A node's state and fade position.
///
/// The fade is a position in `0..=len` samples: `len` is fully processed
/// output, 0 is fully in `mode`. Every change passes through `Active`, so a
/// switch between two inactive states ramps up and back down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeRuntime {
    target: NodeState,
    /// The inactive state the fade currently blends toward, or `Active` at full gain.
    mode: NodeState,
    pos: u32,
    len: u32,
    /// Suspended output has been zeroed since it settled.
    zeroed: bool,
}

impl NodeRuntime {
    pub fn new(len: u32) -> Self {
        Self {
            target: NodeState::Active,
            mode: NodeState::Active,
            pos: len,
            len,
            zeroed: false,
        }
    }

    pub fn target(&self) -> NodeState {
        self.target
    }

    /// Start a fade toward `state` from wherever the current fade is.
    pub fn set_target(&mut self, state: NodeState) {
        self.target = state;
        self.zeroed = false;
    }

    /// Start in `state` with no fade, e.g. for a state set at build time.
    pub fn set_immediate(&mut self, state: NodeState) {
        self.target = state;
        self.mode = state;
        self.pos = if state == NodeState::Active {
            self.len
        } else {
            0
        };
        self.zeroed = false;
    }

    pub fn plan(&mut self) -> Plan {
        let settled_active = self.target == NodeState::Active && self.pos == self.len;
        let settled_off = self.mode == self.target && self.pos == 0;
        match self.target {
            NodeState::Active if settled_active => Plan::Active,
            NodeState::Muted if settled_off => Plan::Muted,
            NodeState::Bypassed if settled_off => Plan::Bypassed,
            NodeState::Suspended if settled_off => {
                let zero = !self.zeroed;
                self.zeroed = true;
                Plan::Suspended { zero }
            }
            _ => Plan::Fading,
        }
    }

    /// Advance the fade one sample, returning the gains for the processed
    /// output (wet) and the dry input.
    pub fn step(&mut self) -> (f32, f32) {
        // Ramp up toward Active, or to leave one inactive state for another.
        let up = self.target == NodeState::Active
            || (self.mode != NodeState::Active && self.mode != self.target);
        if up {
            self.pos = (self.pos + 1).min(self.len);
            if self.pos == self.len {
                self.mode = NodeState::Active;
            }
        } else {
            self.mode = self.target;
            self.pos = self.pos.saturating_sub(1);
        }

        let g = self.pos as f32 / self.len as f32;
        let dry = if self.mode == NodeState::Bypassed {
            1.0 - g
        } else {
            0.0
        };
        (g, dry)
    }

    /// The gains from here on, leaving `self` where it is; lets every channel
    /// replay the same curve.
    pub fn gains(mut self) -> impl Iterator<Item = (f32, f32)> {
        std::iter::repeat_with(move || self.step())
    }

    /// Advance the fade `n` samples.
    pub fn advance(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: u32 = 8;

    /// Wet and dry gains over `n` samples.
    fn ramp(rt: &mut NodeRuntime, n: usize) -> (Vec<f32>, Vec<f32>) {
        (0..n).map(|_| rt.step()).unzip()
    }

    #[test]
    fn new_node_is_settled_active() {
        assert_eq!(NodeRuntime::new(LEN).plan(), Plan::Active);
    }

    #[test]
    fn mute_reaches_exact_zero_after_len_samples() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_target(NodeState::Muted);
        assert_eq!(rt.plan(), Plan::Fading);

        let (wet, dry) = ramp(&mut rt, LEN as usize);
        let expected: Vec<f32> = (0..LEN).rev().map(|p| p as f32 / LEN as f32).collect();
        assert_eq!(wet, expected);
        assert_eq!(dry, vec![0.0; LEN as usize]);
        assert_eq!(rt.plan(), Plan::Muted);
    }

    #[test]
    fn bypass_crossfades_wet_into_dry() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_target(NodeState::Bypassed);
        let (wet, dry) = ramp(&mut rt, LEN as usize);
        for (w, d) in wet.iter().zip(&dry) {
            assert_eq!(w + d, 1.0);
        }
        assert_eq!(rt.plan(), Plan::Bypassed);
    }

    #[test]
    fn suspended_zeroes_once() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_immediate(NodeState::Suspended);
        assert_eq!(rt.plan(), Plan::Suspended { zero: true });
        assert_eq!(rt.plan(), Plan::Suspended { zero: false });
    }

    #[test]
    fn retarget_mid_fade_continues_from_current_gain() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_target(NodeState::Muted);
        ramp(&mut rt, 3); // pos 5

        rt.set_target(NodeState::Active);
        let (wet, _) = ramp(&mut rt, 3);
        assert_eq!(wet, vec![6.0 / 8.0, 7.0 / 8.0, 1.0]);
        assert_eq!(rt.plan(), Plan::Active);
    }

    #[test]
    fn switching_between_inactive_states_passes_through_active() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_immediate(NodeState::Muted);
        rt.set_target(NodeState::Bypassed);

        // Up to full gain with no dry signal, then down crossfading into dry.
        let (wet, dry) = ramp(&mut rt, 2 * LEN as usize);
        assert_eq!(wet[LEN as usize - 1], 1.0);
        assert!(dry[..LEN as usize].iter().all(|&d| d == 0.0));
        assert_eq!(dry[2 * LEN as usize - 1], 1.0);
        assert_eq!(rt.plan(), Plan::Bypassed);
    }

    #[test]
    fn gains_replays_without_advancing() {
        let mut rt = NodeRuntime::new(LEN);
        rt.set_target(NodeState::Bypassed);
        let replay: Vec<_> = rt.gains().take(5).collect();
        assert_eq!(rt.gains().take(5).collect::<Vec<_>>(), replay);
        assert_eq!(replay[4], (3.0 / 8.0, 5.0 / 8.0));

        // Advancing picks up where the replay stopped.
        rt.advance(5);
        assert_eq!(rt.gains().next(), Some((2.0 / 8.0, 6.0 / 8.0)));
    }

    #[test]
    fn fade_samples_is_five_ms() {
        assert_eq!(fade_samples(48_000), 240);
        assert_eq!(fade_samples(100), 1);
    }
}

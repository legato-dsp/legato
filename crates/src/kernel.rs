use crate::{
    builder::{ResourceBuilderView, ValidationError},
    config::Config,
    dsl::ir::{DSLParams, IRMacro, Object},
    kernel_plan::{KernelPlan, PlanSrc, PortOracle, ValueSlot, resolve_plan},
    msg::NodeMessage,
    nodes::{
        audio::{
            allpass::Allpass,
            hadamard::HadamardMixer,
            householder::HouseholderMixer,
            noise::Noise,
            onepole::OnePole,
            ops::{ApplyOp, ApplyOpKind, mult_node_factory},
            pan::Pan,
            saw::Saw,
            sine::Sine,
            svf::Svf,
            tap::DelayTap,
        },
        control::map::Map,
    },
    persample::PerSampleNode,
    ports::Ports,
    resources::ResourceBuilder,
};
use std::collections::HashMap;

/// A finite enum of per-sample nodes.
///
/// We're using this to prevent the v-table jump with
/// per-sample processing, as this can kill performance.
///
/// We still have custom nodes, if feedback is required
/// and your kernel is not available.
#[derive(Clone)]
pub enum KernelNode {
    Sine(Sine),
    Saw(Saw),
    Svf(Svf),
    OnePole(OnePole),
    Allpass(Allpass),
    Tap(DelayTap),
    Op(ApplyOp),
    Map(Map),
    Noise(Noise),
    Householder(HouseholderMixer),
    Hadamard(HadamardMixer),
    Pan(Pan),
}

/// This macro lets us quickly write rules for all kernels
macro_rules! dispatch {
    ($self:expr, $inner:ident => $body:expr) => {
        match $self {
            KernelNode::Sine($inner) => $body,
            KernelNode::Saw($inner) => $body,
            KernelNode::Svf($inner) => $body,
            KernelNode::OnePole($inner) => $body,
            KernelNode::Allpass($inner) => $body,
            KernelNode::Tap($inner) => $body,
            KernelNode::Op($inner) => $body,
            KernelNode::Map($inner) => $body,
            KernelNode::Noise($inner) => $body,
            KernelNode::Householder($inner) => $body,
            KernelNode::Hadamard($inner) => $body,
            KernelNode::Pan($inner) => $body,
        }
    };
}

impl PerSampleNode for KernelNode {
    fn ports(&self) -> &Ports {
        dispatch!(self, inner => PerSampleNode::ports(inner))
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        dispatch!(self, inner => inner.tick(in_frame, out_frame))
    }

    fn handle_msg(&mut self, msg: NodeMessage) {
        dispatch!(self, inner => PerSampleNode::handle_msg(inner, msg))
    }
}

/// Build a node from kernels that process one sample at a time.
///
/// `seed` is the node's stable identity seed from its [`PlanNode`], used by
/// nodes carrying random state. It is passed in rather than drawn from a
/// global counter so that construction is a pure function of its arguments —
/// see [`PlanNode::identity_seed`] for why that matters.
pub fn build_kernel_node(
    node_type: &str,
    rb: &mut ResourceBuilderView,
    p: &DSLParams,
    seed: u32,
) -> Result<KernelNode, ValidationError> {
    let op = |kind: ApplyOpKind, default: f32, chans: usize, p: &DSLParams| {
        mult_node_factory(
            p.get_f32("val").unwrap_or(default),
            p.get_usize("chans").unwrap_or(chans),
            kind,
        )
    };

    Ok(match node_type {
        "sine" => KernelNode::Sine(Sine::from_params(rb, p)?),
        "saw" => KernelNode::Saw(Saw::from_params(rb, p)?),
        "svf" => KernelNode::Svf(Svf::from_params(rb, p)?),
        "onepole" => KernelNode::OnePole(OnePole::from_params(rb, p)?),
        "allpass" => KernelNode::Allpass(Allpass::from_params(rb, p)?),
        "tap" => KernelNode::Tap(DelayTap::from_params(rb, p)?),
        "map" => KernelNode::Map(Map::from_params(rb, p)?),
        // An explicit `seed:` pins the stream; otherwise it comes from the
        // node's plan identity, so it is stable across builds but still
        // distinct per node and per kernel instantiation.
        "noise" => KernelNode::Noise(Noise::with_seed(p.get_u32("seed").unwrap_or(seed))),
        "householder" => KernelNode::Householder(HouseholderMixer::from_params(rb, p)?),
        "hadamard" => KernelNode::Hadamard(HadamardMixer::from_params(rb, p)?),
        "pan" => KernelNode::Pan(Pan::from_params(rb, p)?),
        // These match block rate defaults, perhaps we make a single source of truth in the future?
        "mult" => KernelNode::Op(op(ApplyOpKind::Mult, 1.0, 1, p)),
        "add" => KernelNode::Op(op(ApplyOpKind::Add, 0.0, 1, p)),
        "sub" => KernelNode::Op(op(ApplyOpKind::Subtract, 0.0, 1, p)),
        "div" => KernelNode::Op(op(ApplyOpKind::Div, 0.0, 1, p)),
        "gain" => KernelNode::Op(op(ApplyOpKind::Gain, 1.0, 2, p)),
        other => {
            return Err(ValidationError::NotKernelCapable(format!(
                "node type '{other}' has no per-sample implementation"
            )));
        }
    })
}

/// Where one summed contribution to an interior input port comes from.
///
/// This is the interpreter's packed form of [`PlanSrc`]. The plan's `delayed`
/// flag is deliberately dropped here: this backend gets previous-sample
/// semantics for free from the persistent `values` table, since a back edge's
/// source has not yet run this tick. Codegen, which has no such table, is the
/// consumer that needs the flag.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    /// The kernel's exterior input frame (a virtual port).
    External(u32),
    /// A slot in the persistent output table.
    /// If it holds the last sample, that is our one sample delay.
    Internal(ValueSlot),
}

/// Per-node indexing into the flat runtime tables, in execution order.
#[derive(Clone, Copy, Debug)]
struct NodeLayout {
    /// First entry in `port_sources` for this node
    first_in_port: u32,
    n_in: u32,
    /// First `values` slot of this node's outputs.
    first_value_slot: u32,
    n_out: u32,
}

/// A per-sample subgraph, executable as one [`PerSampleNode`].
///
/// Nodes are stored in topological order of the cycle-broken interior graph.
/// Each node ticks straight into its slice of `values`, so downstream nodes
/// read current-sample values and feedback readers read previous-sample
/// values.
///
/// All wiring is held in two flat, contiguous tables rather than nested Vecs
/// so the per-sample walk touches sequential memory:
/// - `port_sources[p] = (start, len)` — the sources for input port `p` are
///   `src_pool[start..start + len]`, with ports grouped per node in
///   execution order.
/// - `src_pool` — every [`Src`] in the kernel, in that same walk order.
#[derive(Clone)]
pub struct KernelGraph {
    /// Interior nodes in execution order.
    nodes: Vec<KernelNode>,
    /// Geometry per node, parallel to `nodes`.
    layouts: Box<[NodeLayout]>,
    /// `(start, len)` into `src_pool` for each input port.
    port_sources: Box<[(u32, u32)]>,
    /// The flattened source lists behind `port_sources`.
    src_pool: Box<[Src]>,
    /// One slot per interior output port. Persists across ticks — this
    /// persistence *is* the z⁻¹ on feedback edges (see `tick`).
    values: Box<[f32]>,
    /// Value slots exposed as the kernel's exterior outputs (the sink's).
    out_slots: Box<[ValueSlot]>,
    scratch_in: Box<[Option<f32>]>,
    ports: Ports,
}

impl PerSampleNode for KernelGraph {
    fn ports(&self) -> &Ports {
        &self.ports
    }

    fn tick(&mut self, in_frame: &[Option<f32>], out_frame: &mut [f32]) {
        for i in 0..self.nodes.len() {
            let layout = self.layouts[i];

            for p in 0..layout.n_in as usize {
                let (start, len) = self.port_sources[layout.first_in_port as usize + p];
                let sources = &self.src_pool[start as usize..(start + len) as usize];

                let mut acc = 0.0;
                let mut patched = false;
                for src in sources {
                    match *src {
                        Src::External(e) => {
                            if let Some(v) = in_frame[e as usize] {
                                acc += v;
                                patched = true;
                            }
                        }
                        // This read is where feedback gets its one-sample
                        // delay. This comes from the previous sample, assuming
                        // we had a cycle and broke it here.
                        Src::Internal(ValueSlot(slot)) => {
                            acc += self.values[slot as usize];
                            patched = true;
                        }
                    }
                }
                self.scratch_in[p] = patched.then_some(acc);
            }

            let first = layout.first_value_slot as usize;
            let n_out = layout.n_out as usize;

            self.nodes[i].tick(
                &self.scratch_in[..layout.n_in as usize],
                &mut self.values[first..first + n_out],
            );
        }

        for (out, &ValueSlot(slot)) in out_frame.iter_mut().zip(self.out_slots.iter()) {
            *out = self.values[slot as usize];
        }
    }
}

/// A [`PortOracle`] that answers by constructing the node and reading its
/// `ports()`, so port-arity rules live only in the node implementations.
///
/// The probe builds against a **scratch** [`ResourceBuilder`], never the real
/// one. `tap` allocates a delay line on construction ([`ResourceBuilder::add_delay_line`]),
/// so probing against the caller's builder would allocate every delay line
/// twice — once to look at, once to keep. The scratch builder is dropped with
/// its throwaway allocations when the probe finishes.
///
/// `config` is only needed because constructors take a sample rate; no node's
/// port count depends on it.
pub struct ProbeOracle<'a> {
    config: &'a Config,
}

impl<'a> ProbeOracle<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }
}

impl PortOracle for ProbeOracle<'_> {
    fn ports_for(&mut self, node_type: &str, params: &DSLParams) -> Result<Ports, ValidationError> {
        let mut scratch = ResourceBuilder::default();
        let mut external_buffer_keys = HashMap::new();
        let mut delay_keys = HashMap::new();
        let mut view = ResourceBuilderView {
            config: self.config,
            resource_builder: &mut scratch,
            external_buffer_keys: &mut external_buffer_keys,
            delay_keys: &mut delay_keys,
        };

        // Seed is irrelevant to port shape; construction is pure, so this
        // probe has no effect the caller could observe.
        Ok(build_kernel_node(node_type, &mut view, params, 0)?
            .ports()
            .clone())
    }
}

impl KernelGraph {
    /// Pack a resolved [`KernelPlan`] into the interpreter's flat runtime
    /// tables and construct the DSP state for each node.
    ///
    /// The plan lists nodes in execution order with value slots still in
    /// declaration order, which is exactly the layout the tick loop wants: the
    /// wiring tables are walked front to back, while `Src::Internal` slot
    /// references stay valid across the reorder.
    pub fn from_plan(
        plan: &KernelPlan,
        rb: &mut ResourceBuilderView,
    ) -> Result<Self, ValidationError> {
        let mut nodes: Vec<KernelNode> = Vec::with_capacity(plan.nodes.len());
        let mut layouts: Vec<NodeLayout> = Vec::with_capacity(plan.nodes.len());
        let mut port_sources: Vec<(u32, u32)> = Vec::new();
        let mut src_pool: Vec<Src> = Vec::new();

        for node in &plan.nodes {
            nodes.push(build_kernel_node(
                &node.node_type,
                rb,
                &DSLParams::new(&node.params),
                node.identity_seed,
            )?);

            layouts.push(NodeLayout {
                first_in_port: port_sources.len() as u32,
                n_in: node.n_in() as u32,
                first_value_slot: node.slot_base.0,
                n_out: node.n_out as u32,
            });

            for port in &node.inputs {
                port_sources.push((src_pool.len() as u32, port.len() as u32));
                src_pool.extend(port.iter().map(|src| match *src {
                    PlanSrc::Exterior(i) => Src::External(i),
                    // `delayed` is intentionally discarded — see `Src`.
                    PlanSrc::Interior { slot, .. } => Src::Internal(slot),
                }));
            }
        }

        Ok(KernelGraph {
            nodes,
            layouts: layouts.into_boxed_slice(),
            port_sources: port_sources.into_boxed_slice(),
            src_pool: src_pool.into_boxed_slice(),
            values: vec![0.0; plan.total_slots].into_boxed_slice(),
            out_slots: plan.output_slots.clone().into_boxed_slice(),
            scratch_in: vec![None; plan.max_node_inputs()].into_boxed_slice(),
            ports: plan.ports(),
        })
    }
}

/// Lower a kernel definition plus one instantiation's params into an
/// executable [`KernelGraph`].
///
/// Two stages: [`resolve_plan`] works out the topology with no DSP state
/// involved, then [`KernelGraph::from_plan`] builds the state and packs the
/// runtime tables. The codegen backend replaces only the second stage.
///
/// `instance_salt` should be the alias of the node instantiating this kernel;
/// it keeps sibling instantiations (poly voices) from sharing RNG seeds.
pub fn lower_kernel(
    ir_macro: &IRMacro,
    instance_params: &Object,
    instance_salt: &str,
    rb: &mut ResourceBuilderView,
) -> Result<KernelGraph, ValidationError> {
    let plan = resolve_plan(
        ir_macro,
        instance_params,
        instance_salt,
        &mut ProbeOracle::new(rb.config),
    )?;
    KernelGraph::from_plan(&plan, rb)
}

pub const EXAMPLE_PLATE_KERNEL_PATCH: &str = r#"
    // mod_range_l/r: LFO excursion of the two modulated tank allpasses, in
    // ms. Whole-array values template fine ($mod_range_l below); only
    // per-element templates inside an array literal do not.
    kernel plate(
        predelay = 10.0,
        bandwidth_a = 0.0005,
        damping = 0.3,
        decay = 0.5,
        wet = 0.3,
        dry = 0.7,
        mod_range_l = [22.3111, 22.8487],
        mod_range_r = [30.2408, 30.7784]
    ) {
        in in_l in_r

        audio {
            // input chain: mono sum -> predelay -> bandwidth one-pole -> 4 diffusers
            mult: mono { val: 0.5 },
            tap: pre { delay_length: $predelay, chans: 1 },
            onepole: bw { a: $bandwidth_a, chans: 1 },
            allpass: diff1 { delay_length: 4.7713, feedback: 0.75, chans: 1 },
            allpass: diff2 { delay_length: 3.5953, feedback: 0.75, chans: 1 },
            allpass: diff3 { delay_length: 12.7348, feedback: 0.625, chans: 1 },
            allpass: diff4 { delay_length: 9.3075, feedback: 0.625, chans: 1 },

            // LFO modulatings the first tank allpasses
            sine: lfo_l { freq: 0.7 },
            sine: lfo_r { freq: 0.7, phase: 0.25 },
            allpass: tank_ap_l { delay_length: 22.5799, feedback: -0.7, chans: 1 },
            allpass: tank_ap_r { delay_length: 30.5096, feedback: -0.7, chans: 1 },

            // main tank delays + damping + decay per branch
            tap: del_a { delay_length: 149.6254, chans: 1 },
            onepole: damp_l { a: $damping, chans: 1 },
            mult: decay_l { val: $decay },
            tap: del_b { delay_length: 124.9958, chans: 1 },

            tap: del_c { delay_length: 141.6955, chans: 1 },
            onepole: damp_r { a: $damping, chans: 1 },
            mult: decay_r { val: $decay },
            tap: del_d { delay_length: 106.28, chans: 1 },

            // second tank allpasses from primitives: w = in + 0.5 d, out = d - 0.5 w
            add: ap2_l_w { val: 0.0 },
            tap: ap2_l_d { delay_length: 60.4818, chans: 1 },
            mult: ap2_l_fb { val: 0.5 },
            mult: ap2_l_ff { val: -0.5 },
            add: ap2_l_out { val: 0.0 },

            add: ap2_r_w { val: 0.0 },
            tap: ap2_r_d { delay_length: 89.2444, chans: 1 },
            mult: ap2_r_fb { val: 0.5 },
            mult: ap2_r_ff { val: -0.5 },
            add: ap2_r_out { val: 0.0 },

            // output tap matrix: one tap node per read offset
            tap: yl1 { delay_length: 8.9379, chans: 1 },
            tap: yl2 { delay_length: 99.9295, chans: 1 },
            tap: yl3 { delay_length: 64.2788, chans: 1 },
            tap: yl4 { delay_length: 67.0676, chans: 1 },
            tap: yl5 { delay_length: 66.866, chans: 1 },
            tap: yl6 { delay_length: 6.2834, chans: 1 },
            tap: yl7 { delay_length: 35.8187, chans: 1 },

            tap: yr1 { delay_length: 11.8612, chans: 1 },
            tap: yr2 { delay_length: 121.8708, chans: 1 },
            tap: yr3 { delay_length: 41.2621, chans: 1 },
            tap: yr4 { delay_length: 89.8156, chans: 1 },
            tap: yr5 { delay_length: 70.9318, chans: 1 },
            tap: yr6 { delay_length: 11.2563, chans: 1 },
            tap: yr7 { delay_length: 4.0657, chans: 1 },

            mult: gl1 { val: 0.6 },
            mult: gl2 { val: 0.6 },
            mult: gl3 { val: -0.6 },
            mult: gl4 { val: 0.6 },
            mult: gl5 { val: -0.6 },
            mult: gl6 { val: -0.6 },
            mult: gl7 { val: -0.6 },

            mult: gr1 { val: 0.6 },
            mult: gr2 { val: 0.6 },
            mult: gr3 { val: -0.6 },
            mult: gr4 { val: 0.6 },
            mult: gr5 { val: -0.6 },
            mult: gr6 { val: -0.6 },
            mult: gr7 { val: -0.6 },

            mult: wet_l { val: $wet },
            mult: wet_r { val: $wet },
            mult: dry_l { val: $dry },
            mult: dry_r { val: $dry },
            add: out { val: 0.0, chans: 2 },
        }

        control {
            map: lfo_l_ms { range: [-1.0, 1.0], new_range: $mod_range_l },
            map: lfo_r_ms { range: [-1.0, 1.0], new_range: $mod_range_r },
        }

        // input chain
        in_l >> mono[0]
        in_r >> mono[0]
        mono >> pre[0] >> bw[0] >> diff1[0] >> diff2[0] >> diff3[0] >> diff4[0]

        // LFO -> ms -> modulated tank allpass delay
        lfo_l >> lfo_l_ms >> tank_ap_l.delay_length
        lfo_r >> lfo_r_ms >> tank_ap_r.delay_length

        // figure-eight: each branch takes the diffused input plus the other
        // branch's tail (this closes the tank cycle; one of the two returns
        // picks up the implicit z-1)
        diff4 >> tank_ap_l[0]
        del_d >> tank_ap_l[0]
        diff4 >> tank_ap_r[0]
        del_b >> tank_ap_r[0]

        // left branch
        tank_ap_l >> del_a[0]
        del_a >> damp_l[0] >> decay_l[0] >> ap2_l_w[0]
        ap2_l_fb >> ap2_l_w[0]
        ap2_l_w >> ap2_l_d[0]
        ap2_l_d >> ap2_l_fb[0]
        ap2_l_d >> ap2_l_out[0]
        ap2_l_w >> ap2_l_ff[0]
        ap2_l_ff >> ap2_l_out[0]
        ap2_l_out >> del_b[0]

        // right branch
        tank_ap_r >> del_c[0]
        del_c >> damp_r[0] >> decay_r[0] >> ap2_r_w[0]
        ap2_r_fb >> ap2_r_w[0]
        ap2_r_w >> ap2_r_d[0]
        ap2_r_d >> ap2_r_fb[0]
        ap2_r_d >> ap2_r_out[0]
        ap2_r_w >> ap2_r_ff[0]
        ap2_r_ff >> ap2_r_out[0]
        ap2_r_out >> del_d[0]

        // left output taps (source signal = the line each 480L tap reads)
        tank_ap_r >> yl1[0] >> gl1[0] >> wet_l[0]
        tank_ap_r >> yl2[0] >> gl2[0] >> wet_l[0]
        ap2_r_w   >> yl3[0] >> gl3[0] >> wet_l[0]
        ap2_r_out >> yl4[0] >> gl4[0] >> wet_l[0]
        tank_ap_l >> yl5[0] >> gl5[0] >> wet_l[0]
        ap2_l_w   >> yl6[0] >> gl6[0] >> wet_l[0]
        ap2_l_out >> yl7[0] >> gl7[0] >> wet_l[0]

        // right output taps
        tank_ap_l >> yr1[0] >> gr1[0] >> wet_r[0]
        tank_ap_l >> yr2[0] >> gr2[0] >> wet_r[0]
        ap2_l_w   >> yr3[0] >> gr3[0] >> wet_r[0]
        ap2_l_out >> yr4[0] >> gr4[0] >> wet_r[0]
        tank_ap_r >> yr5[0] >> gr5[0] >> wet_r[0]
        ap2_r_w   >> yr6[0] >> gr6[0] >> wet_r[0]
        ap2_r_out >> yr7[0] >> gr7[0] >> wet_r[0]

        // wet/dry into the stereo collector
        wet_l >> out[0]
        wet_r >> out[1]
        in_l >> dry_l[0]
        in_r >> dry_r[0]
        dry_l >> out[0]
        dry_r >> out[1]

        { out }
    }
"#;

pub const EXAMPLE_KARPLUS_KERNEL_PATCH: &str = r#"
    kernel karplus(
        damping = 0.5,
        decay = 0.99,
        pluck = 0.995
    ) {
        in gate freq

        audio {
            // --- excitation: gate edge -> windowed, gate-masked noise burst ---
            noise: exc_src {},
            onepole: gate_follow { a: $pluck, chans: 1 },
            sub: env { val: 0.0, chans: 1 },
            mult: burst { val: 1.0, chans: 1 },
            mult: exc { val: 1.0, chans: 1 },

            // --- tuning: freq (Hz) -> 1/freq (s) -> ms -> delay_length ---
            sine: one { freq: 0.0, phase: 0.25 },
            div: period_s { val: 220.0 },
            mult: period_ms { val: 1000.0 },

            // --- the string loop ---
            add: mix { val: 0.0, chans: 1 },
            tap: string { delay_length: 4.5, chans: 1, capacity: 48000 },
            onepole: loop_lp { a: $damping, chans: 1 },
            mult: fb { val: $decay },
        }

        // excitation: env = gate - slow_follow(gate); burst = noise * env; exc = burst * gate
        gate >> gate_follow[0]
        gate >> env[0]
        gate_follow >> env[1]
        exc_src >> burst[0]
        env >> burst[1]
        burst >> exc[0]
        gate >> exc[1]

        // tuning: 1.0 / freq -> * 1000 -> ms into the delay length port
        one >> period_s[0]
        freq >> period_s[1]
        period_s >> period_ms[0]
        period_ms >> string.delay_length

        // string loop: exc + tail -> delay -> loop lowpass -> feedback gain
        exc >> mix[0]
        fb  >> mix[1]
        mix >> string[0] >> loop_lp[0] >> fb[0]

        // fb >> mix closes the cycle -> the engine's implicit z-1 lives here.
        { string }
    }
"#;

pub const EXAMPLE_MODTAP_KERNEL_PATCH: &str = r#"
    // Four modulated normalized-feedback comb taps -> stereo (mono in, 2 out).
    // Odd taps pan left, even taps pan right; distinct delays + independent LFO
    // phases decorrelate the two sides for width.
    //
    // Each tap is its OWN feedback loop, written as a crossfade comb:
    //     m_i = (1 - feedback) * in + feedback * t_i
    //         = in + feedback * (t_i - in)
    // The second form is what the graph builds (feed the feedback gain the
    // DIFFERENCE t_i - in, via a `sub`, so no param arithmetic is needed). Its
    // DC gain is exactly 1 for ANY feedback, so turning feedback up makes the
    // echoes denser without making the output louder -- energy-conserving, like
    // a real delay. That keeps the LINEAR 1/4 output near full scale at all
    // settings (no soft-clip / saturation needed), while feedback is still
    // per-tap so the loop gain is exactly `feedback` with no shared-feedback
    // coherent-mode trap and no mixing matrix.
    //
    //   depth    - modulation excursion in ms (one shared 4-chan mult)
    //   rate     - LFO frequency in Hz (sine.freq)
    //   feedback - per-tap loop gain 0..~0.95 (regeneration / echo density)
    kernel modtap4(
        depth = 3.0,
        rate = 0.35,
        feedback = 0.7
    ) {
        in in

        audio {
            // per-tap input mix: dry input + this tap's feedback contribution
            // feedback*(t_i - in). (both fan into port [0] and sum.)
            add: m1 { val: 0.0, chans: 1 },
            add: m2 { val: 0.0, chans: 1 },
            add: m3 { val: 0.0, chans: 1 },
            add: m4 { val: 0.0, chans: 1 },

            // phase-staggered LFOs, one shared per-channel depth scale.
            sine: lfo1 { freq: $rate, phase: 0.0 },
            sine: lfo2 { freq: $rate, phase: 0.25 },
            sine: lfo3 { freq: $rate, phase: 0.5 },
            sine: lfo4 { freq: $rate, phase: 0.75 },
            mult: depth { val: $depth, chans: 4 },

            // base delay per tap (ms), mutually incommensurate. modulated time
            // = base +/- depth ms.
            add: dt1 { val: 71.0, chans: 1 },
            add: dt2 { val: 113.0, chans: 1 },
            add: dt3 { val: 173.0, chans: 1 },
            add: dt4 { val: 241.0, chans: 1 },

            // the four delay lines (capacity covers longest tap + mod, 1s @ 48k)
            tap: t1 { delay_length: 71.0, chans: 1, capacity: 48000 },
            tap: t2 { delay_length: 113.0, chans: 1, capacity: 48000 },
            tap: t3 { delay_length: 173.0, chans: 1, capacity: 48000 },
            tap: t4 { delay_length: 241.0, chans: 1, capacity: 48000 },

            // per-tap difference t_i - in, so the feedback gain crossfades
            // (normalizes) the comb instead of just adding regeneration.
            sub: d1 { val: 0.0, chans: 1 },
            sub: d2 { val: 0.0, chans: 1 },
            sub: d3 { val: 0.0, chans: 1 },
            sub: d4 { val: 0.0, chans: 1 },

            // one shared per-tap feedback gain (4 independent channels).
            mult: fb { val: $feedback, chans: 4 },

            // stereo wet output: odd taps (t1,t3) -> left, even taps (t2,t4) ->
            // right. each side scales its 2-tap sum by 0.35 -- lower than a
            // naive 1/2 because summing only two normalized combs lets their
            // echo trains align transiently (a 2-tap side has a higher crest
            // factor than the mono 4, and it grows with feedback), so 0.35
            // keeps the peak <= ~0.96 even at feedback 0.95. the taps have
            // distinct delays and independent LFO phases, so L/R are
            // decorrelated -> real width. `out` is a 2-channel collector.
            mult: out_l { val: 0.35, chans: 1 },
            mult: out_r { val: 0.35, chans: 1 },
            add: out { val: 0.0, chans: 2 },
        }

        // shared modulation: LFO -> * depth (per channel) -> + base -> delay_length
        lfo1 >> depth[0]
        lfo2 >> depth[1]
        lfo3 >> depth[2]
        lfo4 >> depth[3]

        depth[0] >> dt1[0] >> t1.delay_length
        depth[1] >> dt2[0] >> t2.delay_length
        depth[2] >> dt3[0] >> t3.delay_length
        depth[3] >> dt4[0] >> t4.delay_length

        // input fans into every tap's mix node (port [0], summed with feedback)
        in >> m1[0]
        in >> m2[0]
        in >> m3[0]
        in >> m4[0]

        // tap input mix -> delay line
        m1 >> t1[0]
        m2 >> t2[0]
        m3 >> t3[0]
        m4 >> t4[0]

        // normalized per-tap feedback: d_i = t_i - in; then
        // m_i = in + feedback * d_i = (1-feedback)*in + feedback*t_i.
        // the d_i -> fb -> m_i edges close the four loops (implicit z-1 here).
        t1 >> d1[0]
        t2 >> d2[0]
        t3 >> d3[0]
        t4 >> d4[0]
        in >> d1[1]
        in >> d2[1]
        in >> d3[1]
        in >> d4[1]

        d1 >> fb[0] >> m1[0]
        d2 >> fb[1] >> m2[0]
        d3 >> fb[2] >> m3[0]
        d4 >> fb[3] >> m4[0]

        // stereo split: odd taps left, even taps right (each a 1/2 unity sum),
        // into the 2-channel collector.
        t1 >> out_l[0]
        t3 >> out_l[0]
        t2 >> out_r[0]
        t4 >> out_r[0]
        out_l >> out[0]
        out_r >> out[1]

        { out }
    }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{BlockSize, Config},
        dsl::{lower::ast_to_graph, parse::legato_parser},
        resources::ResourceBuilder,
    };

    /// Parse `src`, lower it, and hand back the kernel definition `name`.
    fn kernel_def(src: &str, name: &str) -> IRMacro {
        let ast = legato_parser(src).expect("kernel test source should parse");
        let graph = ast_to_graph(ast);
        graph
            .macro_registry
            .get(name)
            .unwrap_or_else(|| panic!("kernel '{name}' missing from registry"))
            .clone()
    }

    fn build(def: &IRMacro, params: Object) -> Result<KernelGraph, ValidationError> {
        let config = Config::new(48_000, BlockSize::Block64, 1, 0);
        let mut resource_builder = ResourceBuilder::default();
        let mut external = HashMap::new();
        let mut delays = HashMap::new();
        let mut view = ResourceBuilderView {
            config: &config,
            resource_builder: &mut resource_builder,
            external_buffer_keys: &mut external,
            delay_keys: &mut delays,
        };
        lower_kernel(def, &params, "test", &mut view)
    }

    /// y[n] = x[n] + fb * y[n-1], built from `add` + `mult` with a feedback
    /// edge. The mult -> add edge closes the cycle and must read the previous
    /// sample (implicit z⁻¹) — verified exactly against the recurrence.
    #[test]
    fn feedback_cycle_gets_unit_delay() {
        let src = r#"
            kernel fb_loop(fb = 0.5) {
                in audio_in

                audio {
                    add { val: 0.0 },
                    mult { val: $fb }
                }

                audio_in >> add[0]
                add >> mult[0]
                mult >> add[1]

                { add }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "fb_loop");
        let mut kg = build(&def, Object::new()).expect("kernel should build");

        assert_eq!(PerSampleNode::ports(&kg).audio_in.len(), 1);
        assert_eq!(PerSampleNode::ports(&kg).audio_out.len(), 1);

        let input = [1.0f32, 0.0, 0.0, 0.0, 2.0, 0.0];
        let mut expected_state = 0.0f32;
        let mut out = [0.0f32];

        for (n, &x) in input.iter().enumerate() {
            kg.tick(&[Some(x)], &mut out);
            expected_state = x + 0.5 * expected_state;
            assert_eq!(
                out[0], expected_state,
                "feedback recurrence diverged at sample {n}"
            );
        }
    }

    /// Declaration order must not dictate execution order: `mult` is declared
    /// before the `add` that feeds it, yet a forward chain has to flow within
    /// a single sample: out = (x + 1) * 2.
    #[test]
    fn topo_order_ignores_declaration_order() {
        let src = r#"
            kernel chain() {
                in audio_in

                audio {
                    mult { val: 2.0 },
                    add { val: 1.0 }
                }

                audio_in >> add[0]
                add >> mult[0]

                { mult }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "chain");
        let mut kg = build(&def, Object::new()).expect("kernel should build");

        let mut out = [0.0f32];
        kg.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 8.0, "chain must run add before mult in one sample");
    }

    /// Kernel default params flow into interior nodes via templates and are
    /// overridable at the instantiation site.
    #[test]
    fn instance_params_override_defaults() {
        let src = r#"
            kernel scaled(amount = 2.0) {
                in audio_in

                audio {
                    mult { val: $amount }
                }

                audio_in >> mult[0]

                { mult }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "scaled");

        let mut with_default = build(&def, Object::new()).unwrap();
        let mut out = [0.0f32];
        with_default.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 6.0);

        let mut overridden = build(&def, crate::object! { "amount" => 5.0f32 }).unwrap();
        overridden.tick(&[Some(3.0)], &mut out);
        assert_eq!(out[0], 15.0);
    }

    /// `map` (a control node) is kernel-capable: scaling works per-sample,
    /// e.g. for shaping a modulator inside a feedback structure.
    #[test]
    fn map_scales_inside_kernel() {
        let src = r#"
            kernel lfo_scaled() {
                in mod_in

                control {
                    map { range: [-1.0, 1.0], new_range: [0.0, 10.0] }
                }

                mod_in >> map

                { map }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "lfo_scaled");
        let mut kg = build(&def, Object::new()).unwrap();

        let mut out = [0.0f32];
        kg.tick(&[Some(-1.0)], &mut out);
        assert_eq!(out[0], 0.0);
        kg.tick(&[Some(0.0)], &mut out);
        assert_eq!(out[0], 5.0);
        kg.tick(&[Some(1.0)], &mut out);
        assert_eq!(out[0], 10.0);
    }

    /// The reference plate kernel (the dogfood) must parse, lower with its
    /// cycles broken, and tick without blowing up.
    #[test]
    fn plate_kernel_lowers_and_ticks() {
        let src = format!("{EXAMPLE_PLATE_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
        let def = kernel_def(&src, "plate");
        let mut kg = build(&def, Object::new()).expect("plate kernel should lower");

        assert_eq!(PerSampleNode::ports(&kg).audio_in.len(), 2);
        assert_eq!(PerSampleNode::ports(&kg).audio_out.len(), 2);

        // Impulse in, then run the tank for a while: output stays finite and
        // the reverb tail is actually audible.
        let mut out = [0.0f32; 2];
        let mut energy = 0.0f32;
        for n in 0..48_000 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            kg.tick(&[Some(x), Some(x)], &mut out);
            assert!(
                out[0].is_finite() && out[1].is_finite(),
                "plate tank blew up at sample {n}"
            );
            energy += out[0] * out[0] + out[1] * out[1];
        }
        assert!(energy > 1e-4, "plate tail was silent");
    }

    /// The modulated comb bank must lower (cycles broken on the feedback
    /// edges), tick without blowing up, and produce a wet stereo tail from a
    /// single input impulse (mono in, 2 out).
    #[test]
    fn modtap_kernel_lowers_and_ticks() {
        let src = format!("{EXAMPLE_MODTAP_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
        let def = kernel_def(&src, "modtap4");
        let mut kg = build(&def, Object::new()).expect("modtap kernel should lower");

        assert_eq!(PerSampleNode::ports(&kg).audio_in.len(), 1);
        assert_eq!(PerSampleNode::ports(&kg).audio_out.len(), 2);

        let mut out = [0.0f32; 2];
        let mut energy = 0.0f32;
        for n in 0..48_000 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            kg.tick(&[Some(x)], &mut out);
            assert!(
                out[0].is_finite() && out[1].is_finite(),
                "modtap blew up at sample {n}"
            );
            energy += out[0] * out[0] + out[1] * out[1];
        }
        assert!(energy > 1e-4, "modtap produced no wet signal");
    }

    /// Feedback must actually recirculate. Fire a single impulse, then run
    /// silence, and measure the *late* tail (well past the longest 241 ms tap,
    /// so with zero feedback it should be near silent — every first-pass tap
    /// has already fired). Higher feedback has to leave more energy in that
    /// late window — if the implicit z⁻¹ on the per-tap `d -> fb -> m` cycle
    /// edges were dropped, the loops wouldn't sustain and the late tail would
    /// not grow with feedback.
    #[test]
    fn modtap_feedback_recirculates() {
        let src = format!("{EXAMPLE_MODTAP_KERNEL_PATCH} audio {{ sine }} {{ sine }}");
        let def = kernel_def(&src, "modtap4");

        // Late window starts at 700 ms @ 48k — ~3x the longest (241 ms) tap, so
        // any energy there arrived via the feedback loop, not a first pass.
        let late_start = (0.700 * 48_000.0) as usize;
        let total = 3 * 48_000; // 3 s tail

        let late_energy = |feedback: f32| -> f32 {
            let mut kg = build(&def, crate::object! { "feedback" => feedback })
                .expect("modtap should lower");
            let mut out = [0.0f32; 2];
            let mut e = 0.0f32;
            for n in 0..total {
                let x = if n == 0 { 1.0 } else { 0.0 };
                kg.tick(&[Some(x)], &mut out);
                assert!(
                    out[0].is_finite() && out[1].is_finite(),
                    "diverged at {n} (fb={feedback})"
                );
                if n >= late_start {
                    e += out[0] * out[0] + out[1] * out[1];
                }
            }
            e
        };

        let e_none = late_energy(0.0);
        let e_mid = late_energy(0.5);
        let e_high = late_energy(0.85);

        eprintln!("late-tail energy: fb0={e_none:e} fb0.5={e_mid:e} fb0.85={e_high:e}");

        // With no feedback the late window is essentially silent...
        assert!(
            e_none < 1e-6,
            "expected near-silence past 700ms with no feedback, got {e_none:e}"
        );
        // ...and feedback monotonically fills that late tail.
        assert!(
            e_mid > 100.0 * e_none.max(1e-30),
            "feedback 0.5 did not add a recirculating tail (fb0={e_none:e}, fb0.5={e_mid:e})"
        );
        assert!(
            e_high > e_mid,
            "more feedback must sustain longer (fb0.5={e_mid:e}, fb0.85={e_high:e})"
        );
    }

    #[test]
    fn non_kernel_capable_node_errors() {
        let src = r#"
            kernel bad() {
                in audio_in

                audio {
                    sampler { sampler_name: "amen" }
                }

                audio_in >> sampler

                { sampler }
            }
            audio { sine }
            { sine }
        "#;

        let def = kernel_def(src, "bad");
        match build(&def, Object::new()) {
            Err(ValidationError::NotKernelCapable(_)) => {}
            Err(other) => panic!("expected NotKernelCapable, got {other:?}"),
            Ok(_) => panic!("expected NotKernelCapable, got a built kernel"),
        }
    }
}

//! Ports define the inputs and outputs for a [`Node`].
//!
//! A bare `>>` maps onto default ports, and only ports marked with
//! [`PortBuilder::default_in`] / [`PortBuilder::default_out`] are defaults.
//! Leave them unmarked when there is no obvious choice (a voice's
//! gate/freq/vel/trig bundle, an oscillator's `freq`), so the user has to
//! name the port.
//!
//! For example, here we probably want our audio output from `stereo_node`
//! to flow into the `stereo_filter` inputs, if say `stereo_filter` had a
//! third `cutoff` parameter:
//!
//! stereo_node >> stereo_filter
//!
//! This is nicer IMO than writing:
//!
//! stereo_node[0..2] >> stereo_filter[0..2]

#[derive(Clone, Debug)]
pub struct PortMeta {
    pub name: &'static str,
    /// Position within this port's list, see [`Signal`].
    pub index: usize,
    /// Whether a bare `>>` auto-maps onto this port.
    pub default: bool,
    pub kind: PortKind,
}

/// What a port carries, and what an input receives when nothing is connected to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    Stream(Unpatched),
    Midi(MidiUnpatched),
}

impl PortKind {
    pub fn signal(&self) -> Signal {
        match self {
            PortKind::Stream(_) => Signal::Stream,
            PortKind::Midi(_) => Signal::Midi,
        }
    }
}

impl Default for PortKind {
    fn default() -> Self {
        PortKind::Stream(Unpatched::Absent)
    }
}

/// Which of a node's port lists an index refers to. Each list is indexed from zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Signal {
    /// Audio and control rate `f32` blocks.
    #[default]
    Stream,
    Midi,
}

/// How the executor presents a stream input that has no incoming connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unpatched {
    /// The node sees `None`, typically falling back to an internal parameter.
    #[default]
    Absent,
    /// The node sees a block of zeros.
    Silent,
    /// Building the graph fails.
    Required,
}

/// How the executor presents a MIDI input that has no incoming connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MidiUnpatched {
    #[default]
    Empty,
    /// Building the graph fails.
    Required,
    /// The node sees the host's MIDI for this block.
    Host,
}

#[derive(Clone, Debug, Default)]
pub struct Ports {
    pub ports_in: Vec<PortMeta>,
    pub ports_out: Vec<PortMeta>,
    pub midi_in: Vec<PortMeta>,
    pub midi_out: Vec<PortMeta>,
}

impl Ports {
    pub fn inputs(&self, signal: Signal) -> &[PortMeta] {
        match signal {
            Signal::Stream => &self.ports_in,
            Signal::Midi => &self.midi_in,
        }
    }

    pub fn outputs(&self, signal: Signal) -> &[PortMeta] {
        match signal {
            Signal::Stream => &self.ports_out,
            Signal::Midi => &self.midi_out,
        }
    }

    /// What a bare `>>` reads from: MIDI if a MIDI output is marked default
    /// or the node only has MIDI outputs, otherwise stream.
    pub fn default_out_signal(&self) -> Signal {
        let midi_marked = self.midi_out.iter().any(|p| p.default);
        if midi_marked || (self.ports_out.is_empty() && !self.midi_out.is_empty()) {
            Signal::Midi
        } else {
            Signal::Stream
        }
    }

    pub fn default_in_of(&self, signal: Signal) -> Vec<usize> {
        default_indices(self.inputs(signal))
    }

    pub fn default_out_of(&self, signal: Signal) -> Vec<usize> {
        default_indices(self.outputs(signal))
    }

    pub fn find_port_in(&self, name: &String) -> Option<PortMeta> {
        if let Some(port) = self.ports_in.iter().find(|x| x.name == name) {
            return Some(port.clone());
        }
        None
    }
    pub fn find_port_out(&self, name: &String) -> Option<PortMeta> {
        if let Some(port) = self.ports_out.iter().find(|x| x.name == name) {
            return Some(port.clone());
        }
        None
    }

    /// Input indices a bare `>>` targets: the ports marked default.
    pub fn default_in(&self) -> Vec<usize> {
        default_indices(&self.ports_in)
    }

    /// Input indices marked with [`PortBuilder::default_in`]: the node's
    /// declared signal path. Bypass uses these.
    pub fn marked_default_in(&self) -> Vec<usize> {
        self.marked_default_in_of(Signal::Stream)
    }

    pub fn marked_default_in_of(&self, signal: Signal) -> Vec<usize> {
        self.inputs(signal)
            .iter()
            .filter(|p| p.default)
            .map(|p| p.index)
            .collect()
    }

    /// Output indices a bare `>>` reads from: the ports marked default.
    pub fn default_out(&self) -> Vec<usize> {
        default_indices(&self.ports_out)
    }
}

/// The port `instance` of `instances` reads from a group named `name`: the
/// only port of that name, or the `instance`-th when there is one per instance.
pub(crate) fn member_index(
    ports: &[PortMeta],
    name: &str,
    instance: usize,
    instances: usize,
) -> Result<usize, String> {
    let group: Vec<usize> = ports
        .iter()
        .filter(|p| p.name == name)
        .map(|p| p.index)
        .collect();
    match group.len() {
        0 => Err(format!("no port named '{name}'")),
        1 => Ok(group[0]),
        n if n == instances => Ok(group[instance]),
        n => Err(format!(
            "'{name}' names {n} ports but is fanned across {instances} instances; \
             the counts must match"
        )),
    }
}

/// The ports a bare `>>` maps onto: only those marked default. A node whose
/// ports have no obvious default marks none, so a bare `>>` must name one.
pub(crate) fn default_indices(ports: &[PortMeta]) -> Vec<usize> {
    ports
        .iter()
        .filter(|p| p.default)
        .map(|p| p.index)
        .collect()
}

impl From<PortBuilder> for Ports {
    fn from(builder: PortBuilder) -> Self {
        assert_unique_across_kinds(&builder.port_audio_in, &builder.midi_in, "input");
        assert_unique_across_kinds(&builder.port_audio_out, &builder.midi_out, "output");
        Ports {
            ports_in: builder.port_audio_in,
            ports_out: builder.port_audio_out,
            midi_in: builder.midi_in,
            midi_out: builder.midi_out,
        }
    }
}

pub trait Ported {
    fn get_ports(&self) -> &Ports;
}

/// (signal, start, end) of a batch of ports added in one builder call.
type Batch = (Signal, usize, usize);

#[derive(Default)]
pub struct PortBuilder {
    port_audio_in: Vec<PortMeta>,
    port_audio_out: Vec<PortMeta>,
    midi_in: Vec<PortMeta>,
    midi_out: Vec<PortMeta>,
    last_in: Option<Batch>,
    last_out: Option<Batch>,
}

fn push_port(list: &mut Vec<PortMeta>, name: &'static str, kind: PortKind) {
    let index = list.len();
    list.push(PortMeta {
        name,
        index,
        default: false,
        kind,
    });
}

impl PortBuilder {
    fn push_in(&mut self, name: &'static str) {
        push_port(&mut self.port_audio_in, name, PortKind::default());
    }

    fn push_out(&mut self, name: &'static str) {
        push_port(&mut self.port_audio_out, name, PortKind::default());
    }

    pub fn audio_in(mut self, count: usize) -> Self {
        let start = self.port_audio_in.len();
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count));
        }
        self.last_in = Some((Signal::Stream, start, self.port_audio_in.len()));
        self
    }

    pub fn audio_out(mut self, count: usize) -> Self {
        let start = self.port_audio_out.len();
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count));
        }
        self.last_out = Some((Signal::Stream, start, self.port_audio_out.len()));
        self
    }

    pub fn audio_in_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_in.len();
        for name in names {
            self.push_in(name);
        }
        self.last_in = Some((Signal::Stream, start, self.port_audio_in.len()));
        self
    }

    pub fn audio_out_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_out.len();
        for name in names {
            self.push_out(name);
        }
        self.last_out = Some((Signal::Stream, start, self.port_audio_out.len()));
        self
    }

    pub fn control_in(mut self, count: usize) -> Self {
        let start = self.port_audio_in.len();
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count));
        }
        self.last_in = Some((Signal::Stream, start, self.port_audio_in.len()));
        self
    }

    pub fn control_out(mut self, count: usize) -> Self {
        let start = self.port_audio_out.len();
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count));
        }
        self.last_out = Some((Signal::Stream, start, self.port_audio_out.len()));
        self
    }

    pub fn control_in_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_in.len();
        for name in names {
            self.push_in(name);
        }
        self.last_in = Some((Signal::Stream, start, self.port_audio_in.len()));
        self
    }

    pub fn control_out_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_out.len();
        for name in names {
            self.push_out(name);
        }
        self.last_out = Some((Signal::Stream, start, self.port_audio_out.len()));
        self
    }

    /// `count` repeats of `names`, laid out instance-major. A name then refers
    /// to a group of `count` ports that `node.name >> inst(*).port` splits one
    /// per instance.
    pub fn control_out_group(mut self, names: &[&'static str], count: usize) -> Self {
        let start = self.port_audio_out.len();
        for _ in 0..count {
            for name in names {
                self.push_out(name);
            }
        }
        self.last_out = Some((Signal::Stream, start, self.port_audio_out.len()));
        self
    }

    pub fn midi_in_named(mut self, names: &[&'static str]) -> Self {
        let start = self.midi_in.len();
        for &name in names {
            push_port(
                &mut self.midi_in,
                name,
                PortKind::Midi(MidiUnpatched::Empty),
            );
        }
        self.last_in = Some((Signal::Midi, start, self.midi_in.len()));
        self
    }

    pub fn midi_out_named(mut self, names: &[&'static str]) -> Self {
        let start = self.midi_out.len();
        for &name in names {
            push_port(
                &mut self.midi_out,
                name,
                PortKind::Midi(MidiUnpatched::Empty),
            );
        }
        self.last_out = Some((Signal::Midi, start, self.midi_out.len()));
        self
    }

    /// Mark the most recently added input batch as the bare-`>>` target.
    pub fn default_in(mut self) -> Self {
        let batch = self
            .last_in
            .expect("default_in() requires a preceding input batch");
        for p in self.in_batch(batch) {
            p.default = true;
        }
        self
    }

    /// Mark the most recently added output batch as the bare-`>>` source.
    pub fn default_out(mut self) -> Self {
        let (signal, start, end) = self
            .last_out
            .expect("default_out() requires a preceding output batch");
        let list = match signal {
            Signal::Stream => &mut self.port_audio_out,
            Signal::Midi => &mut self.midi_out,
        };
        for p in &mut list[start..end] {
            p.default = true;
        }
        self
    }

    /// The most recently added stream input batch reads as silence when unpatched.
    pub fn silent_in(self) -> Self {
        self.set_last_in(
            "silent_in()",
            Signal::Stream,
            PortKind::Stream(Unpatched::Silent),
        )
    }

    /// The most recently added input batch must be connected for the graph to build.
    pub fn required_in(self) -> Self {
        let kind = match self.last_in {
            Some((Signal::Midi, ..)) => PortKind::Midi(MidiUnpatched::Required),
            _ => PortKind::Stream(Unpatched::Required),
        };
        self.set_last_in("required_in()", kind.signal(), kind)
    }

    /// The most recently added MIDI input batch reads the host's MIDI when unpatched.
    pub fn host_fallback(self) -> Self {
        self.set_last_in(
            "host_fallback()",
            Signal::Midi,
            PortKind::Midi(MidiUnpatched::Host),
        )
    }

    fn set_last_in(mut self, method: &str, expected: Signal, kind: PortKind) -> Self {
        let batch = self
            .last_in
            .unwrap_or_else(|| panic!("{method} requires a preceding input batch"));
        assert!(
            batch.0 == expected,
            "{method} applies to {expected:?} inputs, but the last input batch is {:?}",
            batch.0
        );
        for p in self.in_batch(batch) {
            p.kind = kind;
        }
        self
    }

    fn in_batch(&mut self, (signal, start, end): Batch) -> &mut [PortMeta] {
        match signal {
            Signal::Stream => &mut self.port_audio_in[start..end],
            Signal::Midi => &mut self.midi_in[start..end],
        }
    }

    pub fn build(self) -> Ports {
        self.into()
    }
}

/// Panics if a MIDI port shares a name with a stream port on the same side,
/// so a named connection always resolves to one port.
fn assert_unique_across_kinds(stream: &[PortMeta], midi: &[PortMeta], side: &str) {
    if let Some(p) = midi
        .iter()
        .find(|m| stream.iter().any(|s| s.name == m.name))
    {
        panic!(
            "MIDI {side} '{}' shares its name with a stream {side}; names must be unique per side",
            p.name
        );
    }
}

fn default_audio_in_name(i: usize, total: usize) -> &'static str {
    match total {
        1 => "in",
        2 => {
            if i == 0 {
                "l"
            } else {
                "r"
            }
        }
        _ => "in",
    }
}

fn default_audio_out_name(i: usize, total: usize) -> &'static str {
    match total {
        1 => "out",
        2 => {
            if i == 0 {
                "l"
            } else {
                "r"
            }
        }
        _ => "out",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn names(v: &Vec<PortMeta>) -> Vec<&'static str> {
        v.iter().map(|p| p.name).collect()
    }

    fn indices(v: &Vec<PortMeta>) -> Vec<usize> {
        v.iter().map(|p| p.index).collect()
    }

    #[test]
    fn test_default_audio_in_mono() {
        let ports = PortBuilder::default().audio_in(1).build();

        assert_eq!(names(&ports.ports_in), vec!["in"]);
        assert_eq!(indices(&ports.ports_in), vec![0]);
    }

    #[test]
    fn test_two_chans() {
        let chans = 2;
        let ports = PortBuilder::default().audio_out(chans).build();

        assert_eq!(ports.ports_out.iter().len(), 2);
    }

    #[test]
    fn test_default_audio_in_stereo() {
        let ports = PortBuilder::default().audio_in(2).build();

        assert_eq!(names(&ports.ports_in), vec!["l", "r"]);
        assert_eq!(indices(&ports.ports_in), vec![0, 1]);
    }

    #[test]
    fn test_default_audio_out_stereo() {
        let ports = PortBuilder::default().audio_out(2).build();

        assert_eq!(names(&ports.ports_out), vec!["l", "r"]);
        assert_eq!(indices(&ports.ports_out), vec![0, 1]);
    }

    #[test]
    fn test_named_audio_in() {
        let ports = PortBuilder::default()
            .audio_in_named(&["fm", "sidechain"])
            .build();

        assert_eq!(names(&ports.ports_in), vec!["fm", "sidechain"]);
        assert_eq!(indices(&ports.ports_in), vec![0, 1]);
    }

    #[test]
    fn test_named_audio_out() {
        let ports = PortBuilder::default()
            .audio_out_named(&["dry", "wet"])
            .build();

        assert_eq!(names(&ports.ports_out), vec!["dry", "wet"]);
        assert_eq!(indices(&ports.ports_out), vec![0, 1]);
    }

    #[test]
    fn test_mixed_audio_in() {
        let ports = PortBuilder::default()
            .audio_in(1) // ["in"]
            .audio_in_named(&["mod1", "mod2"]) // appended, indices continue
            .build();

        assert_eq!(names(&ports.ports_in), vec!["in", "mod1", "mod2"]);
        assert_eq!(indices(&ports.ports_in), vec![0, 1, 2]);
    }

    #[test]
    fn test_mixed_audio_out() {
        let ports = PortBuilder::default()
            .audio_out(1) // ["out"]
            .audio_out_named(&["aux"]) // appended
            .build();

        assert_eq!(names(&ports.ports_out), vec!["out", "aux"]);
        assert_eq!(indices(&ports.ports_out), vec![0, 1]);
    }

    #[test]
    fn test_all_port_categories() {
        let ports = PortBuilder::default()
            .audio_in(2)
            .audio_in_named(&["lfo"])
            .audio_out_named(&["dry", "wet"])
            .build();

        assert_eq!(names(&ports.ports_in), vec!["l", "r", "lfo"]);
        assert_eq!(names(&ports.ports_out), vec!["dry", "wet"]);

        assert_eq!(indices(&ports.ports_in), vec![0, 1, 2]);
        assert_eq!(indices(&ports.ports_out), vec![0, 1]);
    }

    #[test]
    fn test_named_and_default_share_flat_index_space() {
        let ports = PortBuilder::default()
            .audio_in(2)
            .default_in()
            .control_in_named(&["cutoff", "q"])
            .build();

        assert_eq!(names(&ports.ports_in), vec!["l", "r", "cutoff", "q"]);
        assert_eq!(indices(&ports.ports_in), vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_default_in_marks_last_batch() {
        let ports = PortBuilder::default()
            .audio_in(2)
            .default_in()
            .control_in_named(&["cutoff", "q"])
            .build();

        assert_eq!(ports.default_in(), vec![0, 1]);
    }

    #[test]
    fn unmarked_ports_are_not_defaults() {
        let ports = PortBuilder::default().audio_in(2).audio_out(2).build();
        assert!(ports.default_in().is_empty());
        assert!(ports.default_out().is_empty());
    }

    proptest! {
        // `default_in` returns exactly the indices of the batches marked with
        // `default_in()`.
        #[test]
        fn prop_default_in_tracks_marked_batches(
            batches in prop::collection::vec((1usize..=4, any::<bool>()), 1..6)
        ) {
            let mut builder = PortBuilder::default();
            let mut marked: Vec<usize> = Vec::new();
            let mut next = 0;
            for (size, mark) in &batches {
                builder = builder.audio_in(*size);
                if *mark {
                    builder = builder.default_in();
                    marked.extend(next..next + size);
                }
                next += size;
            }
            let ports = builder.build();
            prop_assert_eq!(ports.default_in(), marked);
        }
    }

    #[test]
    fn midi_ports_are_indexed_apart_from_stream_ports() {
        let ports = PortBuilder::default()
            .midi_in_named(&["notes"])
            .control_in_named(&["clock"])
            .midi_in_named(&["extra"])
            .midi_out_named(&["out"])
            .build();

        assert_eq!(names(&ports.ports_in), vec!["clock"]);
        assert_eq!(names(&ports.midi_in), vec!["notes", "extra"]);
        assert_eq!(indices(&ports.midi_in), vec![0, 1]);
        assert_eq!(indices(&ports.ports_in), vec![0]);
    }

    #[test]
    fn unpatched_behaviour_applies_to_the_last_batch_of_its_kind() {
        let ports = PortBuilder::default()
            .midi_in_named(&["host"])
            .host_fallback()
            .midi_in_named(&["needed"])
            .required_in()
            .audio_in(1)
            .silent_in()
            .build();

        assert_eq!(ports.midi_in[0].kind, PortKind::Midi(MidiUnpatched::Host));
        assert_eq!(
            ports.midi_in[1].kind,
            PortKind::Midi(MidiUnpatched::Required)
        );
        assert_eq!(ports.ports_in[0].kind, PortKind::Stream(Unpatched::Silent));
    }

    #[test]
    #[should_panic(expected = "host_fallback() applies to Midi inputs")]
    fn host_fallback_on_a_stream_batch_panics() {
        PortBuilder::default().audio_in(1).host_fallback();
    }

    #[test]
    #[should_panic(expected = "silent_in() applies to Stream inputs")]
    fn silent_in_on_a_midi_batch_panics() {
        PortBuilder::default().midi_in_named(&["notes"]).silent_in();
    }

    #[test]
    #[should_panic(expected = "shares its name with a stream input")]
    fn a_midi_name_cannot_repeat_a_stream_name() {
        PortBuilder::default()
            .audio_in(1)
            .midi_in_named(&["in"])
            .build();
    }

    #[test]
    fn bare_output_is_midi_when_marked_or_the_only_kind() {
        let midi_only = PortBuilder::default().midi_out_named(&["out"]).build();
        let both = PortBuilder::default()
            .audio_out(1)
            .midi_out_named(&["notes"])
            .build();
        let marked = PortBuilder::default()
            .audio_out(1)
            .midi_out_named(&["notes"])
            .default_out()
            .build();

        assert_eq!(midi_only.default_out_signal(), Signal::Midi);
        assert_eq!(both.default_out_signal(), Signal::Stream);
        assert_eq!(marked.default_out_signal(), Signal::Midi);
    }

    #[test]
    fn test_zero_in_zero_out() {
        let ports = PortBuilder::default().audio_in(0).audio_out(0).build();

        assert!(ports.ports_in.iter().len() == 0);
        assert!(ports.ports_out.iter().len() == 0);
    }
}

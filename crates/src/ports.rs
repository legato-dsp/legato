#[derive(Clone, Debug)]
pub struct PortMeta {
    pub name: &'static str,
    pub index: usize,
    /// Whether a bare `>>` auto-maps onto this port.
    pub default: bool,
}

#[derive(Clone, Debug)]
pub struct Ports {
    pub ports_in: Vec<PortMeta>,
    pub ports_out: Vec<PortMeta>,
}

impl Ports {
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

    /// Input indices a bare `>>` targets: the ports marked default, or every input if none are.
    pub fn default_in(&self) -> Vec<usize> {
        default_indices(&self.ports_in)
    }

    /// Output indices a bare `>>` reads from: the ports marked default, or every output if none are.
    pub fn default_out(&self) -> Vec<usize> {
        default_indices(&self.ports_out)
    }
}

fn default_indices(ports: &[PortMeta]) -> Vec<usize> {
    let marked: Vec<usize> = ports
        .iter()
        .filter(|p| p.default)
        .map(|p| p.index)
        .collect();
    if marked.is_empty() {
        ports.iter().map(|p| p.index).collect()
    } else {
        marked
    }
}

impl From<PortBuilder> for Ports {
    fn from(builder: PortBuilder) -> Self {
        Ports {
            ports_in: builder.port_audio_in,
            ports_out: builder.port_audio_out,
        }
    }
}

pub trait Ported {
    fn get_ports(&self) -> &Ports;
}

#[derive(Default)]
pub struct PortBuilder {
    port_audio_in: Vec<PortMeta>,
    port_audio_out: Vec<PortMeta>,
    last_in: Option<(usize, usize)>,
    last_out: Option<(usize, usize)>,
}

impl PortBuilder {
    fn push_in(&mut self, name: &'static str) {
        let index = self.port_audio_in.len();
        self.port_audio_in.push(PortMeta {
            name,
            index,
            default: false,
        });
    }

    fn push_out(&mut self, name: &'static str) {
        let index = self.port_audio_out.len();
        self.port_audio_out.push(PortMeta {
            name,
            index,
            default: false,
        });
    }

    pub fn audio_in(mut self, count: usize) -> Self {
        let start = self.port_audio_in.len();
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count));
        }
        self.last_in = Some((start, self.port_audio_in.len()));
        self
    }

    pub fn audio_out(mut self, count: usize) -> Self {
        let start = self.port_audio_out.len();
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count));
        }
        self.last_out = Some((start, self.port_audio_out.len()));
        self
    }

    pub fn audio_in_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_in.len();
        for name in names {
            self.push_in(name);
        }
        self.last_in = Some((start, self.port_audio_in.len()));
        self
    }

    pub fn audio_out_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_out.len();
        for name in names {
            self.push_out(name);
        }
        self.last_out = Some((start, self.port_audio_out.len()));
        self
    }

    pub fn control_in(mut self, count: usize) -> Self {
        let start = self.port_audio_in.len();
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count));
        }
        self.last_in = Some((start, self.port_audio_in.len()));
        self
    }

    pub fn control_out(mut self, count: usize) -> Self {
        let start = self.port_audio_out.len();
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count));
        }
        self.last_out = Some((start, self.port_audio_out.len()));
        self
    }

    pub fn control_in_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_in.len();
        for name in names {
            self.push_in(name);
        }
        self.last_in = Some((start, self.port_audio_in.len()));
        self
    }

    pub fn control_out_named(mut self, names: &[&'static str]) -> Self {
        let start = self.port_audio_out.len();
        for name in names {
            self.push_out(name);
        }
        self.last_out = Some((start, self.port_audio_out.len()));
        self
    }

    /// Mark the most recently added input batch as the bare-`>>` target.
    pub fn default_in(mut self) -> Self {
        let (start, end) = self
            .last_in
            .expect("default_in() requires a preceding input batch");

        for p in &mut self.port_audio_in[start..end] {
            p.default = true;
        }

        self
    }

    /// Mark the most recently added output batch as the bare-`>>` source.
    pub fn default_out(mut self) -> Self {
        let (start, end) = self
            .last_out
            .expect("default_out() requires a preceding output batch");
        for p in &mut self.port_audio_out[start..end] {
            p.default = true;
        }
        self
    }

    pub fn build(self) -> Ports {
        self.into()
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
    fn test_default_in_falls_back_to_all_when_unmarked() {
        let ports = PortBuilder::default().audio_in(2).build();
        assert_eq!(ports.default_in(), vec![0, 1]);
    }

    proptest! {
        // `default_in` returns exactly the indices of the batches marked with
        // `default_in()`, or every input when no batch is marked.
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
            let want = if marked.is_empty() { (0..next).collect() } else { marked };
            prop_assert_eq!(ports.default_in(), want);
        }
    }

    #[test]
    fn test_zero_in_zero_out() {
        let ports = PortBuilder::default().audio_in(0).audio_out(0).build();

        assert!(ports.ports_in.iter().len() == 0);
        assert!(ports.ports_out.iter().len() == 0);
    }
}

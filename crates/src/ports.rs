/// Signal vs control value; independent of rate, it only governs how portless endpoints auto-map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    Audio,
    Control,
}

#[derive(Clone, Debug)]
pub struct PortMeta {
    pub name: &'static str,
    pub index: usize,
    pub kind: PortKind,
}

#[derive(Clone, Debug)]
pub struct Ports {
    pub audio_in: Vec<PortMeta>,
    pub audio_out: Vec<PortMeta>,
}

impl Ports {
    pub fn find_port_in(&self, name: &String) -> Option<PortMeta> {
        if let Some(port) = self.audio_in.iter().find(|x| x.name == name) {
            return Some(port.clone());
        }
        None
    }
    pub fn find_port_out(&self, name: &String) -> Option<PortMeta> {
        if let Some(port) = self.audio_out.iter().find(|x| x.name == name) {
            return Some(port.clone());
        }
        None
    }
}

impl From<PortBuilder> for Ports {
    fn from(builder: PortBuilder) -> Self {
        Ports {
            audio_in: builder.port_audio_in,
            audio_out: builder.port_audio_out,
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
}

impl PortBuilder {
    fn push_in(&mut self, name: &'static str, kind: PortKind) {
        let index = self.port_audio_in.len();
        self.port_audio_in.push(PortMeta { name, index, kind });
    }

    fn push_out(&mut self, name: &'static str, kind: PortKind) {
        let index = self.port_audio_out.len();
        self.port_audio_out.push(PortMeta { name, index, kind });
    }

    pub fn audio_in(mut self, count: usize) -> Self {
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count), PortKind::Audio);
        }
        self
    }

    pub fn audio_out(mut self, count: usize) -> Self {
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count), PortKind::Audio);
        }
        self
    }

    pub fn audio_in_named(mut self, names: &[&'static str]) -> Self {
        for name in names {
            self.push_in(name, PortKind::Audio);
        }
        self
    }

    pub fn audio_out_named(mut self, names: &[&'static str]) -> Self {
        for name in names {
            self.push_out(name, PortKind::Audio);
        }
        self
    }

    pub fn control_in(mut self, count: usize) -> Self {
        for i in 0..count {
            self.push_in(default_audio_in_name(i, count), PortKind::Control);
        }
        self
    }

    pub fn control_out(mut self, count: usize) -> Self {
        for i in 0..count {
            self.push_out(default_audio_out_name(i, count), PortKind::Control);
        }
        self
    }

    pub fn control_in_named(mut self, names: &[&'static str]) -> Self {
        for name in names {
            self.push_in(name, PortKind::Control);
        }
        self
    }

    pub fn control_out_named(mut self, names: &[&'static str]) -> Self {
        for name in names {
            self.push_out(name, PortKind::Control);
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

    fn names(v: &Vec<PortMeta>) -> Vec<&'static str> {
        v.iter().map(|p| p.name).collect()
    }

    fn indices(v: &Vec<PortMeta>) -> Vec<usize> {
        v.iter().map(|p| p.index).collect()
    }

    #[test]
    fn test_default_audio_in_mono() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in(1)
        .build();

        assert_eq!(names(&ports.audio_in), vec!["in"]);
        assert_eq!(indices(&ports.audio_in), vec![0]);
    }

    #[test]
    fn test_two_chans() {
        let chans = 2;
        let ports = PortBuilder::default().audio_out(chans).build();

        assert_eq!(ports.audio_out.iter().len(), 2);
    }

    #[test]
    fn test_default_audio_in_stereo() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in(2)
        .build();

        assert_eq!(names(&ports.audio_in), vec!["l", "r"]);
        assert_eq!(indices(&ports.audio_in), vec![0, 1]);
    }

    #[test]
    fn test_default_audio_out_stereo() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_out(2)
        .build();

        assert_eq!(names(&ports.audio_out), vec!["l", "r"]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
    }

    #[test]
    fn test_named_audio_in() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in_named(&["fm", "sidechain"])
        .build();

        assert_eq!(names(&ports.audio_in), vec!["fm", "sidechain"]);
        assert_eq!(indices(&ports.audio_in), vec![0, 1]);
    }

    #[test]
    fn test_named_audio_out() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_out_named(&["dry", "wet"])
        .build();

        assert_eq!(names(&ports.audio_out), vec!["dry", "wet"]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
    }

    #[test]
    fn test_mixed_audio_in() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in(1) // ["in"]
        .audio_in_named(&["mod1", "mod2"]) // appended, indices continue
        .build();

        assert_eq!(names(&ports.audio_in), vec!["in", "mod1", "mod2"]);
        assert_eq!(indices(&ports.audio_in), vec![0, 1, 2]);
    }

    #[test]
    fn test_mixed_audio_out() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_out(1) // ["out"]
        .audio_out_named(&["aux"]) // appended
        .build();

        assert_eq!(names(&ports.audio_out), vec!["out", "aux"]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
    }

    #[test]
    fn test_all_port_categories() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in(2)
        .audio_in_named(&["lfo"])
        .audio_out_named(&["dry", "wet"])
        .build();

        assert_eq!(names(&ports.audio_in), vec!["l", "r", "lfo"]);
        assert_eq!(names(&ports.audio_out), vec!["dry", "wet"]);

        assert_eq!(indices(&ports.audio_in), vec![0, 1, 2]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
    }

    #[test]
    fn test_control_shares_flat_index_space() {
        let ports = PortBuilder::default()
            .audio_in(2)
            .control_in_named(&["cutoff", "q"])
            .build();

        assert_eq!(names(&ports.audio_in), vec!["l", "r", "cutoff", "q"]);
        assert_eq!(indices(&ports.audio_in), vec![0, 1, 2, 3]);
        let kinds: Vec<PortKind> = ports.audio_in.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![
                PortKind::Audio,
                PortKind::Audio,
                PortKind::Control,
                PortKind::Control
            ]
        );
    }

    #[test]
    fn test_zero_in_zero_out() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
        }
        .audio_in(0)
        .audio_out(0)
        .build();

        assert!(ports.audio_in.iter().len() == 0);
        assert!(ports.audio_out.iter().len() == 0);
    }
}

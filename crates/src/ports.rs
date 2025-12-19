#[derive(Clone, Debug)]
pub struct PortMeta {
    pub name: &'static str,
    pub index: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum NodeKind {
    Audio,
    Control,
}

#[derive(Clone, Debug)]
pub struct Ports {
    pub audio_in: Vec<PortMeta>,
    pub audio_out: Vec<PortMeta>,
    pub control_in: Vec<PortMeta>,
    pub control_out: Vec<PortMeta>,
}

impl Ports {
    pub fn find_port_in(&self, name: &String) -> Option<(PortMeta, NodeKind)> {
        if let Some(port) = self.audio_in.iter().find(|x| x.name == name) {
            return Some((port.clone(), NodeKind::Audio));
        }
        if let Some(port) = self.control_in.iter().find(|x| x.name == name) {
            return Some((port.clone(), NodeKind::Control));
        }
        None
    }
    pub fn find_port_out(&self, name: &String) -> Option<(PortMeta, NodeKind)> {
        if let Some(port) = self.audio_out.iter().find(|x| x.name == name) {
            return Some((port.clone(), NodeKind::Audio));
        }
        if let Some(port) = self.control_out.iter().find(|x| x.name == name) {
            return Some((port.clone(), NodeKind::Control));
        }
        None
    }
}

impl From<PortBuilder> for Ports {
    fn from(builder: PortBuilder) -> Self {
        Ports {
            audio_in: builder.port_audio_in,
            audio_out: builder.port_audio_out,
            control_in: builder.port_control_in,
            control_out: builder.port_control_out,
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
    port_control_in: Vec<PortMeta>,
    port_control_out: Vec<PortMeta>,
}

impl PortBuilder {
    pub fn audio_in(mut self, count: usize) -> Self {
        for i in 0..count {
            self.port_audio_in.push(PortMeta {
                name: default_audio_in_name(i, count),
                index: i,
            });
        }
        self
    }

    pub fn audio_out(mut self, count: usize) -> Self {
        for i in 0..count {
            self.port_audio_out.push(PortMeta {
                name: default_audio_out_name(i, count),
                index: i,
            });
        }
        self
    }

    pub fn control_in(mut self, count: usize) -> Self {
        for i in 0..count {
            self.port_control_in.push(PortMeta {
                name: "ctrl_in",
                index: i,
            });
        }
        self
    }

    pub fn control_out(mut self, count: usize) -> Self {
        for i in 0..count {
            self.port_control_out.push(PortMeta {
                name: "ctrl_out",
                index: i,
            });
        }
        self
    }

    pub fn audio_in_named(mut self, names: &[&'static str]) -> Self {
        let index = self.port_audio_in.len();
        for (i, name) in names.iter().enumerate() {
            self.port_audio_in.push(PortMeta {
                name,
                index: index + i,
            });
        }
        self
    }

    pub fn audio_out_named(mut self, names: &[&'static str]) -> Self {
        let index = self.port_audio_out.len();
        for (i, name) in names.iter().enumerate() {
            self.port_audio_out.push(PortMeta {
                name,
                index: index + i,
            });
        }
        self
    }

    pub fn control_in_named(mut self, names: &[&'static str]) -> Self {
        let index = self.port_control_in.len();
        for (i, name) in names.iter().enumerate() {
            self.port_control_in.push(PortMeta {
                name,
                index: index + i,
            });
        }
        self
    }

    pub fn control_out_named(mut self, names: &[&'static str]) -> Self {
        let index = self.port_control_out.len();
        for (i, name) in names.iter().enumerate() {
            self.port_control_out.push(PortMeta {
                name,
                index: index + i,
            });
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
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
        }
        .audio_out_named(&["dry", "wet"])
        .build();

        assert_eq!(names(&ports.audio_out), vec!["dry", "wet"]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
    }

    #[test]
    fn test_named_control_in() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
            port_control_in: vec![],
            port_control_out: vec![],
        }
        .control_in_named(&["cutoff", "res"])
        .build();

        assert_eq!(names(&ports.control_in), vec!["cutoff", "res"]);
        assert_eq!(indices(&ports.control_in), vec![0, 1]);
    }

    #[test]
    fn test_mixed_audio_in() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
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
            port_control_in: vec![],
            port_control_out: vec![],
        }
        .audio_in(2)
        .audio_in_named(&["lfo"])
        .audio_out_named(&["dry", "wet"])
        .control_in(1)
        .control_out_named(&["env"])
        .build();

        assert_eq!(names(&ports.audio_in), vec!["l", "r", "lfo"]);
        assert_eq!(names(&ports.audio_out), vec!["dry", "wet"]);
        assert_eq!(names(&ports.control_in), vec!["ctrl_in"]);
        assert_eq!(names(&ports.control_out), vec!["env"]);

        assert_eq!(indices(&ports.audio_in), vec![0, 1, 2]);
        assert_eq!(indices(&ports.audio_out), vec![0, 1]);
        assert_eq!(indices(&ports.control_in), vec![0]);
        assert_eq!(indices(&ports.control_out), vec![0]);
    }

    #[test]
    fn test_zero_in_zero_out() {
        let ports = PortBuilder {
            port_audio_in: vec![],
            port_audio_out: vec![],
            port_control_in: vec![],
            port_control_out: vec![],
        }
        .audio_in(0)
        .audio_out(0)
        .control_in(0)
        .control_out(0)
        .build();

        assert!(ports.audio_in.iter().len() == 0);
        assert!(ports.audio_out.iter().len() == 0);
        assert!(ports.control_in.iter().len() == 0);
        assert!(ports.control_out.iter().len() == 0);
    }
}

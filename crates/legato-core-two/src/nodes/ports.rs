pub struct PortMeta {
    pub name: &'static str,
    pub index: usize
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PortRate {
    Audio,
    Control
}

pub struct Ports {
    pub audio_in: Option<Vec<PortMeta>>,
    pub audio_out: Option<Vec<PortMeta>>,
    pub control_in: Option<Vec<PortMeta>>,
    pub control_out: Option<Vec<PortMeta>>,
}

pub trait Ported {
    fn get_ports(&self) -> &Ports;
}
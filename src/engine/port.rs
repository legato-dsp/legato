pub enum PortBehavior {
    Default, // Input: Take the first sample, Output: Fill the frame
    Sum,
    SumNormalized,
    Mute,
}

pub struct Port {
    pub name: &'static str,
    pub index: usize,
    pub behavior: PortBehavior,
}

pub trait Ported {
    fn get_input_ports(&self) -> &'static [Port];
    fn get_output_ports(&self) -> &'static [Port]; 
}
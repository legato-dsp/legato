use crate::nodes::{Node, NodeInputs, ports::{PortBuilder, Ported, Ports}};


pub enum MixDownType {

}

/// A simple node for mixing down N -> M channels, assuming N/M is a track.
/// 
/// In the future, we will maybe add a matrix mixer for more interesting options
pub struct MixDown {
    chans_in: usize,
    chans_out: usize,
    ports: Ports
}

impl MixDown {
    pub fn new(chans_in: usize, chans_out: usize) -> Self {
        Self {
            chans_in,
            chans_out,
            ports: PortBuilder::default()
                .audio_in(chans_in)
                .audio_out(chans_out)
                .build()
        }   
    }
}

impl Node for MixDown {
    fn process<'a>(&mut self, ctx: &mut crate::runtime::context::AudioContext, 
            ai: &NodeInputs,
            ao: &mut NodeInputs,
            _: &NodeInputs,
            _: &mut NodeInputs,
        ) {

        
        
    }
}






impl Ported for MixDown {
    fn get_ports(&self) -> &Ports {
        &self.ports
    }
}
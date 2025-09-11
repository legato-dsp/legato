use generic_array::{ArrayLength, GenericArray};
use typenum::{Unsigned, U1, U2};

/// This will determine how ports audio will fan in and out, etc.
/// TODO: Actually implement this
#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum MultipleInputBehavior {
    #[default]
    Default, // Input: Take the first sample, Output: Fill the frame
    Sum,
    SumNormalized,
    Mute,
}


#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum SampleAlg {
    UpSample(UpsampleAlg),
    DownSample(DownsampleAlg)
}
impl Default for SampleAlg {
    fn default() -> Self {
        SampleAlg::UpSample(UpsampleAlg::ZOH)
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub struct PortMeta {
    pub name: &'static str,
    pub index: usize,
}

/// Ports are responsible to present the preferred algorithm for up and down sampling.
/// 
/// For instance, if a user connects a lower fidelity control rate LFO to an audio rate, 
/// it would likely be better to do something like a filter, lerp, etc. than sample and hold.
/// 
/// Similarly, something really sensitive to clock values should take the first or last 
/// sample, as opposed to taking an average.

pub struct AudioInputPort {
    pub meta: PortMeta,
    pub input_behavior: MultipleInputBehavior,
    pub resample: UpsampleAlg,
}
pub struct AudioOutputPort {
    pub meta: PortMeta,
}
pub struct  ControlInputPort {
    pub meta: PortMeta,
    pub input_behavior: MultipleInputBehavior,
    pub resample: DownsampleAlg,
}
pub struct ControlOutputPort {
    pub meta: PortMeta,
}

/// Ports can specify how the incoming signal should be resampled for audio. 
#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
pub enum UpsampleAlg {
    #[default]
    ZOH,
    Lerp, // TODO: Consider more options?
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
pub enum PortRate {
    #[default]
    Audio,
    Control
}


#[derive(Debug, PartialEq, Eq, Clone, Copy, Default, Hash)]
pub enum DownsampleAlg {
    #[default]
    FirstSample, // First sample
    // LastSample, // Lowest latency
    // Average,
    // TODO: one pole filter, more options?
}

pub trait Ported<Ai, Ao, Ci, Co>
where
    Ai: Unsigned + ArrayLength,
    Ao: Unsigned + ArrayLength,
    Ci: Unsigned + ArrayLength,
    Co: Unsigned + ArrayLength,
{
    fn get_audio_inputs(&self) -> &GenericArray<AudioInputPort, Ai>;
    fn get_audio_outputs(&self) -> &GenericArray<AudioOutputPort, Ao>;
    fn get_control_inputs(&self) -> &GenericArray<ControlInputPort, Ci>;
    fn get_control_outputs(&self) -> &GenericArray<ControlOutputPort, Co>;
}

pub struct Ports<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength
{
    pub audio_inputs: Option<GenericArray<AudioInputPort, Ai>>,
    pub audio_outputs: Option<GenericArray<AudioOutputPort, Ao>>,
    pub control_inputs: Option<GenericArray<ControlInputPort, Ci>>,
    pub control_outputs: Option<GenericArray<ControlOutputPort, Co>>,   
}
impl<Ai, Ao, Ci, Co> Ports<Ai, Ao, Ci, Co>
where
    Ai: ArrayLength,
    Ao: ArrayLength,
    Ci: ArrayLength,
    Co: ArrayLength
{
    pub fn get_audio_inputs(&self) -> Option<&[AudioInputPort]> {
        self.audio_inputs.as_deref()
    }
    pub fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]> {
        self.audio_outputs.as_deref()
    }
    pub fn get_control_inputs(&self) -> Option<&[ControlInputPort]> {
        self.control_inputs.as_deref()
    }
    pub fn get_control_outputs(&self) -> Option<&[ControlOutputPort]> {
        self.control_outputs.as_deref()
    }
}

/// A trait allowing us to erase the specific input and output
/// types to store them more easily.
pub trait PortedErased {
    fn get_audio_inputs(&self) -> Option<&[AudioInputPort]>;
    fn get_audio_outputs(&self) -> Option<&[AudioOutputPort]>;
    fn get_control_inputs(&self) -> Option<&[ControlInputPort]>;
    fn get_control_outputs(&self) -> Option<&[ControlOutputPort]>;
}

/// Utility type for one channel
pub type Mono = U1;
/// Utility type for two channels
pub type Stereo = U2;

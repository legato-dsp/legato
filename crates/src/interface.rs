use std::marker::PhantomData;

use assert_no_alloc::assert_no_alloc;
use cpal::{
    BuildStreamError, Device, FromSample, Host, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::config::Config;
use crate::input::{CpalInputError, DeviceSelection};
use crate::{LegatoApp, input::build_input_stream};

#[derive(Debug)]
pub enum InterfaceError {
    NoDefaultOutputDevice,
    OutputDeviceNotFound(String),
    EnumerateDevices(cpal::DevicesError),
    BuildOutputStream(BuildStreamError),
    PlayOutputStream(cpal::PlayStreamError),
    Input(CpalInputError),
}

impl std::fmt::Display for InterfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultOutputDevice => write!(f, "No default output device available"),
            Self::OutputDeviceNotFound(n) => write!(f, "Output device not found: {n}"),
            Self::EnumerateDevices(e) => write!(f, "Failed to enumerate devices: {e}"),
            Self::BuildOutputStream(e) => write!(f, "Failed to build output stream: {e}"),
            Self::PlayOutputStream(e) => write!(f, "Failed to start output stream: {e}"),
            Self::Input(e) => write!(f, "Input error: {e}"),
        }
    }
}

impl std::error::Error for InterfaceError {}

impl From<CpalInputError> for InterfaceError {
    fn from(e: CpalInputError) -> Self {
        Self::Input(e)
    }
}

pub struct InputSpec {
    pub producer: rtrb::Producer<f32>,
    pub chans: usize,
    pub device: DeviceSelection,
}

pub struct AudioInterfaceBuilder<'a> {
    host: &'a Host,
    config: Config,
    output_device: DeviceSelection,
    inputs: Vec<InputSpec>,
    visualization: Option<rtrb::Producer<f32>>,
}

impl<'a> AudioInterfaceBuilder<'a> {
    pub fn new(host: &'a Host, config: Config) -> Self {
        Self {
            host,
            config,
            output_device: DeviceSelection::Default,
            inputs: Vec::new(),
            visualization: None,
        }
    }

    pub fn output_device(mut self, sel: DeviceSelection) -> Self {
        self.output_device = sel;
        self
    }

    pub fn input(mut self, spec: InputSpec) -> Self {
        self.inputs.push(spec);
        self
    }

    pub fn visualization_producer(mut self, p: rtrb::Producer<f32>) -> Self {
        self.visualization = Some(p);
        self
    }

    /// This function takes ownership of the LegatoApp
    ///
    /// It is responsible now for the audio runtime, as well as the input and output
    /// CPAL threads.
    ///
    /// If Host is dropped, we lose the connection to the specific audio API we are using.
    pub fn build(self, app: LegatoApp) -> Result<AudioInterface<'a>, InterfaceError> {
        let output_device = resolve_output_device(self.host, &self.output_device)?;

        let stream_config = StreamConfig {
            channels: self.config.channels as u16,
            sample_rate: cpal::SampleRate(self.config.sample_rate as u32),
            buffer_size: cpal::BufferSize::Fixed(self.config.block_size as u32),
        };

        let mut input_streams = Vec::with_capacity(self.inputs.len());
        for spec in self.inputs {
            let stream = build_input_stream(
                self.host,
                spec.producer,
                spec.chans,
                self.config.sample_rate as u32,
                self.config.block_size,
                spec.device,
            )?;
            input_streams.push(stream);
        }

        let output_stream =
            build_output_stream(&output_device, &stream_config, app, self.visualization)?;
        output_stream
            .play()
            .map_err(InterfaceError::PlayOutputStream)?;

        Ok(AudioInterface {
            _output_stream: output_stream,
            _input_streams: input_streams,
            _host: PhantomData,
        })
    }
}

pub struct AudioInterface<'a> {
    _output_stream: Stream,
    _input_streams: Vec<Stream>,
    _host: PhantomData<&'a Host>,
}

impl<'a> AudioInterface<'a> {
    pub fn builder(host: &'a Host, config: Config) -> AudioInterfaceBuilder<'a> {
        AudioInterfaceBuilder::new(host, config)
    }

    pub fn run_forever(self) -> ! {
        loop {
            std::thread::park();
        }
    }
}

fn resolve_output_device(host: &Host, sel: &DeviceSelection) -> Result<Device, InterfaceError> {
    match sel {
        DeviceSelection::Default => host
            .default_output_device()
            .ok_or(InterfaceError::NoDefaultOutputDevice),
        DeviceSelection::ByName(name) => {
            let lower = name.to_lowercase();
            host.output_devices()
                .map_err(InterfaceError::EnumerateDevices)?
                .find(|d| {
                    d.name()
                        .map(|n| n.to_lowercase().contains(&lower))
                        .unwrap_or(false)
                })
                .ok_or_else(|| InterfaceError::OutputDeviceNotFound(name.clone()))
        }
    }
}

fn build_output_stream(
    device: &Device,
    stream_config: &StreamConfig,
    mut app: LegatoApp,
    mut viz: Option<rtrb::Producer<f32>>,
) -> Result<Stream, InterfaceError> {
    let cfg = stream_config.clone();
    device
        .build_output_stream(
            stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                assert_no_alloc(|| write_block(data, &cfg, &mut app, viz.as_mut()))
            },
            |err| eprintln!("Output stream error: {err}"),
            None,
        )
        .map_err(InterfaceError::BuildOutputStream)
}

#[inline(always)]
fn write_block<T>(
    output: &mut [T],
    config: &StreamConfig,
    app: &mut LegatoApp,
    visualization_producer: Option<&mut rtrb::Producer<f32>>, // TODO: Something more eloquent for per node outputs?
) where
    T: SizedSample + FromSample<f64>,
{
    let next_block_view = app.next_block(None);
    let next_block = &next_block_view.channels[0..next_block_view.chans];
    let chans = config.channels as usize;

    if let Some(producer) = visualization_producer
        && next_block.len() >= 2
    {
        write_stereo_mixdown(producer, next_block[0], next_block[1]);
    }

    for (frame_index, frame) in output.chunks_mut(chans).enumerate() {
        for (channel, sample) in frame.iter_mut().enumerate() {
            *sample = T::from_sample(next_block[channel][frame_index] as f64);
        }
    }
}

#[inline]
fn write_stereo_mixdown(producer: &mut rtrb::Producer<f32>, l: &[f32], r: &[f32]) {
    let n = l.len().min(r.len());
    for i in 0..n {
        let _ = producer.push((l[i] + r[i]) * 0.5);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_mixdown_averages_channels() {
        let (mut p, mut c) = rtrb::RingBuffer::<f32>::new(8);
        let l = [0.0, 1.0, 0.5, -0.5];
        let r = [1.0, 0.0, 0.5, 0.5];
        write_stereo_mixdown(&mut p, &l, &r);
        assert_eq!(c.pop().ok(), Some(0.5));
        assert_eq!(c.pop().ok(), Some(0.5));
        assert_eq!(c.pop().ok(), Some(0.5));
        assert_eq!(c.pop().ok(), Some(0.0));
    }

    #[test]
    fn stereo_mixdown_drops_when_full() {
        let (mut p, mut c) = rtrb::RingBuffer::<f32>::new(2);
        let l = [1.0, 1.0, 1.0, 1.0];
        let r = [1.0, 1.0, 1.0, 1.0];
        write_stereo_mixdown(&mut p, &l, &r);
        assert_eq!(c.pop().ok(), Some(1.0));
        assert_eq!(c.pop().ok(), Some(1.0));
        assert!(c.pop().is_err());
    }
}

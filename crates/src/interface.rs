//! A wrapper around `cpal` for generic input and output devices, as well as visualization streams.

use std::marker::PhantomData;

use assert_no_alloc::assert_no_alloc;
use cpal::{
    Device, FromSample, Host, SizedSample, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

use crate::config::Config;
use crate::input::{CpalInputError, DeviceSelection};
use crate::{LegatoApp, input::build_input_stream};

#[derive(Debug)]
pub enum InterfaceError {
    NoDefaultOutputDevice,
    OutputDeviceNotFound(String),
    EnumerateDevices(cpal::Error),
    BuildOutputStream(cpal::Error),
    PlayOutputStream(cpal::Error),
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

/// A builder that can construct an [`AudioInterface`] struct.
///
/// This is used to conveniently add various features, such as
/// adding a visualization producter/receiver pair, or adding
/// external input devices.
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
            sample_rate: self.config.sample_rate as u32,
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
                .find(|d| d.to_string().to_lowercase().contains(&lower))
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
    let mut cursor = BlockCursor::new(&app, stream_config.channels as usize);
    device
        .build_output_stream(
            stream_config.clone(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                assert_no_alloc(|| cursor.write(data, &mut app, viz.as_mut()))
            },
            |err| eprintln!("Output stream error: {err}"),
            None,
        )
        .map_err(InterfaceError::BuildOutputStream)
}

/// Serves interleaved callbacks of any length from fixed-size graph blocks.
///
/// `BufferSize::Fixed` is a request, not a guarantee: devices may deliver
/// shorter, longer or varying callbacks. Blocks are pulled on demand, so
/// this adds no latency.
struct BlockCursor {
    block: Box<[f32]>, // graph chans * block_size, planar
    block_size: usize,
    graph_chans: usize,
    device_chans: usize,
    pos: usize,
}

impl BlockCursor {
    fn new(app: &LegatoApp, device_chans: usize) -> Self {
        let config = app.get_config();
        Self {
            block: vec![0.0; config.channels * config.block_size].into(),
            block_size: config.block_size,
            graph_chans: config.channels,
            device_chans,
            pos: config.block_size,
        }
    }

    #[inline(always)]
    fn write<T>(
        &mut self,
        output: &mut [T],
        app: &mut LegatoApp,
        mut visualization_producer: Option<&mut rtrb::Producer<f32>>, // TODO: Something more eloquent for per node outputs?
    ) where
        T: SizedSample + FromSample<f64>,
    {
        for frame in output.chunks_mut(self.device_chans) {
            if self.pos == self.block_size {
                self.pull(app, visualization_producer.as_deref_mut());
            }
            for (channel, sample) in frame.iter_mut().enumerate() {
                let value = if channel < self.graph_chans {
                    self.block[channel * self.block_size + self.pos]
                } else {
                    0.0
                };
                *sample = T::from_sample(value as f64);
            }
            self.pos += 1;
        }
    }

    fn pull(&mut self, app: &mut LegatoApp, viz: Option<&mut rtrb::Producer<f32>>) {
        let view = app.next_block();
        let chans = &view.channels[..view.chans];

        if let Some(producer) = viz
            && chans.len() >= 2
        {
            write_stereo_mixdown(producer, chans[0], chans[1]);
        }

        for (c, out) in self.block.chunks_exact_mut(self.block_size).enumerate() {
            match chans.get(c) {
                Some(chan) => out.copy_from_slice(chan),
                None => out.fill(0.0),
            }
        }
        self.pos = 0;
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
    use crate::builder::{LegatoBuilder, Unconfigured};

    fn sine_app() -> LegatoApp {
        let config = Config {
            sample_rate: 48_000,
            block_size: 64,
            channels: 2,
            rt_capacity: 0,
        };
        let src = r#"
            audio {
                sine { freq: 440.0, chans: 2 },
            }

            { sine }
        "#;
        LegatoBuilder::<Unconfigured>::new(config)
            .build_dsl(src)
            .expect("graph should build")
            .0
    }

    fn render(callbacks: &[usize], device_chans: usize) -> Vec<f32> {
        let mut app = sine_app();
        let mut cursor = BlockCursor::new(&app, device_chans);
        let mut out = Vec::new();
        for &frames in callbacks {
            let mut data = vec![f32::NAN; frames * device_chans];
            cursor.write(&mut data, &mut app, None);
            out.extend(data);
        }
        out
    }

    #[test]
    fn callback_size_does_not_change_the_rendered_stream() {
        let fixed = render(&[64; 5], 2);
        let ragged = render(&[1, 37, 100, 3, 90, 89], 2);
        assert_eq!(fixed, ragged);
    }

    #[test]
    fn extra_device_channels_are_silent() {
        let out = render(&[100], 3);
        assert!(out.chunks(3).all(|frame| frame[2] == 0.0));
        assert!(out.chunks(3).any(|frame| frame[0] != 0.0));
    }

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

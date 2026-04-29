use cpal::{Device, Host, StreamConfig, traits::HostTrait};

use crate::config::Config;

pub struct AudioInterface<'a> {
    _host: &'a Host,
    pub output_device: Device,
    pub stream_config: StreamConfig,
}

impl<'a> AudioInterface<'a> {
    pub fn new(config: &Config, host: &'a Host) -> Self {
        let output_device = host.default_output_device()
            .expect("Not output device available");

        let stream_config = cpal::StreamConfig {
            channels: config.channels as u16,
            sample_rate: cpal::SampleRate(config.sample_rate as u32),
            buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
        };

        Self {
            _host: host,
            output_device,
            stream_config
        }
    }
}

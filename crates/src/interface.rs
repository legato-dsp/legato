use cpal::{Device, StreamConfig, traits::HostTrait};

use crate::config::Config;

pub struct AudioInterface {
    pub device: Device,
    pub stream_config: StreamConfig,
}

impl AudioInterface {
    pub fn default_with_config(config: &Config) -> Self {
        // TODO: More hosts
        #[cfg(feature = "jack")]
        let host = cpal::host_from_id(cpal::HostId::Jack);
        #[cfg(feature = "asio")]
        let host = cpal::host_from_id(cpal::HostId::Asio);

        #[cfg(not(any(feature = "jack", feature = "asio")))]
        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .expect("No output device available");

        let stream_config = cpal::StreamConfig {
            channels: config.channels as u16,
            sample_rate: cpal::SampleRate(config.sample_rate as u32),
            buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
        };

        Self {
            device,
            stream_config,
        }
    }
}

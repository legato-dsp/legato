use cpal::{
    Device, Host, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

/// Which device to open.
pub enum DeviceSelection {
    /// Use the host's default input device.
    Default,
    /// Find the first device whose name contains this string (case-insensitive).
    ByName(String),
}

pub struct CpalInputConfig<'a> {
    pub producer: rtrb::Producer<f32>,
    pub chans: usize,
    pub host: &'a Host,
    pub sample_rate: u32,
    pub device: DeviceSelection,
}

#[derive(Debug)]
pub enum CpalInputError {
    NoDefaultDevice,
    DeviceNotFound(String),
    DevicesEnumerationFailed(cpal::DevicesError),
    UnsupportedConfig(cpal::SupportedStreamConfigsError),
    NoMatchingConfig(String),
    BuildStreamFailed(cpal::BuildStreamError),
    PlayStreamFailed(cpal::PlayStreamError),
    ChannelMismatch { requested: usize, available: usize },
}

impl std::fmt::Display for CpalInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultDevice => write!(f, "No default input device available"),
            Self::DeviceNotFound(n) => write!(f, "Input device not found: {n}"),
            Self::DevicesEnumerationFailed(e) => write!(f, "Failed to enumerate devices: {e}"),
            Self::UnsupportedConfig(e) => write!(f, "Unsupported stream config: {e}"),
            Self::NoMatchingConfig(e) => write!(f, "{e}"),
            Self::BuildStreamFailed(e) => write!(f, "Failed to build input stream: {e}"),
            Self::PlayStreamFailed(e) => write!(f, "Failed to start input stream: {e}"),
            Self::ChannelMismatch {
                requested,
                available,
            } => write!(
                f,
                "Requested {requested} channels but device only has {available}"
            ),
        }
    }
}

impl std::error::Error for CpalInputError {}

pub struct CpalInputStream<'a> {
    pub stream: Stream,
    // Keep host and device alive for the lifetime of the stream.
    _device: Device,
    _host: &'a Host,
}

pub fn start_cpal_input(
    config: CpalInputConfig,
    block_size: usize,
) -> Result<CpalInputStream, CpalInputError> {
    let device = select_device(&config.host, &config.device)?;
    let stream_config = choose_config(&device, config.chans, config.sample_rate, block_size)?;

    println!(
        "negotiated: {}Hz, {} ch, buffer ~{:?}",
        stream_config.sample_rate.0,
        stream_config.channels,
        stream_config.buffer_size
    );

    let stream = build_stream(device.clone(), stream_config, config.producer, config.chans)?;

    Ok(CpalInputStream {
        stream,
        _device: device,
        _host: config.host,
    })
}

fn select_device(host: &Host, selection: &DeviceSelection) -> Result<Device, CpalInputError> {
    match selection {
        DeviceSelection::Default => host
            .default_input_device()
            .ok_or(CpalInputError::NoDefaultDevice),

        DeviceSelection::ByName(name) => {
            let lower = name.to_lowercase();
            host.input_devices()
                .map_err(CpalInputError::DevicesEnumerationFailed)?
                .find(|d| {
                    d.name()
                        .map(|n| n.to_lowercase().contains(&lower))
                        .unwrap_or(false)
                })
                .ok_or_else(|| CpalInputError::DeviceNotFound(name.clone()))
        }
    }
}

fn choose_config(
    device: &Device,
    chans: usize,
    sample_rate: u32,
    block_size: usize,
) -> Result<StreamConfig, CpalInputError> {
    // Walk supported configs and find an f32 one that matches our config
    let supported = device
        .supported_input_configs()
        .map_err(CpalInputError::UnsupportedConfig)?
        .collect::<Vec<_>>();

    let chans_u16 = chans as u16;
    let sr = cpal::SampleRate(sample_rate);

    // Prefer an exact f32 match.
    let exact = supported.iter().find(|c| {
        c.channels() == chans_u16
            && c.sample_format() == cpal::SampleFormat::F32
            && c.min_sample_rate() <= sr
            && sr <= c.max_sample_rate()
    });

    if let Some(cfg) = exact {
        let mut selected_cfg: StreamConfig = cfg.clone().with_sample_rate(sr).into();
        selected_cfg.buffer_size = cpal::BufferSize::Fixed(block_size as u32);
        Ok(selected_cfg)
    } 
        else {
            Err(CpalInputError::NoMatchingConfig("Could not find matching CPAL config!".into()))
        }
}

fn build_stream(
    device: Device,
    stream_config: StreamConfig,
    mut producer: rtrb::Producer<f32>,
    _chans: usize,
) -> Result<Stream, CpalInputError> {
    let err_fn = |e| eprintln!("[cpal_input] stream error: {e}");

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let available = producer.slots();
                let to_write = available.min(data.len());

                if to_write < data.len() {
                    eprintln!(
                        "[cpal_input] overrun: dropping {} samples",
                        data.len() - to_write
                    );
                }

                if to_write == 0 {
                    return;
                }

                if let Ok(mut chunk) = producer.write_chunk(to_write) {
                    let (first, second) = chunk.as_mut_slices();

                    let mid = first.len();

                    first.copy_from_slice(&data[..mid]);
                    second.copy_from_slice(&data[mid..to_write]);

                    chunk.commit_all();
                }
            },
            err_fn,
            None,
        )
        .map_err(CpalInputError::BuildStreamFailed)?;

    stream.play().map_err(CpalInputError::PlayStreamFailed)?;
    Ok(stream)
}

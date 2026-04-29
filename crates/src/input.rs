use assert_no_alloc::assert_no_alloc;
use cpal::{
    Device, Host, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};

/// The different kinds of device selection available.
///
/// Default just hands off to cpal default
///
/// By name tries to find a match
pub enum DeviceSelection {
    Default,
    ByName(String),
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
            } => {
                write!(
                    f,
                    "Requested {requested} channels but device only has {available}"
                )
            }
        }
    }
}

impl std::error::Error for CpalInputError {}

pub(crate) fn build_input_stream(
    host: &Host,
    mut producer: rtrb::Producer<f32>,
    chans: usize,
    sample_rate: u32,
    block_size: usize,
    selection: DeviceSelection,
) -> Result<Stream, CpalInputError> {
    let device = select_device(host, &selection)?;
    let stream_config = choose_config(&device, chans, sample_rate, block_size)?;

    let err_fn = |e| eprintln!("[cpal_input] stream error: {e}");
    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                assert_no_alloc(|| {
                    // Write up to the incoming block size
                    let to_write = producer.slots().min(data.len());
                    if to_write == 0 {
                        return;
                    }
                    // Here, we break into two chunks, to do two copies.
                    // This is needed because we don't have one continous slice with a ring buffer
                    if let Ok(mut chunk) = producer.write_chunk(to_write) {
                        let (first, second) = chunk.as_mut_slices();
                        let mid = first.len();
                        first.copy_from_slice(&data[..mid]);
                        second.copy_from_slice(&data[mid..to_write]);
                        chunk.commit_all(); // One nice transaction, I believe a bit faster than one sample at a time
                    }
                })
            },
            err_fn,
            None,
        )
        .map_err(CpalInputError::BuildStreamFailed)?;
    stream.play().map_err(CpalInputError::PlayStreamFailed)?;
    Ok(stream)
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
    let supported = device
        .supported_input_configs()
        .map_err(CpalInputError::UnsupportedConfig)?
        .collect::<Vec<_>>();

    let chans_u16 = chans as u16;
    let sr = cpal::SampleRate(sample_rate);

    let exact = supported.iter().find(|c| {
        c.channels() == chans_u16
            && c.sample_format() == cpal::SampleFormat::F32
            && c.min_sample_rate() <= sr
            && sr <= c.max_sample_rate()
    });

    if let Some(cfg) = exact {
        let mut selected: StreamConfig = cfg.clone().with_sample_rate(sr).into();
        selected.buffer_size = cpal::BufferSize::Fixed(block_size as u32);
        Ok(selected)
    } else {
        Err(CpalInputError::NoMatchingConfig(
            "Could not find matching CPAL input config".into(),
        ))
    }
}

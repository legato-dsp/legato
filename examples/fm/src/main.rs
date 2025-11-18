use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    core::out::start_application_audio_thread,
    dsl::{ApplicationConfig, build_application},
};
use typenum::{U2, U128, U4096, Unsigned};

fn main() {
    type BlockSize = U4096;
    type ControlSize = U128;
    type ChannelCount = U2;

    const SAMPLE_RATE: usize = 44_100;
    const CAPACITY: usize = 12;
    const DECIMATION_FACTOR: f32 = 32.0;
    const CONTROL_RATE: usize = SAMPLE_RATE / DECIMATION_FACTOR as usize;

    let graph = String::from(
        r#"
        audio {
            sine_mono: mod { freq: 550.0 },
            sine_stereo: carrier { freq: 440.0 },
            mult_mono: fm_gain { val: 1000.0 }
        }

        mod[0] >> fm_gain[0] >> carrier[0]

        { carrier }
    "#,
    );

    let (application, _) = build_application::<BlockSize, ControlSize, ChannelCount>(
        &graph,
        ApplicationConfig {
            intitial_capacity: CAPACITY,
            sample_rate: SAMPLE_RATE,
            control_rate: CONTROL_RATE,
        },
    )
    .expect("Could not build application");

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let config = StreamConfig {
        channels: U2::to_u16(),
        sample_rate: SampleRate(SAMPLE_RATE as u32),
        buffer_size: cpal::BufferSize::Fixed(BlockSize::to_u32()),
    };

    start_application_audio_thread(&device, &config, application).expect("Audio thread panic!")
}

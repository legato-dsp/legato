use std::path::Path;

use cpal::{SampleRate, StreamConfig, traits::HostTrait};
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    midi::{MidiPortKind, start_midi_thread},
    out::start_application_audio_thread,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"

        audio {
            sine: sine_one { freq: 440.0, chans: 2 },
            sine: sine_two { freq: 440.0, chans: 2 },
            sine: sine_three { freq: 440.0, chans: 2 },
            sine: sine_four { freq: 440.0, chans: 2 },
            sine: sine_five{ freq: 440.0, chans: 2 },

            track_mixer { tracks: 5, chans_per_track: 2, gain: [0.3, 0.3, 0.3, 0.3, 0.3] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[1] >> sine_one.freq
        poly_voice[4] >> sine_two.freq
        poly_voice[7] >> sine_three.freq
        poly_voice[10] >> sine_four.freq
        poly_voice[13] >> sine_five.freq

        sine_one >> track_mixer[0..2]
        sine_two >> track_mixer[2..4]
        sine_three >> track_mixer[4..6]
        sine_four >> track_mixer[6..8]
        sine_five >> track_mixer[8..10]

        { track_mixer }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 512,
        channels: 2,
        initial_graph_capacity: 4,
    };

    let ports = PortBuilder::default().audio_out(2).build();

    let (midi_rt_fe, _writer_fe) = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, mut frontend) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph);

    let _ = frontend.load_sample(
        &String::from("amen"),
        Path::new("../samples/amen.wav"),
        2,
        config.sample_rate as u32,
    );

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    let device = host.default_output_device().unwrap();

    let stream_config = StreamConfig {
        channels: config.channels as u16,
        sample_rate: SampleRate(config.sample_rate as u32),
        buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
    };

    start_application_audio_thread(&device, stream_config, app).expect("Audio thread panic!");
}

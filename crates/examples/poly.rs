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
    // Note: In reality, you would not do this. A custom node or subgraph is preferable.

    let graph = String::from(
        r#"

        patch voice(
            freq = 440.0,
            attack = 200.0,
            decay = 200.0,
            sustain = 0.3,
            release = 200.0
        ) {
            in freq gate

            audio {
                sine { freq: $freq },
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
            }

            gate >> adsr.gate

            freq >> sine.freq
            sine >> adsr[1]

            { adsr }
        }

        patches {
            voice: v1 { },
            voice: v2 { },
            voice: v3 { },
            voice: v4 { },
            voice: v5 { },
        }

        audio {
            track_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 600, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 1, delay_length: [ 459, 643 ] },
            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [0.4, 0.6, 0.6] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.6, 0.8] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        // gate
        poly_voice[0] >> v1.gate
        poly_voice[3] >> v2.gate
        poly_voice[6] >> v3.gate
        poly_voice[9] >> v4.gate
        poly_voice[12] >> v5.gate

        // freq
        poly_voice[1] >> v1.freq
        poly_voice[4] >> v2.freq
        poly_voice[7] >> v3.freq
        poly_voice[10] >> v4.freq
        poly_voice[13] >> v5.freq

        v1 >> track_mixer[0]
        v2 >> track_mixer[1]
        v3 >> track_mixer[2]
        v4 >> track_mixer[3]
        v5 >> track_mixer[4]

        track_mixer >> mono_fan_out

        mono_fan_out >> master[0..2]
        mono_fan_out >> dw1[0..2]

    
        dr1[0..2] >> master[2..4]

        // feedback    
        dr1 >> feedback[0..2]
        dr2 >> feedback[2..4]

        feedback >> dw1

        dr2[0] >> master[4..6]

        { master }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
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

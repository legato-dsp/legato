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

        audio {
            sine: sine_one { freq: 440.0, chans: 1 },
            sine: sine_two { freq: 440.0, chans: 1 },
            sine: sine_three { freq: 440.0, chans: 1 },
            sine: sine_four { freq: 440.0, chans: 1 },
            sine: sine_five { freq: 440.0, chans: 1 },

            adsr: adsr_one { attack: 100.0, decay: 700.0, sustain: 0.3, release: 400.0, chans: 1 },
            adsr: adsr_two { attack: 100.0, decay: 700.0, sustain: 0.3, release: 400.0, chans: 1 },
            adsr: adsr_three { attack: 100.0, decay: 700.0, sustain: 0.3, release: 400.0, chans: 1 },
            adsr: adsr_four { attack: 100.0, decay: 700.0, sustain: 0.3, release: 400.0, chans: 1 },
            adsr: adsr_five { attack: 100.0, decay: 700.0, sustain: 0.3, release: 400.0, chans: 1 },

            track_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 600, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 1, delay_length: [ 459, 643 ] },
            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [1.0, 0.3, 0.2] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        // sine waves
        sine_one >> adsr_one[1]
        sine_two >> adsr_two[1]
        sine_three >> adsr_three[1]
        sine_four >> adsr_four[1]
        sine_five >> adsr_five[1]

        // gate
        poly_voice[0] >> adsr_one.gate
        poly_voice[3] >> adsr_two.gate
        poly_voice[6] >> adsr_three.gate
        poly_voice[9] >> adsr_four.gate
        poly_voice[12] >> adsr_five.gate

        // freq
        poly_voice[1] >> sine_one.freq
        poly_voice[4] >> sine_two.freq
        poly_voice[7] >> sine_three.freq
        poly_voice[10] >> sine_four.freq
        poly_voice[13] >> sine_five.freq

        adsr_one >> track_mixer[0]
        adsr_two >> track_mixer[1]
        adsr_three >> track_mixer[2]
        adsr_four >> track_mixer[3]
        adsr_five >> track_mixer[4]

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

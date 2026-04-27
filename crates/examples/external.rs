use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    input::{CpalInputConfig, DeviceSelection, start_cpal_input},
    interface::AudioInterface,
    out::start_application_audio_thread,
    ports::PortBuilder,
};

fn main() {
    let graph = String::from(
        r#"
        patch basic_verb(){
            in audio_in
            audio {
                // Allpass structure
                allpass: allpass_one { delay_length: 111.0, feedback: 0.3, chans: 2},
                allpass: allpass_two { delay_length: 317.0, feedback: 0.3, chans: 2},
                allpass: allpass_three { delay_length: 511.0, feedback: 0.3, chans: 2},
                // Feedback structure
                delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
                delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
                delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 473 ] },
                // Dampen
                svf { cutoff: 2400.0, q: 0.7, type: lowpass, chans: 2 },
                // More density
                hadamard { chans: 4 },
                // Feedback
                track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.4, 0.4] },
                // Dry wet mixer
                track_mixer: wet_dry { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
            }

            audio_in >> allpass_one[0..2]
            allpass_one[0..2] >> allpass_two[0..2]
            allpass_two[0..2] >> allpass_three[0..2]

            allpass_three[0..2] >> dw1[0..2]
            allpass_three[0..2] >> wet_dry[0..2]

            dr1[0..2] >> wet_dry[2..4]
            dr2[0..2] >> wet_dry[4..6]

            dr1 >> hadamard[0..2]
            dr2 >> hadamard[2..4]

            // feedback    
            hadamard[0..2] >> feedback[0..2]
            hadamard[2..4] >> feedback[2..4]

            feedback >> svf[0..2]
            
            svf >> dw1

            { wet_dry}
        }

        patches {
            basic_verb {}
        }
    
        audio {
            external { interface_name: "one", chans: 1 },
            mono_fan_out { chans: 2 },
        }

        external >> mono_fan_out
        mono_fan_out >> basic_verb

        { basic_verb }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 1024,
        channels: 2,
        rt_capacity: 0,
    };

    // Spawn prod consumer pair

    let (producer, consumer) = rtrb::RingBuffer::new(48_000); // 1 second of headroom

    let input_config = CpalInputConfig {
        producer,
        chans: 1,
        sample_rate: config.sample_rate as u32,
        device: DeviceSelection::Default,
    };

    let res = start_cpal_input(input_config, config.block_size).unwrap();

    let ports = PortBuilder::default().audio_out(2).build();

    let (app, _) = LegatoBuilder::<Unconfigured>::new(config, ports)
        .register_audio_input("one", consumer, 1, config.block_size)
        .build_dsl(&graph);

    let interface = AudioInterface::default_with_config(&config);

    start_application_audio_thread(interface, app).expect("Audio thread panic!");
}

# Legato

Legato is a WIP audiograph framework for developing graph-based audio programs.

It takes inspiration from tools like PureData and FunDSP, with a focus on an improved DX
and easier extendability. Legato is a block-based audio engine, that uses dynamic dispatch for node design.

Rather than using Typenum or other techniques to match arity, the DSL and builders will
match common usecases, and allocate blocks for you based on the number of ports per node and sample rate.

Legato is designed to have numerous escape hatches. The DSL is converted to the IR which can easily be serialized
down the line. The IR maps to specific instructions in the builder. Additionally, users can easily add their own
nodes to the framework at initial runtime. 

Legato has been designed around CPAL for the time being, and there are a number of examples using this, but it 
should be trivial to interface with any library or tool that can deal with F32 PCM streams. Legato uses a plannar
layout throughout, in order to optimize for channel based, SIMD processing.

## Features

- Wait/lock-free block based audio runtime
- Midi runtime that covers common use cases in a similar manner to PureData or other utilities
- A number of common audio nodes (sine, SVF, FIR, delay read/write, allpass, etc.)
- The `LegatoFrontend` can be used to send messages to nodes
- Automate parameters on nodes at audio rate via graph connections
- Define reusable and nestable patches in the DSL

## Usage

Legato still needs a bit more polish before a 0.1.0 release. 

For the time being, a barebones sine wave synthesizer with a delay, and CPAL output, could look like the following:

```rust,no_run
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    midi::{MidiPortKind, start_midi_thread},
};

fn main() {
    // In the future, this will be it's own file, with a LSP
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
            voice * 5 { }
        }

        audio {
            track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
            mono_fan_out { chans: 2 },

            delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
            delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
            delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 643 ] },

            track_mixer: master { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
            
            track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.5, 0.5] }
        }

        midi { 
            poly_voice { chan: 0, voices: 5 }
        }

        poly_voice[0:13:3] >> voice(*).gate
        poly_voice[1:13:3] >> voice(*).freq
        voice(*) >> osc_mixer[0..5]

        osc_mixer >> mono_fan_out

        mono_fan_out >> master[0..2]
        mono_fan_out >> dw1[0..2]

        dr1[0..2] >> master[2..4]
        dr2[0..2] >> master[4..6]

        // feedback    
        dr1 >> feedback[0..2]
        dr2 >> feedback[2..4]

        feedback >> dw1

        { master }
    "#,
    );

    let config = Config {
        sample_rate: 48_000,
        block_size: 4096,
        channels: 2,
        rt_capacity: 0,
    };

    let midi_rt_fe = start_midi_thread(
        256,
        "my_port",
        MidiPortKind::Index(0),
        MidiPortKind::Index(0),
        "my_port",
    )
    .unwrap();

    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config)
        .set_midi_runtime(midi_rt_fe)
        .build_dsl(&graph)
        .unwrap();

    #[cfg(target_os = "macos")]
    let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available");

    #[cfg(target_os = "linux")]
    let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

    AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio")
        .run_forever();
}
```

### Developing Custom Nodes

In order to design a custom node, you need to meet the following criteria:

1. Design the node and `impl Node` for it. The Sweep node (shipped in `legato::nodes::audio::sweep`) is a relatively uncomplicated example.

```rust
use std::time::Duration;

use legato::{
    context::AudioContext,
    node::{Inputs, Node},
    ports::{PortBuilder, Ports},
};

#[derive(Clone)]
pub struct Sweep {
    phase: f32,
    range: [f32; 2],
    duration: Duration,
    elapsed: usize,
    ports: Ports,
}

impl Sweep {
    pub fn new(range: &[f32], duration: Duration, chans: usize) -> Self {
        let mut new_range = [0.0; 2];
        new_range.copy_from_slice(range);
        Self {
            phase: 0.0,
            range: new_range,
            duration,
            elapsed: 0,
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for Sweep {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut [&mut [f32]]) {
        let config = ctx.get_config();

        let fs = config.sample_rate as f32;

        let block_size = ctx.get_config().block_size;

        let mut min = self.range[0];
        let max = self.range[1];

        min = min.clamp(1.0, max);

        for n in 0..block_size {
            let t = (self.elapsed as f32 / fs).min(self.duration.as_secs_f32());
            let freq = min * ((max / min).powf(t / self.duration.as_secs_f32()));
            self.elapsed += 1;

            self.phase += freq / fs;
            self.phase = self.phase.fract();

            let sample = (self.phase * std::f32::consts::TAU).sin();

            for chan in ao.iter_mut() {
                chan[n] = sample;
            }
        }
    }
    fn ports(&self) -> &Ports {
        &self.ports
    }
}
```

2. Implement `NodeDefinition`, which gives the node a name, its required/optional DSL
   parameters, and a factory function that builds it from parsed params. This is also
   what makes the node self-registering: `Sweep::spec()` is derived from the consts below
   for free.

```rust
# use std::time::Duration;
# use legato::{
#     context::AudioContext,
#     node::{Inputs, Node},
#     ports::{PortBuilder, Ports},
# };
# #[derive(Clone)]
# pub struct Sweep {
#     phase: f32,
#     range: [f32; 2],
#     duration: Duration,
#     elapsed: usize,
#     ports: Ports,
# }
# impl Sweep {
#     pub fn new(range: &[f32], duration: Duration, chans: usize) -> Self {
#         let mut new_range = [0.0; 2];
#         new_range.copy_from_slice(range);
#         Self { phase: 0.0, range: new_range, duration, elapsed: 0, ports: PortBuilder::default().audio_out(chans).build() }
#     }
# }
# impl Node for Sweep {
#     fn process(&mut self, _: &mut AudioContext, _: &Inputs, _: &mut [&mut [f32]]) {}
#     fn ports(&self) -> &Ports { &self.ports }
# }
use legato::{
    builder::{ResourceBuilderView, ValidationError},
    dsl::ir::DSLParams,
    node::DynNode,
    spec::NodeDefinition,
};

impl NodeDefinition for Sweep {
    const NAME: &'static str = "sweep";
    const DESCRIPTION: &'static str = "Frequency sweep oscillator over a configurable range";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["duration", "range", "chans"];

    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        let chans = p.get_usize("chans").unwrap_or(2);
        let duration = p
            .get_duration_ms("duration")
            .unwrap_or(Duration::from_secs_f32(5.0));
        let range = p.get_array_f32("range").unwrap_or([40., 48_000.].into());
        Ok(Box::new(Self::new(&range, duration, chans)))
    }
}
```

3. Register it on the builder, under whichever namespace it should live in (built-in
   oscillators live under `"audio"`, for example).

```rust,ignore
let builder = builder.register_node("audio", Sweep::spec());
```

## License

Licensed under AGPL-3.0, with additional permissions outlined in the source repository.


### Contribution

All contributions are made under AGPL-3.0, with additional permissions outlined in the source repository.
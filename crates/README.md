# Legato

## Introduction

Legato is a graph-based audio framework and DSL for developing audio applications in Rust.

It allows users to define and spawn reusable, nested patches, and handle graph connections.

It takes a lot of inspiration from tools like MaxMSP, PureData, and VCVRack, but rather than using a
UI to define your audio applications, you use a minimal DSL, stepping into Rust when you want to 
extend the framework.

This enables users to make and share numerous reusable applications, effects, and devices that can
be tracked in `git`.

Legato currently has example CLAP/VST plugin integrations and well as a Nix-based PiSound flake
that can be used for making a minimal synthesizer/groovebox.

## Core Concepts

Legato applications are modeled as a directed acyclical graph, meaning that there are no feedback loops 
allowed in the application.

If you do require feedback loops, you can use the experimental `kernel` feature to define interpreted
or generated nodes that can handle per-sample feedback loops. This is great for defining and prototyping
reverb/fm/chorus and other effects.

Legato nodes are expected to operate on blocks of audio, i.e 512 samples. This offers stronger cache locality
and enables SIMD acceleration when possible.

Legato is somewhat heavily tied to `nightly` SIMD. 

SIMD allows for more efficient vectorized extensions that can do math on numerous destinations at the same time. 

This can in some cases offer up to a 8-16x speedup, and can be enabled with a nightly toolchain and the `simd` feature.

## Usage

Below is a minimal polyphonic synthesizer with a sin wave. Note that we define what a voice is, and then
spawn this same shape in the graph 5 times. These patches can nest within each other, please look in the 
`/examples` directory for more examples, and the parent repository for various integrations.

```rust,no_run
use legato::prelude::*;

let graph = String::from(
    r#"
    patch voice(
        attack = 50.0,
        decay = 30.0,
        sustain = 0.3,
        release = 50.0
    ) {
        in freq gate

        audio {
            saw { chans: 1 },
            adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
        }

        freq >> saw
        gate >> adsr.gate
        saw >> adsr[1]

        { adsr }
    }

    patches {
        voice * 5 { },
    }

    audio {
        svf { chans: 1, cutoff: 5400.0, q: 0.4, type: "lowpass" },
        track_mixer: osc_mixer { tracks: 5, chans_per_track: 1, gain: [0.1, 0.1, 0.1, 0.1, 0.1] },
        mono_fan_out { chans: 2 },
    }

    midi {
        poly_voice { chan: 0, voices: 5 }
    }

    poly_voice[0:13:3] >> voice(*).gate
    poly_voice[1:13:3] >> voice(*).freq
    voice(*) >> osc_mixer[0..5]

    osc_mixer >> svf

    svf >> mono_fan_out

    { mono_fan_out }
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
    .expect("graph should build");

#[cfg(target_os = "macos")]
let host = cpal::host_from_id(cpal::HostId::CoreAudio).expect("JACK host not available");

#[cfg(target_os = "linux")]
let host = cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available");

AudioInterface::builder(&host, config)
    .build(app)
    .expect("Failed to start audio")
    .run_forever();
```

### Developing Custom Nodes

If you find the DSL somewhat limiting, it's probably time to write a custom node.

Custom nodes have the following requirements:

1. Design the node and `impl Node` for it. The Sweep node (shipped in `legato::nodes::audio::sweep`) is a relatively uncomplicated example.

```rust
use std::time::Duration;

use legato::prelude::*;

/// Keep all of the owned state for the node here
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
            // This defines the arity for your node and enough space for its outputs
            ports: PortBuilder::default().audio_out(chans).build(),
        }
    }
}

impl Node for Sweep {
    fn process(&mut self, ctx: &mut AudioContext, _: &Inputs, ao: &mut [&mut [f32]]) {
        // The [`AudioContext`] contains shared state, such as global config, and shared resources like samples or LUT
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

            // Write our to each channel
            for chan in ao.iter_mut() {
                chan[n] = sample;
            }
        }
    }
    // The Executor calls this on construction to find this Node's arity
    fn ports(&self) -> &Ports {
        &self.ports
    }
    // Additionally, we have the optional handle_message function for message passing...
}
```

2. Implement `NodeDefinition`, which gives the node a name, its required/optional DSL
   parameters, and a factory function that builds it from parsed params.

```rust
# use std::time::Duration;
# use legato::prelude::*;
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

impl NodeDefinition for Sweep {
    const NAME: &'static str = "sweep";
    const DESCRIPTION: &'static str = "Frequency sweep oscillator over a configurable range";
    const REQUIRED_PARAMS: &'static [&'static str] = &[];
    const OPTIONAL_PARAMS: &'static [&'static str] = &["duration", "range", "chans"];

    // The unused [`ResourceBuilderView`] gives nodes the ability to share allocated audio pools,
    // and [`DSLParams`] enables us to expect certain optional or required parameters from the DSL.
    fn create(
        _rb: &mut ResourceBuilderView,
        p: &DSLParams,
    ) -> Result<Box<dyn DynNode>, ValidationError> {
        // Getters return `Err` on a wrongly typed param and `Ok(None)` when it is absent.
        // Use `p.require_*` for params listed in REQUIRED_PARAMS.
        let chans = p.get_usize("chans")?.unwrap_or(2);
        
        let duration = p
            .get_duration_ms("duration")?
            .unwrap_or(Duration::from_secs_f32(5.0));
        
        let range = p.get_array_f32("range")?.unwrap_or([40., 48_000.].into());
        
        Ok(Box::new(Self::new(&range, duration, chans)))
    }
}
```

3. Register the node with the builder and add it to your desired namespace.

```rust,ignore
let builder = builder.register_node("audio", Sweep::spec());
```

4. You can now use your node in the DSL, spawn it many times, create reusable patches, etc.

## License

Licensed under AGPL-3.0, with additional permissions outlined in the source repository.


### Contribution

All contributions are made under AGPL-3.0, with additional permissions outlined in the source repository.

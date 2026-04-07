<img width="801" height="163" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />

### What is Legato?

Legato is a WIP real time audio graph framework for Rust, that aims to combine the graph based processing of tools like PureData or MaxMSP,
with the utilities found in more robust frameworks like JUCE.

It takes some inspiration from a few DSP libraries, with some requirements changed to make it behave more like existing audio graph solutions.

Legato does not aim to be a live coding environment, rather a library to allow developers to create **hardware** or **VSTs**.

### What Is Planned?

Legato will have a CLI and split repo setup, similar to Tauri, where technical users can easily add custom nodes in the Rust layer, while managing the graph with a language server, (similar to PureData, SuperCollider, etc.)

The reason I would prefer something like this, is that I have found the escape hatches with these other toolkits a bit of a hastle to use. I want to easily define and add my own downstream node designs, and use the graph more for orchestration and quickly brainstorming ideas.

My goal is to make it so accesible that technical and non-technical users alike can make a basic synthesizer, a Nix image, and get it on a small machine like a Raspberry Pi.

So, a user might quickly throw together a FDN reverb in the graph, which is absolutely fine, but if they want a specific mixer, the ability to send data to external processes or other threads, etc., they can easily create a custom node to do so, and even have the language server respond.

The license is currently AGPLv3 with an additional clause designed to alleviate the need to distribute source for a number of common usecases.

I would also be really interested in using something like Automerge to make a cooperative DAW-like application once I have the Legato DX to where I want it to be.

### Getting Started

At the moment, Legato is somewhat tightly coupled to Nix, and I would suggest this for development, as it's also going to be the current defacto deployment target (I may look into Zephyr in the future, for now Rt patches?). 

If you use the DSL (WIP), you can construct a graph easily (more in /examples).

This example (examples/poly.rs), shows a custom patch, polyphonic midi setup, feedback delay network, connections, and more:

```rust
patch voice(
            attack = 200.0,
            decay = 200.0,
            sustain = 0.3,
            release = 200.0
        ) {
            in freq gate

            audio {
                sine: mod,
                sine: carrier,
                adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 1 },
                mult: freq_mult,
                mult: fm_gain { val: 1000.0 },
                add: fm_add
            }

            control {
                signal: ratio { name: "ratio", min: 1.0, max: 100.0, default: 1.5 }
            }

            freq >> freq_mult[0]

            ratio >> freq_mult[1]

            freq_mult >> mod.freq

            mod >> fm_gain[0]


            freq >> fm_add[0]
            fm_gain >> fm_add[1]

            fm_add >> carrier.freq

            gate >> adsr.gate

            carrier >> adsr[1]

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
```

There are also some developer utilities like a spectrogram or example FIR filter generation scripts.

```
nix run .#apps.x86_64-linux.spectrogram -- --path ./example.wav --out ./example.png
```

### A Note On Safety

Experimenting with audio software can be dangerous at times. Use at your own risk. 

Exercise extra risk when working with any feedback delay networks, gain, wavefolding, etc. 

It may be wise to simply use laptop speakers at low volume when developing, and to clamp gain to a specific amount in order to prevent any hardware damage.

## Roadmap

### Planned Features For 0.1.0

- More nodes (pitch shifter, convolution, iir filters)
- Matrix mixers
- Semi-tuned NixOS images, perhaps also Zephyr?
- WASM bindings?
- FIR filter creation, windows, etc. 
- Fancy docs (Nuxt)
- A number of examples (FM, reverb, some midi stuff)
- VST, CLAP, etc. support? Likely will look for contributions here, or just use the NIH-Plug repo.

### Immediate Cleanup

Here are a number of issues to keep an eye on, that need to be cleaned up rather soon.

- Single tap delay node for delay compensation
- Unify node creation spec and node logic
- I want to move over/resampling to be graph logic and DSL. So, rather than wrapping a number of nodes, the executor implicitly adds polyphase FIR filters on the boundaries.
- Transparent build pipeline with `cargo publish`
<img width="1602" height="326" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />






### What is Legato?

Legato is a WIP real time audio graph framework for Rust, that aims to combine the graph based processing of tools like PureData or MaxMSP,
with the utilities found in more robust frameworks like JUCE.

It takes some inspiration from a few Rust DSP libraries, mostly FunDSP, with some requirements changed to make it behave more like existing audio graph solutions.

Legato does not aim to be a live coding environment, rather a library to allow developers to create hardware or VSTs.

### Getting Started

At the moment, it's fairly DIY. There are a few examples for setting this up with CPAL. 

If you use the DSL (WIP), you can construct a graph easily (more in /examples), like so:

```rust
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
    
```

There will also be a number of different scripts to graph data.

```
nix run .#apps.x86_64-linux.spectrogram -- --path ./example.wav --out ./example.png
```

## Roadmap

### Planned Features For 0.1.0

- Minimal DSL or macros for graph construction
- Port automapping
- "Pipes" to allow users to instatiate or modify nodes
- Convenient abstractions for the UI layer
- SIMD integration for hot paths like FIR, interpolation, etc.
- Semi-tuned NixOS images
- MIDI context (will poll or block dedicated thread, handle voicings) and graph
- Fancy docs
- Working wavetable demo
- More interpolation algs.
- IIR filters (biquad, onepole)

#### Cleanup Issues

Here are a number of issues to keep an eye on, that need to be cleaned up rather soon.

- We likely can use an interior graph rate, and do block rate adapting similar to some other solutions (maybe three latency levels?).
- Ports likely don't need to be tied to generic array. This is making it annoying to say spawn an N channel node.
- Framesize trait is a bit gross. Perhaps there is a better way, I am especially grossed out by the Prod and Mul bounds.
- Do we add an FFT node? Or, should we assume that users can use their own FFT library? I kind of like the second, in MaxMSP I thought it was awkward except for visualizations.

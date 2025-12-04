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
- Port automapping in DSL
- "Pipes" to allow users to instatiate or modify nodes -> replicate(4) gives us four nodes, something like 
    offset({ param: "gain", alg: "linear", bounds: [0.2, 0.8]}) is planned as well.
- Convenient abstractions for the UI layer. I think I will be targetting Tauri, Iced, and Flutter for examples for the time being.
- SIMD integration for hot paths like FIR, interpolation, etc.
- Semi-tuned NixOS images
- WASM bindings
- FFI bindings
- MIDI context (will poll or block dedicated thread, handle voicings) and graph
- Fancy docs
- A number of examples (Wavetable FM, reverb, some midi stuff)
- More interpolation algs.
- IIR filters (biquad, onepole, SVF)

### Cleanup

Here are a number of issues to keep an eye on, that need to be cleaned up rather soon.

- We likely can use an interior graph rate, and do block rate adapting similar to some other solutions (maybe three latency levels?). I also want to encode the lanes available given the arch. to the frame, i.e 16 for avx512, 8 for avx256, etc. if there is a large benefit to using SIMD. Then, we can have have a block adappter to the audio callback
- Ports likely don't need to be tied to generic array. This is making it annoying to say spawn an N channel node.
- Framesize trait is a bit gross. Perhaps there is a better way, I am especially grossed out by the Prod and Mul bounds. Perhaps a dedicated rate adadpter is the solution rather than trying to use the type system?
- Should we use petgraph instead? I actually think it might be wise to continue with an internal implementation, as it could be easier to do something stack allocated in the future? For instance using arrays or something like heapless, and I am not sure if Petgraph supports this out of the box?
- SIMD, is it better to be explicit, or, might it be better to use things like chunks_mut to hint to the compiler that certain optimizations are available? Hot paths like FIR and interpolation will need to be benchmarked with both approaches.
- Do we add an FFT node? Or, should we assume that users can use their own FFT library? I kind of like the second, in MaxMSP I thought it was awkward except for visualizations, but I will workshop some spectral effects and see what is most intuitive.
- We need delay compensation for the runtime, which will be on a per-port trait. Basically, we just run all of the nodes for some block size (say 4096), which incurs some latency, but then we can easily 

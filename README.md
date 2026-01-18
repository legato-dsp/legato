<img width="1602" height="326" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />






### What is Legato?

Legato is a WIP real time audio graph framework for Rust, that aims to combine the graph based processing of tools like PureData or MaxMSP,
with the utilities found in more robust frameworks like JUCE.

It takes some inspiration from a few Rust DSP libraries, mostly FunDSP, with some requirements changed to make it behave more like existing audio graph solutions.

Legato does not aim to be a live coding environment, rather a library to allow developers to create hardware or VSTs.

### Getting Started

At the moment, it's fairly DIY. There are a few examples for setting this up with CPAL. 

If you use the DSL (WIP), you can construct a graph easily (more in /examples).

This example (examples/delay.rs) show custom pipes, node renaming, slice mapping, and setting a sink. 

```rust
let graph = String::from(
    r#"
    audio {
        sampler { sampler_name: "amen" } | logger(),
        delay_write: dw1 { delay_name: "d_one", chans: 2 },
        delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 200, 240 ] },
        delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 310, 330 ] },
        track_mixer { tracks: 3, chans_per_track: 2, gain: [1.0, 0.2, 0.2] }
    }

    sampler[0..2] >> track_mixer[0..2]
    sampler[0..2] >> dw1[0..2]
    dr1[0..2] >> track_mixer[2..4]
    dr2[0] >> track_mixer[4..6]

    { track_mixer }
    "#,
);
    
```

There will also be a number of different scripts to graph data.

```
nix run .#apps.x86_64-linux.spectrogram -- --path ./example.wav --out ./example.png
```

## Roadmap

### Planned Features For 0.1.0

- More nodes (pitch shifter, convolution, iir filters)
- Matrix mixers
- Semi-tuned NixOS images, perhaps also Zephyr?
- WASM bindings?
- FFI bindings?
- MIDI context (will poll or block dedicated thread, handle voicings) and midi graph?
- Fancy docs
- A number of examples (FM, reverb, some midi stuff)
- VST, CLAP, etc. support? Likely will look for contributions here

### Cleanup

Here are a number of issues to keep an eye on, that need to be cleaned up rather soon.

- Single tap delay node for delay compensation.
- Better oversampling logic (kind of half-assed at the moment, needs a half-band or more efficient filter)
- One continous buffer for work executor redesign 
- Bitflags or something similar for user defined params rather than static string comparison?
- Unify node and spec construction
- Chans sometimes means different things in different contexts for node definition. There needs to be a consistent way
    to say, hey, this node takes n + m chans in, o chans out
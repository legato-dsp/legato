<img width="1602" height="326" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />

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

This example (examples/delay.rs) show custom pipes, node renaming, slice mapping, and setting a sink. 

```
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
- Rewrite parsing logic in Chumsky, which I believe will give much better error messages
- VST, CLAP, etc. support? Likely will look for contributions here, or just use the NIH-Plug repo.

### Immediate Cleanup

Here are a number of issues to keep an eye on, that need to be cleaned up rather soon.

- Single tap delay node for delay compensation
- Better oversampling logic (kind of half-assed at the moment, needs a half-band or more efficient filter)
- Bitflags or something similar for user defined params rather than static string comparison?
- Unify node creation spec and node logic

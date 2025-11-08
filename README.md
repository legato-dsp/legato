<img width="1602" height="326" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />






### What is Legato?

Legato is a WIP audio graph framework for Rust, that aims to combine the graph based processing of tools like PureData and MaxMSP,
with the utilities found in more robust frameworks like JUCE.

It takes some inspiration from a few Rust DSP libraries, mostly FunDSP, with some requirements changed to make it behave more like PureData.

### Getting Started

At the moment, it's fairly DIY. There are a few examples for setting this up with CPAL. There will also be a number of different scripts to graph data.

```
nix run .#apps.x86_64-linux.spectrogram -- --path ./example.wav --out ./example.png
```


### Planned Features For 0.1.0

- Minimal DSL or macros for graph construction
- SIMD integration for hot paths like FIR, interpolation, etc.
- Semi-tuned NixOS images
- MIDI context and graph
- Fancy docs and examples
- Symponia integration instead of FFMPEG

# Legato

This is a WIP audio graph framework for Rust.

### Getting Started

At the moment, it's fairly DIY. There are a few examples for setting this up with CPAL. There will also be a number of different scripts to graph data.

```
# tweak your system here

nix run .#apps.x86_64-linux.spectrogram -- --path ./example.wav --out ./example.png
```




### Planned Features

- Minimal DSL or macros for graph construction
- SIMD integration for hot paths like FIR, interpolation, etc.
- Tuned NixOS images
- MIDI context and graph
- Fancy docs and examples
- Symponia integration instead of FFMPEG
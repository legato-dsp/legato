# How to Contribute?

*Please, before doing any significant work, open an issue and make sure that we are aligned in our goals and requirements.*

## Getting Started

*NixOS is the preferred OS for development, as we can easily manage dependencies, write tests, and have some useful scripts for common operations.*

On NixOS

```bash
git clone https://github.com/legato-dsp/legato.git 
cd legato
nix develop . # alternatively just direnv allow
```

On Mac (brew)

```bash
git clone https://github.com/legato-dsp/legato.git 
cd legato
# /crates
brew install rust 
brew install ffmpeg
# /docs
brew install pnpm
# /scripts
brew install uv python3     
```

Windows is not currently a priority. Contributions here are welcome.


## Project Alignment

If you're interesting in contributions, there are a few areas that I would love to go in in the future, requiring a variety of expertise.

At this moment, my goal is to have Legato provide an incredible developer experience to all kinds of developers, and to make audio programming, and even Rust, accesible to people that may not have had the chance, or been intimidated in doing so. 

This means that I am trying to strike a balance between performance, ownership(taking this where you go, open source), and accesibility.

I would feel quite guilty if I could not move in a significant body of someone's work. That being said, I hope to have a nice ecosystem in place, and if you cannot find a home for some work in the repo, I am sure that there will soon be showcases or other things available.

## Code Standards

- In any DSP code, avoid any sort of heap allocation, locks, or system calls on the audio thread
- cargo clippy + cargo fmt before PR

## Good First Issues

I have a few areas that I could need some help with from a variety of developers:

### Systems Experts
- Profiling, SIMD optimizations, quality benchmarks (cyclictest? or something bespoke?)
- Memory mapped file streaming? Not sure if a good solution but this seems like low hanging fruit.
- Performance tuned NixOS images (RT patch? Some other conifg?). My goal is to optimize the DX for something in the embedded linux space. I would love to just have a simple Nix flake used for deployment and development alike.
- CI/CD pipeline once this is ready for deeper downstream usage

### Embedded Developers
- I would love to get the point where we can run applications on something like Zephyr.

### DSP Experts
- More efficient filters for oversampling
- More filter design in general
- Higher order interpolation algorithms
- A number of spectral effect algorithms
- Bindings for easy VST3 or CLAP plugins
- BLIP/BLEP/ADAA/etc. wavetables and FM examples
- Partioned FFT convolution for long IR
- Maybe even wave digital filter and other analog modeling techniques? This will likely not be something I can dive into until 2026+

### Rustaceans
- Help me find a better solution than Typenum and generic arrays. I tried some nightly features but was not a huge fan.
- Help me design the types for a better oversampling solution
- Ergonomic no_std
- I likely will be focusing on Iced and Tauri examples. If anyone wants to contribute that would be great
- General test coverage expansion

### Compiler Nerds
- Replace my DSL with something more robust?
- Help my DSL have better error messages
- Implement a LSP 

### UI/UX Enthusiasts
- Make some high quality WASM bindings and examples
- Help with design system for UI templates

### Graphics
- I would love to have some demos with shaders, perhaps some spectral analysis + texture buffer demo?
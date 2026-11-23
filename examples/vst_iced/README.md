# Legato VST Example

This uses the `nice-plug` project, as well as the `iced` integration `nice-plug-iced`.

The goal of `legato` is to provide a reusable ecosystem of nodes that can be easily extended in Rust, and then used in a variety of settings, such as VST, synthesizer, effect pedal, etc. development.

### Requirements

This example enables the `simd` feature by default, which requires a nightly Rust toolchain (1.99) as Legato uses nightly SIMD for acceleration in certain hot paths.

You can disable this feature and build with a traditional toolchain if required.

For `nix` users, there is also a flake that uses the Legato dev shell, but for the more supply chain paranoid users, you can again simply make a dev shell with Rust nightly.

Additionally, you will need to download two fonts that I did not feel comfortable including in the repository for licensing reasons. See the *fonts* section below.

### Building

*Note: This is only tested so far on aarch64 MacOS and x86 NixOS*

From this directory:

`cargo nice-plug bundle legato_grain_delay --release`

Copy to your system's VST3 folder, on MacOS mine is found here:

`cp -R target/bundled/legato_grain_delay.vst3 /Library/Audio/Plug-Ins/VST3/`

You can also run it as a standalone app with `cargo run --release`.

This is not yet packaged for Nix but may be in the future.

You may run into signing issues on certain platforms. This happened to me once with Ableton when I had a JUCE integration, but this has not happened to me yet with this repository.

### Architecture

Legato applications have a wait/lock/allocation-free runtime, as well as a control thread. Legato applications can support MIDI runtimes as well but that is not seen here.

We spawn the runtime in `engine.rs`, and pull blocks whenever the host requests. This block rate is defined interiorly and is separate from the host block size. We also take in information about our current playback rate, and use this to make a `phasor` that can keep certain parameters in-sync musically.

On the UI thread, we control all of the various parameters with the `Knob` struct for now. Every param has a knob. **Grain Sync** and **Delay Sync** in the header toggle tempo sync, and while this is engaged we are tempo-synced liked previously mentioned.

*TODO: I set a max delay capacity at 4 seconds, if this is more than a whole note, this may not work as expected, but this is a demo for now*

The graph is defined in the `.legato` file. This shows the topology of the graph, default parameters, etc. This is a fairly basic example, see the other examples for features such as reusable macros/patches, interpreted feedback structures, disabling/muting nodes, etc.

#### Parameters

We have the following parameters that can be used on the knobs of via host automation:

- grain-size: the size of the grain... this can be tied to host tempo when `Grain Sync` is clicked.
- grains: number of overlapping grains, from 1-8.
- pitch: grain playback rate, from -12 to 12 semitones. It's fairly strange with anything other than more stable intervals.
- shape: from 0.001 or so to 0.5, where 0.5 is more Hann-Shaped, and 0.001 is more rectangular.
- feedback: amount of feedback in the delay loop
- jitter: amount in which grain position is deviated. Some is always applied, otherwise you can get some nasty correlation.
- dampening: internal lowpass in the delay loop. TODO: highpass as well
- diffusion: allpass array with tunings borrowed from Mutable Instruments Clouds
- Decay: Dattorro-esque reverb decay
- Reverb Mix: The reverb mix...
- Delay: Length of the delay, can be tempo-synced
- Mix: Wet-dry mix

### Fonts

I am a big fan of the fonts from [Fontshare](https://www.fontshare.com). However, for licensing reasons, they aren't included in the repository.

You can download them and place them here:

```
assets/fonts/Satoshi-Light.otf
assets/fonts/Chillax-Regular.otf
```

### Using This Outside of the Repository

This example depends on Legato by path. To use it as a template, replace the dependency with the published crate:

```toml
legato = { version = "0.4.0", features = ["simd"] }
```

### Future Ideas

- My original grain delay implementation used a Tukey window, as I could interpolate between different window shapes. However, I would like to explore more windows.
- This shape worked really well for the granular synth example in `grain.rs` in Legato, but I am maybe not in love with the character here for a delay.
- I need to add a highpass filter to the interior loop as well, as the low-end can become a bit much.
- I would like to add LFO and step modulation for various parameters
- Distortion, panning, and more would also be fun when combined with modulation
- I am interested in trying out some new reverb topologies. This one is based on the Dattorro plate reverb that I saw in a YouTube talk, but something with a little more upfront density would be interesting.

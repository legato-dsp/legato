# Legato VST Example

This is an example showing how you can export a Legato project as a VST3/CLAP plugin.

This is a stereo grain delay effect with reverb, based on [`crates/examples/grain_delay.rs`](../../crates/examples/grain_delay.rs).

The host's main input is fed into the graph through an `external` node.

It demonstrates two of the three ways that users can control Legato applications: plugin params and message passing.
Pitch (±12 semitones), grain size, grain count, shape, jitter, feedback, damping, diffusion, and reverb decay are
sent as messages, while the dry/wet mix drives a smoothed `signal` node.

With **Delay Sync** on, the delay time follows a note division (including dotted and triplet values) of the host
tempo, otherwise it's set in ms. Either way it's sent to `grain_delay` as a `delay_length` message, clamped to
the 4s line allocated by `max_delay_length` in `.legato`.

With **Grain Sync** on, grain length follows a note division of the host tempo instead of Grain Size. The plugin
renders a 0..1 phasor from the host transport (one cycle per grain, aligned to the beat grid) and feeds it to the
graph as a second host input, which `grain_delay` reads on its `phase` port. Grains then spawn on even divisions of
each cycle, and jitter delays them by up to that fraction of a division. When the transport is stopped, the phasor
keeps running at the host tempo.

If you need audio rate modulation, this is possible with a SPSC using the `external` node in Legato, you can see the examples in
[`crates/examples`](../../crates/examples).

The actual Legato DSL code is located in the `.legato` file in the project root.

### Credit to Nice Plug

This was made possible by the `nice-plug` crate, which you can read more about [here](https://codeberg.org/RustAudio/nice-plug).

### Building

From this directory:

`cargo nice-plug bundle legato-vst --release`

Copy to your system's VST3 folder, on MacOS mine is found here:

`cp -R target/bundled/legato-vst.vst3 /Library/Audio/Plug-Ins/VST3/`

You can also run it as a standalone app with `cargo run --release`.

### Using This Outside of the Repository

This example depends on Legato by path. To use it as a template, replace the dependency with the published crate:

```toml
legato = { version = "0.4.0", default-features = false }
```

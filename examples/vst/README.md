# Legato VST Example

This is an example showing how you can export a Legato project as a VST3/CLAP plugin.

This is a small polysynth with reverb.

It demonstrates two of the three ways that users can control Legato applications: plugin params and message passing.

For audio rate modulation, this is possible with a SPSC using the `external` node in Legato, you can see the examples in
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
legato = { version = "0.1.1", default-features = false }
```

### TODO

This isn't actually nixified, only just a basic dev shell.

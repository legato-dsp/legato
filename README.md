<img width="801" height="163" alt="Logo" src="https://github.com/user-attachments/assets/c15ecbbf-604c-450d-843f-d6108f96700a" />

## What is Legato?

Simply put, Legato is an opinionated DX for creative audio applications.

It aims to fuse the quick prototyping of tools like MaxMSP/PureData, with the performance and utilities found in frameworks like JUCE or KFR. Rather than providing a UI solution, Legato aims for a minimal DSL for graph orchestration. 

Users can bring their own UI and custom nodes using Rust, avoiding complicated SDKs.

## Example Patch

Here is a quick example of the DSL. 

```rust
patch voice(
    attack = 1200.0,
    decay = 300.0,
    sustain = 0.8,
    release = 700.0
) {
    in freq gate

    audio {
        sine: lfo { freq: 0.1 },
        grain { sampler_name: "main", chans: 2, size: 70, shape: 0.5, scan: 0.05 },
        adsr { attack: $attack, decay: $decay, sustain: $sustain, release: $release, chans: 2 },
    }

    control { 
        map { range: [-1.0, 1.0], new_range: [100, 300] }
    }

    lfo >> map
    map >> grain.size

    freq >> grain.freq
    gate >> grain.trig

    gate >> adsr.gate
    grain >> adsr[1..3]

    { adsr }
}

patches {
    voice * 3 { },
}

audio {
    track_mixer { tracks: 3, chans_per_track: 2 },
}

midi {
    poly_voice { chan: 0, voices: 3 }
}

patches {
    plate: verb { predelay: 32.0, decay: 0.4, damping: 0.3, wet: 0.8, dry: 0.2 }
}

poly_voice[0:10:3] >> voice(*).gate
poly_voice[1:10:3] >> voice(*).freq

voice(*)[0] >> track_mixer[0:6:2]
voice(*)[1] >> track_mixer[1:6:2]

track_mixer >> verb

{ verb }
```

It does NOT aim to be an incredibly feature complete langauge, a la CSound or Super Collider. 

For more complicated functionality, I encourage users to add their own custom nodes, written in Rust, at application startup.

Legato is generally block based, but the `kernel` feature can be used for per-sample nodes in the DSL, at the cost of some performance (IMO, strong for prototyping!)

## Integrations (TBD)

For now, I have a NixOS + PiSound flake that I have really enjoyed working with. That being said, I hope to onboard more hardware, particular SBCs, as well as writing a number of VST examples. Contributions here are more than welcome.

## Additional Tooling

The repository comes with a few example scripts that may be helpful for testing aliasing, designing filters, etc. For the time being, I parked the oversampling feature, but the blocks are already here, I am more working on the semantics for the language.

There is a vibecoded Tree Sitter grammar in my personal repository, that I made by basically sticking an agent at my projects Chumsky grammar. It works for quick syntax highlighting via Zed, Helix, NeoVim, etc.

## License

Legato is AGPLv3, with additional permissions to remove the source disclosure requirements for most creative applications. You can read more about this in the ADDITIONAL_PERMISSIONS distributed with the repository.
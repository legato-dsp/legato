---
title: Getting Started
---


### What is Legato?

Legato is a realtime audio framework and DSL to quickly build audio applications in Rust. It takes inspiration from a few different tools, like PureData, SuperCollider, FunDSP and MaxMSP, but it tries a slightly different workflow.

The DSL is purposefully minimal, there are no evaluations, for loops, branching, etc. It is purely for graph definitions, and these definitions map directly to builder operations on the runtime.

Additionally, users can define custom nodes in Rust, and then use them in the DSL. This prevents users from having to learn something like CSound or SuperCollider, and you can simply define your node in Rust and take advantage of the modern toolchain and safety guarantees.

Additionally, users can also use patches to instantiate reusable macros of nodes. These are all inlined into the same graph, reusing the same underlying flat allocation.

**Here is a not so good reverb example:**

```rust
patch basic_verb(){
    in audio_in // These are virtual ports
    audio {
        // Allpass structure
        allpass: allpass_one { delay_length: 111.0, feedback: 0.2, chans: 2},
        allpass: allpass_two { delay_length: 189.0, feedback: 0.2, chans: 2},
        allpass: allpass_three { delay_length: 213.0, feedback: 0.2, chans: 2},
        // Feedback structure
        delay_write: dw1 { delay_name: "d_one", delay_length: 2000.0, chans: 2 },
        delay_read: dr1 { delay_name: "d_one", chans: 2, delay_length: [ 938, 731 ] },
        delay_read: dr2 { delay_name: "d_one", chans: 2, delay_length: [ 459, 473 ] },
        onepole { cutoff: 2400.0, chans: 2 },
        // Feedback
        track_mixer: feedback { tracks: 2, chans_per_track: 2, gain: [0.4, 0.4] },
        // Dry wet mixer
        track_mixer: wet_dry { tracks: 3, chans_per_track: 2, gain: [0.4, 0.5, 0.5] },
    }

    audio_in >> allpass_one[0..2]
    allpass_one[0..2] >> allpass_two[0..2]
    allpass_two[0..2] >> allpass_three[0..2]

    allpass_three[0..2] >> dw1[0..2]
    allpass_three[0..2] >> wet_dry[0..2]

    dr1[0..2] >> wet_dry[2..4]
    dr2[0..2] >> wet_dry[4..6]

    // feedback    
    dr1 >> feedback[0..2]
    dr2 >> feedback[2..4]

    feedback >> onepole[0..2]
    
    onepole >> dw1

    { wet_dry}
}

patches {
    basic_verb {}
}
```


### Development Environment

The easiest way to start is to clone the sample repository:
```shell
git clone --depth 1 https://github.com/legato-dsp/legato-flake-template
```

Alternatively, you can simply start a [new Rust project and add Legato](https://crates.io/crates/legato), and take what you need.

Legato currently uses [cpal](https://crates.io/crates/cpal) for cross-platform audio, but this can be sidestepped if desired. To get usable audio, you may have to play around with your sample rate, block size, etc. depending on your operating system and audio backend.


### Custom Nodes

The node trait (simplified), looks something like this:

```rust
/// Optional inputs. Vary the logic depending on if inputs are present.
pub type Inputs<'a> = [Option<&'a [f32]>]; // Planar layout, i.e [[L,L],[R,R]]

/// The node trait any audio processing node must implement.
pub trait Node {
    /// Where audio processing occurs for your node.
    fn process(
        &mut self,
        ctx: &mut AudioContext,
        inputs: &Inputs,
        outputs: &mut [&mut [f32]],
    );

    /// Pass messages to your node.
    fn handle_msg(&mut self, _msg: NodeMessage) {}

    /// Get port information for your node.
    fn ports(&self) -> &Ports;
}
```



### FAQ


#### When Should I Use a Custom Node or Patch?

One goal of Legato is to remove the requirement to learn an entirely new programming language or complex user interface. This will hopefully allow users to declare custom nodes in Rust, while using the DSL purely for graph orchestration.

Additionally, Legato also offers a patch system, which allows users to instantiate macros/patches of nodes into the graph. Additionally, users can bypass the DSL and use the builder directly. This is useful if you were to want to say spawn 32 nodes and give them a specific programatic value or so in a range.

Custom nodes can also be a strong performance optimization. Imaging if you wanted to run say 12 allpass filters in a row. If you do this in the graph layer, there is some overhead to writing out to each node's buffer. You could also create a custom node, that runs say 12 allpass filters in a row, on the same underyling buffer. This could greatly accelerate your usecase.

If you have a simple chorus that you want to spawn a few times, a synthesizer voice, etc. a patch is likely the correct tool.

#### Should I Use CPAL?

For most users, CPAL is a strong option. It handles the annoyance of having to deal with a number of different audio APIs.

Legato does have a number of escape hatches, and if desired, you can simply call the next_block() function on the runtime and use these samples in another context.

#### Can You Explain the License?

The source of truth here is the LICENSE, CONTRIBUTING and ADDITIONAL_PERMISSIONS distributed in the repository, everything below is informal advice.

Legato is first and foremost AGPLv3, and any contributions will fall under this. 

At the end of the day, you can do whatever you want with it, provided you follow the terms of the AGPLv3 license. 

However, I want to balance having an open source engine, and hopefully the ability to one day monetize the project in order to advance the development. I'm hoping to help balance this by waiving the source disclosure for some creative projects. In summary, VSTs, software synths/grooveboxes, creative applications, without DAW or AI functionality, can monetize their products without any worries of disclosing source. Please check the CONTRIBUTING distributed with the repository for the actual underlying agreement.
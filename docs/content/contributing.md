---
title: Contributing
---

## Guidelines 

**Before working on any new functionality, please open an issue beforehand.**

Due to the plugin-oriented nature of Legato, it is unlikely that nodes will be added for every possible use case. However, if something is sufficiently general (e.g., pitch shifting, polyphase resampling), it may be considered.

Any contributions will fall under the license and additional permissions distributed with the repository.

Lastly, I am to have a diverse, respectful community of people that just want to make cool things, and run it on their hardware. 

Please see the [Berlin Code of Conduct](https://berlincodeofconduct.org/en) for general guidelines of what is and is not acceptable.

## Development Setup

Developers are strongly encouraged to use Nix to manage dependencies.

This makes benchmarks, development configurations, and tests reproducible.

For now, this is as simple as cloning the repository and using the provided Nix development shell, optionally via `direnv`. While Nix is not required, contributions are expected to build and run in the Nix environment.

If you do not wish to use Nix (fair enough), you will need a Rust nightly toolchain.

## Code Style Guide

* Prefer modeling state and events with enums or bitflags
* Avoid locking primitives in real-time code
* Avoid syscalls or heap allocations in the audio thread
* Non-trivial nodes should include benchmarks using Criterion, with black-boxed inputs and outputs
* unsafe with measurable performance gains is acceptable, but safety should be preferred under most circumstances, provided UB is checked with Miri.
* Non-trivial code should include at least a few relevant test cases

### Concurrency and Lock-Free Expectations

Code that runs on the audio thread must be lock-free.

Avoid mutexes, condition variables, and blocking synchronization primitives.

If using channels, queues, or ring buffers, they must be lock-free and non-blocking under all expected conditions. Do not assume that a “channel” is suitable for real-time use without verifying its implementation.

## General Performance Guidelines

When optimizing DSP code, generally follow these steps:

* Use flat buffers or cache-optimized data structures
* Consider structuring algorithms to use `chunks_exact` where possible for auto-vectorization, if gains are observed when benchmarking
* Prefer branching at function entry rather than in hot DSP paths.
* Use branchless solutions where appropriate
* Use polynomial approximations for operations that cannot be efficiently vectorized
* Avoid heap allocations and system calls in hot paths; if allocation is required, use arenas or preallocation
* Avoid mutexes or blocking primitives in audio-thread code
* Once these steps are complete, consider `std::simd` (nightly) for further optimization, but make this yields measurable performance gains

## Pull Requests and Review Process

All pull requests must adhere to the following:

* Every PR must reference an existing issue
* The scope of the PR should match the associated issue
* All CI checks must pass
* Performance-relevant changes must include benchmarks or justification
* Non-trivial changes must include appropriate tests
* PRs should include a clear description of design decisions and trade-offs

PRs that do not meet these requirements may be closed without review.

## Logo Usage

The Legato logo may only be used in non-monetized contexts.

Community spaces such as Matrix or Discord are acceptable.

Commercial packages of nodes or other educational material that do not adhere to AGPLv3 or are not fair use are not permitted.

## AI Policy

AI tools may be used for auxiliary tasks such as generating test harnesses, proofreading documentation, refactoring traits, or general review.

However, pull requests that show clear signs of low-effort AI generation will not be accepted.

Legato aims to help HUMANS make art. The project WILL NOT incorporate or promote visuals, art, music, or other creative works that were not made by HUMANS on official channels.

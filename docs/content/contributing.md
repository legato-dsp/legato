---
title: Contribution Guidlines
---

## General Principles
Before contributing, please understand the project's goals and community standards. Legato aims to help **humans** make art and run it on their own hardware.

### AI Policy
AI tools may be used for auxiliary tasks (e.g., generating test harnesses, proofreading, refactoring). However:
* **Low-effort AI:** Pull requests that show clear signs of low-effort AI generation will not be accepted. You are expected to be able to explain all the decisions in your PR.
* **Human-Centric Art:** The project **will not** incorporate or promote visuals, art, music, or other creative works that were not made by **humans** on official channels.

### Community & Licensing
* **Code of Conduct:** We strive for a diverse, respectful community. Please see the *[Berlin Code of Conduct](https://berlincodeofconduct.org/en)* for general guidelines.
* **Licensing:** Any contributions will fall under the license and additional permissions distributed with the repository.
* **Logo Usage:** The Legato logo may only be used in non-monetized contexts (e.g., Matrix or Discord). Commercial packages or educational materials not adhering to AGPLv3 or fair use are not permitted.

## Getting Started
**Before working on any new functionality, please open an issue.** Due to the plugin-oriented nature of Legato, it is unlikely that nodes will be added for every possible use case. However, if something is sufficiently general (e.g., pitch shifting, polyphase resampling), it will be considered.

### Development Setup
Developers are strongly encouraged to use **Nix** to manage dependencies. This ensures benchmarks, configurations, and tests are reproducible.
* **Standard Setup:** Clone the repository and use the provided Nix development shell (optionally via direnv). 
* **Non-Nix Setup:** If you choose not to use Nix, you will need a **Rust nightly** toolchain. Note that contributions are still expected to build and run in the Nix environment.

## Technical Standards
Legato maintains strict requirements to ensure stability and real-time audio performance.

### Code Style & Safety
* **Modeling:** Prefer modeling state and events with `enums` or `bitflags`.
* **Safety:** `unsafe` code with measurable performance gains is acceptable, provided safety is preferred elsewhere and Undefined Behavior (UB) is checked with **Miri**.
* **Testing:** Non-trivial code should include at least a few relevant test cases and benchmarks using **Criterion** (with black-boxed inputs/outputs).

### Real-Time & Concurrency
Code that runs on the audio thread **must be wait/lock-free**.
* **Prohibited:** Avoid mutexes, condition variables, syscalls, heap allocations, or any blocking synchronization primitives in the audio thread.
* **Synchronization:** If using channels, queues, or ring buffers, they must be lock-free and non-blocking. Do not assume a standard channel is real-time safe without verification.

### Performance Optimization
When optimizing DSP code, follow these steps in order:
1.  **Data Structures:** Use flat buffers or cache-optimized structures.
2.  **Vectorization:** Use chunks_exact where possible for auto-vectorization. Consider polynomial approximations for operations that cannot be efficiently vectorized, or if the cost is too high.
3.  **Branching:** Prefer branching at function entry rather than in hot DSP paths; use branchless solutions where appropriate.
4.  **Memory:** Avoid heap allocations in hot paths; use arenas or preallocation if required.
5.  **SIMD:** Once logic is optimized, consider std::simd(nightly) if it yields measurable gains.


## Pull Requests and Review
All pull requests must adhere to the following checklist:

* **Issue Reference:** Every PR must reference an existing issue, and the scope must match that issue.
* **Validation:** All CI checks must pass. Non-trivial changes must include tests.
* **Benchmarks:** Performance-relevant changes must include benchmarks or detailed justification.
* **Documentation:** PRs should include a clear description of design decisions and trade-offs.

*Note: PRs that do not meet these requirements may be closed without review.*

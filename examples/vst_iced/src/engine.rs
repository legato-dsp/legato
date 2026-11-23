use legato::{prelude::*, resources::AudioInputKey};
use nice_plug::prelude::Transport;

use crate::params::{Division, GrainDelayParams, MAX_DELAY_MS};

/// Generally we want this lower, we have this much latency,
/// at the cost of performance.
const GRAPH_BLOCK_SIZE: usize = 64;
pub const CHANNELS: usize = 2;

/// The `external` nodes in `.legato` read these host inputs by name.
const HOST_INPUT: &str = "main";
const PHASE_INPUT: &str = "phase";

const FALLBACK_TEMPO: f64 = 120.0;

const GRAIN_NODE: &str = "grains";
const REVERB_NODE: &str = "verb";
const MIX_PARAM: &str = "mix";

const GRAPH: &str = include_str!("../.legato");

/// A snapshot of the plugin params for one host block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub delay_ms: f32,
    pub sync: bool,
    pub division: Division,
    pub grain_size: f32,
    pub grains: i32,
    pub pitch: f32,
    pub shape: f32,
    pub jitter: f32,
    pub feedback: f32,
    pub damping: f32,
    pub diffusion: f32,
    pub reverb_decay: f32,
    pub reverb_mix: f32,
    pub mix: f32,
}

pub fn delay_ms(synced: Option<Division>, free_ms: f32, tempo: f64) -> f32 {
    synced
        .map_or(free_ms, |division| division.ms(tempo))
        .min(MAX_DELAY_MS)
}

impl GrainDelayParams {
    pub fn settings(&self, clock: &HostClock) -> Settings {
        let synced = self.delay_sync.value().then(|| self.delay_division.value());
        Settings {
            delay_ms: delay_ms(synced, self.delay_time.value(), clock.tempo),
            sync: self.sync.value(),
            division: self.division.value(),
            grain_size: self.grain_size.value(),
            grains: self.grains.value(),
            pitch: self.pitch.value(),
            shape: self.shape.value(),
            jitter: self.jitter.value(),
            feedback: self.feedback.value(),
            damping: self.damping.value(),
            diffusion: self.diffusion.value(),
            reverb_decay: self.reverb_decay.value(),
            reverb_mix: self.reverb_mix.value(),
            mix: self.mix.value(),
        }
    }
}

impl Settings {
    /// `grain_delay` and `plate480` have no control inputs for these yet, so they
    /// are messages that are applied unsmoothed. If this makes any pops with automation
    /// I will change the upstream node, they should probably have this anyways.
    fn node_params(&self) -> [(&'static str, &'static str, f32); 12] {
        [
            (GRAIN_NODE, "delay_length", self.delay_ms),
            (GRAIN_NODE, "sync", if self.sync { 1.0 } else { 0.0 }),
            (GRAIN_NODE, "grain_size", self.grain_size),
            (GRAIN_NODE, "overlap", self.grains as f32),
            (GRAIN_NODE, "pitch", self.pitch),
            (GRAIN_NODE, "shape", self.shape),
            (GRAIN_NODE, "jitter", self.jitter),
            (GRAIN_NODE, "feedback", self.feedback),
            (GRAIN_NODE, "damping", self.damping),
            (GRAIN_NODE, "diffusion", self.diffusion),
            (REVERB_NODE, "decay", self.reverb_decay),
            (REVERB_NODE, "mix", self.reverb_mix),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostClock {
    pub tempo: f64,
    /// Position in quarter notes, while the transport is playing.
    pub beat: Option<f64>,
}

impl HostClock {
    pub fn from_transport(transport: &Transport) -> Self {
        Self {
            tempo: transport.tempo.unwrap_or(FALLBACK_TEMPO),
            beat: transport.pos_beats().filter(|_| transport.playing),
        }
    }
}

/// A 0..1 ramp that completes one cycle per grain, locked to the host's beat grid.
///
/// While the transport is stopped it keeps running at the host tempo from wherever
/// it was, so grains keep spawning and the phase does not drift.
struct Phasor {
    sample_rate: f64,
    free_beat: f64,
    buffer: Box<[f32]>,
}

impl Phasor {
    fn new(sample_rate: f64, max_frames: usize) -> Self {
        Self {
            sample_rate,
            free_beat: 0.0,
            buffer: vec![0.0; max_frames].into(),
        }
    }

    /// Render out a frame of phase values given our host clock
    fn render(&mut self, clock: HostClock, beats_per_cycle: f64, frames: usize) -> &[f32] {
        let beats_per_sample = clock.tempo / 60.0 / self.sample_rate;
        let start = clock.beat.unwrap_or(self.free_beat);
        let out = &mut self.buffer[..frames];

        for (i, x) in out.iter_mut().enumerate() {
            let beat = start + i as f64 * beats_per_sample;
            // Rounding to f32 must not land on 1.0, which reads as an extra cycle
            *x = ((beat / beats_per_cycle).rem_euclid(1.0) as f32).min(1.0 - f32::EPSILON);
        }
        self.free_beat = start + frames as f64 * beats_per_sample;
        out
    }
}

pub struct Engine {
    app: LegatoApp,
    frontend: LegatoFrontend,
    adapter: BlockAdapter,
    host_input: AudioInputKey,
    phase_input: AudioInputKey,
    phasor: Phasor,
    sent: Option<Settings>,
}

impl Engine {
    pub fn new(sample_rate: usize, max_frames: usize) -> Self {
        let config = Config {
            sample_rate,
            block_size: GRAPH_BLOCK_SIZE,
            channels: CHANNELS,
            rt_capacity: 0,
        };
        let (app, frontend) = LegatoBuilder::<Unconfigured>::new(config)
            .register_host_audio_input(HOST_INPUT, CHANNELS)
            .register_host_audio_input(PHASE_INPUT, 1)
            .build_dsl(GRAPH)
            .expect("grain delay graph should build");
        let key = |name| {
            app.audio_input_key(name)
                .expect("host input was registered")
        };
        let (host_input, phase_input) = (key(HOST_INPUT), key(PHASE_INPUT));
        let adapter = BlockAdapter::new(&app);

        Self {
            app,
            frontend,
            adapter,
            host_input,
            phase_input,
            phasor: Phasor::new(sample_rate as f64, max_frames),
            sent: None,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.adapter.latency_samples()
    }

    fn sync_params(&mut self, settings: Settings) {
        let _ = self.frontend.set_param(MIX_PARAM, settings.mix);

        let sent = self.sent.replace(settings).map(|s| s.node_params());
        for (i, (node, param_name, value)) in settings.node_params().into_iter().enumerate() {
            // Only update parameters that have changed
            if sent.map(|s| s[i].2) != Some(value) {
                let msg = NodeMessage::SetParam(ParamPayload {
                    param_name,
                    value: RtValue::F32(value),
                });
                let _ = self.frontend.send_node_msg(node, msg);
            }
        }
    }

    /// The phasor goes in as a second host input, so the block adapter delays it
    /// exactly as much as the audio and host latency compensation lines both up.
    pub fn process(&mut self, settings: Settings, clock: HostClock, io: &mut [&mut [f32]]) {
        self.sync_params(settings);

        let frames = io.first().map_or(0, |c| c.len());

        // A host exceeding the max buffer size from `activate` holds the phasor
        // for a block rather than allocating on the audio thread.
        let phase = (frames <= self.phasor.buffer.len())
            .then(|| self.phasor.render(clock, settings.division.beats(), frames));

        let phase_chans = phase.map(|p| [p]);
        let aux = phase_chans.as_ref().map(|p| (self.phase_input, &p[..]));

        // Send to Legato for processing
        self.adapter.process(
            &mut self.app,
            Some(self.host_input),
            io,
            aux.as_slice(),
            std::iter::empty(),
        );
    }

    pub fn reset(&mut self) {
        self.adapter.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 48_000;

    const PLAYING_120: HostClock = HostClock {
        tempo: 120.0,
        beat: Some(0.0),
    };

    #[test]
    fn graph_accepts_every_synced_param() {
        let mut engine = Engine::new(SR, 512);
        let settings = GrainDelayParams::default().settings(&PLAYING_120);
        assert!(engine.frontend.set_param(MIX_PARAM, settings.mix).is_ok());
        for (node, param_name, value) in settings.node_params() {
            let msg = NodeMessage::SetParam(ParamPayload {
                param_name,
                value: RtValue::F32(value),
            });
            let res = engine.frontend.send_node_msg(node, msg);
            assert!(res.is_ok(), "{node}.{param_name}: {res:?}");
        }
    }

    fn wet_energy(settings: Settings) -> f32 {
        let mut engine = Engine::new(SR, 512);
        let mut clock = PLAYING_120;
        let mut energy = 0.0;
        for block in 0..(2 * SR / 512) {
            let mut left: Vec<f32> = (0..512)
                .map(|i| (((block * 512 + i) * 7919) % 101) as f32 / 50.0 - 1.0)
                .collect();
            let mut right = left.clone();
            engine.process(settings, clock, &mut [&mut left, &mut right]);
            energy += left.iter().map(|x| x * x).sum::<f32>();
            clock.beat = clock.beat.map(|b| b + 512.0 * 2.0 / SR as f64);
        }
        energy
    }

    #[test]
    fn free_and_synced_grains_both_reach_output() {
        let mut settings = GrainDelayParams::default().settings(&PLAYING_120);
        settings.mix = 1.0;
        assert!(wet_energy(settings) > 1.0);
        settings.sync = true;
        assert!(wet_energy(settings) > 1.0);
    }

    #[test]
    fn phasor_cycles_once_per_division_on_the_beat_grid() {
        let mut phasor = Phasor::new(SR as f64, 512);
        // At 120 bpm a quarter note is 24k samples; start half way through one
        let clock = HostClock {
            tempo: 120.0,
            beat: Some(10.5),
        };
        let p = phasor
            .render(clock, Division::Quarter.beats(), 512)
            .to_vec();
        assert!((p[0] - 0.5).abs() < 1e-6);
        assert!((p[1] - p[0] - 1.0 / 24_000.0).abs() < 1e-6);

        let p = phasor.render(clock, Division::Eighth.beats(), 512);
        assert!(p[0].abs() < 1e-6, "10.5 beats is on the eighth-note grid");
    }

    #[test]
    fn phasor_free_runs_on_from_where_the_transport_stopped() {
        let mut phasor = Phasor::new(SR as f64, 512);
        let playing = HostClock {
            tempo: 120.0,
            beat: Some(3.25),
        };
        let last = *phasor.render(playing, 1.0, 512).last().unwrap();
        let stopped = HostClock {
            beat: None,
            ..playing
        };
        let next = phasor.render(stopped, 1.0, 512)[0];
        assert!((next - last - 1.0 / 24_000.0).abs() < 1e-6);
    }

    #[test]
    fn phasor_never_reaches_one() {
        let mut phasor = Phasor::new(SR as f64, 4096);
        let clock = HostClock {
            tempo: 173.0,
            beat: Some(1.0 - 1e-12),
        };
        assert!(phasor.render(clock, 1.0, 4096).iter().all(|&p| p < 1.0));
    }

    #[test]
    fn synced_delay_follows_tempo_and_clamps_to_capacity() {
        assert_eq!(delay_ms(Some(Division::DottedEighth), 1000.0, 120.0), 375.0);
        assert_eq!(delay_ms(Some(Division::Whole), 1000.0, 30.0), MAX_DELAY_MS);
        assert_eq!(delay_ms(None, 250.0, 30.0), 250.0);
    }
}

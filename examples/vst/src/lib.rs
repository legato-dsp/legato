use egui::{Margin, Vec2};
use legato::{
    nodes::audio::grain_delay::{MAX_OVERLAP, MAX_PITCH_SEMITONES},
    prelude::*,
    resources::AudioInputKey,
};
use nice_plug::{context::gui::GuiContext, editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier,
    create_egui_editor, resizable_window::ResizableWindow, widgets,
};
use std::sync::Arc;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 700.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);

/// The fixed block size the Legato graph runs at.
/// Generally we want this lower, we have this much latency
const GRAPH_BLOCK_SIZE: usize = 64;
const CHANNELS: usize = 2;

/// The `external` nodes in `.legato` read these host inputs by name.
const HOST_INPUT: &str = "main";
const PHASE_INPUT: &str = "phase";

/// Used for the phasor when the host doesn't report a tempo.
///
/// This phasor locks the interior graph rate for the
const FALLBACK_TEMPO: f64 = 120.0;

/// Matches `max_delay_length` in `.legato`: a whole note at 60 bpm.
const MAX_DELAY_MS: f32 = 4000.0;

const GRAIN_NODE: &str = "grains";
const REVERB_NODE: &str = "verb";
const MIX_PARAM: &str = "mix";

/// The actual graph used for plugin construction
const GRAPH: &str = include_str!("../.legato");

pub struct GrainDelayEditor {
    gui_ctx: Option<GuiContext>,
    params: Arc<GrainDelayParams>,
}

fn param_row<P: Param>(ui: &mut egui::Ui, param: &P, setter: &ParamSetter) {
    ui.label(param.name());
    ui.add(widgets::ParamSlider::for_param(param, setter));
}

impl NiceEguiApp for GrainDelayEditor {
    fn build(
        &mut self,
        _egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.gui_ctx = Some(nice_gui_ctx);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.gui_ctx = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_ctx) = self.gui_ctx.as_ref() else {
            return;
        };
        let setter = gui_ctx.param_setter();
        let p = &self.params;

        ResizableWindow::new("res-wind")
            .min_size(Vec2::new(MIN_WINDOW_SIZE.width, MIN_WINDOW_SIZE.height))
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.heading("Legato Grain Delay");
                        param_row(ui, &p.delay_sync, &setter);
                        if p.delay_sync.value() {
                            param_row(ui, &p.delay_division, &setter);
                        } else {
                            param_row(ui, &p.delay_time, &setter);
                        }
                        param_row(ui, &p.sync, &setter);
                        if p.sync.value() {
                            param_row(ui, &p.division, &setter);
                        } else {
                            param_row(ui, &p.grain_size, &setter);
                        }
                        param_row(ui, &p.grains, &setter);
                        for param in [
                            &p.pitch,
                            &p.shape,
                            &p.jitter,
                            &p.feedback,
                            &p.damping,
                            &p.diffusion,
                            &p.reverb_decay,
                            &p.mix,
                        ] {
                            param_row(ui, param, &setter);
                        }
                    });
            });
    }
}

/// A note value, for delay times and grain lengths synced to the host tempo.
///
/// The ids keep saved projects stable if variants are added or reordered.
#[derive(Enum, Clone, Copy, Debug, PartialEq)]
pub enum Division {
    #[id = "1/32"]
    #[name = "1/32"]
    ThirtySecond,
    #[id = "1/16t"]
    #[name = "1/16T"]
    SixteenthTriplet,
    #[id = "1/16"]
    #[name = "1/16"]
    Sixteenth,
    #[id = "1/16d"]
    #[name = "1/16D"]
    DottedSixteenth,
    #[id = "1/8t"]
    #[name = "1/8T"]
    EighthTriplet,
    #[id = "1/8"]
    #[name = "1/8"]
    Eighth,
    #[id = "1/8d"]
    #[name = "1/8D"]
    DottedEighth,
    #[id = "1/4t"]
    #[name = "1/4T"]
    QuarterTriplet,
    #[id = "1/4"]
    #[name = "1/4"]
    Quarter,
    #[id = "1/4d"]
    #[name = "1/4D"]
    DottedQuarter,
    #[id = "1/2"]
    #[name = "1/2"]
    Half,
    #[id = "1/1"]
    #[name = "1/1"]
    Whole,
}

impl Division {
    /// Length in quarter notes, the unit hosts report positions in.
    pub fn beats(self) -> f64 {
        match self {
            Self::ThirtySecond => 1.0 / 8.0,
            Self::SixteenthTriplet => 1.0 / 6.0,
            Self::Sixteenth => 1.0 / 4.0,
            Self::DottedSixteenth => 3.0 / 8.0,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::Eighth => 1.0 / 2.0,
            Self::DottedEighth => 3.0 / 4.0,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::Quarter => 1.0,
            Self::DottedQuarter => 3.0 / 2.0,
            Self::Half => 2.0,
            Self::Whole => 4.0,
        }
    }

    pub fn ms(self, tempo: f64) -> f32 {
        (self.beats() * 60_000.0 / tempo) as f32
    }
}

#[derive(Params)]
pub struct GrainDelayParams {
    #[id = "delay_sync"]
    pub delay_sync: BoolParam,
    #[id = "delay_division"]
    pub delay_division: EnumParam<Division>,
    #[id = "delay_time"]
    pub delay_time: FloatParam,
    #[id = "sync"]
    pub sync: BoolParam,
    #[id = "division"]
    pub division: EnumParam<Division>,
    #[id = "grain_size"]
    pub grain_size: FloatParam,
    #[id = "grains"]
    pub grains: IntParam,
    #[id = "pitch"]
    pub pitch: FloatParam,
    #[id = "shape"]
    pub shape: FloatParam,
    #[id = "jitter"]
    pub jitter: FloatParam,
    #[id = "feedback"]
    pub feedback: FloatParam,
    #[id = "damping"]
    pub damping: FloatParam,
    #[id = "diffusion"]
    pub diffusion: FloatParam,
    #[id = "reverb_decay"]
    pub reverb_decay: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for GrainDelayParams {
    /// Where all of the params for our plugin are declared.
    ///
    /// A few notes for those that are new:
    ///
    /// - We define a name, default value, range
    /// - We also have a few different float ranges, which can change scaling
    /// - We can also format the string value, which is quite cool
    ///
    /// Defaults match the `grains` and `verb` nodes in `.legato`.
    fn default() -> Self {
        let unit = |name, default, min, max| {
            FloatParam::new(name, default, FloatRange::Linear { min, max })
                .with_value_to_string(formatters::v2s_f32_rounded(3))
        };
        let percent = |name, default| {
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage())
        };
        Self {
            delay_sync: BoolParam::new("Delay Sync", false),
            delay_division: EnumParam::new("Delay Division", Division::DottedEighth),
            delay_time: FloatParam::new(
                "Delay Time",
                1000.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: MAX_DELAY_MS,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            sync: BoolParam::new("Grain Sync", false),
            division: EnumParam::new("Grain Division", Division::Eighth),
            // Long grains pitched down drift far through the delay line, so we cap
            // this well below the node's 3s limit to keep a 1s delay usable.
            grain_size: FloatParam::new(
                "Grain Size",
                400.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            grains: IntParam::new(
                "Grains",
                2,
                IntRange::Linear {
                    min: 1,
                    max: MAX_OVERLAP as i32,
                },
            ),
            pitch: FloatParam::new(
                "Pitch",
                0.0,
                FloatRange::Linear {
                    min: -MAX_PITCH_SEMITONES,
                    max: MAX_PITCH_SEMITONES,
                },
            )
            .with_step_size(1.0)
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            shape: unit("Shape", 0.01, 0.001, 0.5),
            jitter: percent("Jitter", 0.0),
            feedback: percent("Feedback", 0.8),
            damping: unit("Damping", 0.8, 0.0001, 0.999),
            diffusion: percent("Diffusion", 1.0),
            reverb_decay: unit("Reverb Decay", 0.8, 0.0, 0.9),
            mix: percent("Mix", 0.5),
        }
    }
}

/// A snapshot of the plugin params for one host block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Resolved from the delay division and host tempo when synced.
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
    pub mix: f32,
}

/// The delay time sent to the graph: a note value at the host tempo when synced.
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
            mix: self.mix.value(),
        }
    }
}

impl Settings {
    /// The `(node, param, value)` messages these settings send to the graph.
    ///
    /// `grain_delay` and `plate480` have no control inputs for these yet, so they
    /// are messages, drained once per graph block and applied unsmoothed.
    fn node_params(&self) -> [(&'static str, &'static str, f32); 11] {
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
        ]
    }
}

/// Where the host is in time, for one process call.
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
/// it was, so grains keep spawning.
struct Phasor {
    sample_rate: f64,
    /// Where a stopped transport would be, in quarter notes.
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

/// Everything that depends on the sample rate, built in [`Plugin::activate`].
pub struct Engine {
    app: LegatoApp,
    frontend: LegatoFrontend,
    adapter: BlockAdapter,
    host_input: AudioInputKey,
    phase_input: AudioInputKey,
    phasor: Phasor,
    /// The last settings sent to the graph, so we only send messages on change.
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

    /// Forward plugin params to the graph.
    fn sync_params(&mut self, settings: Settings) {
        // Mix feeds smoothed `signal` nodes from a lock-free atomic store.
        let _ = self.frontend.set_param(MIX_PARAM, settings.mix);

        // Hold onto our last value to only send messages when required
        let sent = self.sent.replace(settings).map(|s| s.node_params());
        for (i, (node, param_name, value)) in settings.node_params().into_iter().enumerate() {
            if sent.map(|s| s[i].2) != Some(value) {
                let msg = NodeMessage::SetParam(ParamPayload {
                    param_name,
                    value: RtValue::F32(value),
                });
                let _ = self.frontend.send_node_msg(node, msg);
            }
        }
    }

    /// Render the host's audio through the Legato graph, in place.
    ///
    /// The phasor goes in as a second host input, so the block adapter delays it
    /// exactly as much as the audio and host latency compensation lines both up.
    pub fn process(&mut self, settings: Settings, clock: HostClock, io: &mut [&mut [f32]]) {
        self.sync_params(settings);
        let frames = io.first().map_or(0, |c| c.len());
        // Hosts never exceed the max buffer size from `activate`, but if one does,
        // the phasor holds for a block rather than allocating on the audio thread.
        let phase = (frames <= self.phasor.buffer.len())
            .then(|| self.phasor.render(clock, settings.division.beats(), frames));
        let phase_chans = phase.map(|p| [p]);
        let aux = phase_chans.as_ref().map(|p| (self.phase_input, &p[..]));
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

pub struct LegatoGrainDelay {
    params: Arc<GrainDelayParams>,
    editor_state: Arc<EguiEditorState>,
    engine: Option<Engine>,
}

impl Default for LegatoGrainDelay {
    fn default() -> Self {
        Self {
            params: Arc::new(GrainDelayParams::default()),
            editor_state: EguiEditorState::from_size(MIN_WINDOW_SIZE, 1.0),
            engine: None,
        }
    }
}

impl Plugin for LegatoGrainDelay {
    const NAME: &'static str = "Legato Grain Delay";
    const VENDOR: &'static str = "Legato";
    const URL: &'static str = "https://github.com/legato-dsp/legato";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(CHANNELS as u32),
        main_output_channels: NonZeroU32::new(CHANNELS as u32),
        ..AudioIOLayout::const_default()
    }];

    type Editor = EguiEditor<GrainDelayEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_egui_editor(
            self.editor_state.clone(),
            RepaintNotifier::new(),
            EguiNiceSettings::new().with_resize_hint(RESIZE_HINT),
            GrainDelayEditor {
                gui_ctx: None,
                params: self.params.clone(),
            },
        )
    }

    /// Startup the actual plugin
    ///
    /// This is where all of the allocation and initial construction will occur
    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        let engine = Engine::new(
            buffer_config.sample_rate as usize,
            buffer_config.max_buffer_size as usize,
        );
        context.set_latency_samples(engine.latency_samples() as u32);
        self.engine = Some(engine);
        true
    }

    fn reset(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        if let Some(engine) = &mut self.engine {
            let clock = HostClock::from_transport(context.transport());
            engine.process(self.params.settings(&clock), clock, buffer.as_slice());
        }
        // Keep processing so delay/reverb tails ring out
        ProcessStatus::KeepAlive
    }
}

/// Where the plugin metadata lives
impl ClapPlugin for LegatoGrainDelay {
    const CLAP_ID: &'static str = "dev.legato.grain-delay";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A Legato granular delay with reverb");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Delay,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LegatoGrainDelay {
    const VST3_CLASS_ID: [u8; 16] = *b"LegatoGrainDly!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nice_export_clap!(LegatoGrainDelay);
nice_export_vst3!(LegatoGrainDelay);

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
        // Two seconds of noise-ish input, so there's always something to grain
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

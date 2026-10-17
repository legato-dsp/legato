use egui::{Margin, Vec2};
use legato::prelude::*;
use nice_plug::{context::gui::GuiContext, editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier,
    create_egui_editor, resizable_window::ResizableWindow, widgets,
};
use std::sync::Arc;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 320.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);

/// The fixed block size the Legato graph runs at.
/// Generally we want this lower, we have this much latency
const GRAPH_BLOCK_SIZE: usize = 64;
const CHANNELS: usize = 2;

/// The MIDI channel the graph's `poly_voice` listens on.
/// For this example, host notes on any channel are sent here.
const GRAPH_MIDI_CHANNEL: u8 = 0;

const REVERB_NODE: &str = "verb";
const CUTOFF_PARAM: &str = "cutoff";
const Q_PARAM: &str = "q";
const MIX_PARAM: &str = "mix";

/// The actual graph used for plugin construction
const GRAPH: &str = include_str!("../.legato");

pub struct SynthEditor {
    gui_ctx: Option<GuiContext>,
    params: Arc<SynthParams>,
}

impl NiceEguiApp for SynthEditor {
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

        ResizableWindow::new("res-wind")
            .min_size(Vec2::new(MIN_WINDOW_SIZE.width, MIN_WINDOW_SIZE.height))
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.heading("Legato Poly Saw");
                        // Make the same generic label and slider for each param
                        for param in [
                            &self.params.cutoff,
                            &self.params.resonance,
                            &self.params.decay,
                            &self.params.damping,
                            &self.params.bandwidth,
                            &self.params.mix,
                        ] {
                            ui.label(param.name());
                            ui.add(widgets::ParamSlider::for_param(param, &setter));
                        }
                    });
            });
    }
}

#[derive(Params)]
pub struct SynthParams {
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    #[id = "resonance"]
    pub resonance: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "damping"]
    pub damping: FloatParam,
    #[id = "bandwidth"]
    pub bandwidth: FloatParam,
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for SynthParams {
    /// Where all of the params for our synth are declared.
    ///
    /// A few notes for those that are new:
    ///
    /// - We define a name, default value, range
    /// - We also have a few different float ranges, which can change scaling
    /// - We can also format the string value, which is quite cool
    fn default() -> Self {
        // Plate ranges match the Legato Plate node
        let unit = |name, default, max| {
            FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max })
                .with_value_to_string(formatters::v2s_f32_rounded(3))
        };
        Self {
            // Ranges match the `signal` nodes in `.legato`.
            cutoff: FloatParam::new(
                "Cutoff",
                2400.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_hz_then_khz(1))
            .with_string_to_value(formatters::s2v_f32_hz_then_khz()),
            resonance: FloatParam::new(
                "Resonance",
                0.7,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 10.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            decay: unit("Decay", 0.8, 0.9),
            damping: unit("Damping", 0.3, 0.999),
            bandwidth: unit("Bandwidth", 0.9995, 1.0),
            mix: FloatParam::new("Reverb Mix", 0.3, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_unit("%")
                .with_value_to_string(formatters::v2s_f32_percentage(0))
                .with_string_to_value(formatters::s2v_f32_percentage()),
        }
    }
}

/// A snapshot of the plugin params for one host block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub cutoff: f32,
    pub resonance: f32,
    pub decay: f32,
    pub damping: f32,
    pub bandwidth: f32,
    pub mix: f32,
}

impl SynthParams {
    pub fn settings(&self) -> Settings {
        Settings {
            cutoff: self.cutoff.value(),
            resonance: self.resonance.value(),
            decay: self.decay.value(),
            damping: self.damping.value(),
            bandwidth: self.bandwidth.value(),
            mix: self.mix.value(),
        }
    }
}

/// Converts a host note event to a Legato MIDI message at its frame in the host block.
pub fn to_legato_midi<S>(event: NoteEvent<S>) -> Option<(usize, MidiMessage)> {
    // Cast our floating point to a MIDI velocity value
    let velocity = |v: f32| (v.clamp(0.0, 1.0) * 127.0).round() as u8;
    // Cast our [`NoteEvent`] to MIDI that Legato can read
    let (timing, data) = match event {
        NoteEvent::NoteOn {
            timing,
            key,
            velocity: v,
            ..
        } => (
            timing,
            MidiMessageKind::NoteOn {
                note: key.number()?,
                velocity: velocity(v),
            },
        ),
        NoteEvent::NoteOff {
            timing,
            key,
            velocity: v,
            ..
        } => (
            timing,
            MidiMessageKind::NoteOff {
                note: key.number()?,
                velocity: velocity(v),
            },
        ),
        NoteEvent::Choke { timing, key, .. } => (
            timing,
            MidiMessageKind::NoteOff {
                note: key.number()?,
                velocity: 0,
            },
        ),
        _ => return None,
    };
    Some((
        timing as usize,
        MidiMessage {
            data,
            channel_idx: GRAPH_MIDI_CHANNEL,
        },
    ))
}

/// Everything that depends on the sample rate, built in [`Plugin::activate`].
pub struct Engine {
    app: LegatoApp,
    frontend: LegatoFrontend,
    adapter: BlockAdapter,
    /// The last plate settings sent to the graph, so we only send messages on change.
    sent: Option<Settings>,
}

impl Engine {
    pub fn new(sample_rate: usize) -> Self {
        let config = Config {
            sample_rate,
            block_size: GRAPH_BLOCK_SIZE,
            channels: CHANNELS,
            rt_capacity: 0,
        };
        let (app, frontend) = LegatoBuilder::<Unconfigured>::new(config)
            .build_dsl(GRAPH)
            .expect("synth graph should build");
        let adapter = BlockAdapter::new(&app);

        Self {
            app,
            frontend,
            adapter,
            sent: None,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.adapter.latency_samples()
    }

    /// Forward plugin params to the graph.
    fn sync_params(&mut self, settings: Settings) {
        // These feed smoothed `signal` nodes from a lock-free atomic store.
        let _ = self.frontend.set_param(CUTOFF_PARAM, settings.cutoff);
        let _ = self.frontend.set_param(Q_PARAM, settings.resonance);
        let _ = self.frontend.set_param(MIX_PARAM, settings.mix);

        // `plate480` has no control inputs yet, so its params are messages, drained once per
        // graph block and applied unsmoothed.
        //
        // Could be nice to add this later for audio rate, but `plate480` was more for quick demos

        // Hold onto our last value to only send messages when required
        let sent = self.sent.replace(settings);
        let plate_params = [
            ("decay", settings.decay, sent.map(|s| s.decay)),
            ("damping", settings.damping, sent.map(|s| s.damping)),
            ("bandwidth", settings.bandwidth, sent.map(|s| s.bandwidth)),
        ];
        // If any param values do not match our current values, send a message and update
        for (param_name, value, prev) in plate_params {
            if Some(value) != prev {
                let msg = NodeMessage::SetParam(ParamPayload {
                    param_name,
                    value: RtValue::F32(value),
                });
                let _ = self.frontend.send_node_msg(REVERB_NODE, msg);
            }
        }
    }

    /// Render into IO from the internal Legato app
    ///
    /// It looks like midi messages here are already pre-sorted,
    /// we pass these in as well where they will be adapted to fit
    /// Legato's internal midi tooling.
    pub fn process(
        &mut self,
        settings: Settings,
        io: &mut [&mut [f32]],
        midi: impl IntoIterator<Item = (usize, MidiMessage)>,
    ) {
        self.sync_params(settings);
        self.adapter.process(&mut self.app, None, io, &[], midi);
    }

    // TODO: Nodes do not actually have a reset trait yet, good
    // first issue if anyone is interested!
    pub fn reset(&mut self) {
        self.adapter.reset();
    }
}

pub struct LegatoSynth {
    params: Arc<SynthParams>,
    editor_state: Arc<EguiEditorState>,
    engine: Option<Engine>,
}

impl Default for LegatoSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(SynthParams::default()),
            editor_state: EguiEditorState::from_size(MIN_WINDOW_SIZE, 1.0),
            engine: None,
        }
    }
}

impl Plugin for LegatoSynth {
    const NAME: &'static str = "Legato Poly Saw";
    const VENDOR: &'static str = "Legato";
    const URL: &'static str = "https://github.com/legato-dsp/legato";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(CHANNELS as u32),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;

    type Editor = EguiEditor<SynthEditor>;
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
            SynthEditor {
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
        let engine = Engine::new(buffer_config.sample_rate as usize);
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
            // Host events arrive in timing order, which the block adapter expects.
            let midi = std::iter::from_fn(|| context.next_event()).filter_map(to_legato_midi);
            engine.process(self.params.settings(), buffer.as_slice(), midi);
        }
        // Keep processing so reverb/tails ring out
        ProcessStatus::KeepAlive
    }
}

/// Where the plugin metadata lives
impl ClapPlugin for LegatoSynth {
    const CLAP_ID: &'static str = "dev.legato.poly-saw";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A Legato polyphonic sawtooth synth");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for LegatoSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"LegatoPolySaw!!!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nice_export_clap!(LegatoSynth);
nice_export_vst3!(LegatoSynth);

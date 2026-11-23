pub mod app;
pub mod engine;
pub mod knob;
pub mod metrics;
pub mod params;
pub mod theme;

use std::sync::Arc;

use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceSettings, application, create_iced_editor,
    iced::PollSubNotifier,
};

use app::App;
use engine::{CHANNELS, Engine, HostClock};
use metrics::DESIGN_SIZE;
use params::GrainDelayParams;

const WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(DESIGN_SIZE.width, DESIGN_SIZE.height);
const MIN_WINDOW_SIZE: LogicalSize<f32> =
    LogicalSize::new(DESIGN_SIZE.width / 2.0, DESIGN_SIZE.height / 2.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);

pub struct LegatoGrainDelay {
    params: Arc<GrainDelayParams>,
    editor_state: Arc<IcedEditorState>,
    notifier: PollSubNotifier,
    engine: Option<Engine>,
}

impl Default for LegatoGrainDelay {
    fn default() -> Self {
        Self {
            params: Arc::new(GrainDelayParams::default()),
            editor_state: IcedEditorState::from_size(WINDOW_SIZE, 1.0),
            notifier: PollSubNotifier::new(),
            engine: None,
        }
    }
}

impl Plugin for LegatoGrainDelay {
    const NAME: &'static str = "Legato Grain Delay (iced)";
    const VENDOR: &'static str = "Legato";
    const URL: &'static str = "https://github.com/legato-dsp/legato";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(CHANNELS as u32),
        main_output_channels: NonZeroU32::new(CHANNELS as u32),
        ..AudioIOLayout::const_default()
    }];

    type Editor = IcedEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_iced_editor(
            self.editor_state.clone(),
            self.params.clone(),
            self.notifier.clone(),
            IcedNiceSettings::new().with_resize_hint(RESIZE_HINT),
            |params, ctx| {
                Ok(application(params, ctx, App::new, App::update, App::view)
                    .font(theme::SATOSHI_BYTES)
                    .font(theme::CHILLAX_BYTES)
                    .subscription(App::subscription)
                    .run())
            },
        )
    }

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
        // Legato adds a small amount of latency from the internal block adapter
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

impl ClapPlugin for LegatoGrainDelay {
    const CLAP_ID: &'static str = "dev.legato.grain-delay-iced";
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
    const VST3_CLASS_ID: [u8; 16] = *b"LegatoGrainIced!";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nice_export_clap!(LegatoGrainDelay);
nice_export_vst3!(LegatoGrainDelay);

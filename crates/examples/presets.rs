use cpal::Host;
use legato::{
    FrontendError,
    batch::{Batch, BatchBuilder},
    node_state::NodeState::{self, *},
    prelude::*,
};
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

const PATCH: &str = include_str!("../demos/presets.legato");

/// `voice * 4` in the patch.
const VOICES: usize = 4;

/// The effects, with their toggle key and the state each takes when off.
const EFFECTS: [(&str, char, NodeState); 3] = [
    ("tone", 't', Bypassed),
    ("echo", 'e', Muted),
    ("room", 'r', Bypassed),
];

/// A preset consisting of how many voices we are using, FM params,
/// effects values, envelope, etc. These can then be sent to the runtime
/// to update the nodes we want to target.
struct Preset {
    name: &'static str,
    /// How many of the 4 voices play; the rest are suspended.
    voices: usize,
    /// FM kernel params: ratio, index, feedback.
    fm: [f32; 3],
    /// Envelope: attack, decay, sustain, release.
    env: [f32; 4],
    cut: f32,
    q: f32,
    /// Whether each of [`EFFECTS`] is on.
    effects: [bool; 3],
    room_mix: f32,
}

const PRESETS: [Preset; 4] = [
    Preset {
        name: "Duo",
        voices: 2,
        fm: [2.0, 200.0, 20.0],
        env: [3.0, 220.0, 0.1, 300.0],
        cut: 3000.0,
        q: 0.8,
        effects: [true, true, false],
        room_mix: 0.3,
    },
    Preset {
        name: "Bells",
        voices: 4,
        fm: [3.5, 300.0, 30.0],
        env: [2.0, 900.0, 0.0, 1200.0],
        cut: 1800.0,
        q: 0.7,
        effects: [false, true, true],
        room_mix: 0.4,
    },
    Preset {
        name: "Growl",
        voices: 2,
        fm: [1.0, 900.0, 200.0],
        env: [8.0, 180.0, 0.4, 200.0],
        cut: 900.0,
        q: 1.2,
        effects: [true, false, false],
        room_mix: 0.3,
    },
    Preset {
        name: "Shimmer",
        voices: 4,
        fm: [7.0, 150.0, 10.0],
        env: [200.0, 600.0, 0.5, 1500.0],
        cut: 6000.0,
        q: 0.5,
        effects: [true, true, true],
        room_mix: 0.6,
    },
];

fn effect_state(i: usize, on: bool) -> NodeState {
    if on { Active } else { EFFECTS[i].2 }
}

/// Play the first `voices` voices and suspend the rest.
fn voice_states(batch: &mut BatchBuilder, voices: usize) -> Result<(), FrontendError> {
    batch.activate(&format!("voice(0..{voices})"))?;
    // `voice(4..4)` selects nothing, which is an error, so skip it.
    if voices < VOICES {
        batch.suspend(&format!("voice({voices}..{VOICES})"))?;
    }
    Ok(())
}

impl Preset {
    /// Everything the preset changes, applied in one block.
    fn batch(&self, frontend: &LegatoFrontend) -> Result<Batch, FrontendError> {
        let mut batch = frontend.batch();
        voice_states(&mut batch, self.voices)?;
        // Suspended voices take the params too, so they match when they return.
        for (param, val) in ["ratio", "index", "feedback"].into_iter().zip(self.fm) {
            batch.set("voice(*).op", param, val)?;
        }
        for (param, val) in ["attack", "decay", "sustain", "release"]
            .into_iter()
            .zip(self.env)
        {
            batch.set("voice(*).env", param, val)?;
        }
        for (i, (node, ..)) in EFFECTS.iter().enumerate() {
            batch.set_state(node, effect_state(i, self.effects[i]))?;
        }
        batch.set_param("cut", self.cut)?;
        batch.set("tone", "q", self.q)?;
        batch.set("room", "mix", self.room_mix)?;
        Ok(batch.finish())
    }
}

fn build(config: Config) -> (LegatoApp, LegatoFrontend, Vec<Batch>) {
    let (app, frontend) = LegatoBuilder::<Unconfigured>::new(config)
        .build_dsl(PATCH)
        .expect("patch should build");
    // Built once; sending one again only shares it.
    let batches = PRESETS
        .iter()
        .map(|p| p.batch(&frontend).expect("preset should resolve"))
        .collect();
    (app, frontend, batches)
}

#[cfg(all(target_os = "linux", feature = "jack"))]
fn make_host() -> Host {
    cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available")
}

#[cfg(target_os = "macos")]
fn make_host() -> Host {
    cpal::host_from_id(cpal::HostId::CoreAudio).expect("CoreAudio host not available")
}

#[cfg(not(any(target_os = "macos", all(target_os = "linux", feature = "jack"))))]
fn make_host() -> Host {
    cpal::default_host()
}

/// What the frontend last sent; the audio thread owns the real states.
struct Ui {
    preset: usize,
    voices: usize,
    effects: [bool; 3],
    status: String,
}

fn on_off(on: bool, off: NodeState) -> Span<'static> {
    let (text, color) = if on {
        ("Active".to_string(), Color::Green)
    } else {
        (format!("{off:?}"), Color::DarkGray)
    };
    Span::styled(text, Style::default().fg(color))
}

fn draw(frame: &mut Frame, ui: &Ui) {
    let [presets, playing, help] = Layout::vertical([
        Constraint::Length(PRESETS.len() as u16 + 2),
        Constraint::Length(EFFECTS.len() as u16 + 3),
        Constraint::Min(1),
    ])
    .areas(frame.area());

    let lines: Vec<Line> = PRESETS
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let style = if i == ui.preset {
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Cyan)
            } else {
                Style::default()
            };
            Line::styled(format!(" {}  {}", i + 1, p.name), style)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Presets ")),
        presets,
    );

    let mut lines = vec![Line::from(format!(" v  voices {}/{VOICES}", ui.voices))];
    lines.extend(EFFECTS.iter().enumerate().map(|(i, (node, key, off))| {
        Line::from(vec![
            Span::raw(format!(" {key}  {node:<6}")),
            on_off(ui.effects[i], *off),
        ])
    }));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Playing ")),
        playing,
    );

    frame.render_widget(
        Paragraph::new(format!(
            " 1-4 preset · v voices · t e r toggle · q quit   {}",
            ui.status
        ))
        .style(Style::default().fg(Color::DarkGray)),
        help,
    );
}

fn main() -> std::io::Result<()> {
    let config = Config {
        sample_rate: 48_000,
        block_size: 256,
        channels: 2,
        rt_capacity: 0,
    };

    let (app, mut frontend, batches) = build(config);

    // Make a default host based on your host OS and features
    let host = make_host();

    // Once we've built this we now have an audio thread.
    //
    // Applications without a UI thread can park here.
    let _ = AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio");

    let mut ui = Ui {
        preset: 0,
        voices: PRESETS[0].voices,
        effects: PRESETS[0].effects,
        status: String::new(),
    };

    frontend.send(&batches[0]).unwrap();

    let mut terminal = ratatui::init();

    loop {
        terminal.draw(|frame| draw(frame, &ui))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let KeyCode::Char(c) = key.code else {
            continue;
        };
        // Only record what was actually sent.
        let result = match c {
            'q' => break,
            '1'..='4' => {
                let i = c as usize - '1' as usize;
                frontend.send(&batches[i]).map(|()| {
                    ui.preset = i;
                    ui.voices = PRESETS[i].voices;
                    ui.effects = PRESETS[i].effects;
                })
            }
            // Several nodes per voice, so this goes in a batch.
            'v' => {
                let voices = ui.voices % VOICES + 1;
                let batch = {
                    let mut batch = frontend.batch();
                    voice_states(&mut batch, voices).map(|()| batch.finish())
                };
                batch
                    .and_then(|batch| frontend.send(&batch))
                    .map(|()| ui.voices = voices)
            }
            _ => match EFFECTS.iter().position(|e| e.1 == c) {
                Some(i) => frontend
                    .set_state(EFFECTS[i].0, effect_state(i, !ui.effects[i]))
                    .map(|()| ui.effects[i] = !ui.effects[i]),
                None => Ok(()),
            },
        };
        ui.status = result.err().map(|e| e.to_string()).unwrap_or_default();
    }
    ratatui::restore();
    Ok(())
}

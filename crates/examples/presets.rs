//! A self-playing instrument whose presets are batches: each one switches
//! layers and effects and sets their params in a single block.
//!
//! Keys: `1`-`4` presets, `l` / `b` toggle the lead and bell layers, `t` / `e`
//! / `r` toggle the tone filter, echo and room, `q` quits.
//!
//! `cargo run --example presets -- --check` renders every preset offline and
//! prints its level instead of opening an audio device.

use cpal::Host;
use legato::{
    FrontendError,
    batch::Batch,
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

/// The nodes a preset switches, with the state each toggles to when not Active.
const SWITCHES: [(&str, char, NodeState); 5] = [
    ("lead", 'l', Suspended),
    ("bell", 'b', Suspended),
    ("tone", 't', Bypassed),
    ("echo", 'e', Muted),
    ("room", 'r', Bypassed),
];

struct Preset {
    name: &'static str,
    /// Whether each of [`SWITCHES`] is on.
    on: [bool; 5],
    cut: f32,
    q: f32,
    /// Lead envelope: attack, decay, sustain, release.
    lead_env: [f32; 4],
    room_mix: f32,
}

const PRESETS: [Preset; 4] = [
    Preset {
        name: "Pluck",
        on: [true, false, true, true, false],
        cut: 2400.0,
        q: 0.9,
        lead_env: [3.0, 160.0, 0.1, 200.0],
        room_mix: 0.3,
    },
    Preset {
        name: "Bells",
        on: [false, true, false, true, true],
        cut: 1800.0,
        q: 0.7,
        lead_env: [4.0, 180.0, 0.2, 250.0],
        room_mix: 0.35,
    },
    Preset {
        name: "Dark pad",
        on: [true, true, true, false, true],
        cut: 500.0,
        q: 0.4,
        lead_env: [350.0, 400.0, 0.7, 900.0],
        room_mix: 0.6,
    },
    Preset {
        name: "Dry",
        on: [true, true, false, false, false],
        cut: 1800.0,
        q: 0.7,
        lead_env: [4.0, 180.0, 0.2, 250.0],
        room_mix: 0.3,
    },
];

fn state(i: usize, on: bool) -> NodeState {
    if on { Active } else { SWITCHES[i].2 }
}

impl Preset {
    /// Everything the preset changes, applied in one block.
    fn batch(&self, frontend: &LegatoFrontend) -> Result<Batch, FrontendError> {
        let mut batch = frontend.batch();
        for (i, (node, ..)) in SWITCHES.iter().enumerate() {
            batch.set_state(node, state(i, self.on[i]))?;
        }
        batch.set_param("cut", self.cut)?;
        batch.set("tone", "q", self.q)?;
        for (param, val) in ["attack", "decay", "sustain", "release"]
            .into_iter()
            .zip(self.lead_env)
        {
            batch.set("lead_env", param, val)?;
        }
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

/// Render a few seconds of each preset and print its peak and RMS.
fn check(config: Config) {
    let (mut app, mut frontend, batches) = build(config);
    let blocks = 3 * config.sample_rate / config.block_size;
    for (preset, batch) in PRESETS.iter().zip(&batches) {
        frontend.send(batch).unwrap();
        let (mut peak, mut sum, mut n) = (0.0f32, 0.0f64, 0usize);
        for _ in 0..blocks {
            let out = app.next_block();
            for chan in &out.channels[..out.chans] {
                for &y in *chan {
                    assert!(y.is_finite(), "{} produced {y}", preset.name);
                    peak = peak.max(y.abs());
                    sum += (y * y) as f64;
                    n += 1;
                }
            }
        }
        let rms = (sum / n as f64).sqrt();
        println!("{:<10} peak {peak:.3}  rms {rms:.4}", preset.name);
    }
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

struct Ui {
    preset: usize,
    /// What the frontend last sent for each of [`SWITCHES`]; the audio thread
    /// owns the real states.
    on: [bool; 5],
    status: String,
}

fn draw(frame: &mut Frame, ui: &Ui) {
    let [presets, switches, help] = Layout::vertical([
        Constraint::Length(PRESETS.len() as u16 + 2),
        Constraint::Length(SWITCHES.len() as u16 + 2),
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

    let lines: Vec<Line> = SWITCHES
        .iter()
        .enumerate()
        .map(|(i, (node, key, _))| {
            let s = state(i, ui.on[i]);
            let color = if s == Active {
                Color::Green
            } else {
                Color::DarkGray
            };
            Line::from(vec![
                Span::raw(format!(" {key}  {node:<6}")),
                Span::styled(format!("{s:?}"), Style::default().fg(color)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Nodes ")),
        switches,
    );

    frame.render_widget(
        Paragraph::new(format!(
            " 1-4 preset · l b t e r toggle · q quit   {}",
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
    if std::env::args().any(|a| a == "--check") {
        check(config);
        return Ok(());
    }

    let (app, mut frontend, batches) = build(config);
    let host = make_host();
    let _audio = AudioInterface::builder(&host, config)
        .build(app)
        .expect("Failed to start audio");

    let mut ui = Ui {
        preset: 0,
        on: PRESETS[0].on,
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
        let result = match c {
            'q' => break,
            // Only record what was actually sent.
            '1'..='4' => {
                let i = c as usize - '1' as usize;
                frontend.send(&batches[i]).map(|()| {
                    ui.preset = i;
                    ui.on = PRESETS[i].on;
                })
            }
            _ => match SWITCHES.iter().position(|s| s.1 == c) {
                Some(i) => frontend
                    .set_state(SWITCHES[i].0, state(i, !ui.on[i]))
                    .map(|()| ui.on[i] = !ui.on[i]),
                None => Ok(()),
            },
        };
        ui.status = result.err().map(|e| e.to_string()).unwrap_or_default();
    }
    ratatui::restore();
    Ok(())
}

use std::{fs, path::PathBuf};

use cpal::Host;
use legato::{
    builder::{LegatoBuilder, Unconfigured},
    config::Config,
    interface::AudioInterface,
    spec::NodeDefinition,
};
use ratatui::{
    Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

// A codegen kernel node so the `modtap` demo can reference it from the DSL.
legato_macros::include_node!("kernels/modtap4.legato", "modtap4");

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(feature = "jack")]
fn make_host() -> Host {
    cpal::host_from_id(cpal::HostId::Jack).expect("JACK host not available")
}

#[cfg(not(feature = "jack"))]
fn make_host() -> Host {
    cpal::default_host()
}

struct Demo {
    title: String,
    desc: String,
    source: String,
}

/// Add a bit of extra metadata to the demos here, with a name and description
///
/// This could eventually make its way into the DSL, but not needed for now.
fn parse_demo(path: &PathBuf, source: String) -> Demo {
    let mut title = None;
    let mut desc: Vec<String> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(comment) = trimmed.strip_prefix("//") {
            let comment = comment.trim();
            if let Some(v) = comment.strip_prefix("title:") {
                title = Some(v.trim().to_string());
            } else if let Some(v) = comment.strip_prefix("desc:") {
                desc.push(v.trim().to_string());
            }
        } else if !trimmed.is_empty() {
            break;
        }
    }

    let title = title.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
            .to_string()
    });

    Demo {
        title,
        desc: desc.join(" "),
        source,
    }
}

/// Load all of the demos in the nearby directory
fn load_demos(dir: &PathBuf) -> std::io::Result<Vec<Demo>> {
    let mut demos: Vec<Demo> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "legato"))
        .filter_map(|p| fs::read_to_string(&p).ok().map(|s| parse_demo(&p, s)))
        .collect();
    demos.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Ok(demos)
}

/// Build the audio interface, and create the legato application from the source file
fn build_interface<'a>(
    host: &'a Host,
    config: Config,
    source: &str,
) -> Result<AudioInterface<'a>, String> {
    let (app, _frontend) = LegatoBuilder::<Unconfigured>::new(config)
        .register_node("audio", Modtap4::spec())
        .build_dsl(source)
        .map_err(|e| format!("{e:?}"))?;
    AudioInterface::builder(host, config)
        .build(app)
        .map_err(|e| e.to_string())
}

/// Which panel is currently in use
#[derive(PartialEq)]
enum Focus {
    List,
    Config,
}

const SAMPLE_RATES: [usize; 4] = [44_100, 48_000, 88_200, 96_000];
const BLOCK_SIZES: [usize; 7] = [64, 128, 256, 512, 1024, 2048, 4096];
const CHANNELS: [usize; 2] = [1, 2];

/// Step `value` through `options` by `delta`, clamping at the ends. Snaps to the
/// nearest option first if `value` is not already one of them.
fn step(value: usize, options: &[usize], delta: isize) -> usize {
    let pos = options.iter().position(|&o| o == value).unwrap_or_else(|| {
        let mut best = 0;
        let mut best_diff = usize::MAX;
        for (i, &o) in options.iter().enumerate() {
            let diff = o.abs_diff(value);
            if diff < best_diff {
                best_diff = diff;
                best = i;
            }
        }
        best
    });
    let next = (pos as isize + delta).clamp(0, options.len() as isize - 1) as usize;
    options[next]
}

struct App {
    demos: Vec<Demo>,
    list_state: ListState,
    scroll: u16,
    playing: Option<usize>,
    status: String,
    focus: Focus,
    config: Config,
    config_field: usize,
}

impl App {
    fn selected(&self) -> Option<&Demo> {
        self.list_state.selected().and_then(|i| self.demos.get(i))
    }

    fn select_next(&mut self) {
        if self.demos.is_empty() {
            return;
        }
        let last = self.demos.len() - 1;
        let next = match self.list_state.selected() {
            Some(i) if i >= last => 0,
            Some(i) => i + 1,
            None => 0,
        };
        self.list_state.select(Some(next));
        self.scroll = 0;
    }

    fn select_prev(&mut self) {
        if self.demos.is_empty() {
            return;
        }
        let last = self.demos.len() - 1;
        let prev = match self.list_state.selected() {
            Some(0) | None => last,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(prev));
        self.scroll = 0;
    }

    /// Adjust the focused config field. Returns true if a value changed.
    fn adjust_config(&mut self, delta: isize) -> bool {
        let before = self.config;
        match self.config_field {
            0 => self.config.sample_rate = step(self.config.sample_rate, &SAMPLE_RATES, delta),
            1 => self.config.block_size = step(self.config.block_size, &BLOCK_SIZES, delta),
            2 => self.config.channels = step(self.config.channels, &CHANNELS, delta),
            _ => {
                self.config.rt_capacity =
                    (self.config.rt_capacity as isize + delta * 64).max(0) as usize
            }
        }
        self.config != before
    }
}

fn config_lines(app: &App) -> Vec<Line<'static>> {
    let fields = [
        ("sample rate", format!("{} Hz", app.config.sample_rate)),
        ("block size", format!("{}", app.config.block_size)),
        ("channels", format!("{}", app.config.channels)),
        ("rt capacity", format!("{}", app.config.rt_capacity)),
    ];
    fields
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let line = Line::from(format!(" {label:<12}{value}"));
            if app.focus == Focus::Config && app.config_field == i {
                line.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                line
            }
        })
        .collect()
}

fn draw(frame: &mut Frame, app: &mut App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(outer[0]);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(6)])
        .split(panes[0]);

    let items: Vec<ListItem> = app
        .demos
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let marker = if app.playing == Some(i) { "♪ " } else { "  " };
            ListItem::new(format!("{marker}{}", d.title))
        })
        .collect();

    let list_title = if app.focus == Focus::List {
        " Patches "
    } else {
        " patches "
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, left[0], &mut app.list_state);

    let config_title = if app.focus == Focus::Config {
        " Config ◄ ► "
    } else {
        " config "
    };
    let config_widget = Paragraph::new(config_lines(app))
        .block(Block::default().borders(Borders::ALL).title(config_title));
    frame.render_widget(config_widget, left[1]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(panes[1]);

    let (desc, source) = match app.selected() {
        Some(d) => (d.desc.clone(), d.source.clone()),
        None => (
            "No .legato patches found in the demos directory.".to_string(),
            String::new(),
        ),
    };

    let desc_widget = Paragraph::new(desc).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Description "),
    );
    frame.render_widget(desc_widget, right[0]);

    let source_widget = Paragraph::new(source)
        .scroll((app.scroll, 0))
        .block(Block::default().borders(Borders::ALL).title(" Source "));
    frame.render_widget(source_widget, right[1]);

    let help = Line::from(vec![
        "  ↑/↓".into(),
        " select   ".into(),
        "Tab".into(),
        " config   ".into(),
        "Enter".into(),
        " play   ".into(),
        "r".into(),
        " reload   ".into(),
        "q".into(),
        " quit    ".into(),
        app.status.clone().into(),
    ])
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(help), outer[1]);
}

fn main() -> std::io::Result<()> {
    let demos_dir: PathBuf = std::env::var("LEGATO_DEMOS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demos"));

    let config = Config {
        sample_rate: env_or("LEGATO_SAMPLE_RATE", 44_100),
        block_size: env_or("LEGATO_BLOCK_SIZE", 256),
        channels: env_or("LEGATO_CHANNELS", 2),
        rt_capacity: env_or("LEGATO_RT_CAPACITY", 0),
    };

    let host = make_host();
    let mut playing_interface: Option<AudioInterface> = None;

    let demos = load_demos(&demos_dir).unwrap_or_default();
    let mut list_state = ListState::default();

    if !demos.is_empty() {
        list_state.select(Some(0));
    }

    let mut app = App {
        demos,
        list_state,
        scroll: 0,
        playing: None,
        status: String::new(),
        focus: Focus::List,
        config,
        config_field: 0,
    };

    let mut terminal = ratatui::init();
    loop {
        terminal.draw(|frame| draw(frame, &mut app))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => break,
            KeyCode::Tab => {
                app.focus = if app.focus == Focus::List {
                    Focus::Config
                } else {
                    Focus::List
                };
            }
            KeyCode::Up => match app.focus {
                Focus::List => app.select_prev(),
                Focus::Config => app.config_field = app.config_field.saturating_sub(1),
            },
            KeyCode::Down => match app.focus {
                Focus::List => app.select_next(),
                Focus::Config => app.config_field = (app.config_field + 1).min(3),
            },
            KeyCode::Left | KeyCode::Right if app.focus == Focus::Config => {
                let delta = if key.code == KeyCode::Right { 1 } else { -1 };
                if app.adjust_config(delta) {
                    if let Some(i) = app.playing {
                        playing_interface = None;
                        match build_interface(&host, app.config, &app.demos[i].source) {
                            Ok(interface) => {
                                playing_interface = Some(interface);
                                app.status = format!(
                                    "playing {} @ {} Hz",
                                    app.demos[i].title, app.config.sample_rate
                                );
                            }
                            Err(e) => {
                                app.playing = None;
                                app.status = format!("error: {e}");
                            }
                        }
                    }
                }
            }
            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(8),
            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(8),
            KeyCode::Char('r') => {
                app.demos = load_demos(&demos_dir).unwrap_or_default();
                if app.list_state.selected().unwrap_or(0) >= app.demos.len() {
                    app.list_state.select((!app.demos.is_empty()).then_some(0));
                }
                app.playing = None;
                playing_interface = None;
                app.status = "reloaded".into();
            }
            KeyCode::Enter => {
                // When we change, drop the interface (kill the audio thread), and spawn a new one
                if let Some(i) = app.list_state.selected() {
                    playing_interface = None;
                    match build_interface(&host, app.config, &app.demos[i].source) {
                        Ok(interface) => {
                            playing_interface = Some(interface);
                            app.playing = Some(i);
                            app.status = format!("playing {}", app.demos[i].title);
                        }
                        Err(e) => {
                            app.playing = None;
                            app.status = format!("error: {e}");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    ratatui::restore();
    drop(playing_interface);
    Ok(())
}

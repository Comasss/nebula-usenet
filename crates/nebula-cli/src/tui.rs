//! `nebula tui`: la cola de un nebula-server en la terminal, actualizada cada segundo.
//! Flechas para elegir, p pausa o reanuda la descarga, espacio la cola entera, d la quita y q sale.
use crate::client::Client;
use crate::fmt::{self, num, str};
use crate::{Fail, Res};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

struct App {
    queue: Value,
    state: TableState,
    message: String,
}

impl App {
    fn jobs(&self) -> Vec<Value> {
        self.queue.get("jobs").and_then(Value::as_array).cloned().unwrap_or_default()
    }
    fn selected(&self) -> Option<Value> {
        self.state.selected().and_then(|i| self.jobs().get(i).cloned())
    }
}

pub async fn run(c: Client) -> Res {
    // Antes de tomar la terminal: si no hay servidor, el error se ve normal.
    let queue = c.invoke("get_queue", Value::Null).await?;
    let mut terminal = ratatui::init();
    let mut app = App { queue, state: TableState::default().with_selected(Some(0)), message: String::new() };
    let res = event_loop(&c, &mut terminal, &mut app).await;
    ratatui::restore();
    res
}

async fn event_loop(c: &Client, terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Res {
    let mut last = Instant::now();
    loop {
        terminal.draw(|f| draw(f, app)).map_err(|e| Fail::new(e.to_string()))?;
        let wait = Duration::from_millis(1000).saturating_sub(last.elapsed());
        if event::poll(wait.min(Duration::from_millis(250))).map_err(|e| Fail::new(e.to_string()))? {
            if let Event::Key(k) = event::read().map_err(|e| Fail::new(e.to_string()))? {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                let n = app.jobs().len();
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') if n > 0 => {
                        app.state.select(Some((app.state.selected().unwrap_or(0) + 1).min(n - 1)));
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.state.select(Some(app.state.selected().unwrap_or(0).saturating_sub(1)));
                    }
                    KeyCode::Char(' ') => {
                        let paused = app.queue.get("paused").and_then(Value::as_bool).unwrap_or(false);
                        app.message = action(c, "set_paused", json!({ "paused": !paused })).await;
                        last = Instant::now() - Duration::from_secs(2);
                    }
                    KeyCode::Char('p') => {
                        if let Some(j) = app.selected() {
                            let cmd = if str(&j, "status") == "paused" { "resume_job" } else { "pause_job" };
                            app.message = action(c, cmd, json!({ "id": str(&j, "id") })).await;
                            last = Instant::now() - Duration::from_secs(2);
                        }
                    }
                    KeyCode::Char('d') | KeyCode::Delete => {
                        if let Some(j) = app.selected() {
                            app.message = action(c, "remove_job", json!({ "id": str(&j, "id"), "deleteFiles": false })).await;
                            if app.message.is_empty() {
                                app.message = t!("cli.removed", name = str(&j, "name"));
                            }
                            last = Instant::now() - Duration::from_secs(2);
                        }
                    }
                    _ => {}
                }
            }
        }
        if last.elapsed() >= Duration::from_secs(1) {
            match c.invoke("get_queue", Value::Null).await {
                Ok(q) => app.queue = q,
                Err(e) => app.message = e.msg,
            }
            let n = app.jobs().len();
            if n == 0 {
                app.state.select(None);
            } else if app.state.selected().is_none_or(|i| i >= n) {
                app.state.select(Some(n - 1));
            }
            last = Instant::now();
        }
    }
}

async fn action(c: &Client, cmd: &str, args: Value) -> String {
    match c.invoke(cmd, args).await {
        Ok(_) => String::new(),
        Err(e) => e.msg,
    }
}

fn draw(f: &mut Frame, app: &mut App) {
    let [top, mid, bottom] = Layout::vertical([Constraint::Length(3), Constraint::Min(3), Constraint::Length(1)]).areas(f.area());
    let q = &app.queue;
    let paused = q.get("paused").and_then(Value::as_bool).unwrap_or(false);
    let jobs = app.jobs();
    let state = if paused { t!("cli.queuePaused") } else { fmt::speed(num(q, "speed")) };
    let head = t!("cli.queueHead", count = jobs.len(), remaining = fmt::bytes(num(q, "remaining")), eta = fmt::eta(num(q, "eta")), state = state);
    let limit = num(q, "speedLimit");
    let limit = if limit == 0 { t!("cli.noLimit") } else { t!("cli.limit", speed = fmt::speed(limit * 1024)) };
    f.render_widget(
        Paragraph::new(vec![Line::from(head).bold(), Line::from(limit).dim()]).block(Block::bordered().title(" Nebula ")),
        top,
    );

    let header = Row::new(["cli.col.name", "cli.col.status", "%", "cli.col.size", "cli.col.speed", "cli.col.eta", "cli.col.category"].map(|k| {
        Cell::from(if k.starts_with("cli.") { nebula_core::i18n::text(k) } else { k.to_string() })
    }))
    .style(Style::new().add_modifier(Modifier::BOLD));
    let rows = jobs.iter().map(|j| {
        let status = str(j, "status");
        let color = match status {
            "downloading" => Color::Cyan,
            "paused" => Color::Yellow,
            "failed" => Color::Red,
            "verifying" | "repairing" | "extracting" | "moving" => Color::Magenta,
            _ => Color::Gray,
        };
        Row::new([
            Cell::from(str(j, "name").to_string()),
            Cell::from(fmt::status(status)).style(Style::new().fg(color)),
            Cell::from(format!("{:5.1}", j.get("percent").and_then(Value::as_f64).unwrap_or(0.0))),
            Cell::from(format!("{} / {}", fmt::bytes(num(j, "done")), fmt::bytes(num(j, "total")))),
            Cell::from(fmt::speed(num(j, "speed"))),
            Cell::from(fmt::eta(num(j, "eta"))),
            Cell::from(match str(j, "category") {
                "" | "*" => "—".to_string(),
                c => c.to_string(),
            }),
        ])
    });
    let widths = [
        Constraint::Fill(1),
        Constraint::Length(13),
        Constraint::Length(6),
        Constraint::Length(21),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::new().reversed())
        .block(Block::bordered().title(if jobs.is_empty() { format!(" {} ", t!("cli.queueEmpty")) } else { String::new() }));
    f.render_stateful_widget(table, mid, &mut app.state);

    let help = if app.message.is_empty() { t!("cli.tui.help") } else { app.message.clone() };
    f.render_widget(Paragraph::new(help).dim(), bottom);
}

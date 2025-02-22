use bs_keygen::{do_keygen, KeygenConfig};
use bs_signing::{do_sign, SigningConfig};
use crossterm::event::{self, Event};
mod bs_client;
mod bs_keygen;
mod bs_signing;

use futures::executor::block_on;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::Widget,
    style::{Color, Style, Stylize},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use sha2::Digest;
use std::{
    fs, io,
    time::{Duration, Instant},
};
use tokio::time::timeout;
use tui_textarea::TextArea;

#[derive(Debug, PartialEq)]
enum AppMode {
    Menu,
    Create,
    Sign,
    GetAddress,
}

#[derive(Debug, Default)]
struct CreateState {
    threshold: u8,
    number_of_parties: u8,
    participant_index: u8,
    selected_field: usize,
    cursor_visible: bool,
}

#[derive(Debug, Default)]
struct SignState {
    participant_index: u8,
    psbt: TextArea<'static>,
    selected_field: usize,
}

#[derive(Debug, Default)]
struct GetAddressState {
    participant_index: u8,
    selected_field: usize,
}

#[derive(Debug)]
pub struct App {
    mode: AppMode,
    create_state: CreateState,
    sign_state: SignState,
    get_address_state: GetAddressState,
    exit: bool,
    last_blink: Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            mode: AppMode::Menu,
            create_state: CreateState::default(),
            sign_state: SignState::default(),
            get_address_state: GetAddressState::default(),
            exit: false,
            last_blink: Instant::now(),
        }
    }
}

impl App {
    pub fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<impl ratatui::backend::Backend>,
    ) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;

            if self.last_blink.elapsed() > Duration::from_millis(500) {
                match self.mode {
                    AppMode::Create => {
                        self.create_state.cursor_visible = !self.create_state.cursor_visible
                    }
                    _ => {}
                }
                self.last_blink = Instant::now();
            }

            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key_event) = event::read()? {
                    self.handle_key_event(key_event);
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        frame.render_widget(Paragraph::new(""), frame.area());
        match self.mode {
            AppMode::Menu => self.render_menu(frame),
            AppMode::Create => self.render_create(frame),
            AppMode::Sign => self.render_sign(frame),
            AppMode::GetAddress => self.render_get_address(frame),
        }
    }

    fn render_menu(&mut self, frame: &mut Frame) {
        let main_block = Block::bordered()
            .title(" BoomerSig (Main Menu)".bold())
            .border_set(border::THICK);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
            .split(main_block.inner(frame.area()));

        let menu_items = vec!["Create Multisig", "Sign Multisig", "Get Address"];
        let mut text = Text::default();
        for (i, item) in menu_items.iter().enumerate() {
            let style = if i == self.create_state.selected_field {
                Style::default().blue().bold()
            } else {
                Style::default()
            };
            text.lines.push(Line::from(vec![Span::styled(
                format!("▶ {} ", item),
                style,
            )]));
        }

        frame.render_widget(
            Paragraph::new(text)
                .block(Block::default().title("Main Menu"))
                .centered(),
            chunks[1],
        );

        let instructions = Line::from(vec![
            " Navigate ".into(),
            "▲/▼".blue().bold(),
            " Select ".into(),
            "Enter".blue().bold(),
            " Quit ".into(),
            "Q".blue().bold(),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(instructions))
                .block(Block::default())
                .centered(),
            chunks[2],
        );

        frame.render_widget(main_block, frame.area());
    }

    fn render_create(&mut self, frame: &mut Frame) {
        let main_block = Block::bordered()
            .title(" BoomerSig (Create Multisig) ".bold())
            .border_set(border::THICK);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
            .split(main_block.inner(frame.area()));

        let fields = [
            ("Threshold", self.create_state.threshold),
            ("Number of Parties", self.create_state.number_of_parties),
            ("Participant Index", self.create_state.participant_index),
        ];

        for (i, (title, value)) in fields.iter().enumerate() {
            let is_selected = i == self.create_state.selected_field;
            let mut text = value.to_string();
            if is_selected && self.create_state.cursor_visible {
                text.push('_');
            }

            let style = if is_selected {
                Style::default().blue().bold()
            } else {
                Style::default()
            };

            frame.render_widget(
                Paragraph::new(text)
                    .block(Block::default().borders(Borders::ALL).title(*title))
                    .style(style),
                chunks[i],
            );
        }

        let instructions = Line::from(vec![
            " Navigate ".into(),
            "▲/▼".blue().bold(),
            " Adjust ".into(),
            "◄/►".blue().bold(),
            " Back ".into(),
            "Esc".blue().bold(),
            " Quit ".into(),
            "Q".blue().bold(),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(instructions))
                .block(Block::default())
                .centered(),
            chunks[3],
        );

        frame.render_widget(main_block, frame.area());
    }

    fn render_sign(&mut self, frame: &mut Frame) {
        let main_block = Block::bordered()
            .title(" BoomerSig (Sign Multisig)".bold())
            .border_set(border::THICK);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(main_block.inner(frame.area()));

        let is_participant_selected = self.sign_state.selected_field == 0;
        let mut participant_text = self.sign_state.participant_index.to_string();
        if is_participant_selected && self.create_state.cursor_visible {
            participant_text.push('_');
        }

        let participant_style = if is_participant_selected {
            Style::default().blue().bold()
        } else {
            Style::default()
        };

        frame.render_widget(
            Paragraph::new(participant_text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Participant Index"),
                )
                .style(participant_style),
            chunks[0],
        );

        let is_psbt_selected = self.sign_state.selected_field == 1;
        let psbt_block = Block::default()
            .borders(Borders::ALL)
            .border_style(if is_psbt_selected {
                Style::default().yellow().bold()
            } else {
                Style::default().dim()
            })
            .title_style(if is_psbt_selected {
                Style::default().yellow().bold()
            } else {
                Style::default()
            })
            .title("PSBT (Critical Field) ▶");

        self.sign_state.psbt.set_block(psbt_block);
        self.sign_state
            .psbt
            .set_cursor_style(Style::default().bg(ratatui::style::Color::Yellow));
        frame.render_widget(&self.sign_state.psbt, chunks[1]);

        let instructions = Line::from(vec![
            " Navigate ".into(),
            "▲/▼".blue().bold(),
            " Edit ".into(),
            "Enter".blue().bold(),
            " Back ".into(),
            "Esc".blue().bold(),
            " Quit ".into(),
            "Q".blue().bold(),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(instructions))
                .block(Block::default())
                .centered(),
            chunks[2],
        );

        frame.render_widget(main_block, frame.area());
    }

    fn render_get_address(&mut self, frame: &mut Frame) {
        let main_block = Block::bordered()
            .title(" BoomerSig (Get Address)".bold())
            .border_set(border::THICK);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Participant Index
                Constraint::Length(3), // OK Button
                Constraint::Min(3),    // Instructions
            ])
            .split(main_block.inner(frame.area()));

        // Render Participant Index Field
        let is_participant_selected = self.get_address_state.selected_field == 0;
        let mut participant_text = self.get_address_state.participant_index.to_string();
        if is_participant_selected && self.create_state.cursor_visible {
            participant_text.push('_');
        }

        let participant_style = if is_participant_selected {
            Style::default().blue().bold()
        } else {
            Style::default()
        };

        frame.render_widget(
            Paragraph::new(participant_text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Participant Index"),
                )
                .style(participant_style),
            chunks[0],
        );

        // Render OK Button
        let is_ok_button_selected = self.get_address_state.selected_field == 1;
        let ok_button = Paragraph::new("OK")
            .block(Block::default().borders(Borders::ALL))
            .style(if is_ok_button_selected {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            });

        frame.render_widget(ok_button, chunks[1]);

        // Render Instructions
        let instructions = Line::from(vec![
            " Navigate ".into(),
            "▲/▼".blue().bold(),
            " Select ".into(),
            "Enter".blue().bold(),
            " Back ".into(),
            "Esc".blue().bold(),
            " Quit ".into(),
            "Q".blue().bold(),
        ]);
        frame.render_widget(
            Paragraph::new(Text::from(instructions))
                .block(Block::default())
                .centered(),
            chunks[2],
        );

        frame.render_widget(main_block, frame.area());
    }

    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) {
        if key_event.code == crossterm::event::KeyCode::Char('q') && self.mode != AppMode::Sign {
            self.exit();
            return;
        }

        match self.mode {
            AppMode::Menu => self.handle_menu_input(key_event),
            AppMode::Create => self.handle_create_input(key_event),
            AppMode::Sign => self.handle_sign_input(key_event),
            AppMode::GetAddress => self.handle_get_address_input(key_event),
        }
    }

    fn handle_menu_input(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            crossterm::event::KeyCode::Up => {
                self.create_state.selected_field =
                    self.create_state.selected_field.saturating_sub(1);
            }
            crossterm::event::KeyCode::Down => {
                if self.create_state.selected_field < 2 {
                    self.create_state.selected_field += 1;
                }
            }
            crossterm::event::KeyCode::Enter => match self.create_state.selected_field {
                0 => self.mode = AppMode::Create,
                1 => self.mode = AppMode::Sign,
                2 => self.mode = AppMode::GetAddress,
                _ => {}
            },
            _ => {}
        }
    }

    fn handle_create_input(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            crossterm::event::KeyCode::Esc => self.mode = AppMode::Menu,
            crossterm::event::KeyCode::Up => {
                if self.create_state.selected_field > 0 {
                    self.create_state.selected_field -= 1;
                }
            }
            crossterm::event::KeyCode::Down => {
                if self.create_state.selected_field < 2 {
                    self.create_state.selected_field += 1;
                }
            }
            crossterm::event::KeyCode::Left => match self.create_state.selected_field {
                0 => self.create_state.threshold = self.create_state.threshold.saturating_sub(1),
                1 => {
                    self.create_state.number_of_parties =
                        self.create_state.number_of_parties.saturating_sub(1)
                }
                2 => {
                    self.create_state.participant_index =
                        self.create_state.participant_index.saturating_sub(1)
                }
                _ => {}
            },
            crossterm::event::KeyCode::Right => match self.create_state.selected_field {
                0 => self.create_state.threshold = self.create_state.threshold.saturating_add(1),
                1 => {
                    self.create_state.number_of_parties =
                        self.create_state.number_of_parties.saturating_add(1)
                }
                2 => {
                    self.create_state.participant_index =
                        self.create_state.participant_index.saturating_add(1)
                }
                _ => {}
            },
            crossterm::event::KeyCode::Enter => {
                let config = KeygenConfig {
                    output: format!("local-share{}.json", self.create_state.participant_index)
                        .into(),
                    address: "http://127.0.0.1:8000".parse().unwrap(),
                    room: "default-keygen".into(),
                    index: self.create_state.participant_index as u16,
                    threshold: self.create_state.threshold as u16,
                    number_of_parties: self.create_state.number_of_parties as u16,
                };

                let _rt = tokio::runtime::Runtime::new().unwrap();
                let ret = _rt
                    .block_on(async { timeout(Duration::from_secs(30), do_keygen(config)).await });

                std::fs::write("ms.json", format!("{ret:?}")).unwrap();
            }
            _ => {}
        }
    }

    fn handle_sign_input(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            crossterm::event::KeyCode::Esc => self.mode = AppMode::Menu,
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Down => {
                self.sign_state.selected_field = (self.sign_state.selected_field + 1) % 2;
            }
            crossterm::event::KeyCode::Enter => {
                let sha256 = |data: &str| -> String {
                    let mut hasher = sha2::Sha256::new();
                    hasher.update(data.as_bytes());
                    let result = hasher.finalize();
                    hex::encode(result)
                };

                let _rt = tokio::runtime::Runtime::new().unwrap();

                if self.sign_state.selected_field == 1 {
                    let data_to_sign = self.sign_state.psbt.lines().join("\n");
                    for i in 0..10 {
                        let room = format!("default-signing{}{}", i, sha256(&data_to_sign.clone()));

                        let config = SigningConfig {
                            room,
                            address: "http://127.0.0.1:8000".parse().unwrap(),
                            parties: vec![1, 2],
                            transaction: true,
                            local_share: format!(
                                "local-share{}.json",
                                self.sign_state.participant_index
                            )
                            .into(),
                            data_to_sign: data_to_sign.clone(),
                            idx: self.sign_state.participant_index as u16,
                        };

                        self.sign_state.psbt = TextArea::new(Vec::new());

                        match _rt.block_on(async {
                            timeout(Duration::from_secs(30), do_sign(config)).await
                        }) {
                            Ok(Ok(ret)) => {
                                ret.signined_tx.clone().map(Self::broadcast_raw_transaction);
                                std::fs::write("output.raw", format!("{:?}", ret));
                                break;
                            }
                            Ok(Err(e)) => std::fs::write("error.raw", format!("{:?}", e)).unwrap(),
                            Err(e) => std::fs::write("error.raw", format!("{:?}", e)).unwrap(),
                        }
                    }
                }
            }
            _ => {
                if self.sign_state.selected_field == 0 {
                    match key_event.code {
                        crossterm::event::KeyCode::Left => {
                            self.sign_state.participant_index =
                                self.sign_state.participant_index.saturating_sub(1)
                        }
                        crossterm::event::KeyCode::Right => {
                            self.sign_state.participant_index =
                                self.sign_state.participant_index.saturating_add(1)
                        }
                        _ => {}
                    }
                } else {
                    self.sign_state.psbt.input(key_event);
                }
            }
        }
    }

    async fn broadcast_raw_transaction(tx: String) -> anyhow::Result<String> {
        let client = reqwest::Client::new();
        let mut res = client
            .post("https://mempool.space/api/tx")
            .body(tx.to_string())
            .send()?;

        Ok(res.text()?)
    }

    fn handle_get_address_input(&mut self, key_event: crossterm::event::KeyEvent) {
        match key_event.code {
            crossterm::event::KeyCode::Esc => self.mode = AppMode::Menu,
            crossterm::event::KeyCode::Up => {
                if self.get_address_state.selected_field > 0 {
                    self.get_address_state.selected_field -= 1;
                }
            }
            crossterm::event::KeyCode::Down => {
                if self.get_address_state.selected_field < 1 {
                    self.get_address_state.selected_field += 1;
                }
            }
            crossterm::event::KeyCode::Enter => {
                match self.get_address_state.selected_field {
                    0 => {
                        // Handle Participant Index input (if needed)
                    }
                    1 => {
                        // Handle OK button press
                        let data_to_sign =
                            "fdd4d9893b23aa6cdb357e1606907c6909a1231595549e698f779a141d4534c7"
                                .to_string();

                        let _rt = tokio::runtime::Runtime::new().unwrap();
                        for i in 0..10 {
                            let room = format!("default-get_key{}", i);
                            let config = SigningConfig {
                                room,
                                address: "http://127.0.0.1:8000".parse().unwrap(),
                                parties: vec![1, 2],
                                transaction: false,
                                local_share: format!(
                                    "local-share{}.json",
                                    self.get_address_state.participant_index
                                )
                                .into(),
                                data_to_sign: data_to_sign.clone(),
                                idx: self.get_address_state.participant_index as u16,
                            };

                            match _rt.block_on(async {
                                timeout(Duration::from_secs(30), do_sign(config.clone())).await
                            }) {
                                Ok(Ok(ret)) => {
                                    std::fs::write("address.raw", format!("{:?}", ret)).unwrap();
                                    break;
                                }
                                Ok(Err(e)) => {
                                    std::fs::write("error.raw", format!("{:?}", e)).unwrap()
                                }
                                Err(e) => std::fs::write("error.raw", format!("{:?}", e)).unwrap(),
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {
                if self.get_address_state.selected_field == 0 {
                    match key_event.code {
                        crossterm::event::KeyCode::Left => {
                            self.get_address_state.participant_index =
                                self.get_address_state.participant_index.saturating_sub(1)
                        }
                        crossterm::event::KeyCode::Right => {
                            self.get_address_state.participant_index =
                                self.get_address_state.participant_index.saturating_add(1)
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

fn main() -> io::Result<()> {
    let _rt = tokio::runtime::Runtime::new().unwrap();

    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    crossterm::terminal::enable_raw_mode()?;

    let mut app = App::default();
    app.sign_state
        .psbt
        .set_placeholder_text("Enter PSBT here...");
    let res = app.run(&mut terminal);

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    Ok(())
}

extern crate pretty_bytes;

use crate::util::stateful::{Named, StatefulList};

use crate::config::Config;
use crate::interface::game::Game;

use pretty_bytes::converter::convert;

use tui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans},
    widgets::{Block, BorderType, Borders, Cell, List, ListItem, Paragraph, Row, Table},
};

const SPLASH: &str = include_str!("../assets/splash.txt");

pub struct App {
    pub mode: Mode,
    pub user: String,
    /// In-progress password entry (classic login). Never persisted.
    pub password: String,
    /// In-progress Steam Guard code entry.
    pub guard_code: String,
    pub highlight: Color,
}

#[derive(PartialEq, Clone)]
pub enum Mode {
    Login,
    LoginQr,
    /// Typing the Steam username (classic login).
    LoginUser,
    /// Typing the Steam password (classic login).
    LoginPass,
    /// Classic login in progress (connecting / awaiting confirmation).
    LoginClassic,
    /// Typing a Steam Guard code prompted during classic login.
    LoginGuard,
    Loading,
    Normal,
    Searching,
    Searched,
    Failed,
    Terminated(String),
}

impl App {
    pub fn new(config: &Config) -> App {
        let user = config.default_user.clone();
        let highlight = config.highlight;
        App {
            mode: if user.is_empty() {
                Mode::Login
            } else {
                Mode::Loading
            },
            user,
            password: String::new(),
            guard_code: String::new(),
            highlight,
        }
    }

    pub fn build_layout() -> Layout {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(2), Constraint::Length(3)].as_ref())
    }
    pub fn build_image_layout() -> Layout {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(1, 1)].as_ref())
    }
    pub fn build_game_layout() -> Layout {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)].as_ref())
    }
    pub fn build_splash_terminated(err: String) -> Paragraph<'static> {
        App::build_infobox(
            "Oh dear...".to_string(),
            format!(
                "Something has crashed.. For more details please refer to the error below:\n\n{}",
                err
            ),
            Alignment::Left,
        )
    }

    pub fn build_splash_err() -> Paragraph<'static> {
        App::build_infobox(
            "aurelia-tui".to_string(),
            format!(
                "{}\n Uhoh. Could not find a session. Have you run `aurelia login`?",
                SPLASH
            ),
            Alignment::Center,
        )
    }

    pub fn build_splash() -> Paragraph<'static> {
        App::build_infobox(
            "aurelia-tui".to_string(),
            SPLASH.to_string(),
            Alignment::Center,
        )
    }

    pub fn build_patience() -> Paragraph<'static> {
        App::build_infobox(
            "Welcome".to_string(),
            "Checking cache (on load, you can press 'r' to invalidate cache)".to_string(),
            Alignment::Left,
        )
    }

    fn build_infobox(title: String, content: String, alignment: Alignment) -> Paragraph<'static> {
        Paragraph::new(content)
            .style(Style::default())
            .alignment(alignment)
            .block(
                Block::default()
                    .borders(Borders::all())
                    .title(title)
                    .border_type(BorderType::Plain),
            )
    }
    pub fn build_query(query: String) -> Paragraph<'static> {
        App::build_infobox(
            "Searching... (press esc to stop)".to_string(),
            query,
            Alignment::Left,
        )
    }
    pub fn build_query_searching(query: String) -> Paragraph<'static> {
        App::build_infobox(
            "Searching... (press Esc to stop, Enter to commit)".to_string(),
            query,
            Alignment::Left,
        )
    }
    pub fn build_loaded(count: i32, of: i32) -> Paragraph<'static> {
        let p = {
            if of < 0 {
                "Calculating...".to_string()
            } else {
                let p = 100. * (count as f32) / (of as f32);
                format!("Loading %{:.1}", p)
            }
        };
        App::build_infobox("Please wait".to_string(), p, Alignment::Left)
    }
    pub fn build_loading() -> Paragraph<'static> {
        App::build_infobox(
            "Please wait".to_string(),
            "Logging in and updating...".to_string(),
            Alignment::Left,
        )
    }
    pub fn build_login(username: String) -> Paragraph<'static> {
        App::build_infobox(
            "Login (Enter to submit)".to_string(),
            username,
            Alignment::Left,
        )
    }

    /// Landing page shown when no Steam session is available. Offers classic
    /// (username/password) login and a QR login.
    pub fn build_login_landing(error: Option<String>) -> Paragraph<'static> {
        // Instructions come first so they stay visible even when the (tall)
        // splash overflows the panel.
        let problem = match error {
            Some(err) => format!("\n {}\n", err),
            None => String::new(),
        };
        App::build_infobox(
            "Sign in".to_string(),
            format!(
                " No Steam session found.{}\n [Enter]  sign in with username & password\n [y]      sign in by QR code (Steam Mobile app)\n [r]      re-check session (after `aurelia login`)\n [q]      quit\n\n{}",
                problem, SPLASH
            ),
            Alignment::Center,
        )
    }

    /// Classic-login username entry.
    pub fn build_login_user(user: String) -> Paragraph<'static> {
        App::build_infobox(
            "Steam username (Enter to continue, Esc to cancel)".to_string(),
            user,
            Alignment::Left,
        )
    }

    /// Classic-login password entry (masked).
    pub fn build_login_pass(len: usize) -> Paragraph<'static> {
        App::build_infobox(
            "Steam password (Enter to sign in, Esc to go back)".to_string(),
            "*".repeat(len),
            Alignment::Left,
        )
    }

    /// Status page while a classic login is connecting / awaiting confirmation.
    pub fn build_login_classic(status: String) -> Paragraph<'static> {
        App::build_infobox(
            "Signing in ([Esc] cancel, [q] quit)".to_string(),
            format!("{}\n\n{}", SPLASH, status),
            Alignment::Center,
        )
    }

    /// Steam Guard code entry, prompted mid-login.
    pub fn build_login_guard(kind: String, code: String) -> Paragraph<'static> {
        let label = match kind.as_str() {
            "email" => "Enter the Steam Guard code emailed to you",
            "device" => "Enter the Steam Guard code from your authenticator",
            _ => "Enter your Steam Guard code",
        };
        App::build_infobox(
            format!("{} (Enter to submit, Esc to cancel)", label),
            code,
            Alignment::Left,
        )
    }

    pub fn build_login_classic_help() -> Paragraph<'static> {
        App::build_infobox(
            "Signing in".to_string(),
            "Please wait — approve on your Steam Mobile app if prompted | [Esc] cancel | [q] quit"
                .to_string(),
            Alignment::Left,
        )
    }

    /// The QR-login page: renders the current challenge as scannable blocks.
    pub fn build_qr(qr: Option<String>) -> Paragraph<'static> {
        let body = match qr {
            Some(url) => format!(
                "Scan with the Steam Mobile app:\n\n{}\n",
                App::render_qr_string(&url)
            ),
            None => "Generating QR code...".to_string(),
        };
        Paragraph::new(body)
            .style(Style::default().fg(Color::Black).bg(Color::White))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::all())
                    .title("Scan to sign in ([Esc] cancel, [q] quit)")
                    .border_type(BorderType::Plain),
            )
    }

    /// Render a challenge URL as a terminal QR code (black modules on white).
    fn render_qr_string(url: &str) -> String {
        match qrcode::QrCode::new(url.as_bytes()) {
            Ok(code) => code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .build(),
            Err(_) => format!("(could not render QR code)\n{}", url),
        }
    }
    pub fn build_login_help() -> Paragraph<'static> {
        App::build_infobox(
            "Sign in".to_string(),
            "[Enter] username/password  |  [y] QR code  |  [r] re-check  |  [q] quit".to_string(),
            Alignment::Left,
        )
    }

    pub fn build_qr_help() -> Paragraph<'static> {
        App::build_infobox(
            "Waiting for scan".to_string(),
            "Scan the code with the Steam Mobile app | [Esc] cancel | [q] quit".to_string(),
            Alignment::Left,
        )
    }

    pub fn build_help() -> Paragraph<'static> {
        App::build_infobox(
            "Help".to_string(),
            "[/] Search | [d]ownload  | [l]ogin | [Enter]xecute | Up (k, w) | Down (j, s) | [q]uit | [Space]team"
                .to_string(),
            Alignment::Left,
        )
    }
    pub fn build_terminated_help() -> Paragraph<'static> {
        App::build_infobox(
            "Woops.".to_string(),
            "Press q to quit.".to_string(),
            Alignment::Left,
        )
    }

    pub fn render_games<'a>(
        highlight: Color,
        game_list: &StatefulList<Game>,
    ) -> (List<'a>, Table<'a>) {
        let games = Block::default()
            .borders(Borders::ALL)
            .title("Games")
            .border_type(BorderType::Plain);

        let items: Vec<_> = game_list
            .activated()
            .iter()
            .map(|game| {
                let modifier = {
                    if let Some(status) = game.get_status() {
                        if status.state == "uninstalled" || status.state.contains("Failed") {
                            Modifier::DIM
                        } else {
                            Modifier::BOLD
                        }
                    } else {
                        Modifier::DIM
                    }
                };
                ListItem::new(Spans::from(vec![Span::styled(
                    game.get_name(),
                    Style::default().add_modifier(modifier),
                )]))
            })
            .collect();

        let list = List::new(items)
            .block(games)
            .highlight_style(Style::default().bg(highlight).add_modifier(Modifier::BOLD));

        let details = match game_list.selected() {
            Some(selected) => {
                selected.query_proton();
                selected.query_info();

                let spacer = Row::new(vec![Cell::from(Span::raw(" "))]);
                // Construct table head (id, name)
                let mut table = vec![
                    Row::new(vec![
                        Cell::from(Span::styled(
                            "ID",
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                        Cell::from(Span::styled(
                            "Name",
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                    ]),
                    Row::new(vec![
                        Cell::from(Span::raw(selected.id.to_string())),
                        Cell::from(Span::raw(selected.get_name())),
                    ]),
                    spacer.clone(),
                ];
                // Construct table details. Developer/publisher/description are
                // filled in lazily from `aurelia info` (see Game::query_info).
                let homepage = selected.homepage.clone();
                let developer = selected.get_developer();
                let publisher = selected.get_publisher();
                let proton = selected.get_proton();
                let description = selected.get_description();
                let mut detail_rows: Vec<(&str, &String)> = vec![
                    ("Homepage", &homepage),
                    ("Developer", &developer),
                    ("Publisher", &publisher),
                    ("Proton Tier", &proton),
                ];
                if !description.is_empty() {
                    detail_rows.push(("Description", &description));
                }
                for &(heading, value) in &detail_rows {
                    table.push(Row::new(vec![
                        Cell::from(Span::styled(
                            heading,
                            Style::default().add_modifier(Modifier::BOLD),
                        )),
                        Cell::from(Span::raw(value.clone())),
                    ]));
                }
                if let Some(status) = selected.get_status() {
                    table.push(spacer.clone());
                    for &(heading, value) in &[
                        ("State", &status.state),
                        ("Installation", &status.installdir),
                        ("Size", &convert(status.size)),
                    ] {
                        table.push(Row::new(vec![
                            Cell::from(Span::styled(
                                heading,
                                Style::default().add_modifier(Modifier::BOLD),
                            )),
                            Cell::from(Span::raw(value.clone())),
                        ]));
                    }
                }
                Table::new(table)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Detail")
                            .border_type(BorderType::Plain),
                    )
                    .widths(&[Constraint::Percentage(15), Constraint::Percentage(85)])
            }
            None => Table::new(vec![Row::new(vec![Cell::from(Span::raw(
                "No game selected...".to_string(),
            ))])]),
        };
        (list, details)
    }
}

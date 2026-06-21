extern crate aurelia_tui;

use std::io;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::Style;
use tui::widgets::{Block, Clear};
use tui::{backend::CrosstermBackend, Terminal};

use tui_image_rgba_updated::{ColorMode, Image};

use aurelia_tui::browse::{Browser, Filter};
use aurelia_tui::theme;
use aurelia_tui::ui;
use aurelia_tui::util::event::{Event, Events};
use aurelia_tui::util::image as artwork;
use aurelia_tui::util::image::ImageSlot;

use aurelia_tui::app::{App, Mode};
use aurelia_tui::client::{Client, State};
use aurelia_tui::config::Config;
use aurelia_tui::interface::aurelia::LoginPhase;

/// Approximate how many rows `text` occupies once word-wrapped to `width`
/// (matches `Paragraph`'s word wrapping closely enough to size its panel).
fn wrapped_lines(text: &str, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let width = width as usize;
    let mut total: u16 = 0;
    for paragraph in text.split('\n') {
        let mut col = 0usize;
        let mut lines_here: u16 = 1;
        for word in paragraph.split_whitespace() {
            let wlen = word.chars().count().max(1);
            if col == 0 {
                col = wlen;
            } else if col + 1 + wlen <= width {
                col += 1 + wlen;
            } else {
                lines_here += 1;
                col = wlen;
            }
        }
        total = total.saturating_add(lines_here);
    }
    total.max(1)
}

fn entry() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnableMouseCapture)?;
    #[allow(unused)]
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    terminal.clear()?;
    terminal.draw(|frame| {
        let layout = App::build_layout();
        let placement = layout.split(frame.size());
        frame.render_widget(App::build_splash(), placement[0]);
        frame.render_widget(App::build_patience(), placement[1]);
    })?;

    let mut img: Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> = None;
    // Artwork is loaded off the UI thread: background downloads publish into this
    // slot, and the currently-requested app id is tracked so stale loads are
    // discarded and unchanged selections are no-ops.
    let image_slot: ImageSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut requested_img_id: Option<i32> = None;

    // Setup event handlers
    let mut config = Config::new()?;
    let mut app = App::new(&config);
    let events = Events::new();
    let client = Client::new();

    // Always probe the session first (stored session or daemon). The health
    // result drives whether we land on the library or the sign-in page.
    client.check_session()?;
    app.mode = Mode::Loading;

    // Seed the browser from the cached library, if present.
    let mut cached = false;
    let mut browser = match client.games() {
        Ok(games) => {
            cached = true;
            Browser::new(games)
        }
        _ => Browser::new(Vec::new()),
    };

    loop {
        terminal.draw(|frame| {
            frame.render_widget(Block::default().style(theme::canvas()), frame.size());

            if app.mode == Mode::Browse {
                // Library browser: tabs / [list | detail+art] / status bar.
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Min(2),
                        Constraint::Length(2),
                    ])
                    .split(frame.size());

                frame.render_widget(ui::tabs::tabs(&browser), chunks[0]);

                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(chunks[1]);

                let list_widget = ui::list::list(&browser);
                frame.render_stateful_widget(list_widget, body[0], &mut browser.state);

                let selected = browser.selected();
                let right = body[1];

                // Right pane, top to bottom: the cover art (takes all leftover
                // height), the fixed-height detail table (sized to exactly its
                // max rows), and the wrapped description sized to its content
                // (capped at 10 lines unless expanded). Disjoint chunks, so the
                // art never overlaps the text.
                let desc_text = selected
                    .as_ref()
                    .map(|g| g.get_description())
                    .unwrap_or_default();
                let table_h = ui::detail::TABLE_HEIGHT;
                let desc_h = if desc_text.is_empty() {
                    0
                } else {
                    let cap = if browser.expand_description {
                        right.height
                    } else {
                        10
                    };
                    let lines = wrapped_lines(&desc_text, right.width.saturating_sub(2)).min(cap);
                    (lines + 2).min(right.height.saturating_sub(table_h.min(right.height)))
                };

                let constraints: Vec<Constraint> = if desc_h > 0 {
                    vec![
                        Constraint::Min(0),
                        Constraint::Length(table_h),
                        Constraint::Length(desc_h),
                    ]
                } else {
                    vec![Constraint::Min(0), Constraint::Length(table_h)]
                };
                let right_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(constraints)
                    .split(right);

                // Cover panel with the centered, aspect-correct artwork.
                let cover_area = right_chunks[0];
                let cover_block = theme::panel("Cover".to_string());
                let inner = cover_block.inner(cover_area);
                frame.render_widget(cover_block, cover_area);
                if let Some(image) = img.clone() {
                    let h = (inner.width / 2).min(inner.height);
                    let w = (h * 2).min(inner.width);
                    if w >= 2 && h >= 1 {
                        let area = Rect {
                            x: inner.x + (inner.width - w) / 2,
                            y: inner.y + (inner.height - h) / 2,
                            width: w,
                            height: h,
                        };
                        frame.render_widget(
                            Image::with_img(image)
                                .color_mode(ColorMode::Rgba)
                                .style(Style::default().bg(theme::BG)),
                            area,
                        );
                    }
                }

                frame.render_widget(ui::detail::detail(selected.as_ref()), right_chunks[1]);
                if desc_h > 0 {
                    frame.render_widget(
                        ui::detail::description(selected.as_ref(), browser.expand_description),
                        right_chunks[2],
                    );
                }

                frame.render_widget(
                    ui::status::status_bar(&browser, client.get_account().as_deref()),
                    chunks[2],
                );

                // Help overlay floats above everything.
                if browser.show_help {
                    let area = ui::centered_rect(64, 84, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::help::help(), area);
                }

                // Steam Cloud overlay floats above everything.
                if browser.show_cloud {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::cloud::cloud(&browser), area);
                }
            } else {
                // Login / loading / terminated screens use the simple two-pane layout.
                let layout = App::build_layout();
                let placement = layout.split(frame.size());
                let help = match app.mode {
                    Mode::Terminated(_) => App::build_terminated_help(),
                    Mode::Login | Mode::Failed => App::build_login_help(),
                    Mode::LoginQr => App::build_qr_help(),
                    Mode::LoginUser => App::build_login_user(app.user.clone()),
                    Mode::LoginPass => App::build_login_pass(app.password.len()),
                    Mode::LoginClassic => App::build_login_classic_help(),
                    Mode::LoginGuard => App::build_login_guard(
                        match client.get_login_phase() {
                            LoginPhase::GuardCode(kind) => kind,
                            _ => String::new(),
                        },
                        app.guard_code.clone(),
                    ),
                    Mode::Loading => match client.get_state() {
                        Ok(State::Loaded(count, of)) => App::build_loaded(count, of),
                        _ => App::build_loading(),
                    },
                    Mode::Browse => App::build_loading(),
                };
                match &app.mode {
                    Mode::Failed => frame.render_widget(App::build_splash_err(), placement[0]),
                    Mode::Login => frame.render_widget(
                        App::build_login_landing(client.get_last_error()),
                        placement[0],
                    ),
                    Mode::LoginQr => {
                        frame.render_widget(App::build_qr(client.get_qr()), placement[0])
                    }
                    Mode::LoginUser => {
                        frame.render_widget(App::build_login_user(app.user.clone()), placement[0])
                    }
                    Mode::LoginPass => {
                        frame.render_widget(App::build_login_pass(app.password.len()), placement[0])
                    }
                    Mode::LoginClassic => {
                        let status = match client.get_login_phase() {
                            LoginPhase::AwaitingConfirmation => {
                                "Signing in — if prompted, approve this login in your Steam Mobile app..."
                                    .to_string()
                            }
                            LoginPhase::DeviceConfirmation => {
                                "Approve this login in your Steam Mobile app, then wait...".to_string()
                            }
                            LoginPhase::Failed(msg) => format!("Login failed: {}", msg),
                            _ => "Signing in...".to_string(),
                        };
                        frame.render_widget(App::build_login_classic(status), placement[0])
                    }
                    Mode::LoginGuard => {
                        let kind = match client.get_login_phase() {
                            LoginPhase::GuardCode(kind) => kind,
                            _ => String::new(),
                        };
                        frame.render_widget(
                            App::build_login_guard(kind, app.guard_code.clone()),
                            placement[0],
                        )
                    }
                    Mode::Terminated(err) => {
                        frame.render_widget(App::build_splash_terminated(err.clone()), placement[0])
                    }
                    Mode::Loading => frame.render_widget(App::build_splash(), placement[0]),
                    Mode::Browse => {}
                }
                frame.render_widget(help, placement[1]);
            }
        })?;

        if let Event::Input(input) = events.next()? {
            match app.mode {
                Mode::Terminated(_) => {
                    if let KeyCode::Char('q') = input {
                        break;
                    }
                }
                Mode::Browse => {
                    if browser.show_cloud {
                        // Steam Cloud overlay: Esc/q close, s syncs and re-fetches.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_cloud(),
                            KeyCode::Char('s') => {
                                if let Some(game) = browser.selected() {
                                    browser.sync_cloud(game.id);
                                }
                            }
                            _ => {}
                        }
                    } else if browser.show_help {
                        // Any key dismisses the help overlay.
                        browser.show_help = false;
                    } else if browser.filtering {
                        // Live text filter focused: typing edits the query.
                        match input {
                            KeyCode::Esc => {
                                browser.filtering = false;
                                browser.clear_query();
                            }
                            KeyCode::Char('\n') | KeyCode::Enter => browser.filtering = false,
                            KeyCode::Backspace => browser.pop_query(),
                            KeyCode::Down => browser.next(),
                            KeyCode::Up => browser.previous(),
                            KeyCode::Char(c) => browser.push_query(c),
                            _ => {}
                        }
                    } else {
                        match input {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('?') => browser.show_help = true,
                            KeyCode::Char('/') => browser.filtering = true,
                            KeyCode::Char('l') => {
                                app.mode = Mode::Login;
                                terminal.hide_cursor()?;
                            }
                            KeyCode::Char('r') => {
                                cached = false;
                                app.mode = Mode::Loading;
                                client.restart()?;
                            }
                            KeyCode::Tab => browser.cycle_filter(true),
                            KeyCode::BackTab => browser.cycle_filter(false),
                            KeyCode::Char('1') => browser.set_filter(Filter::All),
                            KeyCode::Char('2') => browser.set_filter(Filter::Installed),
                            KeyCode::Char('3') => browser.set_filter(Filter::Updates),
                            KeyCode::Char('4') => browser.set_filter(Filter::Favourites),
                            KeyCode::Char('s') => browser.cycle_sort(),
                            KeyCode::Char('i') => browser.toggle_description(),
                            KeyCode::Down | KeyCode::Char('j') => browser.next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.previous(),
                            KeyCode::Char('g') => browser.home(),
                            KeyCode::Char('G') => browser.end(),
                            KeyCode::PageDown => browser.page_down(10),
                            KeyCode::PageUp => browser.page_up(10),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                if let Some(game) = browser.selected() {
                                    client.run(&game)?;
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(game) = browser.selected() {
                                    client.install(&game)?;
                                }
                            }
                            KeyCode::Char('f') => {
                                if let Some(game) = browser.selected() {
                                    if config.favorite_games.contains(&game.id) {
                                        config.favorite_games.retain(|&x| x != game.id);
                                    } else {
                                        config.favorite_games.push(game.id);
                                    }
                                    Config::save(&config)?;
                                    browser.refresh();
                                }
                            }
                            KeyCode::Char('H') => {
                                if let Some(game) = browser.selected() {
                                    config.hidden_games.push(game.id);
                                    Config::save(&config)?;
                                    browser.refresh();
                                }
                            }
                            KeyCode::Char('C') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_cloud(game.id);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Mode::Login | Mode::Failed => match input {
                    KeyCode::Char('q') => {
                        break;
                    }
                    KeyCode::Char('r') => {
                        // A session may have been established out-of-band via
                        // `aurelia login`; re-probe it.
                        cached = false;
                        app.mode = Mode::Loading;
                        client.check_session()?;
                    }
                    KeyCode::Char('y') => {
                        client.login_qr()?;
                        app.mode = Mode::LoginQr;
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        app.user.clear();
                        app.password.clear();
                        app.mode = Mode::LoginUser;
                        terminal.show_cursor()?;
                    }
                    _ => {}
                },
                Mode::LoginQr => match input {
                    KeyCode::Char('q') => {
                        break;
                    }
                    KeyCode::Esc => {
                        // Abandon this attempt; the backend login times out on
                        // its own.
                        app.mode = Mode::Login;
                    }
                    _ => {}
                },
                Mode::LoginUser => match input {
                    KeyCode::Esc => {
                        terminal.hide_cursor()?;
                        app.mode = Mode::Login;
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        let mut user = app.user.clone();
                        user.retain(|c| !c.is_whitespace());
                        if !user.is_empty() {
                            app.user = user;
                            app.password.clear();
                            app.mode = Mode::LoginPass;
                        }
                    }
                    KeyCode::Backspace => {
                        app.user.pop();
                    }
                    KeyCode::Char(c) => {
                        app.user.push(c);
                    }
                    _ => {}
                },
                Mode::LoginPass => match input {
                    KeyCode::Esc => {
                        app.password.clear();
                        app.mode = Mode::LoginUser;
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        if !app.password.is_empty() {
                            terminal.hide_cursor()?;
                            client.login_classic(&app.user, &app.password)?;
                            // Don't keep the password in memory any longer.
                            app.password.clear();
                            app.mode = Mode::LoginClassic;
                        }
                    }
                    KeyCode::Backspace => {
                        app.password.pop();
                    }
                    KeyCode::Char(c) => {
                        app.password.push(c);
                    }
                    _ => {}
                },
                Mode::LoginClassic => match input {
                    KeyCode::Char('q') => {
                        break;
                    }
                    KeyCode::Esc => {
                        app.mode = Mode::Login;
                    }
                    _ => {}
                },
                Mode::LoginGuard => match input {
                    KeyCode::Esc => {
                        terminal.hide_cursor()?;
                        app.mode = Mode::Login;
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        let mut code = app.guard_code.clone();
                        code.retain(|c| !c.is_whitespace());
                        if !code.is_empty() {
                            terminal.hide_cursor()?;
                            client.submit_guard_code(&code)?;
                            app.guard_code.clear();
                            app.mode = Mode::LoginClassic;
                        }
                    }
                    KeyCode::Backspace => {
                        app.guard_code.pop();
                    }
                    KeyCode::Char(c) => {
                        app.guard_code.push(c);
                    }
                    _ => {}
                },
                _ => {}
            }
            // Need a hook to cancel if in loading mode.
            if app.mode == Mode::Loading {
                if let KeyCode::Char('q') = input {
                    break;
                }
            }
            events.release();
        }

        if app.mode == Mode::Loading {
            match client.get_state()? {
                State::Loaded(_, -2) => {
                    if cached {
                        // Used the cached library; allow future reloads.
                        cached = false;
                        terminal.hide_cursor()?;
                        app.mode = Mode::Browse;
                    } else {
                        client.load_games()?;
                    }
                }
                State::LoggedIn => {
                    config.save()?;
                    browser.set_items(client.games()?);
                    terminal.hide_cursor()?;
                    terminal.clear()?;
                    app.mode = Mode::Browse;
                }
                State::Failed => {
                    // No session — fall through to the sign-in landing page.
                    app.mode = Mode::Login;
                }
                _ => {}
            }
        }
        if app.mode == Mode::LoginQr {
            match client.get_state()? {
                State::Loaded(_, -2) => {
                    cached = false;
                    app.mode = Mode::Loading;
                    terminal.clear()?;
                }
                State::Failed => {
                    app.mode = Mode::Login;
                }
                _ => {}
            }
        }
        if app.mode == Mode::LoginClassic || app.mode == Mode::LoginGuard {
            // A Steam Guard prompt mid-login switches us to the code entry page.
            if app.mode == Mode::LoginClassic {
                if let LoginPhase::GuardCode(_) = client.get_login_phase() {
                    app.guard_code.clear();
                    app.mode = Mode::LoginGuard;
                    terminal.show_cursor()?;
                }
            }
            match client.get_state()? {
                State::Loaded(_, -2) => {
                    cached = false;
                    app.mode = Mode::Loading;
                    terminal.clear()?;
                }
                State::Failed => {
                    terminal.hide_cursor()?;
                    app.mode = Mode::Login;
                }
                _ => {}
            }
        }
        if let State::Terminated(err) = client.get_state()? {
            app.mode = Mode::Terminated(err);
        }

        // Drive artwork off the UI thread: `select` only acts when the selection
        // changes (loading a cached image inline, else kicking off a background
        // download), and `poll` adopts a completed download.
        if app.mode == Mode::Browse && !browser.show_help && !browser.show_cloud {
            let selected = browser.selected();
            artwork::select(
                selected.as_ref(),
                &mut requested_img_id,
                &mut img,
                &image_slot,
            );
            artwork::poll(requested_img_id, &mut img, &image_slot);
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture)?;
    terminal.clear()?;
    Ok(())
}

fn main() {
    match entry() {
        Ok(()) => {}
        Err(err) => println!("{:?}", err),
    }
}

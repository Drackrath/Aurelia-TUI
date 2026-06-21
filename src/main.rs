extern crate aurelia_tui;

use std::io;

use crossterm::event::KeyCode;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use tui::style::{Color, Style};
use tui::{backend::CrosstermBackend, layout::Rect, Terminal};

use terminal_light::background_color;

use tui_image_rgba_updated::{ColorMode, Image};

use aurelia_tui::util::event::{Event, Events};
use aurelia_tui::util::image as artwork;
use aurelia_tui::util::image::ImageSlot;
use aurelia_tui::util::stateful::StatefulList;

use aurelia_tui::app::{App, Mode};
use aurelia_tui::client::{Client, State};
use aurelia_tui::interface::aurelia::LoginPhase;
use aurelia_tui::config::Config;
use aurelia_tui::interface::game::Game;

// why isn't this in stdlib for floats?
fn min(a: f32, b: f32) -> f32 {
    if a < b {
        return a;
    }
    b
}

fn entry() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let stdout = io::stdout();
    #[allow(unused)]
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let terminal_bg = background_color()
        .map(|c| c.rgb())
        .map(|c| Color::Rgb(c.r, c.g, c.b))
        .unwrap_or(Color::Gray);

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

    // Attempt to load from cache. If not, continue as usual.
    let mut game_list: StatefulList<Game> = StatefulList::new();
    let mut cached: bool = false;
    match client.games() {
        Ok(games) => {
            game_list = StatefulList::with_items(games);
            cached = true;
        }
        _ => game_list.restart(),
    }

    loop {
        terminal.draw(|frame| {
            let layout = App::build_layout();
            let placement = layout.split(frame.size());
            let help = match app.mode {
                Mode::Normal => App::build_help(),
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
                Mode::Searching => App::build_query_searching(game_list.query.clone()),
                Mode::Searched => App::build_query(game_list.query.clone()),
            };
            match &app.mode {
                Mode::Failed => frame.render_widget(App::build_splash_err(), placement[0]),
                Mode::Login => {
                    frame.render_widget(App::build_login_landing(client.get_last_error()), placement[0])
                }
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
                Mode::Loading => {
                    frame.render_widget(App::build_splash(), placement[0]);
                }
                _ => {
                    let game_layout = App::build_game_layout();
                    let image_layout = App::build_image_layout();

                    let (left, right) = App::render_games(app.highlight, &game_list);
                    let game_placement = game_layout.split(placement[0]);
                    // Incorrect image placement leads to hard crash. Explicitly calculate bounds
                    // here.
                    let image_placement = {
                        let offset_x = game_placement[1].width + game_placement[1].x;
                        let offset_y = game_placement[1].height + game_placement[1].y;
                        let (width, height) = {
                            // 62% is also hardcoded in the window width, and 160 is totally
                            // arbitrary, but the really large images look super goofy.
                            // TODO: Allow for user adjustable widths
                            let width = min((offset_x as f32) * 0.62, 160.0);
                            // Height is counted by row, and there are 10 lines of info.
                            let height = min((offset_y as f32) - 10.0, 80.0);
                            // Take minium, but respect aspect ratio.
                            (
                                min(width, height * 2.0) as u16,
                                min(height, width / 2.0) as u16,
                            )
                        };
                        image_layout.split(Rect {
                            x: offset_x - width,
                            y: offset_y - height,
                            width,
                            height,
                        })
                    };

                    frame.render_stateful_widget(left, game_placement[0], &mut game_list.state);
                    frame.render_widget(right, game_placement[1]);
                    if let Some(image) = img.clone() {
                        frame.render_widget(
                            Image::with_img(image)
                                .color_mode(ColorMode::Rgba)
                                .style(Style::default().bg(terminal_bg)),
                            image_placement[0],
                        )
                    }
                }
            }
            frame.render_widget(help, placement[1]);
        })?;

        if let Event::Input(input) = events.next()? {
            match app.mode {
                Mode::Terminated(_) => {
                    if let KeyCode::Char('q') = input {
                        break;
                    }
                }
                Mode::Normal | Mode::Searched => match input {
                    KeyCode::Char('l') => {
                        app.mode = Mode::Login;
                        terminal.hide_cursor()?;
                        game_list.restart();
                    }
                    KeyCode::Char('q') => {
                        break;
                    }
                    KeyCode::Char('r') => {
                        // Marked cache as false for the potential race condition
                        // (you flush cache prior to login)
                        cached = false;
                        app.mode = Mode::Loading;
                        client.restart()?;
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => {
                        game_list.next();
                    }
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => {
                        game_list.previous();
                    }
                    KeyCode::Char('/') => {
                        app.mode = Mode::Searching;
                        terminal.show_cursor()?;
                        game_list.unselect();
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        if let Some(game) = game_list.selected() {
                            client.run(game)?;
                        }
                    }
                    KeyCode::Char('f') => {
                        if let Some(game) = game_list.selected() {
                            if config.favorite_games.contains(&game.id) {
                                config.favorite_games.retain(|&x| x != game.id);
                            } else {
                                config.favorite_games.push(game.id);
                            }
                            Config::save(&config)?;
                        }
                    }
                    KeyCode::Char('F') => {
                        // Hard refresh to restart games, since bad index can mess things up.
                        game_list = StatefulList::with_items(client.games()?);
                        game_list.query = "♡ ".to_string();
                        app.mode = Mode::Searched;
                    }
                    KeyCode::Char('H') => {
                        if let Some(game) = game_list.selected() {
                            config.hidden_games.push(game.id);
                            Config::save(&config)?;
                            game_list.previous();
                        }
                    }
                    KeyCode::Char(' ') => {
                        client.start_client()?;
                    }
                    KeyCode::Char('d') => {
                        if let Some(game) = game_list.selected() {
                            client.install(game)?;
                        }
                    }
                    KeyCode::Esc => {
                        app.mode = Mode::Normal;
                        game_list.query = "".to_string();
                    }
                    _ => {}
                },
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
                Mode::Searching => match input {
                    KeyCode::Esc => {
                        app.mode = Mode::Normal;
                        terminal.hide_cursor()?;
                        game_list.query = "".to_string();
                    }
                    KeyCode::Char('\n') | KeyCode::Enter => {
                        terminal.hide_cursor()?;
                        app.mode = Mode::Searched;
                    }
                    KeyCode::Backspace => {
                        game_list.query.pop();
                        game_list.restart();
                    }
                    KeyCode::Char(c) => {
                        game_list.query.push(c);
                        game_list.restart();
                    }
                    KeyCode::Down => {
                        game_list.next();
                    }
                    KeyCode::Up => {
                        game_list.previous();
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
                    // If loaded from cache then just used the cache
                    if cached {
                        // Importantly, mark cached as false to allow reloads
                        cached = false;
                        app.mode = Mode::Normal;
                    } else {
                        client.load_games()?;
                    }
                }
                State::LoggedIn => {
                    config.save()?;
                    let query = game_list.query.clone();
                    if query.is_empty() {
                        app.mode = Mode::Normal;
                    } else {
                        app.mode = Mode::Searched;
                    }
                    game_list = StatefulList::with_items(client.games()?);
                    terminal.clear()?;
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
        // download), and `poll` adopts a completed download. Both are cheap, so
        // running them once per loop keeps navigation responsive.
        if matches!(app.mode, Mode::Normal | Mode::Searched | Mode::Searching) {
            artwork::select(
                game_list.selected(),
                &mut requested_img_id,
                &mut img,
                &image_slot,
            );
            artwork::poll(requested_img_id, &mut img, &image_slot);
        }
    }
    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn main() {
    match entry() {
        Ok(()) => {}
        Err(err) => println!("{:?}", err),
    }
}

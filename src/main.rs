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
use aurelia_tui::interface::aurelia::{self, LoginPhase};
use aurelia_tui::util::stateful::Named;

/// Blend `img` toward `bg` so the top-aligned cover art reads as a translucent
/// background: a uniform base blend keeps the overlaid text legible everywhere,
/// and the lower part fades further until it dissolves fully into the panel
/// background at the bottom row. Applied in source-pixel space, so it maps to
/// the same proportion of the rendered art regardless of later resizing.
fn fade_bottom(img: &mut image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, bg: (u8, u8, u8)) {
    let h = img.height();
    if h == 0 {
        return;
    }
    // How far every pixel is pulled toward the background (overall translucency).
    let base = 0.45f32;
    // Below this row the art fades the rest of the way to full background.
    let start = h * 9 / 20;
    let span = (h - start).max(1) as f32;
    let (br, bgc, bb) = bg;
    for y in 0..h {
        let t = if y < start {
            base
        } else {
            base + (1.0 - base) * ((y - start) as f32 / span)
        };
        for x in 0..img.width() {
            let px = img.get_pixel_mut(x, y);
            px[0] = (px[0] as f32 * (1.0 - t) + br as f32 * t) as u8;
            px[1] = (px[1] as f32 * (1.0 - t) + bgc as f32 * t) as u8;
            px[2] = (px[2] as f32 * (1.0 - t) + bb as f32 * t) as u8;
        }
    }
}

/// The RGB components of a theme [`Color::Rgb`] (falls back to black).
fn rgb(color: tui::style::Color) -> (u8, u8, u8) {
    match color {
        tui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    }
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

                // Right pane: a single Detail panel filling the pane, with the
                // cover art painted as its background and the details — now
                // including the description as its own row — listed on top.
                let inner = Rect {
                    x: right.x + 1,
                    y: right.y + 1,
                    width: right.width.saturating_sub(2),
                    height: right.height.saturating_sub(2),
                };

                // Cover art as the Detail background — top-aligned, aspect-
                // correct, and fading out toward the bottom so the lower detail
                // rows stay readable.
                if let Some(mut image) = img.clone() {
                    let h = (inner.width / 2).min(inner.height);
                    let w = (h * 2).min(inner.width);
                    if w >= 2 && h >= 1 {
                        fade_bottom(&mut image, rgb(theme::BG_DARK));
                        let area = Rect {
                            x: inner.x + (inner.width - w) / 2,
                            y: inner.y,
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

                // The (transparent) detail table draws its frame and rows over
                // the art. Wrap the description to ~82% of the inner width.
                let desc_width = inner.width.saturating_mul(82) / 100;
                frame.render_widget(
                    ui::detail::detail(
                        selected.as_ref(),
                        browser.expand_description,
                        desc_width.saturating_sub(1),
                        inner.height,
                    ),
                    right,
                );

                frame.render_widget(
                    ui::status::status_bar(&browser, client.get_account().as_deref()),
                    chunks[2],
                );

                // Achievements overlay floats above everything.
                if browser.show_achievements {
                    let area = ui::centered_rect(72, 82, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::achievements::achievements(&browser), area);
                }

                // Friends overlay floats above everything.
                if browser.show_friends {
                    let area = ui::centered_rect(60, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::friends::friends(&browser), area);
                }

                // Inventory overlay floats above everything.
                if browser.show_inventory {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::inventory::inventory(&browser), area);
                }

                // Market listings overlay floats above everything.
                if browser.show_market {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::market::market(&browser), area);
                }

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

                // Launch-options overlay floats above everything.
                if browser.show_launch {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::launch::launch(&browser), area);
                }

                // Account overlay floats above everything.
                if browser.show_account {
                    let area = ui::centered_rect(50, 55, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::account::account(&browser), area);
                }

                // Settings overlay floats above everything.
                if browser.show_config {
                    let area = ui::centered_rect(55, 60, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::config::config(&browser), area);
                }

                // Uninstall confirmation prompt floats above the library.
                if browser.confirm_uninstall {
                    if let Some(game) = browser.selected() {
                        let area = ui::centered_rect(40, 20, frame.size());
                        frame.render_widget(Clear, area);
                        frame.render_widget(
                            ui::confirm::confirm_uninstall(&game.get_name()),
                            area,
                        );
                    }
                }

                // DLC overlay floats above everything.
                if browser.show_dlc {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::dlc::dlc(&browser), area);
                }

                // Branches overlay floats above everything.
                if browser.show_branches {
                    let area = ui::centered_rect(60, 60, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::branches::branches(&browser), area);
                }

                // Depots overlay floats above everything.
                if browser.show_depots {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::depots::depots(&browser), area);
                }

                // Move (relocate install) prompt floats above everything.
                if browser.show_move {
                    let area = ui::centered_rect(60, 25, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::move_game::move_overlay(&browser), area);
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
                    } else if browser.show_achievements {
                        // Achievements overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_achievements(),
                            KeyCode::Down | KeyCode::Char('j') => browser.ach_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.ach_scroll_up(),
                            _ => {}
                        }
                    } else if browser.show_friends {
                        // Friends overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_friends(),
                            KeyCode::Down | KeyCode::Char('j') => browser.friends_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.friends_scroll_up(),
                            _ => {}
                        }
                    } else if browser.show_inventory {
                        // Inventory overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_inventory(),
                            KeyCode::Down | KeyCode::Char('j') => browser.inv_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.inv_scroll_up(),
                            _ => {}
                        }
                    } else if browser.show_launch {
                        // Launch-options overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_launch(),
                            KeyCode::Down | KeyCode::Char('j') => browser.launch_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.launch_scroll_up(),
                            _ => {}
                        }
                    } else if browser.show_market {
                        // Market listings overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_market(),
                            KeyCode::Down | KeyCode::Char('j') => browser.market_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.market_scroll_up(),
                            _ => {}
                        }
                    } else if browser.confirm_uninstall {
                        // Uninstall confirmation prompt: y confirms, anything else cancels.
                        match input {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(game) = browser.selected() {
                                    aurelia::uninstall(game.id)?;
                                    // Reload the library so the listing reflects
                                    // the now-uninstalled game (mirrors 'r').
                                    cached = false;
                                    app.mode = Mode::Loading;
                                    client.restart()?;
                                }
                                browser.confirm_uninstall = false;
                            }
                            _ => browser.confirm_uninstall = false,
                        }
                    } else if browser.show_dlc {
                        // DLC overlay: navigate and toggle the highlighted DLC.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_dlc(),
                            KeyCode::Down | KeyCode::Char('j') => browser.dlc_next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.dlc_previous(),
                            KeyCode::Char('e') => {
                                if let Some(entry) = browser.selected_dlc() {
                                    let _ = aurelia::set_dlc(entry.app_id as i32, true);
                                    let _ = browser.refresh_dlc();
                                }
                            }
                            KeyCode::Char('x') => {
                                if let Some(entry) = browser.selected_dlc() {
                                    let _ = aurelia::set_dlc(entry.app_id as i32, false);
                                    let _ = browser.refresh_dlc();
                                }
                            }
                            _ => {}
                        }
                    } else if browser.show_branches {
                        // Branches overlay: navigate and switch to the highlighted branch.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_branches(),
                            KeyCode::Down | KeyCode::Char('j') => browser.branch_next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.branch_previous(),
                            KeyCode::Char('\n') | KeyCode::Enter | KeyCode::Char('s') => {
                                if let Some(branch) = browser.selected_branch() {
                                    let _ =
                                        aurelia::set_branch(browser.branch_app_id(), &branch.name);
                                    browser.close_branches();
                                }
                            }
                            _ => {}
                        }
                    } else if browser.show_depots {
                        // Depots overlay: Esc/q close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_depots(),
                            KeyCode::Down | KeyCode::Char('j') => browser.depots_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.depots_scroll_up(),
                            _ => {}
                        }
                    } else if browser.show_move {
                        // Move prompt: type the destination library path, Enter
                        // to relocate (kept open to show status), Esc to cancel.
                        match input {
                            KeyCode::Esc => browser.close_move(),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                let _ = browser.do_move();
                            }
                            KeyCode::Backspace => browser.move_pop(),
                            KeyCode::Char(c) => browser.move_push(c),
                            _ => {}
                        }
                    } else if browser.show_help {
                        // Any key dismisses the help overlay.
                        browser.show_help = false;
                    } else if browser.show_account {
                        // `o` logs out of Steam; any other key dismisses the overlay.
                        match input {
                            KeyCode::Char('o') => {
                                let _ = aurelia::logout();
                                browser.show_account = false;
                                app.mode = Mode::Login;
                            }
                            _ => browser.show_account = false,
                        }
                    } else if browser.show_config {
                        // Settings overlay: dismiss, or toggle presence in place.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_config(),
                            KeyCode::Char('o') => {
                                // Flip whatever the config currently shows (default
                                // offline when unknown), then re-fetch to confirm.
                                let online = browser
                                    .config_info
                                    .as_ref()
                                    .map(|c| c.is_online())
                                    .unwrap_or(false);
                                let _ = aurelia::set_presence(!online);
                                let _ = browser.open_config();
                            }
                            _ => {}
                        }
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
                            KeyCode::Char('A') => {
                                // Fetch the account (blocking) and open the overlay;
                                // ignore failures so a missing session is non-fatal.
                                let _ = browser.open_account();
                            }
                            KeyCode::Char('p') => {
                                // Fetch the launcher configuration (blocking) and
                                // open the settings overlay; ignore failures.
                                let _ = browser.open_config();
                            }
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
                            KeyCode::Char('a') => browser.open_achievements(),
                            KeyCode::Char('F') => browser.open_friends(),
                            KeyCode::Char('I') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_inventory(game.id);
                                }
                            }
                            KeyCode::Char('m') => browser.open_market(),
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
                            KeyCode::Char('x') => {
                                // Only offer uninstall for installed games.
                                if let Some(game) = browser.selected() {
                                    if game.installed {
                                        browser.confirm_uninstall = true;
                                    }
                                }
                            }
                            KeyCode::Char('M') => {
                                // Only offer move for installed games.
                                if let Some(game) = browser.selected() {
                                    if game.installed {
                                        browser.open_move(game.id);
                                    }
                                }
                            }
                            KeyCode::Char('v') => {
                                if let Some(game) = browser.selected() {
                                    client.verify(&game)?;
                                }
                            }
                            KeyCode::Char('D') => {
                                if let Some(game) = browser.selected() {
                                    // Blocking fetch; failure leaves the overlay closed.
                                    let _ = browser.open_dlc(game.id);
                                }
                            }
                            KeyCode::Char('b') => {
                                if let Some(game) = browser.selected() {
                                    // Blocking fetch; failure leaves the overlay closed.
                                    let _ = browser.open_branches(game.id);
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
                            KeyCode::Char('o') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_depots(game.id);
                                }
                            }
                            KeyCode::Char('L') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_launch(game.id);
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
        if app.mode == Mode::Browse && !browser.show_help && !browser.show_dlc && !browser.show_account && !browser.show_config && !browser.show_achievements && !browser.show_cloud && !browser.show_branches && !browser.show_depots && !browser.show_friends && !browser.show_inventory && !browser.show_launch && !browser.show_market && !browser.show_move {
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

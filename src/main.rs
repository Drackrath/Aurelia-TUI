extern crate aurelia_tui;

use std::io;
use std::time::Instant;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::Style;
use tui::widgets::{Block, Clear};
use tui::{backend::CrosstermBackend, Terminal};

use tui_image_rgba_updated::{ColorMode, Image};

use aurelia_tui::browse::{Browser, Filter, InstallPhase, View};
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

/// Scale `src` to fully cover a `tw`×`th` pixel target, preserving its aspect
/// ratio by centre-cropping the overflow (`object-fit: cover`): the result is
/// exactly `tw`×`th`, with the excess trimmed off the top/bottom or left/right.
fn cover_resize(
    src: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    tw: u32,
    th: u32,
) -> image::ImageBuffer<image::Rgba<u8>, Vec<u8>> {
    use image::imageops::{crop_imm, resize, FilterType};
    let (sw, sh) = (src.width(), src.height());
    if sw == 0 || sh == 0 || tw == 0 || th == 0 {
        return src.clone();
    }
    // Largest centred sub-rectangle of `src` matching the target aspect ratio.
    // `sw/sh` vs `tw/th` compared as cross-products to stay in integers.
    let (cw, ch) = if sw as u64 * th as u64 >= sh as u64 * tw as u64 {
        // Source is wider than the target — crop left/right (full height kept).
        let cw = ((sh as u64 * tw as u64) / th as u64) as u32;
        (cw.clamp(1, sw), sh)
    } else {
        // Source is taller than the target — crop top/bottom (full width kept).
        let ch = ((sw as u64 * th as u64) / tw as u64) as u32;
        (sw, ch.clamp(1, sh))
    };
    let ox = (sw - cw) / 2;
    let oy = (sh - ch) / 2;
    let cropped = crop_imm(src, ox, oy, cw, ch).to_image();
    resize(&cropped, tw, th, FilterType::Triangle)
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
    // Capture the mouse so the wheel scrolls the lists. Text selection still
    // works without a toggle: terminals intercept Shift+drag for their own
    // native selection before the app ever sees those events, so the wheel and
    // selecting/copying text coexist (hold Shift while dragging to select).
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
    // The portrait cover art (capsule) is loaded on its own independent pipeline,
    // separate from the wide background art above.
    let mut cover_img: Option<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> = None;
    let cover_slot: ImageSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let mut requested_cover_id: Option<i32> = None;
    // Artwork (and the lazy detail fetches) are only rendered/loaded once input
    // has been idle for a short delay, so scrolling stays smooth — no per-frame
    // image resize, decode, or CLI fetch while flicking through the list.
    let mut last_input_at = Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut artwork_sel_id: Option<i32> = None;
    /// How long input must be idle before artwork/details load and render.
    const ARTWORK_IDLE: std::time::Duration = std::time::Duration::from_millis(200);

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

    // Wall-clock anchor for the list-name marquee. Deriving the scroll step from
    // elapsed time (rather than a per-frame counter) keeps it advancing at a
    // steady pace regardless of what triggered each redraw (tick or keypress).
    let marquee_start = Instant::now();
    /// Milliseconds per one-column marquee step (~4 columns/second).
    const MARQUEE_STEP_MS: u128 = 250;
    // The marquee restarts from the name's first character each time the cursor
    // moves to a new row, so a hovered name always begins at its start with the
    // " • " separator trailing at the end — never opening mid-scroll on the gap.
    let mut marquee_row: Option<usize> = None;
    let mut marquee_base: usize = 0;

    loop {
        // Adopt any async friend-management results (resolved search previews and
        // roster refreshes after an add/remove) before drawing this frame.
        browser.poll_friends_ops();

        // Drain any finished off-thread Workshop browse/search/subscribe worker
        // before rendering, so its result lands on the next frame. Runs on every
        // event (including ticks) and never blocks the UI thread.
        browser.poll_workshop();

        // Reconcile a game's install state once its install finishes, and adopt
        // any targeted single-game refresh that landed — so `installed` (which
        // gates uninstall) tracks reality without a full-screen reload.
        browser.poll_install_completions();
        browser.poll_game_refresh();

        // A just-finished Proton install flips a runtime to [installed]; pick
        // that up by refreshing the list once, then drop the status line. Done
        // before draw since the render closure only borrows `browser` immutably.
        if browser.show_proton && browser.proton_status_line().as_deref() == Some("Installed!") {
            browser.refresh_proton();
            browser.clear_proton_status();
        }

        let global_tick = (marquee_start.elapsed().as_millis() / MARQUEE_STEP_MS) as usize;
        let marquee_sel = browser.selected_index();
        if marquee_sel != marquee_row {
            marquee_row = marquee_sel;
            marquee_base = global_tick;
        }
        let marquee_tick = global_tick.saturating_sub(marquee_base);

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

                // Top bar split into two sections: the library filters on the
                // left and the Friends & Chat tab in its own box on the right,
                // each with its own headline.
                let tab_areas = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Min(20), Constraint::Length(19)])
                    .split(chunks[0]);
                frame.render_widget(ui::tabs::library_tabs(&browser), tab_areas[0]);
                frame.render_widget(ui::tabs::friends_tabs(&browser), tab_areas[1]);

                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(chunks[1]);

                // Left pane: the game list under a library filter, or — when the
                // Friends tab is selected — the Friends & Chat panel in its
                // place (the "All → Friends" transition). The panel sizes its
                // whole-list scroll to the full-height left column.
                if browser.view == View::Friends {
                    let friends_rows = body[0].height.saturating_sub(2) as usize;
                    frame.render_widget(ui::friends::friends(&browser, friends_rows), body[0]);
                } else {
                    let list_widget = ui::list::list(&browser, body[0].width, marquee_tick);
                    frame.render_stateful_widget(list_widget, body[0], &mut browser.state);
                }

                let selected = browser.selected();
                // Game Details fill the whole right column; the Friends & Chat
                // panel is reached via its own tab, so there is no bottom-right
                // sidebar anymore.
                let detail_area = body[1];

                // Detail panel: the cover art is painted as its background and
                // the details — including the description as its own row — are
                // listed on top.
                let inner = Rect {
                    x: detail_area.x + 1,
                    y: detail_area.y + 1,
                    width: detail_area.width.saturating_sub(2),
                    height: detail_area.height.saturating_sub(2),
                };

                let desc_width = inner.width.saturating_mul(82) / 100;

                // The art renders whenever it is loaded. While scrolling it is
                // cleared (see the selection-change handler below), so nothing
                // paints or resizes mid-scroll; a keypress that doesn't change
                // the selection (e.g. launching a game) leaves it in place. The
                // faded background art covers the *whole* pane; the portrait
                // cover art sits on top, filling the region above `ID | Name`.
                if inner.width >= 2 && inner.height >= 1 {
                    if let Some(image) = img.clone() {
                        let mut bg =
                            cover_resize(&image, inner.width as u32, inner.height as u32 * 2);
                        fade_bottom(&mut bg, rgb(theme::BG_DARK));
                        frame.render_widget(
                            Image::with_img(bg)
                                .color_mode(ColorMode::Rgba)
                                .style(Style::default().bg(theme::BG)),
                            inner,
                        );
                    }

                    let content_h = ui::detail::content_height(
                        selected.as_ref(),
                        browser.expand_description,
                        desc_width.saturating_sub(1),
                    )
                    .min(inner.height);
                    let gap = inner.height.saturating_sub(content_h);
                    if gap >= 4 {
                        if let Some(image) = cover_img.clone() {
                            if image.width() > 0 && image.height() > 0 {
                                // Width from the art's aspect: box_w px = 2*gap * W/H.
                                let box_w = ((gap as u32 * 2 * image.width()
                                    / image.height())
                                    as u16)
                                    .clamp(1, inner.width);
                                let thumb = cover_resize(&image, box_w as u32, gap as u32 * 2);
                                frame.render_widget(
                                    Image::with_img(thumb)
                                        .color_mode(ColorMode::Rgba)
                                        .style(Style::default().bg(theme::BG)),
                                    Rect {
                                        x: inner.x,
                                        y: inner.y,
                                        width: box_w,
                                        height: gap,
                                    },
                                );
                            }
                        }
                    }
                }

                // The (transparent) detail table draws its frame and bottom-
                // aligned rows over the art.
                frame.render_widget(
                    ui::detail::detail(
                        selected.as_ref(),
                        browser.expand_description,
                        desc_width.saturating_sub(1),
                        inner.height,
                    ),
                    detail_area,
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

                // Chat view floats above everything (the in-app conversation
                // overlay; a chat can also be popped into its own terminal).
                if browser.show_chat {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::chat::chat(&browser, area.width, area.height), area);
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

                // Community Market search overlay floats above everything.
                if browser.show_market_search {
                    let area = ui::centered_rect(70, 80, frame.size());
                    frame.render_widget(Clear, area);
                    // Pass the inner content height (minus the block border) so the
                    // result window never overflows the overlay.
                    let rows = area.height.saturating_sub(2) as usize;
                    frame.render_widget(ui::market_search::market_search(&browser, rows), area);
                }

                // Workshop overlay floats above everything.
                if browser.show_workshop {
                    let area = ui::centered_rect(72, 80, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::workshop::workshop(&browser), area);
                }

                // Help overlay floats above everything.
                if browser.show_help {
                    let area = ui::centered_rect(64, 84, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::help::help(browser.help_scroll), area);
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

                // Wallet overlay floats above everything.
                if browser.show_wallet {
                    let area = ui::centered_rect(40, 30, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::wallet::wallet(&browser), area);
                }

                // "Not installed — install now?" prompt floats above the library.
                if browser.confirm_install {
                    if let Some(game) = browser.selected() {
                        let area = ui::centered_rect(40, 20, frame.size());
                        frame.render_widget(Clear, area);
                        frame.render_widget(
                            ui::confirm::confirm_install(&game.get_name()),
                            area,
                        );
                    }
                }

                // Install-location picker floats above the library.
                if browser.show_install_picker {
                    let area = ui::centered_rect(64, 40, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::install_picker::install_picker(&browser), area);
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

                // Relink (relink install) prompt floats above everything.
                if browser.show_relink {
                    let area = ui::centered_rect(60, 25, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::relink::relink_overlay(&browser), area);
                }

                // Import (register existing install) prompt floats above everything.
                if browser.show_import {
                    let area = ui::centered_rect(60, 25, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::import::import_overlay(&browser), area);
                }

                // Proton runtimes overlay floats above everything.
                if browser.show_proton {
                    let area = ui::centered_rect(60, 70, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::proton::proton(&browser), area);

                    // Destructive uninstall confirmation floats above the overlay.
                    if browser.confirm_proton_uninstall {
                        if let Some(p) = browser.selected_proton() {
                            let prompt = ui::confirm::confirm_uninstall(&p.name);
                            let confirm_area = ui::centered_rect(40, 20, frame.size());
                            frame.render_widget(Clear, confirm_area);
                            frame.render_widget(prompt, confirm_area);
                        }
                    }
                }

                // Running-games overlay floats above everything.
                if browser.show_running {
                    let area = ui::centered_rect(60, 60, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::running::running(&browser), area);
                }

                // Add-friend (search/add) prompt floats above everything.
                if browser.show_friend_add {
                    let area = ui::centered_rect(60, 30, frame.size());
                    frame.render_widget(Clear, area);
                    frame.render_widget(ui::friend_add::friend_add_overlay(&browser), area);
                }

                // Remove-friend confirmation prompt floats above the friends panel.
                if browser.confirm_friend_remove {
                    if let Some(friend) = browser.selected_friend() {
                        let area = ui::centered_rect(40, 20, frame.size());
                        frame.render_widget(Clear, area);
                        frame.render_widget(
                            ui::confirm::confirm_remove_friend(&friend.display_name()),
                            area,
                        );
                    }
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
                    Mode::LoginUser | Mode::LoginPass => {
                        frame.render_widget(App::build_splash(), placement[0])
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
            // Mark input activity so artwork/details hold off until scrolling
            // (or any keypress burst) settles.
            last_input_at = Instant::now();
            match app.mode {
                Mode::Terminated(_) => {
                    if let KeyCode::Char('q') = input {
                        break;
                    }
                }
                Mode::Browse => {
                    // Any keypress dismisses a transient notice from the prior
                    // action (it is set again below if this action also fails).
                    browser.notice = None;
                    if browser.show_cloud {
                        // Steam Cloud overlay: Esc/q close, s syncs both ways,
                        // d downloads only, u uploads only; each re-fetches.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_cloud(),
                            KeyCode::Char('s') => {
                                if let Some(game) = browser.selected() {
                                    browser.sync_cloud(game.id, aurelia::CloudDirection::Both);
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(game) = browser.selected() {
                                    browser.sync_cloud(game.id, aurelia::CloudDirection::Down);
                                }
                            }
                            KeyCode::Char('u') => {
                                if let Some(game) = browser.selected() {
                                    browser.sync_cloud(game.id, aurelia::CloudDirection::Up);
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
                    } else if browser.show_chat {
                        // Chat view: Esc close, Enter send, typing composes.
                        match input {
                            KeyCode::Esc => browser.close_chat(),
                            KeyCode::Char('\n') | KeyCode::Enter => browser.chat_send(),
                            KeyCode::Backspace => browser.chat_pop(),
                            KeyCode::Char(c) => browser.chat_push(c),
                            _ => {}
                        }
                    } else if browser.show_friend_add {
                        // Add-friend prompt: type the reference, Enter to resolve
                        // it via `friends search`, a to send the request via
                        // `friends add` (kept open to show status), Esc to cancel.
                        match input {
                            KeyCode::Esc => browser.close_friend_add(),
                            KeyCode::Char('\n') | KeyCode::Enter => browser.friend_search(),
                            KeyCode::Char('a') => browser.friend_add_confirm(),
                            KeyCode::Backspace => browser.friend_add_pop(),
                            KeyCode::Char(c) => browser.friend_add_push(c),
                            _ => {}
                        }
                    } else if browser.confirm_friend_remove {
                        // Remove-friend confirmation: y confirms, anything else cancels.
                        match input {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                browser.friend_remove_confirm();
                                browser.confirm_friend_remove = false;
                            }
                            _ => browser.confirm_friend_remove = false,
                        }
                    } else if browser.friends_focused() {
                        // Friends panel focused: j/k move the highlight (the whole
                        // list scrolls under it), c/Enter open the in-app chat, t
                        // pops the chat into a new terminal window, a opens the
                        // add-friend prompt, x removes the highlighted friend
                        // (confirmed), Esc/F/q unfocus.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('F') => {
                                browser.unfocus_friends()
                            }
                            KeyCode::Enter | KeyCode::Char('c') => browser.open_chat(),
                            KeyCode::Char('t') => browser.open_chat_terminal(),
                            KeyCode::Char('a') => browser.open_friend_add(),
                            KeyCode::Char('x') => {
                                if browser.selected_friend().is_some() {
                                    browser.confirm_friend_remove = true;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => browser.friends_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.friends_scroll_up(),
                            // Tab/Shift-Tab keep cycling the tab ring so the
                            // Friends tab is not a dead end — stepping right off
                            // it wraps back to All.
                            KeyCode::Tab => browser.cycle_filter(true),
                            KeyCode::BackTab => browser.cycle_filter(false),
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
                    } else if browser.show_market_search {
                        // Market search overlay: typing edits the query, Enter
                        // runs the search (off-thread), Up/Down move the result
                        // highlight, Tab prices the highlighted result (off-thread),
                        // Esc closes. q/j/k are NOT shortcuts here so they can be
                        // typed into the query.
                        match input {
                            KeyCode::Esc => browser.close_market_search(),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                browser.submit_market_search()
                            }
                            KeyCode::Down => browser.market_result_next(),
                            KeyCode::Up => browser.market_result_previous(),
                            KeyCode::Backspace => browser.market_query_pop(),
                            // Tab prices the highlighted result. A non-character
                            // key is used so every letter stays typeable into the
                            // query (the help overlay documents this).
                            KeyCode::Tab => browser.lookup_market_price(),
                            KeyCode::Char(c) => browser.market_query_push(c),
                            _ => {}
                        }
                    } else if browser.show_workshop {
                        if browser.workshop_comments_open {
                            // Comments sub-pane (over the browse results): Esc
                            // returns to the results, Up/Down scroll. No text is
                            // edited here, so j/k are also accepted as scroll keys.
                            match input {
                                KeyCode::Esc => browser.close_workshop_comments(),
                                KeyCode::Down | KeyCode::Char('j') => {
                                    browser.workshop_comments_scroll_down()
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    browser.workshop_comments_scroll_up()
                                }
                                _ => {}
                            }
                        } else if browser.workshop_browse {
                            // Browse/search pane: the query line takes typed
                            // text (Enter searches off-thread). Navigation/action
                            // use non-text keys so they never collide with what
                            // the user is typing: Up/Down move the highlight,
                            // Tab subscribes/unsubscribes it off-thread, F1/F2
                            // rate it up/down off-thread, F3 opens its comments
                            // sub-pane, and Esc returns to the subscribed list.
                            match input {
                                KeyCode::Esc => browser.workshop_exit_browse(),
                                KeyCode::Char('\n') | KeyCode::Enter => {
                                    browser.start_workshop_search()
                                }
                                KeyCode::Down => browser.workshop_next(),
                                KeyCode::Up => browser.workshop_previous(),
                                KeyCode::Tab => {
                                    browser.workshop_toggle_subscribe_selected()
                                }
                                KeyCode::F(1) => browser.workshop_rate_selected(true),
                                KeyCode::F(2) => browser.workshop_rate_selected(false),
                                KeyCode::F(3) => browser.workshop_open_comments(),
                                KeyCode::Backspace => browser.workshop_pop_query(),
                                KeyCode::Char(c) => browser.workshop_push_query(c),
                                _ => {}
                            }
                        } else {
                            // Subscribed-items list: Esc/q close, j/k scroll,
                            // 'b' opens the browse/search pane.
                            match input {
                                KeyCode::Esc | KeyCode::Char('q') => browser.close_workshop(),
                                KeyCode::Char('b') => browser.workshop_enter_browse(),
                                KeyCode::Down | KeyCode::Char('j') => {
                                    browser.workshop_scroll_down()
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    browser.workshop_scroll_up()
                                }
                                _ => {}
                            }
                        }
                    } else if browser.confirm_install {
                        // "Not installed — install now?" prompt: y opens the
                        // install-location picker for the game, anything cancels.
                        match input {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                browser.confirm_install = false;
                                if let Some(game) = browser.selected() {
                                    if !browser.open_install_picker(game.id) {
                                        let control = browser.begin_install(game.id, None);
                                        client.install(&game, control, None)?;
                                    }
                                }
                            }
                            _ => browser.confirm_install = false,
                        }
                    } else if browser.show_install_picker {
                        // Install-location picker: Up/Down choose a library,
                        // Enter installs the selected game into it, Esc cancels.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                browser.close_install_picker()
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                browser.install_picker_next()
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                browser.install_picker_previous()
                            }
                            KeyCode::Enter | KeyCode::Char('\n') => {
                                if !browser.selected_library_fits() {
                                    // Not enough room — keep the picker open so
                                    // another drive can be chosen.
                                    browser.notice = Some(
                                        "Not enough space on that drive — pick another."
                                            .to_string(),
                                    );
                                } else {
                                    if let (Some(game), Some(library)) = (
                                        browser.selected(),
                                        browser.selected_install_library(),
                                    ) {
                                        let control = browser
                                            .begin_install(game.id, Some(library.clone()));
                                        client.install(&game, control, Some(library))?;
                                    }
                                    browser.close_install_picker();
                                }
                            }
                            _ => {}
                        }
                    } else if browser.confirm_uninstall {
                        // Uninstall confirmation prompt: y confirms, anything else cancels.
                        match input {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                if let Some(game) = browser.selected() {
                                    match aurelia::uninstall(game.id) {
                                        Ok(()) => {
                                            // Reload the library so the listing
                                            // reflects the now-uninstalled game
                                            // (mirrors 'r').
                                            cached = false;
                                            app.mode = Mode::Loading;
                                            client.restart()?;
                                        }
                                        // A failed uninstall must not crash the
                                        // TUI — surface it and stay running.
                                        Err(err) => {
                                            browser.notice =
                                                Some(format!("Uninstall failed: {err}"));
                                        }
                                    }
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
                    } else if browser.show_running {
                        // Running overlay: navigate and stop the highlighted game.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_running(),
                            KeyCode::Down | KeyCode::Char('j') => browser.running_next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.running_previous(),
                            KeyCode::Char('s') | KeyCode::Char('x') => {
                                if let Some(r) = browser.selected_running() {
                                    let _ = aurelia::stop(r.app_id as i32);
                                    let _ = browser.refresh_running();
                                }
                            }
                            _ => {}
                        }
                    } else if browser.show_proton && browser.confirm_proton_uninstall {
                        // Proton uninstall confirmation: y confirms (remove the
                        // custom runtime + refresh), anything else cancels.
                        match input {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                let _ = browser.do_proton_uninstall();
                                browser.confirm_proton_uninstall = false;
                            }
                            _ => browser.confirm_proton_uninstall = false,
                        }
                    } else if browser.show_proton {
                        // Proton overlay: navigate, set the highlighted runtime as
                        // the global default, install it, or uninstall a custom one.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') => browser.close_proton(),
                            KeyCode::Down | KeyCode::Char('j') => browser.proton_next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.proton_previous(),
                            KeyCode::Char('d') => {
                                if let Some(p) = browser.selected_proton() {
                                    let _ = aurelia::proton_default(&p.name);
                                    // Re-fetch so the [default] marker updates,
                                    // keeping the current row highlighted.
                                    let keep = browser.proton_index;
                                    if browser.open_proton().is_ok() {
                                        browser.proton_index =
                                            keep.min(browser.protons.len().saturating_sub(1));
                                    }
                                }
                            }
                            KeyCode::Char('i') => {
                                // Queue a (long-running) download off the UI thread;
                                // progress streams into the proton status cell.
                                if let Some(p) = browser.selected_proton() {
                                    if !p.installed {
                                        browser.clear_proton_status();
                                        let _ = client
                                            .proton_install(&p.name, browser.proton_status.clone());
                                    }
                                }
                            }
                            KeyCode::Char('u') => {
                                // Only custom (installed GE) runtimes are removable.
                                if browser.selected_proton_uninstallable() {
                                    browser.confirm_proton_uninstall = true;
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
                    } else if browser.show_relink {
                        // Relink prompt: type the destination library path, Enter
                        // to relink (kept open to show status), Esc to cancel.
                        match input {
                            KeyCode::Esc => browser.close_relink(),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                let _ = browser.do_relink();
                            }
                            KeyCode::Backspace => browser.relink_pop(),
                            KeyCode::Char(c) => browser.relink_push(c),
                            _ => {}
                        }
                    } else if browser.show_import {
                        // Import prompt: type the library path holding the existing
                        // files, Enter to register (kept open to show status), Esc
                        // to cancel.
                        match input {
                            KeyCode::Esc => browser.close_import(),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                let _ = browser.do_import();
                            }
                            KeyCode::Backspace => browser.import_pop(),
                            KeyCode::Char(c) => browser.import_push(c),
                            _ => {}
                        }
                    } else if browser.show_help {
                        // Help overlay: Esc/q/? close, j/k scroll.
                        match input {
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                                browser.show_help = false;
                            }
                            KeyCode::Down | KeyCode::Char('j') => browser.help_scroll_down(),
                            KeyCode::Up | KeyCode::Char('k') => browser.help_scroll_up(),
                            _ => {}
                        }
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
                    } else if browser.show_wallet {
                        // Any key dismisses the wallet overlay.
                        browser.show_wallet = false;
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
                            KeyCode::Char('?') => {
                                browser.help_scroll = 0;
                                browser.show_help = true;
                            }
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
                            KeyCode::Char('w') => {
                                // Fetch the wallet (blocking) and open the overlay;
                                // ignore failures so a missing session is non-fatal.
                                let _ = browser.open_wallet();
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
                            KeyCode::Char('5') => browser.enter_friends(),
                            KeyCode::Char('s') => browser.cycle_sort(),
                            KeyCode::Char('a') => browser.open_achievements(),
                            KeyCode::Char('F') => browser.toggle_friends_focus(),
                            KeyCode::Char('I') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_inventory(game.id);
                                }
                            }
                            KeyCode::Char('m') => browser.open_market(),
                            KeyCode::Char('S') => browser.open_market_search(),
                            KeyCode::Char('W') => {
                                if let Some(game) = browser.selected() {
                                    browser.open_workshop(game.id);
                                }
                            }
                            KeyCode::Char('i') => browser.toggle_description(),
                            KeyCode::Down | KeyCode::Char('j') => browser.next(),
                            KeyCode::Up | KeyCode::Char('k') => browser.previous(),
                            KeyCode::Char('g') => browser.home(),
                            KeyCode::Char('G') => browser.end(),
                            KeyCode::PageDown => browser.page_down(10),
                            KeyCode::PageUp => browser.page_up(10),
                            KeyCode::Char('\n') | KeyCode::Enter => {
                                if let Some(game) = browser.selected() {
                                    if game.installed {
                                        client.run(&game)?;
                                    } else {
                                        // Not installed — offer to install it
                                        // instead of launching nothing.
                                        browser.confirm_install = true;
                                    }
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(game) = browser.selected() {
                                    // Choose the install location first; fall back
                                    // to the default library if none can be listed.
                                    if !browser.open_install_picker(game.id) {
                                        let control = browser.begin_install(game.id, None);
                                        client.install(&game, control, None)?;
                                    }
                                }
                            }
                            // Space pauses an in-flight install, or resumes a
                            // paused one (the download is left on disk so it
                            // resumes where it left off).
                            KeyCode::Char(' ') => {
                                if let Some(game) = browser.selected() {
                                    match browser.install_phase(&game) {
                                        InstallPhase::Active => browser.pause_install(game.id),
                                        InstallPhase::Paused => {
                                            // Resume into the same library the
                                            // download was started in.
                                            let library =
                                                browser.install_library_for(game.id);
                                            let control = browser
                                                .begin_install(game.id, library.clone());
                                            client.install(&game, control, library)?;
                                        }
                                        InstallPhase::Idle => {}
                                    }
                                }
                            }
                            // Cancel an in-flight or paused install.
                            KeyCode::Char('c') => {
                                if let Some(game) = browser.selected() {
                                    if browser.install_phase(&game) != InstallPhase::Idle {
                                        browser.stop_install(game.id);
                                    }
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
                            KeyCode::Char('K') => {
                                // Only offer relink for installed games.
                                if let Some(game) = browser.selected() {
                                    if game.installed {
                                        browser.open_relink(game.id);
                                    }
                                }
                            }
                            KeyCode::Char('N') => {
                                // Import registers an on-disk install, so it does
                                // not require the game to already be installed.
                                if let Some(game) = browser.selected() {
                                    browser.open_import(game.id);
                                }
                            }
                            KeyCode::Char('v') => {
                                if let Some(game) = browser.selected() {
                                    client.verify(&game)?;
                                }
                            }
                            KeyCode::Char('U') => {
                                if let Some(game) = browser.selected() {
                                    client.update(&game)?;
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
                            KeyCode::Char('P') => {
                                // Blocking fetch; failure leaves the overlay closed.
                                let _ = browser.open_proton();
                            }
                            KeyCode::Char('R') => browser.open_running(),
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
                    // The games cache can be momentarily missing/empty mid-reload
                    // (right after a restart invalidates it). Treat an unreadable
                    // cache as "not ready yet" and stay in Loading to retry next
                    // iteration, rather than crashing the TUI on a parse error.
                    match client.games() {
                        Ok(games) => {
                            config.save()?;
                            browser.set_items(games);
                            terminal.hide_cursor()?;
                            terminal.clear()?;
                            app.mode = Mode::Browse;
                        }
                        Err(_) => {}
                    }
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
        // Adopt any completed market search / price result published off-thread.
        browser.poll_market();

        if app.mode == Mode::Browse && !browser.show_help && !browser.show_dlc && !browser.show_account && !browser.show_config && !browser.show_achievements && !browser.show_cloud && !browser.show_branches && !browser.show_depots && !browser.show_chat && !browser.show_inventory && !browser.show_launch && !browser.show_market && !browser.show_market_search && !browser.show_move && !browser.show_relink && !browser.show_import && !browser.show_proton && !browser.show_running && !browser.show_wallet && !browser.show_workshop && !browser.show_friend_add && !browser.confirm_friend_remove && !browser.confirm_uninstall && !browser.show_install_picker && !browser.confirm_install {
            let selected = browser.selected();
            let cur_sel = selected.as_ref().map(|g| g.id);
            if cur_sel != artwork_sel_id {
                // Selection changed: drop the stale art and forget the requested
                // id so the new art reloads once input settles — nothing shows
                // (or loads) for games merely scrolled past.
                artwork_sel_id = cur_sel;
                img = None;
                cover_img = None;
                requested_img_id = None;
                requested_cover_id = None;
            }
            // Always adopt any in-flight (off-thread) loads.
            artwork::poll(requested_img_id, &mut img, &image_slot);
            artwork::poll(requested_cover_id, &mut cover_img, &cover_slot);
            // Only once input has been idle a moment do we kick off the
            // (off-thread) artwork load and the lazy detail fetches (Proton tier
            // + store metadata) — so flicking through the list doesn't fire a
            // download/CLI call per game and scrolling stays smooth.
            if last_input_at.elapsed() >= ARTWORK_IDLE {
                artwork::select(
                    selected.as_ref(),
                    artwork::ImageKind::Background,
                    &mut requested_img_id,
                    &mut img,
                    &image_slot,
                );
                artwork::select(
                    selected.as_ref(),
                    artwork::ImageKind::Cover,
                    &mut requested_cover_id,
                    &mut cover_img,
                    &cover_slot,
                );
                if let Some(g) = selected.as_ref() {
                    g.query_proton();
                    g.query_info();
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), DisableMouseCapture)?;
    terminal.clear()?;
    Ok(())
}

/// Launch directly into a full-screen chat with a single friend. Used when the
/// binary is invoked as `aurelia-tui --chat <steamid> <name>` — the Friends
/// panel's "open in new terminal" action ([t]) spawns exactly this in a fresh
/// console window. Reuses the shared chat widget and the `Browser` chat state.
fn chat_entry(steamid: u64, name: String) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut browser = Browser::new(Vec::new());
    browser.chat_steamid = steamid;
    browser.chat_partner = name;
    browser.refresh_chat();

    let events = Events::new();
    // Re-fetch history every ~3s (12 ticks at the 250ms tick rate) so incoming
    // messages surface without hammering the CLI on every frame.
    let mut ticks: u32 = 0;
    loop {
        terminal.draw(|frame| {
            let size = frame.size();
            frame.render_widget(Block::default().style(theme::canvas()), size);
            frame.render_widget(ui::chat::chat(&browser, size.width, size.height), size);
        })?;

        match events.next()? {
            Event::Input(input) => {
                match input {
                    KeyCode::Esc => break,
                    KeyCode::Char('\n') | KeyCode::Enter => browser.chat_send(),
                    KeyCode::Backspace => browser.chat_pop(),
                    KeyCode::Char(c) => browser.chat_push(c),
                    _ => {}
                }
            }
            Event::Tick => {
                ticks = ticks.wrapping_add(1);
                if ticks % 12 == 0 {
                    browser.refresh_chat();
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn main() {
    // `aurelia-tui --chat <steamid> <name>` opens a dedicated chat window.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--chat") {
        if let Some(steamid) = args.get(pos + 1).and_then(|s| s.parse::<u64>().ok()) {
            let name = args.get(pos + 2).cloned().unwrap_or_default();
            if let Err(err) = chat_entry(steamid, name) {
                println!("{:?}", err);
            }
            return;
        }
    }

    match entry() {
        Ok(()) => {}
        Err(err) => {
            // entry() bailed before its own cleanup ran, so the terminal is
            // still in raw mode with the mouse captured. Restore both before
            // printing or the error lands in a garbled console.
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), DisableMouseCapture);
            println!("{}", err);
        }
    }
}

#[cfg(test)]
mod cover_tests {
    use super::cover_resize;

    fn img(w: u32, h: u32) -> image::ImageBuffer<image::Rgba<u8>, Vec<u8>> {
        image::ImageBuffer::from_pixel(w, h, image::Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn cover_resize_fills_exact_target_dimensions() {
        // Wide source into a taller target -> crops left/right, fills exactly.
        let out = cover_resize(&img(200, 100), 20, 40);
        assert_eq!((out.width(), out.height()), (20, 40));

        // Tall source into a wider target -> crops top/bottom, fills exactly.
        let out = cover_resize(&img(100, 200), 40, 20);
        assert_eq!((out.width(), out.height()), (40, 20));

        // Source already at the target aspect -> still the exact target size.
        let out = cover_resize(&img(80, 160), 10, 20);
        assert_eq!((out.width(), out.height()), (10, 20));
    }

    #[test]
    fn cover_resize_returns_clone_on_degenerate_input() {
        let zero = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::new(0, 0);
        let out = cover_resize(&zero, 10, 10);
        assert_eq!((out.width(), out.height()), (0, 0));
    }
}

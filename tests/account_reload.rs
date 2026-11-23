//! Account reload feedback over cache.

use aurelia_tui::browse::Browser;
use aurelia_tui::browse::account::ReloadStatus;
use aurelia_tui::interface::aurelia::AccountJson;
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn isolate() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("account-reload");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-aurelia"));
    }
}

fn shown(b: &Browser, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    let area = ui::centered_rect(64, 55, Rect::new(0, 0, w, h));
    t.draw(|f| f.render_widget(ui::account::account(b), area))
        .unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn info(name: &str) -> AccountJson {
    serde_json::from_value(serde_json::json!({ "account_name": name })).unwrap()
}

#[test]
fn reload_keeps_cache_and_reports() {
    isolate();
    let mut b = Browser::new(vec![]);
    b.open_account();
    assert_eq!(b.account_load.status, None, "first load: no reload row");
    b.finish_account_for_test(Ok(info("first")));
    b.poll_account();

    b.refresh_account();
    assert_eq!(b.account_load.status, Some(ReloadStatus::Reloading));
    for (w, h) in [(120, 40), (80, 24)] {
        let out = shown(&b, w, h);
        assert!(out.contains("Reloading…"), "{w}x{h}\n{out}");
        assert!(out.contains("first"), "cached data stays\n{out}");
    }
    b.finish_account_for_test(Ok(info("second")));
    b.poll_account();
    assert_eq!(b.account_load.status, Some(ReloadStatus::Updated));
    assert!(shown(&b, 80, 24).contains("Updated"));
    assert!(shown(&b, 80, 24).contains("second"));

    b.refresh_account();
    b.finish_account_for_test(Err("timed out".into()));
    b.poll_account();
    assert_eq!(
        b.account_load.status,
        Some(ReloadStatus::Failed("timed out".into()))
    );
    let out = shown(&b, 80, 24);
    assert!(out.contains("Failed") && out.contains("second"), "{out}");
    b.close_account();
    assert_eq!(b.account_load.status, None, "cleared on close");
}

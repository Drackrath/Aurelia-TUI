//! Fixtures follow Aurelia's dry_run_json.
use super::*;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/install_options/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn parses_dry_run_with_languages() {
    let value: serde_json::Value = serde_json::from_str(&fixture("dry_run_multi.json")).unwrap();
    let dry = parse_dry_run(value).unwrap();
    assert_eq!(dry.platform, "windows");
    assert_eq!(dry.disk_size, 9_663_676_416);
    assert_eq!(dry.download_size, 5_368_709_120);
    assert_eq!(dry.depot_count, 2);
    assert_eq!(dry.languages, ["english", "german", "schinese"]);
    assert_eq!(dry.default_language.as_deref(), Some("german"));
    assert_eq!(dry.installed_language, None);
    assert!(dry.has_build() && dry.offers_languages());
}

#[test]
fn single_language_and_no_build() {
    let value: serde_json::Value = serde_json::from_str(&fixture("dry_run_single.json")).unwrap();
    let dry = parse_dry_run(value).unwrap();
    assert!(!dry.offers_languages());
    assert!(!dry.has_build());
}

#[test]
fn install_opts_args() {
    assert!(InstallOpts::default().args().is_empty());
    let opts = InstallOpts {
        platform: Some("linux".into()),
        language: Some("german".into()),
    };
    assert_eq!(opts.args(), ["-p", "linux", "--language", "german"]);
}

#[test]
fn platform_choice_cycles_and_parses() {
    assert_eq!(PlatformChoice::Auto.arg(), None);
    assert_eq!(PlatformChoice::Linux.arg(), Some("linux"));
    assert_eq!(PlatformChoice::Linux.next(), PlatformChoice::Auto);
    assert_eq!(
        PlatformChoice::from_installed(Some("Linux")),
        PlatformChoice::Linux
    );
    assert_eq!(
        PlatformChoice::from_installed(Some("macos")),
        PlatformChoice::Auto
    );
    assert_eq!(PlatformChoice::from_installed(None), PlatformChoice::Auto);
}

#[test]
fn language_labels() {
    assert_eq!(language_label("english"), "English");
    assert_eq!(language_label("schinese"), "Simplified Chinese");
    assert_eq!(language_label("tchinese"), "Traditional Chinese");
    assert_eq!(language_label("koreana"), "Korean");
    assert_eq!(language_label("german"), "German");
    assert_eq!(language_label(""), "");
}

#[test]
fn set_language_result_and_progress() {
    assert_eq!(
        finish_message(fixture("set_language_result.json").trim()),
        "language set: German"
    );
    assert!(
        finish_message(fixture("error.json").trim()).starts_with("Failed: app 70 has no klingon")
    );
    assert_eq!(
        progress_line(fixture("progress_line.json").trim()).as_deref(),
        Some("changing language 50.0% · ETA 1m 15s")
    );
    assert_eq!(progress_line("not json"), None);
}

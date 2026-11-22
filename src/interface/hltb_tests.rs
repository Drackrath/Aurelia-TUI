//! Minimal public-response fixtures; the HTTP tests use a local server and no
//! Steam credentials. Live tests are opt-in, like aurelia_integration.rs.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
};

fn game() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/hltb/portal.json")).unwrap()
}
fn page(game: Value) -> String {
    format!(
        r#"<script id="__NEXT_DATA__" type="application/json">{}</script>"#,
        json!({"props":{"pageProps":{"game":{"data":{"game":[game]}}}}})
    )
}
fn page_with_build(game: Value, build: &str) -> String {
    format!(
        r#"<script id="__NEXT_DATA__">{}</script>"#,
        json!({"buildId": build, "props": {"pageProps": {"game": {"data": {"game": [game]}}}}})
    )
}
fn json_page(game: Value) -> String {
    json!({"pageProps": {"game": {"data": {"game": [game]}}}}).to_string()
}
fn verified_html(html: &str, steam_id: i32, expected_id: u64) -> Result<Option<HltbData>, String> {
    let data = html_data(html)?;
    verified_game(&data["props"]["pageProps"], steam_id, expected_id)
}
fn data() -> HltbData {
    verified_html(&page(game()), 400, 7230).unwrap().unwrap()
}

#[test]
fn verification_rejects_wrong_steam_and_page_ids_and_invalid_times() {
    assert_eq!(data().main, Some(11230));
    assert!(
        verified_html(&page(game()), 2012840, 7230)
            .unwrap()
            .is_some()
    );
    assert!(verified_html(&page(game()), 620, 7230).unwrap().is_none());
    assert!(verified_html(&page(game()), -1, 7230).is_err());
    assert!(verified_html(&page(game()), 0, 7230).is_err());
    assert!(verified_html(&page(game()), 400, 999).is_err());
    assert!(verified_html("<html>challenge</html>", 400, 7230).is_err());
    let mut g = game();
    g["profile_steam"] = json!("400");
    g["comp_main"] = json!(0);
    g["comp_plus"] = Value::Null;
    g.as_object_mut().unwrap().remove("comp_100");
    let d = verified_html(&page(g.clone()), 400, 7230).unwrap().unwrap();
    assert_eq!((d.main, d.extra, d.completionist), (None, None, None));
    g["comp_main"] = json!("broken");
    assert!(verified_html(&page(g), 400, 7230).is_err());
}

#[test]
fn versioned_cache_handles_expiry_corruption_and_failed_refreshes() {
    let dir = std::env::temp_dir().join(format!("aurelia-hltb-cache-{}", std::process::id()));
    let path = dir.join("400.json");
    let mut cached = Cached {
        version: CACHE_VERSION,
        steam_id: 400,
        fetched_at: DAY,
        data: Some(data()),
    };
    assert!(cached.fresh(8 * DAY - 1));
    assert!(!cached.fresh(8 * DAY));
    assert!(!cached.fresh(DAY - 1));
    write_cache(&path, &cached).unwrap();
    assert!(matches!(
        lookup_cached(
            Some(&path),
            400,
            DAY,
            |_| {},
            |_| panic!("fresh data must skip network")
        ),
        HltbState::Ready { stale: false, .. }
    ));
    assert!(matches!(
        lookup_cached(
            Some(&path),
            400,
            9 * DAY,
            |_| {},
            |game_id| {
                assert_eq!(game_id, Some(7230));
                Err("schema changed".into())
            }
        ),
        HltbState::Ready { stale: true, .. }
    ));
    assert_eq!(read_cache(&path, 400).unwrap().fetched_at, DAY);
    assert!(read_cache(&path, 620).is_none());
    cached.data = None;
    assert!(cached.fresh(2 * DAY - 1));
    assert!(!cached.fresh(2 * DAY));
    write_cache(&path, &cached).unwrap();
    assert!(matches!(
        lookup_cached(
            Some(&path),
            400,
            DAY,
            |_| {},
            |_| panic!("fresh miss must skip network")
        ),
        HltbState::NotFound
    ));
    cached.version = 0;
    write_cache(&path, &cached).unwrap();
    assert!(read_cache(&path, 400).is_none());
    fs::write(&path, b"broken").unwrap();
    assert!(read_cache(&path, 400).is_none());
    fs::remove_file(&path).unwrap();
    assert!(
        matches!(lookup_cached(Some(&path), 400, DAY, |_| {}, |_| Err("HTTP 429".into())), HltbState::Unavailable(e) if e == "HTTP 429")
    );
    assert!(
        !path.exists(),
        "errors must never become negative cache entries"
    );
    assert!(matches!(
        lookup_cached(Some(&path), 400, now(), |_| {}, |_| Ok(None)),
        HltbState::NotFound
    ));
    assert!(read_cache(&path, 400).unwrap().fresh(now()));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn successful_lookups_persist_and_cache_write_failures_keep_the_result() {
    let dir = std::env::temp_dir().join(format!("aurelia-hltb-persist-{}", std::process::id()));
    let path = dir.join("400.json");
    assert!(matches!(
        lookup_cached(Some(&path), 400, now(), |_| {}, |_| Ok(Some(data()))),
        HltbState::Ready { stale: false, .. }
    ));
    assert_eq!(read_cache(&path, 400).unwrap().data, Some(data()));
    // A new lookup reads the persisted file, with no in-memory result to reuse.
    assert!(matches!(
        lookup_cached(
            Some(&path),
            400,
            now(),
            |_| {},
            |_| panic!("persisted hit must skip network")
        ),
        HltbState::Ready { stale: false, .. }
    ));
    let unwritable = dir.join("directory-instead-of-file.json");
    fs::create_dir(&unwritable).unwrap();
    assert!(matches!(
        lookup_cached(Some(&unwritable), 400, now(), |_| {}, |_| Ok(Some(data()))),
        HltbState::Ready { stale: false, .. }
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn only_one_background_worker_can_be_reserved_and_it_is_released() {
    let _serial = WORKER_TEST_LOCK.lock().unwrap();
    let first = reserve().expect("first worker");
    assert!(reserve().is_none());
    drop(first);
    assert!(reserve().is_some());
}

#[test]
fn expired_estimates_are_published_before_refresh_and_replaced_only_after_success() {
    let dir = std::env::temp_dir().join(format!("aurelia-hltb-publish-{}", std::process::id()));
    let path = dir.join("400.json");
    write_cache(
        &path,
        &Cached {
            version: CACHE_VERSION,
            steam_id: 400,
            fetched_at: DAY,
            data: Some(data()),
        },
    )
    .unwrap();
    let published = std::cell::RefCell::new(None);
    let mut updated = data();
    updated.main = Some(12345);
    let result = lookup_cached(
        Some(&path),
        400,
        9 * DAY,
        |data| {
            *published.borrow_mut() = Some(data);
        },
        |id| {
            assert_eq!(id, Some(7230));
            assert_eq!(
                published.borrow().as_ref(),
                Some(&data()),
                "cached estimates must precede network work"
            );
            assert_eq!(read_cache(&path, 400).unwrap().fetched_at, DAY);
            Ok(Some(updated.clone()))
        },
    );
    assert!(matches!(result, HltbState::Ready { data, stale: false } if data == updated));
    assert_eq!(read_cache(&path, 400).unwrap().data, Some(updated));
    fs::remove_dir_all(dir).unwrap();
}

// A path prefix permits the init route's changing timestamp without weakening
// checks on the method, route, request headers, or posted search payload.
struct Step {
    method: &'static str,
    path: &'static str,
    status: u16,
    body: String,
}
impl Step {
    fn get(path: &'static str, body: impl Into<String>) -> Self {
        Self {
            method: "GET",
            path,
            status: 200,
            body: body.into(),
        }
    }
    fn post(body: impl Into<String>) -> Self {
        Self {
            method: "POST",
            path: "/api/search/site",
            status: 200,
            body: body.into(),
        }
    }
    fn status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}
// Serve only the scripted exchanges on localhost. Finite accept/read budgets
// make a missing request fail the test instead of hanging the test process.
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    worker: thread::JoinHandle<()>,
}
impl Server {
    fn start(steps: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let worker = thread::spawn(move || {
            for step in steps {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("missing {} {} request: {e}", step.method, step.path),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = Vec::new();
                let (header_end, length) = loop {
                    let mut buffer = [0u8; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    raw.extend_from_slice(&buffer[..count]);
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                // TCP may split headers and JSON across separate reads. Do not
                // assert the search payload until Content-Length bytes arrived.
                while raw.len() < header_end + length {
                    let mut buffer = [0u8; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    raw.extend_from_slice(&buffer[..count]);
                }
                let raw = String::from_utf8(raw).unwrap();
                assert!(
                    raw.starts_with(&format!("{} {}", step.method, step.path)),
                    "unexpected request: {raw}"
                );
                captured.lock().unwrap().push(raw);
                // Status zero models a peer dropping the connection before it
                // sends any HTTP response, rather than a normal HTTP failure.
                if step.status == 0 {
                    continue;
                }
                write!(
                    socket,
                    "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    step.status,
                    step.body.len(),
                    step.body
                )
                .unwrap();
            }
        });
        Self {
            base,
            requests,
            worker,
        }
    }
    // Joining also surfaces assertion failures from the server thread; otherwise
    // an incorrectly ordered or missing request could leave a false-green test.
    fn finish(self) -> Vec<String> {
        self.worker.join().unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}
fn discovery() -> Vec<Step> {
    vec![
        Step::get(
            "/",
            r#"<script src="/_next/static/chunks/pages/index-1.js"></script>"#,
        ),
        Step::get(
            "/_next/static/chunks/pages/index-1.js",
            r#"fetch("/api/search/site",{method:"POST",headers:{}})"#,
        ),
    ]
}
fn auth() -> Step {
    Step::get("/api/search/site/init?", r#"{"token":"fixture-token"}"#)
}
fn results(ids: &[u64]) -> String {
    json!({"data":ids.iter().map(|id| json!({"game_id":id,"game_name":"Portal"})).collect::<Vec<_>>()} ).to_string()
}

#[test]
fn search_discovery_learns_the_build_id_before_fetching_its_first_match() {
    let mut steps = discovery();
    steps[0].body.push_str(&format!(
        r#"<script id="__NEXT_DATA__">{}</script>"#,
        json!({"buildId": "homepage-build"})
    ));
    steps.extend([
        auth(),
        Step::post(results(&[7230])),
        Step::get(
            "/_next/data/homepage-build/game/7230.json",
            json_page(game()),
        ),
    ]);
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert_eq!(provider.build_id.as_deref(), Some("homepage-build"));
    assert_eq!(server.finish().len(), 5);
}

#[test]
fn unusable_homepage_build_metadata_keeps_html_search_results_available() {
    for metadata in ["", "not JSON", r#"{}"#, r#"{"buildId":"../outside"}"#] {
        let mut steps = discovery();
        steps[0].body.push_str(&format!(
            r#"<script id="__NEXT_DATA__">{metadata}</script>"#
        ));
        steps.extend([
            auth(),
            Step::post(results(&[7230])),
            Step::get("/game/7230", page_with_build(game(), "game-build")),
        ]);
        let server = Server::start(steps);
        let mut provider = Provider::new(&server.base, None).unwrap();
        provider.build_id = Some("previous-build".into());
        assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
        assert_eq!(provider.build_id.as_deref(), Some("game-build"));
        assert_eq!(server.finish().len(), 5);
    }
}

#[test]
fn game_pages_teach_the_build_id_without_a_homepage_request_then_use_json() {
    let mut updated = game();
    updated["comp_main"] = json!(12345);
    let server = Server::start(vec![
        Step::get("/game/7230", page_with_build(game(), "fixture-build")),
        Step::get(
            "/_next/data/fixture-build/game/7230.json",
            json_page(updated),
        ),
    ]);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert_eq!(
        provider.fetch(400, "Portal", Some(7230)).unwrap(),
        Some(data())
    );
    let refreshed = provider.fetch(400, "Portal", Some(7230)).unwrap().unwrap();
    assert_eq!(refreshed.main, Some(12345));
    assert_eq!(provider.build_id.as_deref(), Some("fixture-build"));
    assert!(provider.endpoint.is_none());
    assert!(provider.token.is_none());
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn a_rotated_json_build_falls_back_to_html_and_learns_the_new_build() {
    let server = Server::start(vec![
        Step::get("/game/7230", page_with_build(game(), "old-build")),
        Step::get("/_next/data/old-build/game/7230.json", "old deployment").status(404),
        Step::get("/game/7230", page_with_build(game(), "new-build")),
        Step::get("/_next/data/new-build/game/7230.json", json_page(game())),
    ]);
    let mut provider = Provider::new(&server.base, None).unwrap();
    for _ in 0..3 {
        assert_eq!(
            provider.fetch(400, "Portal", Some(7230)).unwrap(),
            Some(data())
        );
    }
    assert_eq!(provider.build_id.as_deref(), Some("new-build"));
    assert!(provider.endpoint.is_none());
    assert_eq!(server.finish().len(), 4);
}

#[test]
fn unusable_json_routes_and_formats_fall_back_to_verified_html() {
    let mut invalid_time = game();
    invalid_time["comp_main"] = json!("broken");
    for (body, status) in [
        ("not JSON".into(), 200),
        ("{}".into(), 200),
        (json_page(invalid_time), 200),
        ("unauthorized".into(), 401),
        ("forbidden".into(), 403),
        ("gone".into(), 410),
    ] {
        let server = Server::start(vec![
            Step::get("/_next/data/fixture-build/game/7230.json", body).status(status),
            Step::get("/game/7230", page_with_build(game(), "new-build")),
        ]);
        let mut provider = Provider::new(&server.base, None).unwrap();
        provider.build_id = Some("fixture-build".into());
        assert_eq!(
            provider.fetch(400, "Portal", Some(7230)).unwrap(),
            Some(data())
        );
        assert_eq!(provider.build_id.as_deref(), Some("new-build"));
        assert_eq!(server.finish().len(), 2);
    }
}

#[test]
fn json_and_html_both_require_verified_steam_and_game_ids() {
    let mut wrong_steam = game();
    wrong_steam["profile_steam"] = json!(620);
    wrong_steam["profile_steam_alt"] = json!(0);
    let server = Server::start(vec![Step::get(
        "/_next/data/fixture-build/game/7230.json",
        json_page(wrong_steam),
    )]);
    let mut provider = Provider::new(&server.base, None).unwrap();
    provider.build_id = Some("fixture-build".into());
    assert!(
        provider
            .game(400, 7230, Instant::now() + Duration::from_secs(2))
            .unwrap()
            .is_none()
    );
    assert_eq!(server.finish().len(), 1);

    let mut wrong_id = game();
    wrong_id["game_id"] = json!(999);
    let server = Server::start(vec![
        Step::get(
            "/_next/data/fixture-build/game/7230.json",
            json_page(wrong_id.clone()),
        ),
        Step::get("/game/7230", page_with_build(wrong_id, "new-build")),
    ]);
    let mut provider = Provider::new(&server.base, None).unwrap();
    provider.build_id = Some("fixture-build".into());
    assert!(
        provider
            .game(400, 7230, Instant::now() + Duration::from_secs(2))
            .unwrap_err()
            .to_string()
            .contains("different game page")
    );
    assert_eq!(server.finish().len(), 2);
}

#[test]
fn json_outages_and_rate_limits_do_not_trigger_html_retries() {
    for status in [0, 429, 503] {
        let server = Server::start(vec![
            Step::get("/_next/data/fixture-build/game/7230.json", "unavailable").status(status),
        ]);
        let mut provider = Provider::new(&server.base, None).unwrap();
        provider.build_id = Some("fixture-build".into());
        let error = provider.fetch(400, "Portal", Some(7230)).unwrap_err();
        if status != 0 {
            assert_eq!(error.status, Some(StatusCode::from_u16(status).unwrap()));
        }
        assert_eq!(provider.build_id.as_deref(), Some("fixture-build"));
        if status == 429 {
            assert!(
                provider
                    .fetch(620, "Portal 2", None)
                    .unwrap_err()
                    .to_string()
                    .contains("wait a minute")
            );
        }
        assert_eq!(server.finish().len(), 1);
    }
}

#[test]
fn invalid_build_ids_leave_html_fetching_available() {
    for build in ["", "../outside", "https://example.com"] {
        let server = Server::start(vec![
            Step::get("/game/7230", page_with_build(game(), build)),
            Step::get("/game/7230", page_with_build(game(), build)),
        ]);
        let mut provider = Provider::new(&server.base, None).unwrap();
        for _ in 0..2 {
            assert_eq!(
                provider.fetch(400, "Portal", Some(7230)).unwrap(),
                Some(data())
            );
            assert!(provider.build_id.is_none());
        }
        assert_eq!(server.finish().len(), 2);
    }
}

fn wiki_lookup_response(title: &str) -> String {
    json!({"idlookup": [{"title": {"Page": title}}]}).to_string()
}

fn wiki_page_response(text: &str) -> String {
    json!({"parse": {"title": "Portal", "wikitext": {"*": text}}}).to_string()
}

fn wiki_steps(text: &str) -> Vec<Step> {
    vec![
        Step::get(
            "/w/api.php?action=idlookup&field=steamappid&value=400&format=json",
            wiki_lookup_response("Portal"),
        ),
        Step::get(
            "/w/api.php?action=parse&page=Portal&prop=wikitext&redirects=1&format=json",
            wiki_page_response(text),
        ),
    ]
}

const WIKI_PORTAL: &str = "{{Infobox game\n|developers = {{Infobox game/row/developer|Valve}}\n|steam appid = 400\n|steam appid side = 323170\n|hltb = 7230\n}}";

#[test]
fn wiki_infobox_mapping_requires_unambiguous_numeric_ids_and_the_selected_steam_app() {
    assert_eq!(wiki_game_id(WIKI_PORTAL, 400).unwrap(), Some(7230));
    assert_eq!(wiki_game_id(WIKI_PORTAL, 323170).unwrap(), Some(7230));
    assert!(wiki_game_id(WIKI_PORTAL, 620).is_err());
    assert_eq!(
        wiki_game_id(&WIKI_PORTAL.replace("|hltb = 7230", "|hltb = "), 400).unwrap(),
        None
    );
    assert!(wiki_game_id(&WIKI_PORTAL.replace("7230", "not an ID"), 400).is_err());
    assert!(wiki_game_id(&WIKI_PORTAL.replace("7230", "0"), 400).is_err());
    assert!(
        wiki_game_id(
            &WIKI_PORTAL.replace("|hltb = 7230", "|hltb = 7230\n|hltb = 999"),
            400
        )
        .is_err()
    );
    assert!(wiki_game_id("|steam appid = 400\n|hltb = 7230", 400).is_err());
    assert!(wiki_game_id("{{Infobox game\n|steam appid = 400\n|hltb = 7230", 400).is_err());
    let text = format!("<!-- {{}} |hltb = 999 -->\n{WIKI_PORTAL}\n|hltb = 123");
    assert_eq!(wiki_game_id(&text, 400).unwrap(), Some(7230));
    let text = WIKI_PORTAL.replace("|hltb = 7230", "|hltb = <!-- former ID --> 7230");
    assert_eq!(wiki_game_id(&text, 400).unwrap(), Some(7230));
}

#[test]
fn wiki_primary_lookup_uses_three_requests_and_persists_the_mapping() {
    let dir = std::env::temp_dir().join(format!("aurelia-wiki-primary-{}", std::process::id()));
    let wiki_server = Server::start(wiki_steps(WIKI_PORTAL));
    let hltb_server = Server::start(vec![Step::get("/game/7230", page(game()))]);
    let wiki = PcGamingWiki::new(&wiki_server.base, Some(dir.clone())).unwrap();
    let mut provider = Provider::new(&hltb_server.base, Some(wiki)).unwrap();
    let started = Instant::now();
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "API request starts must be paced"
    );
    assert!(provider.endpoint.is_none());
    assert!(provider.token.is_none());
    let requests = wiki_server.finish();
    assert_eq!(requests.len(), 2);
    for request in requests {
        let headers = request.to_ascii_lowercase();
        assert!(request.contains(WIKI_USER_AGENT));
        assert!(!headers.contains("\r\norigin:"));
        assert!(!headers.contains("\r\nreferer:"));
    }
    assert_eq!(hltb_server.finish().len(), 1);
    let mut restored = PcGamingWiki::new("http://127.0.0.1:0", Some(dir.clone())).unwrap();
    assert_eq!(
        restored
            .lookup(400, Instant::now() - Duration::from_secs(1))
            .unwrap(),
        Some(7230)
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wiki_cache_validates_identity_expiry_and_schema_and_persists_missing_mappings() {
    let mut cached = WikiCached {
        version: WIKI_CACHE_VERSION,
        steam_id: 400,
        fetched_at: DAY,
        game_id: Some(7230),
    };
    assert!(cached.fresh(400, 8 * DAY - 1));
    assert!(!cached.fresh(400, 8 * DAY));
    assert!(!cached.fresh(400, DAY - 1));
    assert!(!cached.fresh(620, DAY));
    cached.version = 0;
    assert!(!cached.fresh(400, DAY));
    cached.version = WIKI_CACHE_VERSION;
    cached.game_id = Some(0);
    assert!(!cached.fresh(400, DAY));
    cached.game_id = None;
    assert!(cached.fresh(400, 2 * DAY - 1));
    assert!(!cached.fresh(400, 2 * DAY));

    let dir = std::env::temp_dir().join(format!("aurelia-wiki-cache-{}", std::process::id()));
    let path = dir.join("400.json");
    fs::create_dir_all(&dir).unwrap();
    fs::write(&path, "broken cache").unwrap();
    let server = Server::start(vec![Step::get("/w/api.php?", r#"{"idlookup":[]}"#)]);
    let mut wiki = PcGamingWiki::new(&server.base, Some(dir.clone())).unwrap();
    assert_eq!(
        wiki.lookup(400, Instant::now() + Duration::from_secs(2))
            .unwrap(),
        None
    );
    let stored: WikiCached = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(stored.fresh(400, now()));
    assert!(stored.game_id.is_none());
    assert_eq!(server.finish().len(), 1);
    let mut restored = PcGamingWiki::new("http://127.0.0.1:0", Some(dir.clone())).unwrap();
    assert_eq!(
        restored
            .lookup(400, Instant::now() - Duration::from_secs(1))
            .unwrap(),
        None
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_unavailable_and_invalid_wiki_mappings_fall_back_to_hltb_search() {
    let cases = vec![
        vec![Step::get("/w/api.php?", r#"{"idlookup":[]}"#)],
        vec![Step::get("/w/api.php?", "unavailable").status(503)],
        vec![Step::get("/w/api.php?", "not JSON")],
        vec![Step::get(
            "/w/api.php?",
            r#"{"idlookup":[{"title":{"Page":"One"}},{"title":{"Page":"Two"}}]}"#,
        )],
        vec![Step::get("/w/api.php?", r#"{"idlookup":[{}]}"#)],
        wiki_steps(&WIKI_PORTAL.replace("|hltb = 7230", "|hltb = ")),
        wiki_steps(&WIKI_PORTAL.replace("|steam appid = 400", "|steam appid = 620")),
    ];
    for steps in cases {
        let wiki_requests = steps.len();
        let wiki_server = Server::start(steps);
        let mut steps = discovery();
        steps.extend([
            auth(),
            Step::post(results(&[7230])),
            Step::get("/game/7230", page(game())),
        ]);
        let hltb_server = Server::start(steps);
        let wiki = PcGamingWiki::new(&wiki_server.base, None).unwrap();
        let mut provider = Provider::new(&hltb_server.base, Some(wiki)).unwrap();
        assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
        assert!(provider.endpoint.is_some());
        assert_eq!(wiki_server.finish().len(), wiki_requests);
        assert_eq!(hltb_server.finish().len(), 5);
    }
}

#[test]
fn wiki_mapping_cannot_bypass_hltb_steam_verification() {
    let wiki_server = Server::start(wiki_steps(&WIKI_PORTAL.replace("7230", "1")));
    let mut wrong_game = game();
    wrong_game["game_id"] = json!(1);
    wrong_game["profile_steam"] = json!(620);
    wrong_game["profile_steam_alt"] = json!(0);
    let mut steps = vec![Step::get("/game/1", page(wrong_game))];
    steps.extend(discovery());
    steps.extend([
        auth(),
        Step::post(results(&[7230])),
        Step::get("/game/7230", page(game())),
    ]);
    let hltb_server = Server::start(steps);
    let wiki = PcGamingWiki::new(&wiki_server.base, None).unwrap();
    let mut provider = Provider::new(&hltb_server.base, Some(wiki)).unwrap();
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert_eq!(wiki_server.finish().len(), 2);
    assert_eq!(hltb_server.finish().len(), 6);
}

#[test]
fn wiki_rate_limits_do_not_disable_hltb_fallback_or_cache_failures() {
    for step in [
        Step::get("/w/api.php?", "limited").status(429),
        Step::get("/w/api.php?", r#"{"error":{"code":"ratelimited"}}"#),
    ] {
        let dir = std::env::temp_dir().join(format!("aurelia-wiki-rate-{}", std::process::id()));
        let wiki_server = Server::start(vec![step]);
        let mut steps = discovery();
        steps.push(auth());
        for _ in 0..2 {
            steps.extend([
                Step::post(results(&[7230])),
                Step::get("/game/7230", page(game())),
            ]);
        }
        let hltb_server = Server::start(steps);
        let wiki = PcGamingWiki::new(&wiki_server.base, Some(dir.clone())).unwrap();
        let mut provider = Provider::new(&hltb_server.base, Some(wiki)).unwrap();
        for _ in 0..2 {
            assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
        }
        assert!(
            provider.retry_after.is_none(),
            "wiki throttling must not become HLTB throttling"
        );
        assert!(provider.wiki.as_ref().unwrap().retry_after.is_some());
        assert!(!dir.join("400.json").exists());
        assert_eq!(wiki_server.finish().len(), 1);
        assert_eq!(hltb_server.finish().len(), 7);
    }
}

#[test]
fn wiki_outages_and_empty_hltb_searches_are_not_cached_as_missing_games() {
    let dir = std::env::temp_dir().join(format!("aurelia-wiki-outage-{}", std::process::id()));
    let path = dir.join("400.json");
    let wiki_server = Server::start(vec![Step::get("/w/api.php?", "unavailable").status(503)]);
    let mut steps = discovery();
    steps.extend([auth(), Step::post(results(&[]))]);
    let hltb_server = Server::start(steps);
    let wiki = PcGamingWiki::new(&wiki_server.base, None).unwrap();
    let mut provider = Provider::new(&hltb_server.base, Some(wiki)).unwrap();
    let state = lookup_cached(
        Some(&path),
        400,
        now(),
        |_| {},
        |id| provider.fetch(400, "Portal", id).map_err(|e| e.to_string()),
    );
    assert!(matches!(state, HltbState::Unavailable(_)));
    assert!(!path.exists());
    assert_eq!(wiki_server.finish().len(), 1);
    assert_eq!(hltb_server.finish().len(), 4);
}

#[test]
fn wiki_request_pacing_obeys_the_original_deadline() {
    let mut wiki = PcGamingWiki::new("http://127.0.0.1:0", None).unwrap();
    wiki.next_request = Some(Instant::now() + Duration::from_secs(2));
    assert!(
        wiki.json(&[], Instant::now() + Duration::from_millis(20))
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
    wiki.next_request = None;
    assert!(
        wiki.json(&[], Instant::now() - Duration::from_secs(1))
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}

#[test]
#[ignore = "requires live PCGamingWiki and HowLongToBeat access"]
fn live_pcgamingwiki_matches() {
    let wiki = PcGamingWiki::new(WIKI_BASE, None).unwrap();
    let mut provider = Provider::new(BASE, Some(wiki)).unwrap();
    for (id, name) in [
        (400, "Portal"),
        (1306630, "Lost Ruins"),
        (599140, "Graveyard Keeper"),
    ] {
        let data = provider
            .fetch(id, name, None)
            .unwrap()
            .expect("verified HLTB match");
        assert!(data.main.is_some(), "{name}");
        assert!(
            provider.endpoint.is_none(),
            "{name}: primary wiki route fell back to search"
        );
        assert!(provider.token.is_none());
        assert!(
            provider.build_id.is_some(),
            "{name}: HTML did not expose a usable build ID"
        );
        // Check the JSON route directly so HTML fallback cannot hide a broken
        // live endpoint. Mocked tests cover the provider's fallback decisions.
        let body = provider
            .text(
                &format!(
                    "/_next/data/{}/game/{}.json",
                    provider.build_id.as_ref().unwrap(),
                    data.game_id
                ),
                Instant::now() + Duration::from_secs(30),
            )
            .unwrap();
        let json: Value = serde_json::from_str(&body).unwrap();
        let refreshed = verified_game(&json["pageProps"], id, data.game_id)
            .unwrap()
            .unwrap();
        assert_eq!(refreshed.game_id, data.game_id);
        assert!(refreshed.main.is_some());
        println!("PCGamingWiki → {name}: {}", data.url());
    }
}

#[test]
fn http_lookup_checks_headers_payload_identity_and_reuses_endpoint_and_token() {
    let mut steps = discovery();
    steps.push(auth());
    for _ in 0..2 {
        steps.extend([
            Step::post(results(&[7230])),
            Step::get("/game/7230", page(game())),
        ]);
    }
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    for _ in 0..2 {
        assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    }
    let requests = server.finish();
    assert_eq!(requests.len(), 7);
    let request = &requests[3];
    assert!(
        request
            .to_ascii_lowercase()
            .contains("x-auth-token: fixture-token")
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("origin: http://127.0.0.1:")
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("referer: http://127.0.0.1:")
    );
    let payload: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(payload["searchTerms"], json!(["Portal"]));
    assert_eq!(payload["searchType"], "games");
}

#[test]
fn rejected_reused_tokens_are_renewed_without_rediscovering_the_endpoint() {
    for status in [401, 403] {
        let mut steps = discovery();
        steps.extend([
            auth(),
            Step::post(results(&[7230])),
            Step::get("/game/7230", page(game())),
            Step::post("expired token").status(status),
            Step::get("/api/search/site/init?", r#"{"token":"replacement-token"}"#),
            Step::post(results(&[7230])),
            Step::get("/game/7230", page(game())),
        ]);
        let server = Server::start(steps);
        let mut provider = Provider::new(&server.base, None).unwrap();
        for _ in 0..2 {
            assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
        }
        let requests = server.finish();
        assert_eq!(requests.len(), 9);
        assert!(
            requests[7]
                .to_ascii_lowercase()
                .contains("x-auth-token: replacement-token")
        );
    }
}

#[test]
fn persistent_authentication_failures_stop_after_one_renewal_and_rediscovery() {
    let mut steps = discovery();
    steps.extend([
        auth(),
        Step::post(results(&[7230])),
        Step::get("/game/7230", page(game())),
        Step::post("expired token").status(401),
        auth(),
        Step::post("rejected fresh token").status(401),
    ]);
    steps.extend(discovery());
    steps.extend([auth(), Step::post("still rejected").status(401)]);
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert_eq!(
        provider.fetch(400, "Portal", None).unwrap_err().status,
        Some(StatusCode::UNAUTHORIZED)
    );
    assert_eq!(server.finish().len(), 12);
}

#[test]
fn expired_matches_refresh_and_reverify_with_one_page_request() {
    let dir = std::env::temp_dir().join(format!("aurelia-hltb-refresh-{}", std::process::id()));
    let path = dir.join("400.json");
    write_cache(
        &path,
        &Cached {
            version: CACHE_VERSION,
            steam_id: 400,
            fetched_at: DAY,
            data: Some(data()),
        },
    )
    .unwrap();
    let mut updated = game();
    updated["comp_main"] = json!(12345);
    let server = Server::start(vec![Step::get("/game/7230", page(updated))]);
    let mut provider = Provider::new(&server.base, None).unwrap();
    let state = lookup_cached(
        Some(&path),
        400,
        9 * DAY,
        |_| {},
        |game_id| {
            provider
                .fetch(400, "Portal", game_id)
                .map_err(|e| e.to_string())
        },
    );
    assert!(matches!(
        state,
        HltbState::Ready {
            data: HltbData {
                main: Some(12345),
                ..
            },
            stale: false
        }
    ));
    assert_eq!(
        read_cache(&path, 400).unwrap().data.unwrap().main,
        Some(12345)
    );
    assert_eq!(server.finish().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn removed_pages_and_changed_steam_associations_fall_back_to_search() {
    let mut wrong_game = game();
    wrong_game["game_id"] = json!(1);
    wrong_game["profile_steam"] = json!(620);
    wrong_game["profile_steam_alt"] = json!(0);
    for first in [
        Step::get("/game/1", "removed").status(404),
        Step::get("/game/1", page(wrong_game)),
    ] {
        let mut steps = vec![first];
        steps.extend(discovery());
        steps.extend([
            auth(),
            Step::post(results(&[7230])),
            Step::get("/game/7230", page(game())),
        ]);
        let server = Server::start(steps);
        let mut provider = Provider::new(&server.base, None).unwrap();
        assert_eq!(
            provider.fetch(400, "Portal", Some(1)).unwrap(),
            Some(data())
        );
        assert_eq!(server.finish().len(), 6);
    }
}

#[test]
fn failed_direct_refreshes_keep_stale_data_without_search_requests() {
    let dir =
        std::env::temp_dir().join(format!("aurelia-hltb-stale-refresh-{}", std::process::id()));
    let path = dir.join("400.json");
    write_cache(
        &path,
        &Cached {
            version: CACHE_VERSION,
            steam_id: 400,
            fetched_at: DAY,
            data: Some(data()),
        },
    )
    .unwrap();
    for status in [200, 429, 503] {
        let server = Server::start(vec![Step::get("/game/7230", "broken page").status(status)]);
        let mut provider = Provider::new(&server.base, None).unwrap();
        let state = lookup_cached(
            Some(&path),
            400,
            9 * DAY,
            |_| {},
            |game_id| {
                provider
                    .fetch(400, "Portal", game_id)
                    .map_err(|e| e.to_string())
            },
        );
        assert!(matches!(state, HltbState::Ready { stale: true, .. }));
        assert_eq!(read_cache(&path, 400).unwrap().fetched_at, DAY);
        if status == 429 {
            assert!(
                provider
                    .fetch(620, "Portal 2", None)
                    .unwrap_err()
                    .to_string()
                    .contains("wait a minute")
            );
        }
        assert_eq!(server.finish().len(), 1);
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_unreadable_candidate_does_not_hide_a_later_verified_match() {
    let mut steps = discovery();
    steps.extend([
        auth(),
        Step::post(results(&[1, 7230])),
        Step::get("/game/1", "broken page"),
        Step::get("/game/7230", page(game())),
    ]);
    let server = Server::start(steps);
    assert_eq!(
        Provider::new(&server.base, None)
            .unwrap()
            .fetch(400, "Portal", None)
            .unwrap(),
        Some(data())
    );
    server.finish();
}

#[test]
fn candidate_errors_are_not_reported_as_missing_games() {
    let mut steps = discovery();
    steps.extend([
        auth(),
        Step::post(results(&[1])),
        Step::get("/game/1", "broken page"),
    ]);
    let server = Server::start(steps);
    let error = Provider::new(&server.base, None)
        .unwrap()
        .fetch(400, "Portal", None)
        .unwrap_err();
    assert!(error.to_string().contains("page data missing"));
    server.finish();
}

#[test]
fn rate_limits_stop_requests_and_apply_a_session_cooldown() {
    let mut steps = discovery();
    steps.extend([auth(), Step::post("rate limited").status(429)]);
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert_eq!(
        provider.fetch(400, "Portal", None).unwrap_err().status,
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
    assert!(
        provider
            .fetch(620, "Portal 2", None)
            .unwrap_err()
            .to_string()
            .contains("wait a minute")
    );
    assert_eq!(server.finish().len(), 4);
}

#[test]
fn a_changed_endpoint_is_rediscovered_once_after_a_404() {
    let mut steps = discovery();
    steps.push(auth().status(404));
    steps.extend(discovery());
    steps.extend([
        auth(),
        Step::post(results(&[7230])),
        Step::get("/game/7230", page(game())),
    ]);
    let server = Server::start(steps);
    assert!(
        Provider::new(&server.base, None)
            .unwrap()
            .fetch(400, "Portal", None)
            .unwrap()
            .is_some()
    );
    assert_eq!(server.finish().len(), 8);
}

#[test]
fn server_failures_are_not_retried_as_endpoint_changes() {
    let mut steps = discovery();
    steps.extend([auth(), Step::post("unavailable").status(503)]);
    let server = Server::start(steps);
    assert_eq!(
        Provider::new(&server.base, None)
            .unwrap()
            .fetch(400, "Portal", None)
            .unwrap_err()
            .status,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    assert_eq!(server.finish().len(), 4);
}

#[test]
fn a_failed_search_does_not_prevent_a_later_lookup() {
    let mut steps = discovery();
    steps.extend([
        auth(),
        Step::post("unavailable").status(503),
        Step::post(results(&[7230])),
        Step::get("/game/7230", page(game())),
    ]);
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert!(provider.fetch(400, "Portal", None).is_err());
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert_eq!(server.finish().len(), 6);
}

#[test]
fn a_dropped_connection_does_not_prevent_a_later_lookup() {
    let mut steps = discovery();
    steps.extend([
        auth(),
        Step::post("").status(0),
        Step::post(results(&[7230])),
        Step::get("/game/7230", page(game())),
    ]);
    let server = Server::start(steps);
    let mut provider = Provider::new(&server.base, None).unwrap();
    assert!(provider.fetch(400, "Portal", None).is_err());
    assert_eq!(provider.fetch(400, "Portal", None).unwrap(), Some(data()));
    assert_eq!(server.finish().len(), 6);
}

#[test]
fn schema_errors_empty_results_and_deadlines_are_distinct() {
    for (response, is_error) in [
        (r#"{"data":[]}"#, false),
        (r#"{"unexpected":[]}"#, true),
        ("not JSON", true),
    ] {
        let mut steps = discovery();
        steps.extend([auth(), Step::post(response)]);
        let server = Server::start(steps);
        let result = Provider::new(&server.base, None)
            .unwrap()
            .fetch(400, "Portal", None);
        if is_error {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_none());
        }
        server.finish();
    }
    let provider = Provider::new(BASE, None).unwrap();
    assert!(
        provider
            .request("/", Instant::now() - Duration::from_secs(1))
            .is_err()
    );
}

#[test]
#[ignore = "requires live HowLongToBeat access"]
fn live_steam_matches() {
    let mut provider = Provider::new(BASE, None).unwrap();
    for (id, name) in [
        (400, "Portal"),
        (1306630, "Lost Ruins"),
        (599140, "Graveyard Keeper"),
    ] {
        let previous_token = provider.token.clone();
        let data = provider
            .fetch(id, name, None)
            .unwrap()
            .expect("verified HLTB match");
        if previous_token.is_some() {
            assert!(
                provider.token == previous_token,
                "{name}: token was renewed unexpectedly"
            );
        }
        assert!(data.main.is_some(), "{name}");
        let mut refresh = Provider::new(BASE, None).unwrap();
        let refreshed = refresh
            .fetch(id, name, Some(data.game_id))
            .unwrap()
            .expect("direct refresh");
        assert_eq!(refreshed.game_id, data.game_id);
        assert!(refreshed.main.is_some());
        assert!(
            refresh.endpoint.is_none(),
            "{name}: direct refresh fell back to search"
        );
        println!(
            "{name}: {:?} / {:?} / {:?} seconds ({})",
            data.main,
            data.extra,
            data.completionist,
            data.url()
        );
    }
}

#[test]
fn missing_tokens_do_not_trigger_a_second_discovery_or_a_search() {
    let mut steps = discovery();
    steps.push(Step::get("/api/search/site/init?", "{}"));
    let server = Server::start(steps);
    assert!(
        Provider::new(&server.base, None)
            .unwrap()
            .fetch(400, "Portal", None)
            .unwrap_err()
            .to_string()
            .contains("token missing")
    );
    assert_eq!(server.finish().len(), 3);
}

#[test]
fn a_slow_response_obeys_the_remaining_lookup_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buffer = [0u8; 4096];
        assert!(socket.read(&mut buffer).unwrap() > 0);
        thread::sleep(Duration::from_millis(150));
        let _ = socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    });
    let provider = Provider::new(&base, None).unwrap();
    let response = provider
        .request("/", Instant::now() + Duration::from_millis(30))
        .unwrap()
        .send();
    let error = Provider::body(response, "slow fixture").unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
    worker.join().unwrap();
}

//! `chat invite` against a stub.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use aurelia_tui::interface::social;

#[test]
fn invite_refusal_surfaces_aurelia_message() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("social-invite-stub");
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = format!(
        "{}/tests/fixtures/social/invite_refused.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let args_log = dir.join("args.txt");
    let stub = dir.join("aurelia");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat '{}'\n",
            args_log.display(),
            fixture
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    unsafe {
        std::env::set_var("AURELIA_BIN", &stub);
    }

    let err = social::chat_invite(76561197960287930, 440, Some("+connect 1.2.3.4"))
        .expect_err("refusal is an error")
        .to_string();
    assert!(err.contains("isn't running on your account"), "{err}");

    let args = std::fs::read_to_string(&args_log).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(
        args,
        [
            "chat",
            "invite",
            "76561197960287930",
            "--app",
            "440",
            "--connect",
            "+connect 1.2.3.4",
            "--json"
        ]
    );
}

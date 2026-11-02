#!/usr/bin/env python3
"""Linux terminal smoke test with a fake Steam backend and browser launcher.

Usage: python3 scripts/test-hltb-terminal.py target/release/aurelia-tui
Needs no Steam login and uses a seeded HLTB cache. The existing ProtonDB lookup
may still make a background request; no real browser or game is launched.
"""
import codecs
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import socket
import struct
import sys
import tempfile
import termios
import time


class Screen:
    """Apply the cursor/erase commands used by the TUI's incremental redraws."""

    def __init__(self, width=120, height=32):
        self.width, self.height = width, height
        self.rows = [[" "] * width for _ in range(height)]
        self.x = self.y = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.pending = ""

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith("\x1b"):
                match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.pending)
                if match is None:
                    # Reads can split an escape sequence between its bytes.
                    return
                self.pending = self.pending[match.end():]
                parameters, _, command = match.groups()
                if command not in "HfABCDEFGdJK":
                    continue  # Colours, styles and cursor visibility.
                values = [int(value or 0) for value in parameters.split(";")]
                count = values[0] or 1
                if command in "Hf":
                    self.y = (values[0] or 1) - 1
                    self.x = ((values[1] or 1) if len(values) > 1 else 1) - 1
                elif command == "A":
                    self.y -= count
                elif command == "B":
                    self.y += count
                elif command == "C":
                    self.x += count
                elif command == "D":
                    self.x -= count
                elif command in "EF":
                    self.y += count if command == "E" else -count
                    self.x = 0
                elif command == "G":
                    self.x = count - 1
                elif command == "d":
                    self.y = count - 1
                elif command == "J" and values[0] in (2, 3):
                    self.rows = [[" "] * self.width for _ in range(self.height)]
                elif command == "K":
                    start = 0 if values[0] in (1, 2) else self.x
                    end = self.x + 1 if values[0] == 1 else self.width
                    self.rows[self.y][start:end] = [" "] * (end - start)
                self.x = min(max(self.x, 0), self.width - 1)
                self.y = min(max(self.y, 0), self.height - 1)
                continue
            character, self.pending = self.pending[0], self.pending[1:]
            if character == "\r":
                self.x = 0
            elif character == "\n":
                self.y = min(self.y + 1, self.height - 1)
            elif character >= " ":
                self.rows[self.y][self.x] = character
                self.x = min(self.x + 1, self.width - 1)

    def text(self):
        return "\n".join("".join(row) for row in self.rows).replace(" ", "").encode()


def run(binary, action, fail_launcher):
    with tempfile.TemporaryDirectory(prefix="aurelia-hltb-terminal-") as directory:
        root = Path(directory)
        game = {
            "id": 400, "name": "Portal", "developer": "Valve", "publisher": "Valve",
            "homepage": "https://store.steampowered.com/app/400", "game_type": "Game",
            "icon_url": None, "cover_url": None, "installed": True,
            "install_dir": "/games/Portal", "update_available": False, "size": 0,
        }
        games = [game]
        if action in ("offline", "cold"):
            games.append(dict(game, id=620, name="Portal 2"))
        (root / "games.json").write_text(json.dumps(games))
        if action == "disabled":
            (root / "config.json").write_text(json.dumps({
                "default_user": "", "hidden_games": [], "favorite_games": [],
                "allowed_games": ["Game", "DLC"], "highlight": {"Rgb": [102, 192, 244]},
                "hltb_enabled": False,
            }))
        (root / "hltb").mkdir()
        (root / "hltb/400.json").write_text(json.dumps({
            "version": 1, "steam_id": 400, "fetched_at": int(time.time()),
            "data": {"game_id": 7230, "game_name": "Portal", "main": 11230,
                     "extra": 18943, "completionist": 37927},
        }))
        if action == "no-match":
            entry = json.loads((root / "hltb/400.json").read_text())
            entry["data"] = None
            (root / "hltb/400.json").write_text(json.dumps(entry))
        if action == "stale":
            entry = json.loads((root / "hltb/400.json").read_text())
            entry["fetched_at"] -= 8 * 86400
            (root / "hltb/400.json").write_text(json.dumps(entry))
        # A local proxy either refuses or holds connections, without contacting
        # HLTB or depending on the machine's actual network connectivity.
        offline_proxy = None
        if action in ("offline", "cold", "stale"):
            if action != "stale":
                (root / "hltb/400.json").unlink()
            offline_proxy = socket.socket()
            offline_proxy.bind(("127.0.0.1", 0))
            if action == "cold":
                # Hold the proxy handshake open while checking the initial UI.
                offline_proxy.listen(2)
        backend = root / "fake-aurelia"
        backend.write_text(f"#!{sys.executable}\n" + '''
import json, sys
if '--version' in sys.argv:
    print('aurelia 0.1.40')
elif sys.argv[1] == 'login':
    print(json.dumps({'logged_in': True, 'account': 'Terminal test'}))
elif sys.argv[1] == 'info':
    print(json.dumps({'app_id': 400, 'name': 'Portal', 'developers': ['Valve'],
                      'publishers': ['Valve'], 'description': 'Aperture Science offers puzzles.'}))
elif sys.argv[1] == 'config':
    print(json.dumps({'error': 'fixture launcher settings unavailable'}))
    sys.exit(1)
else:
    print('{}')
''')
        backend.chmod(0o755)
        opener = root / "xdg-open"
        opener.write_text(
            f"#!{sys.executable}\nimport sys\nfrom pathlib import Path\n"
            f"Path({str(root / 'opened-url')!r}).write_text(sys.argv[1])\n"
            f"sys.exit({int(fail_launcher)})\n"
        )
        opener.chmod(0o755)
        pid, terminal = pty.fork()
        if pid == 0:
            environment = dict(os.environ, AURELIA_BIN=str(backend),
                               AURELIA_TUI_DIR=str(root), AURELIA_TUI_CACHE_DIR=str(root),
                               TERM="xterm-256color", PATH=f"{root}:{os.environ['PATH']}")
            if offline_proxy is not None:
                proxy = f"http://127.0.0.1:{offline_proxy.getsockname()[1]}"
                environment.update(HTTPS_PROXY=proxy, https_proxy=proxy,
                                   HTTP_PROXY=proxy, http_proxy=proxy,
                                   ALL_PROXY=proxy, all_proxy=proxy,
                                   NO_PROXY="", no_proxy="")
            os.execve(binary, [binary], environment)
        fcntl.ioctl(terminal, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))
        output = bytearray()
        screen = Screen()

        def read_until(predicate):
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                if select.select([terminal], [], [], 0.1)[0]:
                    try:
                        data = os.read(terminal, 65536)
                        output.extend(data)
                        screen.feed(data)
                    except OSError:
                        break
                if predicate(screen.text()):
                    return
            raise AssertionError(f"terminal check failed: {bytes(output[-1500:])!r}")

        def config_data():
            # A concurrent fs::write may briefly expose an empty config file.
            try:
                return json.loads((root / "config.json").read_text())
            except (FileNotFoundError, json.JSONDecodeError):
                return {}

        def escape():
            # Send Escape separately so a terminal parser cannot interpret the
            # following letter as one Alt+letter event.
            os.write(terminal, b"\x1b")
            time.sleep(0.1)

        try:
            if action == "cold":
                read_until(lambda text: b"Aperture" in text)
                assert b"HLTB" not in screen.text()
                assert b"Loading" not in screen.text()
                os.write(terminal, b"jf")
                read_until(lambda _: 620 in config_data().get("favorite_games", []))
                assert not (root / "opened-url").exists()
                print("PASS: first lookup has no loading row; navigation stays available")
                return
            if action == "no-match":
                read_until(lambda text: b"Noverifiedmatch" in text and b"Aperture" in text)
                os.write(terminal, b",ct")
                read_until(lambda text: b"HLTB:noverifiedSteammatch" in text)
                assert not (root / "opened-url").exists()
                print("PASS: verified miss stays distinct from Fetch Failed")
                return
            if action == "offline":
                read_until(lambda text: b"FetchFailed" in text and b"Aperture" in text)
                os.write(terminal, b",ct")
                read_until(lambda text: b"retryrequested" in text)
                os.write(terminal, b"jf")
                read_until(lambda _: 620 in config_data().get("favorite_games", []))
                assert not (root / "hltb/400.json").exists()
                assert not (root / "opened-url").exists()
                print("PASS: offline failure, Fetch Failed, retry and library navigation")
                return
            if action == "disabled":
                read_until(lambda text: b"Aperture" in text)
                os.write(terminal, b",ct")
                read_until(lambda text: b"HLTBisdisabled" in text)
                assert not (root / "opened-url").exists()
                assert config_data().get("hltb_enabled") is False
                print("PASS: startup disabled, cached browser action blocked")
                return
            read_until(lambda text: all(value in text for value in
                                       [b"3h7m", b"5h16m", b"10h32m", b"Aperture"]))
            if action == "stale":
                read_until(lambda text: b"Cached" in text)
                assert b"FetchFailed" not in screen.text()
                assert json.loads((root / "hltb/400.json").read_text())["fetched_at"] == entry["fetched_at"]
                os.write(terminal, b",ct")
            elif action == "chord":
                # Bare t and incomplete chords must not open a browser. A
                # favourite save confirms subsequent keys were processed.
                os.write(terminal, b"tf")
                read_until(lambda _: 400 in config_data().get("favorite_games", []))
                assert not (root / "opened-url").exists()
                os.write(terminal, b",c")
                read_until(lambda text: b"Completiontime:presst" in text)
                escape()
                os.write(terminal, b"tf")
                read_until(lambda _: 400 not in config_data().get("favorite_games", []))
                assert not (root / "opened-url").exists()
                # Opening a modal consumes the pending prefix, and filtering
                # treats the entire chord as ordinary text.
                os.write(terminal, b",cp")
                read_until(lambda text: b"Launchersettingsunavailable" in text)
                escape()
                os.write(terminal, b"t/,ct")
                read_until(lambda text: b"/,ct" in text)
                assert not (root / "opened-url").exists()
                escape()
                os.write(terminal, b",ct")
            elif action == "toggle":
                # Local settings must work even though the fake launcher fails
                # config reads. While editing a proxy, h remains ordinary text.
                os.write(terminal, b"p")
                read_until(lambda text: b"HowLongToBeat" in text and b"Launchersettingsunavailable" in text)
                os.write(terminal, b"eh")
                read_until(lambda text: b"h_" in text)
                assert config_data().get("hltb_enabled") is True
                escape()
                os.write(terminal, b"h")
                read_until(lambda _: config_data().get("hltb_enabled") is False)
                escape()
                os.write(terminal, b",ct")
                read_until(lambda text: b"HLTBisdisabled" in text)
                assert not (root / "opened-url").exists()
                # Other settings writes must retain the switch's new value;
                # this catches an out-of-date config copy in the event loop.
                os.write(terminal, b"f")
                read_until(lambda _: 400 in config_data().get("favorite_games", []))
                assert config_data().get("hltb_enabled") is False
                os.write(terminal, b"ph")
                read_until(lambda _: config_data().get("hltb_enabled") is True)
                escape()
                os.write(terminal, b",ct")
            elif action == "menu":
                os.write(terminal, b"\r")
                read_until(lambda text: b"HowLongToBeatpage" in text)
                os.write(terminal, b"howlongtobeat\r")
            else:
                os.write(terminal, b",ct")
            read_until(lambda _: (root / "opened-url").exists())
            assert (root / "opened-url").read_text() == "https://howlongtobeat.com/game/7230"
            if fail_launcher:
                read_until(lambda text: b"Browserlauncherexited" in text)
            print(f"PASS: {action}, launcher {'failure notice' if fail_launcher else 'success'}")
        finally:
            # A failed assertion may leave an editor or overlay open. Escape
            # both before q so teardown does not type q into the edit buffer.
            escape()
            escape()
            os.write(terminal, b"q")
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                exited, status = os.waitpid(pid, os.WNOHANG)
                if exited:
                    break
                if select.select([terminal], [], [], 0.05)[0]:
                    try:
                        os.read(terminal, 65536)
                    except OSError:
                        pass
            else:
                os.kill(pid, signal.SIGTERM)
                os.waitpid(pid, 0)
                raise AssertionError("TUI did not exit after q")
            os.close(terminal)
            if offline_proxy is not None:
                offline_proxy.close()
            assert os.WIFEXITED(status) and os.WEXITSTATUS(status) == 0, f"TUI exit status: {status}"


if __name__ == "__main__":
    if not sys.platform.startswith("linux"):
        raise SystemExit("This smoke test uses Linux ptys and an xdg-open stub.")
    executable = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/aurelia-tui").resolve())
    for method in ["shortcut", "menu"]:
        for fails in [False, True]:
            run(executable, method, fails)
    run(executable, "toggle", False)
    run(executable, "disabled", False)
    run(executable, "offline", False)
    run(executable, "chord", False)
    run(executable, "no-match", False)
    run(executable, "cold", False)
    run(executable, "stale", False)

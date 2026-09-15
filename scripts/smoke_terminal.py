"""Launch the actual TUI in ConPTY; keep this separate from synthetic metric tests."""
import json
from pathlib import Path
import sys
import threading
import time

from winpty import PtyProcess

exe = str(Path(sys.argv[1]).resolve())
out = Path("test-output")
out.mkdir(exist_ok=True)
proc = PtyProcess.spawn([exe, *sys.argv[2:]], dimensions=(40, 160))
chunks = []
errors = []


def drain():
    try:
        while True:
            text = proc.read(8192)
            if text:
                chunks.append(text)
            elif not proc.isalive():
                return
    except EOFError:
        pass
    except Exception as exc:
        errors.append(str(exc))


reader = threading.Thread(target=drain, daemon=True)
reader.start()
try:
    time.sleep(3)
    assert proc.isalive(), "TUI exited before the first metric samples"
    # Preserve a frame from the actual terminal stream, before interaction tests.
    if '--apps' in sys.argv:
        from render_terminal import render_svg
        render_svg(''.join(chunks), out / 'city-live-terminal.svg', 160, 40)
    # Exercise the existing simulation and input loop, then request normal exit.
    for key in ("r", "s", "d", "d", "\t", "\x1b[C", "\x1b[6~", "\x1b[5~", "\x1b"):
        proc.write(key)
        time.sleep(0.3)
    if '--apps' in sys.argv:
        assert 'KERNEL CITY' in ''.join(chunks), 'Application footer not rendered'
        assert 'Application' in ''.join(chunks), 'Selection panel not rendered'
    for rows, columns in [(10, 30), (20, 70), (40, 160)]:
        proc.setwinsize(rows, columns)
        time.sleep(0.3)
        assert proc.isalive(), 'Resize crashed the city'
    proc.write("q")
    deadline = time.monotonic() + 10
    while proc.isalive() and time.monotonic() < deadline:
        time.sleep(0.05)
    assert not proc.isalive(), "TUI did not respond to q"
    reader.join(timeout=2)
    capture = "".join(chunks)
    (out / ("terminal-apps.ansi" if "--apps" in sys.argv else "terminal-classic.ansi")).write_text(capture, encoding="utf-8")
    result = {
        "exit_status": proc.exitstatus,
        "captured_characters": len(capture),
        "reader_errors": errors,
        "terminal": "160 columns x 40 rows",
        "inputs": ["weather", "debug", "select", "page", "resize", "q"],
    }
    (out / ("terminal-apps-result.json" if "--apps" in sys.argv else "terminal-classic-result.json")).write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))
    assert proc.exitstatus == 0, "TUI returned an error"
    assert len(capture) > 1000 and "\x1b[" in capture, "No rendered city received"
finally:
    if proc.isalive():
        proc.terminate(force=True)

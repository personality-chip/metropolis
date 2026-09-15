"""Optional acceptance check with the runner's real Chrome installation."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tomllib

out = Path('test-output')
out.mkdir(exist_ok=True)
candidates = [Path(os.environ.get(base, 'C:/')) / 'Google/Chrome/Application/chrome.exe'
              for base in ['PROGRAMFILES', 'PROGRAMFILES(X86)', 'LOCALAPPDATA']]
chrome = next((p for p in candidates if p.is_file()), None)
if chrome is None:
    report = {'tested': False, 'reason': 'Chrome is not installed on this runner'}
else:
    with tempfile.TemporaryDirectory(prefix='kernel-city-chrome-') as profile:
        browser = subprocess.Popen([str(chrome), '--headless=new', '--no-first-run',
            '--no-default-browser-check', '--disable-background-networking',
            '--user-data-dir=' + profile, 'about:blank'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            time.sleep(2)
            assert browser.poll() is None, 'Installed Chrome did not stay running'
            result = subprocess.run([sys.argv[1], '--snapshot', '--samples', '3'],
                capture_output=True, text=True, encoding='utf-8', timeout=20, check=True)
            frames = [tomllib.loads(c) for c in result.stdout.split('===KERNEL_CITY_SNAPSHOT===')[1:]]
            groups = [a for f in frames for a in f['apps'] if a['id'] == 'app:chrome']
            assert groups and any(browser.pid in [p['pid'] for p in a['processes']] and len(a['processes']) >= 2 for a in groups)
            report = {'tested': True, 'executable': str(chrome), 'launched_pid': browser.pid,
                'maximum_group_processes': max(len(a['processes']) for a in groups),
                'maximum_ram_bytes': max(a['ram_bytes'] for a in groups)}
        finally:
            # Only the tree launched by this test, never an executable-name-wide kill.
            if browser.poll() is None:
                subprocess.run(['taskkill', '/PID', str(browser.pid), '/T', '/F'], capture_output=True, timeout=10)
                browser.wait(timeout=5)
            time.sleep(1)
(out / 'installed-chrome-result.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
print(json.dumps(report, indent=2))

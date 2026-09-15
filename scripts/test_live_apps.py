"""Exercise Windows sysinfo + ETW with real PIDs and known, bounded workloads.
Executable aliases are fixtures, not an installation of Chrome, Steam or VS Code.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib

exe = str(Path(sys.argv[1]).resolve())
fixture = Path(sys.argv[2]).resolve()
out = Path('test-output')
out.mkdir(exist_ok=True)
processes = []
controls = []
expected = {}
capture = None
ui = None
capture_file = (out / 'live-snapshots.toml.txt').open('w', encoding='utf-8')
with tempfile.TemporaryDirectory(prefix='kernel-city-') as directory:
    root = Path(directory)
    try:
        capture = subprocess.Popen([exe, '--snapshot', '--samples', '17'], stdout=capture_file, stderr=subprocess.PIPE, text=True, encoding='utf-8')
        time.sleep(1.5)
        for app, main, helper in [('chrome', 'chrome.exe', 'chrome.exe'), ('steam', 'steam.exe', 'steamwebhelper.exe'), ('vscode', 'Code.exe', 'node.exe')]:
            folder = root / app
            folder.mkdir()
            main_path, helper_path = folder / main, folder / helper
            shutil.copyfile(fixture, main_path)
            if helper_path != main_path:
                shutil.copyfile(fixture, helper_path)
            control = folder / 'control.txt'
            control.write_text('idle')
            controls.append(control)
            process = subprocess.Popen([str(main_path), str(control), str(helper_path)], stdout=subprocess.PIPE, text=True)
            processes.append(process)
            child_pid = int(process.stdout.readline().strip())
            expected[f'app:{app}'] = {process.pid, child_pid}
        time.sleep(3)
        for control in controls:
            control.write_text('active')
        ui = subprocess.Popen([sys.executable, 'scripts/smoke_terminal.py', exe, '--apps'])
        time.sleep(5)
        controls[0].write_text('stop')
        processes[0].wait(timeout=8)
        assert ui.wait(timeout=20) == 0, 'Windows app interaction smoke failed'
        _, stderr = capture.communicate(timeout=30)
        capture_file.close()
        stdout = (out / 'live-snapshots.toml.txt').read_text(encoding='utf-8')
        assert capture.returncode == 0, stderr
        frames = [tomllib.loads(chunk) for chunk in stdout.split('===KERNEL_CITY_SNAPSHOT===')[1:]]
        records = {app: [] for app in expected}
        for frame in frames:
            for app in frame['apps']:
                if app['id'] in expected and expected[app['id']].issubset({p['pid'] for p in app['processes']}):
                    records[app['id']].append(app)
            lots = {lot['app_id']: lot['slot'] for lot in frame.get('lots', [])}
            for app, slot in [('app:chrome', 0), ('app:steam', 1), ('app:vscode', 2)]:
                if app in lots:
                    assert lots[app] == slot, 'Application moved between lots'
        report = {'fixture_aliases': True, 'frames': len(frames), 'apps': {}}
        for app, samples in records.items():
            assert len(samples) >= 5, f'{app} did not aggregate both real PIDs: {len(samples)}'
            peak_cpu = max(m['cpu_percent'] for m in samples)
            ram_growth = max(m['ram_bytes'] for m in samples) - min(m['ram_bytes'] for m in samples)
            peak_io = max(m['io_write_bps'] for m in samples)
            disk = [m['disk_write_bps'] for m in samples if 'disk_write_bps' in m]
            assert peak_cpu > 0.1, f'{app}: CPU did not react'
            assert ram_growth > 64 * 1024 * 1024, f'{app}: RAM did not react'
            assert peak_io > 100_000, f'{app}: I/O did not react'
            assert disk and max(disk) > 0, f'{app}: ETW did not attribute writes; {[f["disk_status"] for f in frames]}'
            report['apps'][app] = {'pids': sorted(expected[app]), 'peak_cpu_percent': peak_cpu, 'ram_growth_bytes': ram_growth,
                'peak_io_write_bps': peak_io, 'peak_disk_write_bps': max(disk)}
        assert any(lot['app_id'] == 'app:chrome' and not lot['alive'] for f in frames for lot in f.get('lots', [])), 'Exit never dimmed Chrome'
        assert not any(lot['app_id'] == 'app:chrome' for lot in frames[-1].get('lots', [])), 'Exited Chrome lot never disappeared'
        (out / 'live-result.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps(report, indent=2))
    finally:
        for control in controls:
            control.write_text('stop')
        for process in processes:
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
        if capture is not None and capture.poll() is None:
            capture.kill()
            capture.wait(timeout=5)

        capture_file.close()

        if ui is not None and ui.poll() is None:
            ui.kill()
            ui.wait(timeout=5)

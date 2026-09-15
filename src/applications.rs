//! Real process samples, independent of city layout and rendering.
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use sysinfo::System;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessKey {
    pub pid: u32,
    pub started: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessMetrics {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub cpu_percent: f32,
    pub ram_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppMetrics {
    pub id: String,
    pub name: String,
    pub cpu_percent: f32,
    pub ram_bytes: u64,
    pub io_read_bps: f64,
    pub io_write_bps: f64,
    pub disk_read_bps: Option<f64>,
    pub disk_write_bps: Option<f64>,
    pub processes: Vec<ProcessMetrics>,
}

impl AppMetrics {
    pub fn disk_bps(&self) -> f64 {
        self.disk_read_bps.unwrap_or(0.0) + self.disk_write_bps.unwrap_or(0.0)
    }
}

#[derive(Default)]
pub struct DiskFrame {
    pub available: bool,
    pub rates: HashMap<ProcessKey, (f64, f64)>,
    pub status: String,
}

#[derive(Clone)]
struct RawProcess {
    key: ProcessKey,
    parent: Option<u32>,
    name: String,
    exe: String,
    cpu: f32,
    ram: u64,
    read: u64,
    write: u64,
}

#[derive(Clone)]
struct Identity { id: String, name: String }
struct Previous { identity: Identity, exe: String, read: u64, write: u64 }

#[derive(Default)]
pub struct ApplicationCollector {
    previous: HashMap<ProcessKey, Previous>,
}

fn normalized(path: &str) -> String {
    // Windows paths are case insensitive; Linux paths remain case sensitive.
    let path = path.replace('\\', "/");
    if cfg!(windows) || path.as_bytes().get(1) == Some(&b':') {
        path.to_lowercase()
    } else { path }
}

fn stem(p: &RawProcess) -> String {
    let name = p.exe.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(&p.name);
    name.to_lowercase().trim_end_matches(".exe").to_string()
}

fn known(p: &RawProcess) -> Option<Identity> {
    let (id, name) = match stem(p).as_str() {
        "chrome" => ("chrome", "Chrome"),
        "steam" | "steamwebhelper" | "steamservice" => ("steam", "Steam"),
        "code" => ("vscode", "VS Code"),
        "code - insiders" => ("vscode-insiders", "VS Code Insiders"),
        "msedge" => ("edge", "Edge"),
        "firefox" => ("firefox", "Firefox"),
        "telegram" => ("telegram", "Telegram"),
        _ => return None,
    };
    Some(Identity { id: format!("app:{id}"), name: name.into() })
}

fn base_identity(p: &RawProcess) -> Identity {
    known(p).unwrap_or_else(|| Identity {
        id: if p.exe.is_empty() { format!("name:{}", stem(p)) } else { format!("exe:{}", p.exe) },
        name: p.name.trim_end_matches(".exe").chars().filter(|c| !c.is_control()).collect(),
    })
}

fn launcher(p: &RawProcess) -> bool {
    matches!(stem(p).as_str(), "explorer" | "system" | "services" | "svchost" |
        "wininit" | "winlogon" | "cmd" | "powershell" | "pwsh" | "bash" | "sh" |
        "zsh" | "windowsterminal" | "wt")
}

impl ApplicationCollector {
    fn identity(&self, p: &RawProcess, all: &HashMap<u32, &RawProcess>) -> Identity {
        if let Some(id) = known(p) { return id; }
        // Keep surviving helpers attached after the parent exits. A PID with a new
        // creation time never inherits another process's counters or identity.
        if let Some(old) = self.previous.get(&p.key).filter(|old| old.exe == p.exe) {
            return old.identity.clone();
        }
        if launcher(p) { return base_identity(p); }
        let mut cursor = p;
        let mut identity = base_identity(p);
        let mut seen = HashSet::from([p.key.pid]);
        while let Some(parent) = cursor.parent.and_then(|pid| all.get(&pid)).copied() {
            if !seen.insert(parent.key.pid) || parent.key.started > cursor.key.started || launcher(parent) { break; }
            let id = self.previous.get(&parent.key).filter(|old| old.exe == parent.exe)
                .map(|old| old.identity.clone()).unwrap_or_else(|| base_identity(parent));
            // Steam launches independent games. Do not turn a game into Steam.
            if id.id == "app:steam" { break; }
            let same_dir = !p.exe.is_empty() && p.exe.rsplit_once('/').map(|v| v.0)
                == parent.exe.rsplit_once('/').map(|v| v.0);
            let helper = matches!(stem(p).as_str(), "node" | "conhost" | "crashpad_handler" |
                "chrome_crashpad_handler" | "code_helper" | "rg" | "ptyhost");
            if same_dir || helper {
                identity = id;
                // A fresh helper may itself have a parent. Walk all the way to
                // the application root instead of caching a helper-only group.
                if known(parent).is_some() || self.previous.get(&parent.key).is_some_and(|old| old.exe == parent.exe) {
                    return identity;
                }
            }
            cursor = parent;
        }
        identity
    }

    pub fn sample(&mut self, sys: &System, seconds: f64, disk: &DiskFrame) -> Vec<AppMetrics> {
        let raw: Vec<_> = sys.processes().iter().filter(|(pid, _)| pid.as_u32() != 0).map(|(pid, p)| {
            let io = p.disk_usage();
            RawProcess {
                key: ProcessKey { pid: pid.as_u32(), started: p.start_time() },
                parent: p.parent().map(|pid| pid.as_u32()), name: p.name().into(),
                exe: normalized(&p.exe().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
                cpu: p.cpu_usage(), ram: p.memory(), read: io.total_read_bytes, write: io.total_written_bytes,
            }
        }).collect();
        self.aggregate(&raw, sys.cpus().len(), seconds, disk)
    }

    fn aggregate(&mut self, raw: &[RawProcess], cores: usize, seconds: f64, disk: &DiskFrame) -> Vec<AppMetrics> {
        let seconds = if seconds.is_finite() && seconds > 0.0 { seconds } else { 1.0 };
        let all: HashMap<_, _> = raw.iter().map(|p| (p.key.pid, p)).collect();
        let mut apps = BTreeMap::<String, AppMetrics>::new();
        let mut previous = HashMap::new();
        for p in raw {
            let id = self.identity(p, &all);
            let app = apps.entry(id.id.clone()).or_insert_with(|| AppMetrics {
                id: id.id.clone(), name: id.name.clone(), cpu_percent: 0.0, ram_bytes: 0,
                io_read_bps: 0.0, io_write_bps: 0.0,
                disk_read_bps: disk.available.then_some(0.0), disk_write_bps: disk.available.then_some(0.0),
                processes: Vec::new(),
            });
            let cpu = if p.cpu.is_finite() { (p.cpu / cores.max(1) as f32).clamp(0.0, 100.0) } else { 0.0 };
            app.cpu_percent = (app.cpu_percent + cpu).min(100.0);
            app.ram_bytes = app.ram_bytes.saturating_add(p.ram);
            if let Some(old) = self.previous.get(&p.key).filter(|old| old.exe == p.exe) {
                app.io_read_bps += p.read.saturating_sub(old.read) as f64 / seconds;
                app.io_write_bps += p.write.saturating_sub(old.write) as f64 / seconds;
            }
            if disk.available {
                let (read, write) = disk.rates.get(&p.key).copied().unwrap_or_default();
                *app.disk_read_bps.as_mut().unwrap() += read;
                *app.disk_write_bps.as_mut().unwrap() += write;
            }
            app.processes.push(ProcessMetrics { pid: p.key.pid, parent_pid: p.parent,
                name: p.name.clone(), cpu_percent: cpu, ram_bytes: p.ram });
            previous.insert(p.key, Previous { identity: id, exe: p.exe.clone(), read: p.read, write: p.write });
        }
        self.previous = previous;
        let mut apps: Vec<_> = apps.into_values().collect();
        for app in &mut apps { app.processes.sort_by_key(|p| p.pid); }
        apps.sort_by(|a, b| b.ram_bytes.cmp(&a.ram_bytes).then(a.id.cmp(&b.id)));
        apps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(pid: u32, parent: Option<u32>, exe: &str) -> RawProcess {
        RawProcess { key: ProcessKey { pid, started: pid as u64 }, parent,
            name: exe.rsplit('/').next().unwrap().into(), exe: normalized(exe),
            cpu: 40.0, ram: 1024, read: 100_000, write: 200_000 }
    }
    #[test]
    fn chrome_steam_code_are_three_groups_with_helpers() {
        let raw = vec![p(10,None,"C:/Chrome/chrome.exe"),p(11,Some(10),"C:/Chrome/chrome.exe"),
            p(20,None,"C:/Steam/steam.exe"),p(21,Some(20),"C:/Steam/bin/steamwebhelper.exe"),
            p(30,None,"C:/Code/Code.exe"),p(31,Some(30),"C:/Code/node.exe")];
        let apps = ApplicationCollector::default().aggregate(&raw,4,1.0,&DiskFrame::default());
        assert_eq!(apps.len(),3);
        for app in apps { assert_eq!(app.processes.len(),2); assert_eq!(app.ram_bytes,2048);
            assert_eq!(app.cpu_percent,20.0); assert_eq!(app.io_write_bps,0.0); assert_eq!(app.disk_write_bps,None); }
    }
    #[test]
    fn fresh_nested_helpers_attach_to_the_application_root() {
        let raw = vec![p(30,None,"C:/Code/Code.exe"), p(31,Some(30),"C:/Code/bin/node.exe"),
            p(32,Some(31),"C:/Code/tools/rg.exe")];
        let apps = ApplicationCollector::default().aggregate(&raw,4,1.0,&DiskFrame::default());
        assert_eq!(apps.len(),1);
        assert_eq!(apps[0].id,"app:vscode");
        assert_eq!(apps[0].processes.len(),3);
    }
    #[test]
    fn steady_io_is_not_differenced_twice_and_pid_reuse_resets_baseline() {
        let mut c = ApplicationCollector::default(); let mut raw = vec![p(10,None,"a.exe")];
        c.aggregate(&raw,1,2.0,&DiskFrame::default());
        for _ in 0..3 { raw[0].write += 4096;
            assert_eq!(c.aggregate(&raw,1,2.0,&DiskFrame::default())[0].io_write_bps,2048.0); }
        raw[0].key.started += 100;
        assert_eq!(c.aggregate(&raw,1,2.0,&DiskFrame::default())[0].io_write_bps,0.0);
    }
    #[test]
    fn orphan_helpers_remain_attached_but_games_are_independent() {
        let mut c = ApplicationCollector::default();
        let code = p(30,None,"C:/Code/Code.exe"); let node = p(31,Some(30),"C:/Code/node.exe");
        c.aggregate(&[code,node.clone()],4,1.0,&DiskFrame::default());
        assert_eq!(c.aggregate(&[node],4,1.0,&DiskFrame::default())[0].id,"app:vscode");
        let apps = c.aggregate(&[p(20,None,"C:/Steam/steam.exe"),p(21,Some(20),"C:/Steam/game.exe")],4,1.0,&DiskFrame::default());
        assert_eq!(apps.len(),2);
    }
    #[test]
    fn parents_are_generation_checked_and_paths_do_not_collide() {
        let mut parent = p(10,None,"C:/Code/Code.exe"); parent.key.started = 500;
        let apps = ApplicationCollector::default().aggregate(&[parent,p(11,Some(10),"C:/Code/node.exe"),
            p(12,Some(13),"C:/A/tool.exe"),p(13,Some(12),"C:/B/tool.exe")],0,0.0,&DiskFrame::default());
        assert_eq!(apps.len(),4);
    }
}

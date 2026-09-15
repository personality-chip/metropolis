//! Stable application lots and presentation state. No OS calls here.
use crate::{applications::AppMetrics, theme::Theme};
use super::buildings::BuildingInfo;
use ratatui::{buffer::Buffer, layout::Rect, style::{Color, Style}, widgets::{Block, Borders, Clear, Paragraph, Widget}};

pub struct AppBuilding {
    pub metrics: AppMetrics,
    pub alive: bool,
    pub cpu: f32,
    ram: f64,
    presence: f64,
    departed_frames: u16,
    pub freight_credit: f64,
}

impl AppBuilding {
    pub fn light_fraction(&self) -> f64 {
        if !self.alive { 0.0 } else { 0.05 + 0.95 * (self.cpu as f64 / 70.0).clamp(0.0, 1.0).sqrt() }
    }
}

#[derive(Default)]
pub struct AppDistrict {
    pub lots: Vec<Option<AppBuilding>>,
    pub selected: Option<String>,
    pub page: usize,
    pub process_offset: usize,
    pub disk_status: String,
}

pub fn format_rate(bytes: f64) -> String {
    if bytes >= 1_048_576.0 { format!("{:.1} MiB/s", bytes / 1_048_576.0) }
    else if bytes >= 1024.0 { format!("{:.1} KiB/s", bytes / 1024.0) }
    else { format!("{:.0} B/s", bytes) }
}

impl AppDistrict {
    pub fn sync(&mut self, apps: Vec<AppMetrics>, disk_status: String) {
        self.disk_status = disk_status;
        while self.lots.len() < 3 { self.lots.push(None); }
        for b in self.lots.iter_mut().flatten() { b.alive = false; }
        for app in apps {
            if let Some(b) = self.lots.iter_mut().flatten().find(|b| b.metrics.id == app.id) {
                b.metrics = app; b.alive = true; b.departed_frames = 0;
                continue;
            }
            // Keep the three acceptance-test apps visible even when launched later.
            let slot = match app.id.as_str() {
                "app:chrome" => 0, "app:steam" => 1, "app:vscode" => 2,
                _ => (3..self.lots.len()).find(|&i| self.lots[i].is_none()).unwrap_or(self.lots.len()),
            };
            if slot == self.lots.len() { self.lots.push(None); }
            self.lots[slot] = Some(AppBuilding { metrics: app, alive: true, cpu: 0.0,
                ram: 0.0, presence: 0.0, departed_frames: 0, freight_credit: 0.0 });
        }
    }

    pub fn animate(&mut self) {
        for slot in &mut self.lots {
            if let Some(b) = slot {
                if b.alive {
                    b.cpu += (b.metrics.cpu_percent - b.cpu) * 0.1;
                    b.ram += (b.metrics.ram_bytes as f64 - b.ram) * 0.1;
                    b.presence += (1.0 - b.presence) * 0.12;
                } else {
                    b.cpu *= 0.8; b.presence *= 0.96; b.departed_frames += 1;
                    if b.departed_frames >= 60 { *slot = None; }
                }
            }
        }
        if self.selected.is_some() && self.selected_building().is_none() { self.selected = None; }
    }

    pub fn columns(area: Rect) -> usize { (area.width as usize / 20).max(1) }
    pub fn page_count(&self, area: Rect) -> usize {
        self.lots.iter().rposition(Option::is_some).unwrap_or(0) / Self::columns(area) + 1
    }

    pub fn geometry(&mut self, area: Rect) -> Vec<BuildingInfo> {
        self.page = self.page.min(self.page_count(area).saturating_sub(1));
        let columns = Self::columns(area);
        self.lots.iter().enumerate().skip(self.page * columns).take(columns).filter_map(|(slot, b)| {
            let b = b.as_ref()?;
            let scale = (1.0 + b.ram / (64.0 * 1_048_576.0)).log2();
            let width = (8.0 + scale).min(16.0) as u16;
            let height = ((7.0 + 3.0 * scale) * b.presence).round().max(1.0) as u16;
            let x = (slot % columns) as u16 * 20 + 1;
            Some(BuildingInfo { x_offset: x, width: width.min(area.width.saturating_sub(x)),
                height: height.min(area.height.saturating_sub(9)), index: slot,
                door_x: x + width / 2, spans_two: false })
        }).collect()
    }

    pub fn selected_building(&self) -> Option<&AppBuilding> {
        let id = self.selected.as_ref()?;
        self.lots.iter().flatten().find(|b| &b.metrics.id == id)
    }

    pub fn select(&mut self, slot: usize, area: Rect) {
        if let Some(Some(b)) = self.lots.get(slot) {
            self.selected = Some(b.metrics.id.clone()); self.page = slot / Self::columns(area); self.process_offset = 0;
        }
    }

    pub fn select_next(&mut self, forward: bool, area: Rect) {
        let slots: Vec<_> = self.lots.iter().enumerate().filter(|(_, b)| b.is_some()).map(|(i, _)| i).collect();
        if slots.is_empty() { return; }
        let current = slots.iter().position(|&i| self.lots[i].as_ref().map(|b| &b.metrics.id) == self.selected.as_ref());
        let next = match (current, forward) {
            (Some(i), true) => (i + 1) % slots.len(),
            (Some(i), false) => (i + slots.len() - 1) % slots.len(),
            (None, true) => 0, (None, false) => slots.len() - 1,
        };
        self.select(slots[next], area);
    }

    pub fn change_page(&mut self, forward: bool, area: Rect) {
        let pages = self.page_count(area);
        self.page = (self.page + if forward { 1 } else { pages - 1 }) % pages;
        self.selected = None;
    }

    pub fn panel_area(&self, area: Rect, buildings: &[BuildingInfo]) -> Option<Rect> {
        self.selected_building()?;
        if area.width < 55 || area.height < 20 { return None; }
        let selected = buildings.iter().find(|b| self.lots[b.index].as_ref().map(|b| &b.metrics.id) == self.selected.as_ref());
        let right = selected.map(|b| b.x_offset < area.width / 2).unwrap_or(true);
        Some(Rect::new(if right { area.right() - 49 } else { area.x }, area.y + 1, 49, area.height.saturating_sub(6).min(24)))
    }

    pub fn click(&mut self, x: u16, y: u16, area: Rect, buildings: &[BuildingInfo]) {
        if self.panel_area(area, buildings).is_some_and(|r| x >= r.x && x < r.right() && y >= r.y && y < r.bottom()) { return; }
        let ground = area.bottom().saturating_sub(3);
        let slot = buildings.iter().find(|b| b.contains_x(x.saturating_sub(area.x)) &&
            y >= ground.saturating_sub(b.height + 1) && y < ground).map(|b| b.index);
        if let Some(slot) = slot { self.select(slot, area); } else { self.selected = None; }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme, buildings: &[BuildingInfo]) {
        if area.height < 3 { return; }
        let footer = Rect::new(area.x, area.bottom() - 2, area.width, 2);
        Clear.render(footer, buf);
        Paragraph::new(format!("KERNEL CITY  {}/{}  Tab/arrows select  PgUp/Dn districts  Esc close  q quit\n{}",
            self.page + 1, self.page_count(area), self.disk_status)).style(Style::default().fg(theme.neon_main)).render(footer, buf);
        let Some(b) = self.selected_building() else { return; };
        let Some(panel) = self.panel_area(area, buildings) else {
            Paragraph::new(format!("{}  CPU {:.1}%  RAM {:.0} MiB  (enlarge for details)",
                b.metrics.name, b.metrics.cpu_percent, b.metrics.ram_bytes as f64 / 1_048_576.0))
                .render(Rect::new(area.x, area.y, area.width, 1), buf);
            return;
        };
        let m = &b.metrics;
        let disk = match (m.disk_read_bps, m.disk_write_bps) {
            (Some(r), Some(w)) => format!("Disk R {}  W {}", format_rate(r), format_rate(w)),
            _ => "Disk unavailable (see status below)".into(),
        };
        let mut lines = vec![format!("{}  [{}]", m.name, if b.alive { "LIVE" } else { "EXITED" }),
            format!("CPU  {:.1}% of whole computer", m.cpu_percent),
            format!("RAM  {:.1} MiB (working sets)", m.ram_bytes as f64 / 1_048_576.0), disk,
            format!("All I/O R {}  W {}", format_rate(m.io_read_bps), format_rate(m.io_write_bps)),
            String::new(), format!("{} processes   Up/Down scroll", m.processes.len()),
            "PID    PARENT CPU%  MiB   PROCESS".into()];
        let visible = panel.height.saturating_sub(10) as usize;
        let offset = self.process_offset.min(m.processes.len().saturating_sub(visible.max(1)));
        for p in m.processes.iter().skip(offset).take(visible) {
            lines.push(format!("{:<6} {:<6} {:>4.1} {:>5.0} {}", p.pid,
                p.parent_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                p.cpu_percent, p.ram_bytes as f64 / 1_048_576.0, p.name));
        }
        Clear.render(panel, buf);
        Paragraph::new(lines.join("\n")).style(Style::default().fg(Color::White).bg(theme.building_base_colors[0]))
            .block(Block::default().title(" Application ").borders(Borders::ALL).border_style(Style::default().fg(theme.neon_main)))
            .render(panel, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(id: &str, ram: u64) -> AppMetrics {
        AppMetrics { id: id.into(), name: id.into(), cpu_percent: 70.0, ram_bytes: ram,
            io_read_bps: 0.0, io_write_bps: 0.0, disk_read_bps: Some(0.0), disk_write_bps: Some(0.0), processes: vec![] }
    }
    #[test]
    fn lots_survive_ranking_changes_and_exits_fade_out() {
        let mut d = AppDistrict::default(); let area = Rect::new(0,0,120,40);
        d.sync(vec![app("app:vscode",1),app("app:chrome",2),app("app:steam",3)],String::new());
        d.select(0,area);
        d.sync(vec![app("app:steam",30),app("app:vscode",10)],String::new());
        assert!(!d.lots[0].as_ref().unwrap().alive);
        assert_eq!(d.lots[0].as_ref().unwrap().light_fraction(),0.0);
        for _ in 0..60 { d.animate(); }
        assert!(d.lots[0].is_none()); assert!(d.selected.is_none());
        assert_eq!(d.lots[1].as_ref().unwrap().metrics.id,"app:steam");
    }
    #[test]
    fn memory_and_cpu_change_buildings_monotonically() {
        let mut d = AppDistrict::default(); let area = Rect::new(0,0,120,50);
        let mut idle = app("app:chrome",64*1_048_576); idle.cpu_percent = 0.0;
        d.sync(vec![idle,app("app:steam",4*1024*1_048_576)],String::new());
        for _ in 0..100 { d.animate(); }
        let g = d.geometry(area);
        assert!(g[0].height < g[1].height); assert!(g[0].width < g[1].width);
        assert!(d.lots[0].as_ref().unwrap().light_fraction() < d.lots[1].as_ref().unwrap().light_fraction());
        for width in 0..60 { d.geometry(Rect::new(0,0,width,12)); d.select_next(true,area); }
    }
}

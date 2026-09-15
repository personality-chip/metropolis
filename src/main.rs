mod args;
mod applications;
mod disk;
mod city;
mod config;
mod logos;
mod theme;

use city::{MetropolisCity, Weather};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseEventKind, MouseButton, EnableMouseCapture, DisableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{error::Error, io, time::{Duration, Instant}};
use sysinfo::System;

fn main() -> Result<(), Box<dyn Error>> {
    let cli_args = args::parse()?;
    let config = config::Config::load();
    
    let weather = match cli_args.weather.as_deref().unwrap_or(&config.appearance.default_weather).to_lowercase().as_str() {
        "rain" => Weather::Rain,
        "snow" => Weather::Snow,
        _ => Weather::Clear,
    };

    if cli_args.snapshot { return snapshots(cli_args.samples.unwrap_or(1)); }
    enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    if cli_args.apps { execute!(stdout, EnableMouseCapture)?; }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    sys.refresh_processes();
    let mut collector = applications::ApplicationCollector::default();
    let mut disk_collector = cli_args.apps.then(|| disk::DiskCollector::new(&sys));
    // Prime lifetime-counter baselines: startup is not a burst of I/O.
    let initial_apps = collector.sample(&sys, 1.0, &applications::DiskFrame::default());
    // DETECT DISTRO
    let distro = cli_args.distro.clone().unwrap_or_else(|| {
        if !config.monolith.override_distro.is_empty() {
            config.monolith.override_distro.clone()
        } else {
            System::name().unwrap_or_else(|| "linux".to_string())
        }
    }).to_lowercase();
    
    let theme_name = cli_args.theme.as_deref().unwrap_or(&config.appearance.global_theme);
    let global_theme = theme::Theme::from_str(theme_name);

    let mut city = MetropolisCity::new(
        distro, 
        weather, 
        global_theme,
        config.appearance.solid_background_color,
        config.monolith.custom_text,
        config.monolith.custom_color,
        config.simulation,
    );
    city.debug_mode = cli_args.debug;
    if cli_args.apps {
        let mut apps = city::applications::AppDistrict::default();
        apps.sync(initial_apps, "Disk collector starting...".into());
        city.applications = Some(apps);
    }
    
    let tick_rate = Duration::from_millis(50); 
    let sysinfo_tick_rate = Duration::from_millis(1000);
    let mut last_tick = Instant::now();
    let mut last_sysinfo_tick = Instant::now();
    let mut proc_names: Vec<String> = Vec::new();
    let mut needs_draw = true;

    loop {
        if needs_draw {
            let draw_start = Instant::now();
            terminal.draw(|f| {
                f.render_widget(&city, f.size());
            })?;
            city.perf.draw_us = draw_start.elapsed().as_micros() as u64;
            city.perf.push_frame();
            needs_draw = false;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == event::KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                            KeyCode::Tab | KeyCode::Right if cli_args.apps => {
                                city.applications.as_mut().unwrap().select_next(true, terminal.size()?);
                            },
                            KeyCode::BackTab | KeyCode::Left if cli_args.apps => {
                                city.applications.as_mut().unwrap().select_next(false, terminal.size()?);
                            },
                            KeyCode::PageDown | KeyCode::PageUp if cli_args.apps => {
                                city.applications.as_mut().unwrap().change_page(key.code == KeyCode::PageDown, terminal.size()?);
                                city.vehicles.clear();
                            },
                            KeyCode::Esc if cli_args.apps => city.applications.as_mut().unwrap().selected = None,
                            KeyCode::Down if cli_args.apps => {
                                let apps = city.applications.as_mut().unwrap();
                                let max = apps.selected_building().map(|b| b.metrics.processes.len()).unwrap_or(0);
                                apps.process_offset = apps.process_offset.saturating_add(1).min(max.saturating_sub(1));
                            },
                            KeyCode::Up if cli_args.apps => {
                                let apps = city.applications.as_mut().unwrap();
                                apps.process_offset = apps.process_offset.saturating_sub(1);
                            },
                            KeyCode::Char('r') => {
                                city.weather = if city.weather == Weather::Rain { Weather::Clear } else { Weather::Rain };
                                needs_draw = true;
                            },
                            KeyCode::Char('s') => {
                                city.weather = if city.weather == Weather::Snow { Weather::Clear } else { Weather::Snow };
                                needs_draw = true;
                            },
                            KeyCode::Char('d') => {
                                city.debug_mode = !city.debug_mode;
                                needs_draw = true;
                            },
                            _ => {}
                        }
                    }
                },
                Event::Mouse(mouse) if cli_args.apps && mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    city.applications.as_mut().unwrap().click(mouse.column, mouse.row, terminal.size()?, &city.buildings_cache);
                    needs_draw = true;
                },
                Event::Resize(_, _) => {
                    city.vehicles.clear();
                    let area = terminal.size()?;
                    if let Some(apps) = &mut city.applications { city.buildings_cache = apps.geometry(area); }
                    needs_draw = true;
                },
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            let mut cpu = sys.global_cpu_info().cpu_usage();
            let mut ram = (sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0;
            let mut disk_usage = city.disk_usage;

            if last_sysinfo_tick.elapsed() >= sysinfo_tick_rate {
                sys.refresh_memory();
                sys.refresh_cpu_usage();
                sys.refresh_processes();
                let elapsed = last_sysinfo_tick.elapsed().as_secs_f64();
                last_sysinfo_tick = Instant::now();
                let disk = disk_collector.as_mut().map(|c| c.sample(&sys, elapsed)).unwrap_or_default();
                let metrics = collector.sample(&sys, elapsed, &disk);

                cpu = sys.global_cpu_info().cpu_usage();
                ram = (sys.used_memory() as f32 / sys.total_memory() as f32) * 100.0;

                let mut procs: Vec<(String, f32)> = sys.processes()
                    .values()
                    .map(|p| (p.name().to_string(), p.cpu_usage()))
                    .collect();
                
                procs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                
                proc_names = procs.into_iter()
                    .filter(|(name, _)| !name.to_lowercase().contains("metropolis"))
                    .take(10)
                    .map(|(name, _)| {
                        let clean = name.split('.').next().unwrap_or(&name);
                        clean.to_uppercase().chars().take(8).collect()
                    })
                    .collect();

                // sysinfo's per-process totals are differenced once, with PID-generation baselines.
                disk_usage = (metrics.iter().map(|a| a.io_read_bps + a.io_write_bps).sum::<f64>() / 250_000.0).min(100.0) as f32;
                if let Some(apps) = &mut city.applications {
                    apps.sync(metrics, disk.status);
                }
            }

            let update_start = Instant::now();
            city.update(terminal.size()?, cpu, ram, disk_usage, proc_names.clone());
            city.perf.update_us = update_start.elapsed().as_micros() as u64;
            last_tick = Instant::now();
            needs_draw = true;
        }
    }

    terminal.show_cursor()?;

    Ok(())
}

// Restore the terminal on both normal exit and propagated I/O errors.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn snapshots(count: usize) -> Result<(), Box<dyn Error>> {
    use serde::Serialize;
    use std::io::Write;
    #[derive(Serialize)]
    struct Lot { slot: usize, app_id: String, alive: bool }
    #[derive(Serialize)]
    struct Snapshot { sample_seconds: f64, disk_status: String, lots: Vec<Lot>, apps: Vec<applications::AppMetrics> }
    let mut sys = System::new_all();
    let mut collector = applications::ApplicationCollector::default();
    let mut disk = disk::DiskCollector::new(&sys);
    collector.sample(&sys, 1.0, &applications::DiskFrame::default());
    let mut district = city::applications::AppDistrict::default();
    let mut last = Instant::now();
    for _ in 0..count {
        std::thread::sleep(Duration::from_secs(1));
        sys.refresh_cpu_usage(); sys.refresh_processes();
        let seconds = last.elapsed().as_secs_f64(); last = Instant::now();
        let frame = disk.sample(&sys, seconds);
        let apps = collector.sample(&sys, seconds, &frame);
        district.sync(apps.clone(), frame.status.clone());
        for _ in 0..20 { district.animate(); }
        let lots = district.lots.iter().enumerate().filter_map(|(slot,b)| b.as_ref().map(|b|
            Lot { slot, app_id: b.metrics.id.clone(), alive: b.alive })).collect();
        let snapshot = Snapshot { sample_seconds: seconds, disk_status: frame.status, lots, apps };
        println!("===KERNEL_CITY_SNAPSHOT===\n{}", toml::to_string(&snapshot)?);
        io::stdout().flush()?;
    }
    Ok(())
}

use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq)]
pub enum VehicleType {
    Spinner,
    Shuttle,
    Police,
    Truck,
}

#[derive(Debug, Clone)]
pub struct Vehicle {
    pub x: f32,
    pub y: f32,
    pub speed: f32,
    pub color: Color,
    pub v_type: VehicleType,
    pub length: u16,
    pub owner: Option<usize>,
    pub destination: f32,
}

pub fn get_sky_lane(rng: &mut impl rand::Rng) -> f32 {
    let lanes = vec![5.0, 8.0, 11.0, 14.0];
    lanes[rng.gen_range(0..lanes.len())]
}

/// Disk reads arrive at a door; writes leave it. Routes stay within their lot.
pub fn update_app_vehicles(
    vehicles: &mut Vec<Vehicle>,
    district: &mut super::applications::AppDistrict,
    buildings: &[super::buildings::BuildingInfo],
    area: ratatui::layout::Rect,
    theme: &crate::theme::Theme,
    config: &crate::config::SimulationConfig,
    rng: &mut impl rand::Rng,
) {
    vehicles.retain_mut(|v| {
        let Some(owner) = v.owner else { return false; };
        if !buildings.iter().any(|b| b.index == owner) ||
            !district.lots.get(owner).and_then(Option::as_ref).is_some_and(|b| b.alive) { return false; }
        v.x += v.speed;
        if v.speed > 0.0 { v.x < v.destination } else { v.x > v.destination }
    });
    for building in buildings {
        let Some(app) = district.lots[building.index].as_mut() else { continue; };
        let bytes = app.metrics.disk_bps();
        if !app.alive || bytes <= 0.0 { app.freight_credit = 0.0; continue; }
        app.freight_credit = (app.freight_credit + (bytes / 65536.0).ln_1p().min(6.0) * 0.05).min(2.0);
        if app.freight_credit < 1.0 || vehicles.len() >= config.max_vehicles ||
            vehicles.iter().filter(|v| v.owner == Some(building.index)).count() >= 4 { continue; }
        app.freight_credit -= 1.0;
        let outgoing = rng.gen_bool((app.metrics.disk_write_bps.unwrap_or(0.0) / bytes).clamp(0.0, 1.0));
        let door = building.door_x as f32;
        let edge = (building.x_offset + 17).min(area.width.saturating_sub(3)) as f32;
        if edge <= door { continue; }
        let color = if outgoing { theme.neon_sub2 } else { theme.neon_sub1 };
        vehicles.push(Vehicle {
            x: if outgoing { door } else { edge },
            y: area.height.saturating_sub(if outgoing { 3 } else { 4 }) as f32,
            speed: (if outgoing { 0.45 } else { -0.45 }) * config.vehicle_speed_multiplier.max(0.1),
            color, v_type: VehicleType::Truck, length: 3, owner: Some(building.index),
            destination: if outgoing { edge } else { door },
        });
    }
}

pub fn update_vehicles(
    vehicles: &mut Vec<Vehicle>,
    chase_cooldown: &mut u32,
    frame_count: u64,
    cpu: f32,
    disk_usage: f32,
    area: ratatui::layout::Rect,
    theme: &crate::theme::Theme,
    config: &crate::config::SimulationConfig,
    rng: &mut impl rand::Rng,
) {
    let pulse = (frame_count as f32 * 0.003).sin() * 0.5 + 0.5; // 0.0 to 1.0
    let c_max = config.max_vehicles as f32;
    // CPU usage maps from a baseline of 5% up to 15% of max_vehicles, creating a sparse sky
    let base_targets = ((cpu / 100.0) * (c_max * 0.15)).max(c_max * 0.05);
    // Pulse gently modulates traffic +/- 20%
    let target_vehicles = (base_targets * (0.8 + 0.4 * pulse)) as usize;
    
    if *chase_cooldown > 0 { *chase_cooldown -= 1; }

    if vehicles.len() < target_vehicles && frame_count % 3 == 0 {
        let y = get_sky_lane(rng);
        let roll = rng.gen_range(0.0..1.0);
        
        if roll < 0.02 && *chase_cooldown == 0 { 
            let speed = rng.gen_range(4.5..6.5);
            vehicles.push(Vehicle { owner: None, destination: 0.0, x: -5.0, y, speed, color: theme.police_red, v_type: VehicleType::Spinner, length: 3 });
            vehicles.push(Vehicle { owner: None, destination: 0.0, x: -15.0, y, speed, color: Color::White, v_type: VehicleType::Police, length: 3 });
            vehicles.push(Vehicle { owner: None, destination: 0.0, x: -25.0, y, speed, color: Color::White, v_type: VehicleType::Police, length: 3 });
            vehicles.push(Vehicle { owner: None, destination: 0.0, x: -35.0, y, speed, color: Color::White, v_type: VehicleType::Police, length: 3 });
            *chase_cooldown = 1200;
        } else if roll < 0.10 {
            let length: u16 = rng.gen_range(4..12);
            let mega_count = vehicles.iter().filter(|v| v.v_type == VehicleType::Shuttle && v.length > 9).count();
            let disk_bonus = (disk_usage / 10.0) as u16; 
            let adj_length = length.saturating_add(disk_bonus).min(25);

            if adj_length <= 9 || mega_count == 0 {
                let color = if adj_length <= 6 {
                    theme.shuttle_colors[0]
                } else if adj_length <= 9 {
                    theme.shuttle_colors[1]
                } else {
                    theme.shuttle_colors[2]
                };
                vehicles.push(Vehicle { owner: None, destination: 0.0, x: -5.0, y, speed: rng.gen_range(0.3..0.6), color, v_type: VehicleType::Shuttle, length: adj_length });
            }
        } else {
            let color = theme.vehicle_colors[rng.gen_range(0..theme.vehicle_colors.len())];
            vehicles.push(Vehicle { owner: None, destination: 0.0, x: -5.0, y, speed: rng.gen_range(0.8..2.2), color, v_type: VehicleType::Spinner, length: 3 });
        }
    }
    let speed_mod = (0.5 + (cpu / 80.0)) * config.vehicle_speed_multiplier;
    vehicles.retain_mut(|v| { v.x += v.speed * speed_mod; v.x < (area.width as f32 + 40.0) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{applications::AppMetrics, city::applications::AppDistrict, config::SimulationConfig, theme::Theme};
    use rand::{SeedableRng, rngs::StdRng};
    #[test]
    fn only_the_writing_building_sends_trucks_and_exit_clears_them() {
        let app = |id: &str, write: f64| AppMetrics { id: id.into(), name: id.into(),
            cpu_percent: 0.0, ram_bytes: 128 * 1_048_576, io_read_bps: 0.0, io_write_bps: 0.0,
            disk_read_bps: Some(0.0), disk_write_bps: Some(write), processes: vec![] };
        let mut d = AppDistrict::default();
        let area = ratatui::layout::Rect::new(0,0,120,40);
        d.sync(vec![app("app:chrome", 0.0), app("app:steam", 100_000_000.0)], String::new());
        for _ in 0..20 { d.animate(); }
        let buildings = d.geometry(area);
        let mut vehicles = Vec::new();
        let mut rng = StdRng::seed_from_u64(42);
        let theme = Theme::from_str("default");
        let config = SimulationConfig::default();
        for _ in 0..20 { update_app_vehicles(&mut vehicles, &mut d, &buildings, area, &theme, &config, &mut rng); }
        assert!(!vehicles.is_empty());
        assert!(vehicles.iter().all(|v| v.owner == Some(1) && v.speed > 0.0));
        d.sync(vec![app("app:chrome", 0.0)], String::new());
        update_app_vehicles(&mut vehicles, &mut d, &buildings, area, &theme, &config, &mut rng);
        assert!(vehicles.is_empty());
    }
}

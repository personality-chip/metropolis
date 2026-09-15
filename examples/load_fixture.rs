//! A bounded workload for Windows CI. These renamed fixtures are not the real apps.
use std::{fs::{self, OpenOptions}, io::{Read, Seek, SeekFrom, Write}, process::{Command, Child}, time::{Duration, Instant}};
fn main() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let control = &args[1];
    let mut child: Option<Child> = if let Some(exe) = args.get(2) {
        let child = Command::new(exe).arg(control).spawn()?;
        println!("{}", child.id()); Some(child)
    } else { None };
    let mut options = OpenOptions::new(); options.create(true).read(true).write(true).truncate(true);
    #[cfg(windows)] {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x80000000); // FILE_FLAG_WRITE_THROUGH, real disk completions
    }
    let path = std::env::temp_dir().join(format!("kernel-city-fixture-{}.bin", std::process::id()));
    let mut file = options.open(&path)?;
    let mut memory = vec![1u8; 8 * 1024 * 1024];
    let block = vec![42u8; 1024 * 1024];
    let mut readback = vec![0u8; block.len()];
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline {
        let mode = fs::read_to_string(control).unwrap_or_else(|_| "stop".into());
        if mode == "stop" { break; }
        if mode == "active" {
            if memory.len() < 128 * 1024 * 1024 { memory.resize(128 * 1024 * 1024, 1); }
            let busy_until = Instant::now() + Duration::from_millis(35);
            while Instant::now() < busy_until {
                for i in (0..memory.len()).step_by(4096) { memory[i] = memory[i].wrapping_add(1); }
                std::hint::black_box(&memory);
            }
            file.seek(SeekFrom::Start(0))?; file.write_all(&block)?; file.sync_all()?;
            file.seek(SeekFrom::Start(0))?; file.read_exact(&mut readback)?;
        }
        std::hint::black_box(&memory);
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Some(child) = &mut child { let _ = child.wait(); }
    drop(file); let _ = fs::remove_file(path);
    Ok(())
}

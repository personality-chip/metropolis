//! Windows disk completions, not GetProcessIoCounters (which includes network I/O).
use crate::applications::DiskFrame;
use sysinfo::System;

#[cfg(not(windows))]
pub struct DiskCollector;
#[cfg(not(windows))]
impl DiskCollector {
    pub fn new(_: &System) -> Self { Self }
    pub fn sample(&mut self, _: &System, _: f64) -> DiskFrame {
        DiskFrame { status: "Disk unavailable: Windows ETW only; All I/O is shown separately".into(), ..DiskFrame::default() }
    }
}

#[cfg(windows)]
pub use windows_disk::DiskCollector;

#[cfg(windows)]
mod windows_disk {
    use super::*;
    use crate::applications::ProcessKey;
    use ferrisetw::{parser::{Parser, Pointer}, provider::{Provider, kernel_providers::*},
        schema_locator::SchemaLocator, trace::{KernelTrace, TraceTrait, TraceProperties}, EventRecord};
    use std::{collections::HashMap, sync::{Arc, Mutex}, thread, time::{Instant, SystemTime, UNIX_EPOCH}};

    #[derive(Default)]
    struct State {
        live: HashMap<u32, ProcessKey>,
        threads: HashMap<u32, u32>,
        requests: HashMap<usize, (Option<ProcessKey>, Instant)>,
        bytes: HashMap<ProcessKey, (u64, u64)>,
        unmatched: u64,
        parse_errors: u64,
        failure: Option<String>,
    }

    fn live_processes(sys: &System) -> HashMap<u32, ProcessKey> {
        sys.processes().iter().map(|(pid, p)| (pid.as_u32(), ProcessKey { pid: pid.as_u32(), started: p.start_time() })).collect()
    }

    pub struct DiskCollector {
        state: Arc<Mutex<State>>,
        trace: Option<KernelTrace>,
        worker: Option<thread::JoinHandle<()>>,
    }

    impl DiskCollector {
        pub fn new(sys: &System) -> Self {
            let state = Arc::new(Mutex::new(State { live: live_processes(sys), ..State::default() }));
            let thread_state = Arc::clone(&state);
            let threads = Provider::kernel(&THREAD_PROVIDER).add_callback(move |record: &EventRecord, schemas: &SchemaLocator| {
                if !matches!(record.opcode(), 1..=4) { return; }
                if let Ok(schema) = schemas.event_schema(record) {
                    let parser = Parser::create(record, &schema);
                    if let (Ok(pid), Ok(tid)) = (parser.try_parse::<u32>("ProcessId"), parser.try_parse::<u32>("TThreadId")) {
                        let mut s = thread_state.lock().unwrap_or_else(|e| e.into_inner());
                        if matches!(record.opcode(), 1 | 3) {
                            s.threads.insert(tid, pid);
                        } else if s.threads.get(&tid) == Some(&pid) { s.threads.remove(&tid); }
                    }
                }
            }).build();
            let disk_state = Arc::clone(&state);
            let flags = KernelProvider::new(DISK_IO_PROVIDER.guid, DISK_IO_PROVIDER.flags | DISK_IO_INIT_PROVIDER.flags);
            let disk = Provider::kernel(&flags).add_callback(move |record: &EventRecord, schemas: &SchemaLocator| {
                let opcode = record.opcode();
                if !matches!(opcode, 10..=13) { return; }
                let mut s = disk_state.lock().unwrap_or_else(|e| e.into_inner());
                let Ok(schema) = schemas.event_schema(record) else { s.parse_errors += 1; return; };
                let parser = Parser::create(record, &schema);
                let Ok(irp) = parser.try_parse::<Pointer>("Irp") else { s.parse_errors += 1; return; };
                if opcode == 12 || opcode == 13 {
                    let tid = parser.try_parse::<u32>("IssuingThreadId").unwrap_or(record.thread_id());
                    let pid = s.threads.get(&tid).copied().or_else(|| (tid == record.thread_id()).then_some(record.process_id()));
                    let key = pid.and_then(|pid| s.live.get(&pid).copied());
                    // Bounded even if a driver never completes requests. Uncorrelated
                    // completions are counted as missing, never assigned by guesswork.
                    if s.requests.len() >= 65_536 { s.requests.clear(); }
                    s.requests.insert(*irp, (key, Instant::now()));
                } else {
                    let Ok(size) = parser.try_parse::<u32>("TransferSize") else { s.parse_errors += 1; return; };
                    if let Some((Some(key), _)) = s.requests.remove(&*irp) {
                        let bytes = s.bytes.entry(key).or_default();
                        if opcode == 10 { bytes.0 = bytes.0.saturating_add(size as u64); }
                        else { bytes.1 = bytes.1.saturating_add(size as u64); }
                    } else { s.unmatched += 1; }
                }
            }).build();
            let name = format!("KernelCityDisk-{}-{}", std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
            let props = TraceProperties { buffer_size: 64, min_buffer: 4, max_buffer: 64, ..TraceProperties::default() };
            let mut collector = Self { state: Arc::clone(&state), trace: None, worker: None };
            match KernelTrace::new().named(name).set_trace_properties(props).enable(threads).enable(disk).stop_if_exist(false).start() {
                Ok((trace, handle)) => {
                    collector.trace = Some(trace);
                    collector.worker = Some(thread::spawn(move || {
                        if let Err(error) = KernelTrace::process_from_handle(handle) {
                            state.lock().unwrap_or_else(|e| e.into_inner()).failure = Some(format!("Disk ETW stopped: {error:?}"));
                        }
                    }));
                }
                Err(error) => state.lock().unwrap_or_else(|e| e.into_inner()).failure = Some(format!(
                    "Disk unavailable: ETW needs administrator access ({error:?})")),
            }
            collector
        }

        pub fn sample(&mut self, sys: &System, seconds: f64) -> DiskFrame {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.live = live_processes(sys);
            s.requests.retain(|_, (_, time)| time.elapsed().as_secs() < 30);
            let unmatched = std::mem::take(&mut s.unmatched);
            let errors = std::mem::take(&mut s.parse_errors);
            let available = self.trace.is_some() && s.failure.is_none() && errors == 0;
            let status = s.failure.clone().unwrap_or_else(|| format!(
                "Disk ETW | {unmatched} unassigned events | {errors} decode errors | All I/O is separate"));
            let seconds = seconds.max(0.001);
            let rates = std::mem::take(&mut s.bytes).into_iter().map(|(key, (r,w))|
                (key, (r as f64 / seconds, w as f64 / seconds))).collect();
            DiskFrame { available, rates, status }
        }
    }

    impl Drop for DiskCollector {
        fn drop(&mut self) {
            if let Some(trace) = self.trace.take() { let _ = trace.stop(); }
            if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        }
    }
}

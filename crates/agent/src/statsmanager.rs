// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::cmp::Reverse;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{Result, bail};
use procfs::Current;
use tonic::{Request, Response, Status};

pub use givc_common::pb::stats::stats_service_server::StatsServiceServer;

#[derive(Debug, Default)]
struct StatsState {
    jiffies: u64,
    totals: [u64; 10],
    processes: HashMap<u32, ProcessSample>,
}

#[derive(Debug, Clone)]
struct ProcessSample {
    name: String,
    utime: u64,
    stime: u64,
    cutime: i64,
    cstime: i64,
    rss: u64,
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    jiffies: u64,
    totals: [u64; 10],
    processes: HashMap<u32, ProcessSample>,
}

#[derive(Debug, Default)]
pub struct StatsController {
    state: Mutex<StatsState>,
}

impl StatsController {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// # Errors
    /// Fails if `/proc` data or host commands cannot be read.
    pub fn get_stats(&self) -> Result<givc_common::pb::stats::StatsResponse> {
        let memory = self.get_memory_stats()?;
        let load = self.get_load_stats()?;
        let process = self.get_process_stats()?;

        Ok(givc_common::pb::stats::StatsResponse {
            memory: Some(memory),
            load: Some(load),
            process: Some(process),
        })
    }

    /// # Errors
    /// Fails if host metadata commands cannot be read.
    pub fn get_sysinfo(&self) -> Result<givc_common::pb::stats::SysinfoResponse> {
        Ok(givc_common::pb::stats::SysinfoResponse {
            ghaf_version: detect_ghaf_version(),
            secure_boot: detect_secure_boot(),
            disk_encrypted: detect_disk_encryption(),
        })
    }

    /// # Errors
    /// Fails if `/proc/meminfo` cannot be read.
    pub fn get_memory_stats(&self) -> Result<givc_common::pb::stats::MemoryStats> {
        let meminfo = procfs::Meminfo::current()?;
        Ok(givc_common::pb::stats::MemoryStats {
            total: meminfo.mem_total,
            free: meminfo.mem_free,
            available: meminfo.mem_available.unwrap_or(0),
            cached: meminfo.cached,
        })
    }

    /// # Errors
    /// Fails if `/proc/loadavg` cannot be read.
    pub fn get_load_stats(&self) -> Result<givc_common::pb::stats::LoadStats> {
        let load = procfs::LoadAverage::current()?;
        Ok(givc_common::pb::stats::LoadStats {
            load1_min: load.one,
            load5_min: load.five,
            load15_min: load.fifteen,
        })
    }

    /// # Errors
    /// Fails if `/proc/stat` or per-process entries cannot be read.
    pub fn get_process_stats(&self) -> Result<givc_common::pb::stats::ProcessStats> {
        let snapshot = collect_process_snapshot()?;
        let mut state = self.state.lock().expect("stats state poisoned");
        Ok(process_stats_from_snapshot(&mut state, snapshot))
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatsServer {
    controller: std::sync::Arc<StatsController>,
}

impl StatsServer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            controller: std::sync::Arc::new(StatsController::new()),
        }
    }
}

#[tonic::async_trait]
impl givc_common::pb::stats::stats_service_server::StatsService for StatsServer {
    async fn get_stats(
        &self,
        _request: Request<givc_common::pb::stats::StatsRequest>,
    ) -> Result<Response<givc_common::pb::stats::StatsResponse>, Status> {
        self.controller
            .get_stats()
            .map(Response::new)
            .map_err(map_err)
    }

    async fn get_sysinfo(
        &self,
        _request: Request<givc_common::pb::stats::StatsRequest>,
    ) -> Result<Response<givc_common::pb::stats::SysinfoResponse>, Status> {
        self.controller
            .get_sysinfo()
            .map(Response::new)
            .map_err(map_err)
    }
}

fn process_stats_from_snapshot(
    state: &mut StatsState,
    snapshot: ProcessSnapshot,
) -> givc_common::pb::stats::ProcessStats {
    let djiffies = snapshot.jiffies.saturating_sub(state.jiffies);
    let user_cycles = snapshot.totals[0].saturating_sub(state.totals[0]);
    let sys_cycles = snapshot.totals[2].saturating_sub(state.totals[2]);

    let mut changes: Vec<(String, u64, u64, u64)> = Vec::new();
    for (pid, current) in &snapshot.processes {
        if let Some(prev) = state.processes.get(pid) {
            let current_cutime = current.cutime.max(0) as u64;
            let prev_cutime = prev.cutime.max(0) as u64;
            let current_cstime = current.cstime.max(0) as u64;
            let prev_cstime = prev.cstime.max(0) as u64;
            let user = current.utime.saturating_sub(prev.utime)
                + current_cutime.saturating_sub(prev_cutime);
            let sys = current.stime.saturating_sub(prev.stime)
                + current_cstime.saturating_sub(prev_cstime);
            let rss = current.rss;
            changes.push((current.name.clone(), user, sys, rss));
        }
    }

    state.jiffies = snapshot.jiffies;
    state.totals = snapshot.totals;
    state.processes = snapshot.processes;

    let mut cpu_changes = changes.clone();
    cpu_changes.sort_by_key(|(_, user, sys, _)| Reverse(user + sys));
    let cpu_processes = cpu_changes
        .into_iter()
        .take(5)
        .map(
            |(name, user, sys, rss)| givc_common::pb::stats::ProcessStat {
                name,
                user: pct(user, djiffies),
                sys: pct(sys, djiffies),
                res_set_size: rss,
            },
        )
        .collect();

    let mut mem_changes = changes;
    mem_changes.sort_by_key(|(_, _, _, rss)| Reverse(*rss));
    let mem_processes = mem_changes
        .into_iter()
        .take(5)
        .map(
            |(name, user, sys, rss)| givc_common::pb::stats::ProcessStat {
                name,
                user: pct(user, djiffies),
                sys: pct(sys, djiffies),
                res_set_size: rss,
            },
        )
        .collect();

    givc_common::pb::stats::ProcessStats {
        cpu_processes,
        mem_processes,
        total: 0,
        running: 0,
        user_cycles,
        sys_cycles,
        total_cycles: djiffies,
    }
}

fn pct(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (value as f32) * 100.0 / (total as f32)
    }
}

fn collect_process_snapshot() -> Result<ProcessSnapshot> {
    let mut totals = [0_u64; 10];
    let mut jiffies = 0_u64;
    let stat = std::fs::read_to_string("/proc/stat")?;
    let mut lines = stat.lines();
    let Some(cpu_line) = lines.next() else {
        bail!("failed to read stats")
    };
    let Some(values) = cpu_line.strip_prefix("cpu ") else {
        bail!("failed to read stats")
    };
    for (idx, field) in values.split_whitespace().take(10).enumerate() {
        let val = field
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("failed to read stats"))?;
        totals[idx] = val;
        jiffies += val;
    }

    let mut processes = HashMap::new();
    for proc in procfs::process::all_processes()? {
        let proc = match proc {
            Ok(proc) => proc,
            Err(_) => continue,
        };
        let stat = match proc.stat() {
            Ok(stat) => stat,
            Err(_) => continue,
        };
        let rss = stat.rss.max(0) as u64;
        processes.insert(
            proc.pid() as u32,
            ProcessSample {
                name: stat.comm,
                utime: stat.utime,
                stime: stat.stime,
                cutime: stat.cutime,
                cstime: stat.cstime,
                rss,
            },
        );
    }

    Ok(ProcessSnapshot {
        jiffies,
        totals,
        processes,
    })
}

fn detect_ghaf_version() -> String {
    run_system_command_output("ghaf-version", &[])
        .ok()
        .and_then(|out| {
            let version = String::from_utf8_lossy(&out).trim().to_owned();
            (!version.is_empty()).then_some(version)
        })
        .unwrap_or_else(|| "Unknown".to_owned())
}

fn detect_secure_boot() -> Option<bool> {
    let out = run_system_command_output("bootctl", &["status"]).ok()?;
    for line in String::from_utf8_lossy(&out).lines() {
        let line = line.trim();
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case("Secure Boot") {
            let value = value.trim().to_ascii_lowercase();
            if value.contains("enable") {
                return Some(true);
            }
            if value.contains("disable") {
                return Some(false);
            }
        }
    }
    None
}

fn detect_disk_encryption() -> Option<bool> {
    let out = run_system_command_output("lsblk", &["-rno", "TYPE"]).ok()?;
    let has_crypt = String::from_utf8_lossy(&out)
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("crypt"));
    Some(has_crypt)
}

fn run_system_command_output(name: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(name).args(args).output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        bail!("binary {name:?} failed")
    }
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(jiffies: u64, user: u64, sys: u64, rss: u64) -> ProcessSnapshot {
        let mut processes = HashMap::new();
        processes.insert(
            1,
            ProcessSample {
                name: "alpha".to_owned(),
                utime: user,
                stime: sys,
                cutime: 0,
                cstime: 0,
                rss,
            },
        );
        ProcessSnapshot {
            jiffies,
            totals: [jiffies, 0, jiffies / 2, 0, 0, 0, 0, 0, 0, 0],
            processes,
        }
    }

    #[test]
    fn first_process_stats_are_empty() {
        let controller = StatsController::new();
        let mut state = controller.state.lock().unwrap();
        let stats = process_stats_from_snapshot(&mut state, snapshot(100, 10, 5, 123));
        assert!(stats.cpu_processes.is_empty());
        assert!(stats.mem_processes.is_empty());
        assert_eq!(stats.total_cycles, 100);
    }

    #[test]
    fn process_stats_use_deltas() {
        let controller = StatsController::new();
        let mut state = controller.state.lock().unwrap();
        let _ = process_stats_from_snapshot(&mut state, snapshot(100, 10, 5, 123));
        let stats = process_stats_from_snapshot(&mut state, snapshot(150, 20, 15, 456));
        assert_eq!(stats.cpu_processes.len(), 1);
        assert_eq!(stats.mem_processes.len(), 1);
        assert_eq!(stats.cpu_processes[0].name, "alpha");
        assert_eq!(stats.mem_processes[0].res_set_size, 456);
        assert_eq!(stats.total_cycles, 50);
    }
}

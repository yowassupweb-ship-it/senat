//! How loaded the machines are: the node itself and each of the person's computers — processor, memory, video card
//! (and the node's disk, which is small and shared with MBOX).
//!
//! The node samples itself every few seconds; a computer's app samples itself and sends it with a small post. The
//! UI reads both from `/api/metrics`. The video card is read through `nvidia-smi` where there is one.

use crate::App;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::{Disks, System};

pub(crate) struct Sampler {
    sys: System,
    gpu: Option<bool>,
}

impl Sampler {
    pub(crate) fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        Sampler { sys, gpu: None }
    }

    /// One reading. Call it every few seconds: processor load is measured between two calls.
    pub(crate) fn sample(&mut self) -> Value {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let cpu = self.sys.global_cpu_usage();
        let (used, total) = (self.sys.used_memory(), self.sys.total_memory());
        let disks = Disks::new_with_refreshed_list();
        // the disk the data lives on: the biggest mounted one is a fair stand-in (the root on a server)
        let disk = disks.list().iter().max_by_key(|d| d.total_space()).map(|d| json!({"used": d.total_space() - d.available_space(), "total": d.total_space()}));
        let gpu = if self.gpu == Some(false) { None } else { nvidia() };
        if self.gpu.is_none() {
            self.gpu = Some(gpu.is_some());
        }
        json!({
            "cpu": (cpu * 10.0).round() / 10.0,
            "cores": self.sys.cpus().len(),
            "ram": {"used": used, "total": total},
            "gpu": gpu,
            "disk": disk,
            "at": senate_core::now_ms(),
        })
    }
}

/// NVIDIA cards: load, memory, temperature. None when there is no card or no driver tool.
fn nvidia() -> Option<Value> {
    let mut c = std::process::Command::new("nvidia-smi");
    c.args(["--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu", "--format=csv,noheader,nounits"]);
    c.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 5 {
        return None;
    }
    let n = |s: &str| s.parse::<f64>().ok();
    Some(json!({"name": f[0], "util": n(f[1]), "mem_used_mb": n(f[2]), "mem_total_mb": n(f[3]), "temp": n(f[4])}))
}

/// The node watches itself.
pub(crate) fn spawn(app: &Arc<App>) {
    let a = app.clone();
    tokio::spawn(async move {
        let sampler = Arc::new(Mutex::new(Sampler::new()));
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let s = sampler.clone();
            if let Ok(v) = tokio::task::spawn_blocking(move || s.lock().unwrap().sample()).await {
                *a.node_metrics.lock().unwrap() = v;
            }
        }
    });
}

/// A computer's app reports itself to the node it works for.
pub(crate) async fn report_loop(hub: String, token: String, machine_id: String, mut stop: tokio::sync::watch::Receiver<bool>) {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().unwrap();
    let sampler = Arc::new(Mutex::new(Sampler::new()));
    loop {
        tokio::select! { _ = tokio::time::sleep(Duration::from_secs(4)) => {}, _ = stop.changed() => {} }
        if *stop.borrow() {
            return;
        }
        let s = sampler.clone();
        let Ok(v) = tokio::task::spawn_blocking(move || s.lock().unwrap().sample()).await else { continue };
        let _ = http.post(format!("{hub}/api/runner/metrics")).bearer_auth(&token).json(&json!({"machine": machine_id, "metrics": v})).send().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_has_processor_and_memory() {
        let mut s = Sampler::new();
        std::thread::sleep(Duration::from_millis(250));
        let v = s.sample();
        assert!(v["cpu"].as_f64().unwrap() >= 0.0);
        assert!(v["ram"]["total"].as_u64().unwrap() > v["ram"]["used"].as_u64().unwrap());
        assert!(v["cores"].as_u64().unwrap() >= 1);
    }
}

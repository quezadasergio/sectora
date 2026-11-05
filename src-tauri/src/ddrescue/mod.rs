use crate::error::{AppError, AppResult};
use crate::jobs::{self, JobStatus};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageProgress {
    pub job_id: String,
    pub rescued_bytes: u64,
    pub err_bytes: u64,
    pub non_tried_bytes: u64,
    pub percent: f64,
    pub current_pos: u64,
    pub status: String,
    pub message: String,
}

struct RunningJob {
    child: Child,
}

pub struct DdrescueState {
    inner: Mutex<HashMap<String, RunningJob>>,
}

impl DdrescueState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

pub fn resolve_ddrescue_bin(app: &AppHandle) -> AppResult<PathBuf> {
    // Prefer bundled sidecar next to the executable / resource dir.
    if let Ok(path) = app.path().resource_dir() {
        let candidates = [
            path.join("binaries").join(sidecar_name()),
            path.join(sidecar_name()),
            path.join("ddrescue"),
            path.join("ddrescue.exe"),
        ];
        for c in candidates {
            if c.exists() {
                return Ok(c);
            }
        }
    }

    // Dev: Tauri places sidecars near target binaries
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let c = dir.join(sidecar_name());
            if c.exists() {
                return Ok(c);
            }
            let c2 = dir.join("ddrescue");
            if c2.exists() {
                return Ok(c2);
            }
        }
    }

    // Fallback: PATH / Homebrew
    for candidate in [
        "ddrescue",
        "/opt/homebrew/bin/ddrescue",
        "/usr/local/bin/ddrescue",
        "/usr/bin/ddrescue",
    ] {
        let p = PathBuf::from(candidate);
        if candidate == "ddrescue" {
            if which_ok("ddrescue") {
                return Ok(p);
            }
        } else if p.exists() {
            return Ok(p);
        }
    }

    Err(AppError::from(
        "No se encontró ddrescue. Ejecuta scripts/fetch-ddrescue.sh o instálalo en el sistema.",
    ))
}

fn sidecar_name() -> String {
    #[cfg(target_os = "windows")]
    {
        format!("ddrescue-{}.exe", target_triple())
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("ddrescue-{}", target_triple())
    }
}

fn target_triple() -> &'static str {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "windows", target_arch = "x86_64"),
    )))]
    {
        "unknown"
    }
}

fn which_ok(cmd: &str) -> bool {
    Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn start_job(
    app: AppHandle,
    state: Arc<DdrescueState>,
    app_data: PathBuf,
    job_id: String,
) -> AppResult<()> {
    {
        let guard = state.inner.lock();
        if guard.contains_key(&job_id) {
            return Err(AppError::from("El trabajo ya está en ejecución"));
        }
    }

    let job = jobs::get_job(&app_data, &job_id)?;
    let bin = resolve_ddrescue_bin(&app)?;

    if let Some(parent) = Path::new(&job.image_path).parent() {
        fs::create_dir_all(parent)?;
    }

    let mut args = vec![
        "-v".to_string(),
        "-b".to_string(),
        "4096".to_string(),
        "-c".to_string(),
        "64".to_string(),
        "-r".to_string(),
        "2".to_string(),
    ];
    if job.force_overwrite {
        args.push("-f".to_string());
    }
    args.push(job.source_path.clone());
    args.push(job.image_path.clone());
    args.push(job.map_path.clone());

    let mut cmd = Command::new(&bin);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Message(format!("No se pudo iniciar ddrescue ({bin:?}): {e}")))?;

    jobs::update_job_status(&app_data, &job_id, JobStatus::Running, None)?;

    let stderr = child.stderr.take();
    {
        let mut guard = state.inner.lock();
        guard.insert(job_id.clone(), RunningJob { child });
    }

    let progress_app = app.clone();
    let progress_state = state.clone();
    let progress_data = app_data.clone();
    let progress_id = job_id.clone();
    let source_size = job.source_size;
    let map_path = job.map_path.clone();

    thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                let _ = progress_app.emit(
                    "ddrescue-log",
                    serde_json::json!({ "jobId": progress_id, "line": line }),
                );
            }
        }

        // Poll mapfile until the process exits.
        loop {
            let finished = {
                let mut guard = progress_state.inner.lock();
                if let Some(rj) = guard.get_mut(&progress_id) {
                    match rj.child.try_wait() {
                        Ok(Some(status)) => {
                            guard.remove(&progress_id);
                            Some(status)
                        }
                        Ok(None) => None,
                        Err(_) => {
                            guard.remove(&progress_id);
                            None
                        }
                    }
                } else {
                    // Paused/cancelled elsewhere.
                    break;
                }
            };

            let progress = parse_mapfile_progress(&map_path, source_size, &progress_id);
            let _ = progress_app.emit("ddrescue-progress", &progress);

            if let Some(status) = finished {
                let final_progress = parse_mapfile_progress(&map_path, source_size, &progress_id);
                let _ = progress_app.emit("ddrescue-progress", &final_progress);

                let job_now = jobs::get_job(&progress_data, &progress_id).ok();
                let already_paused = job_now
                    .as_ref()
                    .map(|j| j.status == JobStatus::Paused || j.status == JobStatus::Cancelled)
                    .unwrap_or(false);
                if already_paused {
                    break;
                }

                let ok = status.success()
                    || (final_progress.non_tried_bytes == 0 && final_progress.rescued_bytes > 0);
                if ok {
                    let _ = jobs::update_job_status(
                        &progress_data,
                        &progress_id,
                        JobStatus::Completed,
                        None,
                    );
                    let _ = progress_app.emit(
                        "ddrescue-finished",
                        serde_json::json!({ "jobId": progress_id, "ok": true }),
                    );
                } else {
                    let _ = jobs::update_job_status(
                        &progress_data,
                        &progress_id,
                        JobStatus::Failed,
                        Some("ddrescue terminó con error".into()),
                    );
                    let _ = progress_app.emit(
                        "ddrescue-finished",
                        serde_json::json!({ "jobId": progress_id, "ok": false }),
                    );
                }
                break;
            }

            thread::sleep(Duration::from_millis(800));
        }
    });

    Ok(())
}

pub fn pause_job(state: &DdrescueState, app_data: &Path, job_id: &str) -> AppResult<()> {
    interrupt_child(state, job_id)?;
    jobs::update_job_status(app_data, job_id, JobStatus::Paused, None)?;
    Ok(())
}

pub fn cancel_job(state: &DdrescueState, app_data: &Path, job_id: &str) -> AppResult<()> {
    interrupt_child(state, job_id)?;
    jobs::update_job_status(app_data, job_id, JobStatus::Cancelled, None)?;
    Ok(())
}

fn interrupt_child(state: &DdrescueState, job_id: &str) -> AppResult<()> {
    let mut guard = state.inner.lock();
    let Some(rj) = guard.get_mut(job_id) else {
        return Err(AppError::from("No hay proceso activo para este trabajo"));
    };

    #[cfg(unix)]
    {
        // SIGINT allows ddrescue to flush the mapfile
        let pid = rj.child.id();
        libc_kill(pid as i32, 2);
    }
    #[cfg(windows)]
    {
        let _ = rj.child.kill();
    }

    // Give it a moment then force-kill if needed
    thread::sleep(Duration::from_millis(500));
    let _ = rj.child.try_wait();
    let _ = rj.child.kill();
    guard.remove(job_id);
    Ok(())
}

#[cfg(unix)]
fn libc_kill(pid: i32, sig: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        let _ = kill(pid, sig);
    }
}

pub fn parse_mapfile_progress(map_path: &str, source_size: u64, job_id: &str) -> ImageProgress {
    let mut progress = ImageProgress {
        job_id: job_id.to_string(),
        status: "unknown".into(),
        ..Default::default()
    };

    let Ok(file) = File::open(map_path) else {
        progress.message = "Esperando mapfile…".into();
        return progress;
    };

    let mut rescued = 0u64;
    let mut err = 0u64;
    let mut non_tried = 0u64;
    let mut current_pos = 0u64;
    let mut current_status = String::new();

    for line in BufReader::new(file).lines().flatten() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            // comments may include "current_pos  current_status"
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 2 && parts[0].starts_with("0x") {
            // current_pos current_status line (non-comment form in some versions)
            if let Ok(pos) = parse_hex(parts[0]) {
                current_pos = pos;
            }
            current_status = parts[1].to_string();
            continue;
        }
        if parts.len() >= 3 && parts[0].starts_with("0x") {
            let Ok(size) = parse_hex(parts[1]) else {
                continue;
            };
            match parts[2] {
                "+" => rescued += size,
                "-" | "/" | "*" => err += size,
                "?" => non_tried += size,
                _ => {}
            }
        }
    }

    // Also try reading status from comment block by re-opening
    if let Ok(mut f) = File::open(map_path) {
        let mut content = String::new();
        let _ = f.read_to_string(&mut content);
        for line in content.lines() {
            let l = line.trim().trim_start_matches('#').trim();
            if let Some(rest) = l.strip_prefix("current_pos:") {
                // older
                let _ = rest;
            }
            // Format: "# current_pos  current_status" then next non-# line
        }
        // Parse two-field status line that appears after the header comments
        let mut lines = content.lines().peekable();
        while let Some(line) = lines.next() {
            if line.contains("current_pos") && line.contains("current_status") {
                if let Some(next) = lines.next() {
                    let parts: Vec<&str> = next.trim().split_whitespace().collect();
                    if parts.len() >= 2 {
                        if let Ok(pos) = parse_hex(parts[0]) {
                            current_pos = pos;
                        }
                        current_status = parts[1].to_string();
                    }
                }
            }
        }
    }

    let accounted = rescued + err + non_tried;
    let total = if source_size > 0 {
        source_size
    } else if accounted > 0 {
        accounted
    } else {
        1
    };
    let percent = (rescued as f64 / total as f64) * 100.0;

    progress.rescued_bytes = rescued;
    progress.err_bytes = err;
    progress.non_tried_bytes = non_tried;
    progress.current_pos = current_pos;
    progress.percent = percent.clamp(0.0, 100.0);
    progress.status = if current_status.is_empty() {
        "running".into()
    } else {
        current_status
    };
    progress.message = format!(
        "Rescatado {:.1}% · errores {} · pendientes {}",
        progress.percent,
        human(err),
        human(non_tried)
    );
    progress
}

fn parse_hex(s: &str) -> Result<u64, std::num::ParseIntError> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u64::from_str_radix(s, 16)
}

fn human(bytes: u64) -> String {
    crate::disk::format_size(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_mapfile_regions() {
        let dir = std::env::temp_dir().join("sectora-mapfile-test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("demo.map");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            "# Mapfile. Created by GNU ddrescue version 1.30\n\
             # current_pos  current_status  current_pass\n\
             0x1000     +               1\n\
             #      pos        size  status\n\
             0x00000000  0x00001000  +\n\
             0x00001000  0x00000200  -\n\
             0x00001200  0x00000E00  ?"
        )
        .unwrap();
        drop(f);
        let p = parse_mapfile_progress(path.to_str().unwrap(), 0x2000, "job1");
        assert_eq!(p.rescued_bytes, 0x1000);
        assert_eq!(p.err_bytes, 0x200);
        assert_eq!(p.non_tried_bytes, 0xE00);
        assert!(p.percent > 40.0 && p.percent < 60.0);
    }
}


mod ddrescue;
mod disk;
mod error;
mod jobs;
mod logging;
mod privileges;
mod recovery;
mod safety;

use ddrescue::DdrescueState;
use error::AppResult;
use jobs::{ImageJob, JobStatus};
use recovery::{CarveHit, RecoverableListing};
use safety::CloneValidation;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn app_data(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| error::AppError::Message(format!("app data: {e}")))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn map_err_str(e: impl ToString) -> error::AppError {
    error::AppError::Message(safety::map_io_error(&e.to_string()))
}

#[tauri::command]
fn list_disks() -> AppResult<Vec<disk::DiskInfo>> {
    disk::list_disks().map_err(|e| map_err_str(e))
}

#[tauri::command]
fn validate_clone_plan(
    source: disk::DiskInfo,
    image_path: String,
    force_overwrite: bool,
    confirm_internal: bool,
    confirm_force: bool,
) -> AppResult<CloneValidation> {
    safety::validate_clone_plan(
        &source,
        &image_path,
        force_overwrite,
        confirm_internal,
        confirm_force,
    )
}

#[tauri::command]
fn create_image_job(
    app: AppHandle,
    source_path: String,
    image_path: String,
    source_name: String,
    source_size: u64,
    force_overwrite: bool,
) -> AppResult<ImageJob> {
    let data = app_data(&app)?;
    let job = jobs::create_job(
        &data,
        source_path,
        image_path,
        source_name,
        source_size,
        force_overwrite,
    )
    .map_err(|e| map_err_str(e))?;
    logging::info(format!("job created {}", job.id));
    Ok(job)
}

#[tauri::command]
fn list_jobs(app: AppHandle) -> AppResult<Vec<ImageJob>> {
    let data = app_data(&app)?;
    Ok(jobs::load_jobs(&data)?.jobs)
}

#[tauri::command]
fn get_job(app: AppHandle, job_id: String) -> AppResult<ImageJob> {
    let data = app_data(&app)?;
    jobs::get_job(&data, &job_id)
}

#[tauri::command]
fn start_image(
    app: AppHandle,
    state: State<'_, Arc<DdrescueState>>,
    job_id: String,
) -> AppResult<()> {
    let data = app_data(&app)?;
    logging::info(format!("start_image {job_id}"));
    ddrescue::start_job(app.clone(), state.inner().clone(), data, job_id).map_err(|e| {
        logging::error(e.to_string());
        map_err_str(e)
    })
}

#[tauri::command]
fn resume_image(
    app: AppHandle,
    state: State<'_, Arc<DdrescueState>>,
    job_id: String,
) -> AppResult<()> {
    let data = app_data(&app)?;
    let _job = jobs::get_job(&data, &job_id)?;
    logging::info(format!("resume_image {job_id}"));
    ddrescue::start_job(app.clone(), state.inner().clone(), data, job_id).map_err(|e| {
        logging::error(e.to_string());
        map_err_str(e)
    })
}

#[tauri::command]
fn pause_image(
    app: AppHandle,
    state: State<'_, Arc<DdrescueState>>,
    job_id: String,
) -> AppResult<()> {
    let data = app_data(&app)?;
    logging::info(format!("pause_image {job_id}"));
    ddrescue::pause_job(state.inner(), &data, &job_id).map_err(|e| map_err_str(e))
}

#[tauri::command]
fn cancel_image(
    app: AppHandle,
    state: State<'_, Arc<DdrescueState>>,
    job_id: String,
) -> AppResult<()> {
    let data = app_data(&app)?;
    logging::info(format!("cancel_image {job_id}"));
    ddrescue::cancel_job(state.inner(), &data, &job_id).map_err(|e| map_err_str(e))
}

#[tauri::command]
fn image_progress(job_id: String, map_path: String, source_size: u64) -> ddrescue::ImageProgress {
    ddrescue::parse_mapfile_progress(&map_path, source_size, &job_id)
}

#[tauri::command]
fn list_recoverable(image_path: String) -> AppResult<RecoverableListing> {
    recovery::list_recoverable(&image_path).map_err(|e| map_err_str(e))
}

#[tauri::command]
fn carve_scan(
    image_path: String,
    max_hits: Option<usize>,
) -> AppResult<Vec<recovery::RecoverableEntry>> {
    recovery::carve_scan(&image_path, max_hits.unwrap_or(200)).map_err(|e| map_err_str(e))
}

#[tauri::command]
fn export_files(
    image_path: String,
    items: Vec<(String, String)>,
) -> AppResult<Vec<(String, u64)>> {
    let mut results = Vec::new();
    for (locator, dest) in items {
        let dest_final = unique_dest_path(&dest);
        let n = recovery::export_entry(&image_path, &locator, &dest_final).map_err(|e| {
            logging::error(format!("export failed: {e}"));
            map_err_str(e)
        })?;
        results.push((dest_final, n));
    }
    Ok(results)
}

fn unique_dest_path(dest: &str) -> String {
    let path = std::path::Path::new(dest);
    if !path.exists() {
        return dest.to_string();
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    for i in 1..10_000 {
        let candidate = parent.join(format!("{stem}-{i}{ext}"));
        if !candidate.exists() {
            return candidate.to_string_lossy().to_string();
        }
    }
    dest.to_string()
}

#[tauri::command]
fn detect_fs(image_path: String) -> AppResult<String> {
    Ok(match recovery::detect_fs(&image_path)? {
        recovery::FsKind::Fat32 => "FAT32".into(),
        recovery::FsKind::ExFat => "exFAT".into(),
        recovery::FsKind::Ntfs => "NTFS".into(),
        recovery::FsKind::Unknown => "desconocido".into(),
    })
}

#[tauri::command]
fn format_bytes(bytes: u64) -> String {
    disk::format_size(bytes)
}

#[tauri::command]
fn platform_info() -> serde_json::Value {
    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "macAppleSiliconOnly": cfg!(all(target_os = "macos", target_arch = "aarch64")),
    })
}

#[tauri::command]
fn privilege_checklist() -> privileges::PrivilegeChecklist {
    privileges::checklist()
}

#[tauri::command]
fn app_meta() -> serde_json::Value {
    serde_json::json!({
        "name": "Sectora",
        "version": env!("CARGO_PKG_VERSION"),
        "license": "MIT",
        "ddrescueLicense": "GPLv2+",
        "filesystems": ["FAT32", "exFAT", "NTFS"],
        "notSupported": ["APFS", "HFS+"],
        "logPath": logging::path().map(|p| p.to_string_lossy().to_string()),
    })
}

#[tauri::command]
fn read_app_log(max_bytes: Option<usize>) -> AppResult<String> {
    logging::read_tail(max_bytes.unwrap_or(32 * 1024))
}

#[tauri::command]
fn map_error_message(message: String) -> String {
    safety::map_io_error(&message)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let dd_state = Arc::new(DdrescueState::new());
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(dd_state)
        .setup(|app| {
            let handle = app.handle().clone();
            if let Ok(data) = app_data(&handle) {
                let _ = logging::init(&data);
                match jobs::reconcile_stale_running(&data) {
                    Ok(n) if n > 0 => {
                        logging::info(format!("reconciled {n} stale running job(s) → paused"));
                    }
                    Ok(_) => {}
                    Err(e) => logging::error(format!("reconcile failed: {e}")),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_disks,
            validate_clone_plan,
            create_image_job,
            list_jobs,
            get_job,
            start_image,
            resume_image,
            pause_image,
            cancel_image,
            image_progress,
            list_recoverable,
            carve_scan,
            export_files,
            detect_fs,
            format_bytes,
            platform_info,
            privilege_checklist,
            app_meta,
            read_app_log,
            map_error_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sectora");
}

#[allow(dead_code)]
fn _carve_type(_: CarveHit) {}
#[allow(dead_code)]
fn _job_status(_: JobStatus) {}

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageJob {
    pub id: String,
    pub source_path: String,
    pub image_path: String,
    pub map_path: String,
    pub source_name: String,
    pub source_size: u64,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub force_overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct JobStore {
    pub jobs: Vec<ImageJob>,
}

fn store_path(app_data: &Path) -> PathBuf {
    app_data.join("jobs.json")
}

pub fn load_jobs(app_data: &Path) -> AppResult<JobStore> {
    let path = store_path(app_data);
    if !path.exists() {
        return Ok(JobStore::default());
    }
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save_jobs(app_data: &Path, store: &JobStore) -> AppResult<()> {
    fs::create_dir_all(app_data)?;
    let path = store_path(app_data);
    let data = serde_json::to_string_pretty(store)?;
    fs::write(path, data)?;
    Ok(())
}

pub fn create_job(
    app_data: &Path,
    source_path: String,
    image_path: String,
    source_name: String,
    source_size: u64,
    force_overwrite: bool,
) -> AppResult<ImageJob> {
    let map_path = format!("{image_path}.map");
    let now = Utc::now();
    let job = ImageJob {
        id: Uuid::new_v4().to_string(),
        source_path,
        image_path,
        map_path,
        source_name,
        source_size,
        status: JobStatus::Pending,
        created_at: now,
        updated_at: now,
        last_error: None,
        force_overwrite,
    };

    let mut store = load_jobs(app_data)?;
    store.jobs.insert(0, job.clone());
    save_jobs(app_data, &store)?;
    Ok(job)
}

pub fn update_job_status(
    app_data: &Path,
    job_id: &str,
    status: JobStatus,
    last_error: Option<String>,
) -> AppResult<ImageJob> {
    let mut store = load_jobs(app_data)?;
    let job = store
        .jobs
        .iter_mut()
        .find(|j| j.id == job_id)
        .ok_or_else(|| AppError::from("Trabajo no encontrado"))?;
    job.status = status;
    job.updated_at = Utc::now();
    job.last_error = last_error;
    let out = job.clone();
    save_jobs(app_data, &store)?;
    Ok(out)
}

pub fn get_job(app_data: &Path, job_id: &str) -> AppResult<ImageJob> {
    let store = load_jobs(app_data)?;
    store
        .jobs
        .into_iter()
        .find(|j| j.id == job_id)
        .ok_or_else(|| AppError::from("Trabajo no encontrado"))
}

/// After an app crash, any job left as Running has no live process — mark Paused so it can resume via mapfile.
pub fn reconcile_stale_running(app_data: &Path) -> AppResult<usize> {
    let mut store = load_jobs(app_data)?;
    let mut n = 0usize;
    for job in store.jobs.iter_mut() {
        if job.status == JobStatus::Running {
            job.status = JobStatus::Paused;
            job.updated_at = Utc::now();
            job.last_error = Some(
                "La aplicación se cerró mientras el clonado estaba en curso. Puedes reanudar desde el mapfile."
                    .into(),
            );
            n += 1;
        }
    }
    if n > 0 {
        save_jobs(app_data, &store)?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn reconcile_marks_running_as_paused() {
        let dir = std::env::temp_dir().join(format!("sectora-jobs-{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut job = create_job(
            &dir,
            "/dev/disk9".into(),
            dir.join("x.img").to_string_lossy().into(),
            "Disk".into(),
            100,
            false,
        )
        .unwrap();
        job.status = JobStatus::Running;
        let mut store = load_jobs(&dir).unwrap();
        store.jobs[0] = job;
        save_jobs(&dir, &store).unwrap();
        let n = reconcile_stale_running(&dir).unwrap();
        assert_eq!(n, 1);
        let again = get_job(&dir, &store.jobs[0].id).unwrap();
        assert_eq!(again.status, JobStatus::Paused);
        let _ = fs::remove_dir_all(&dir);
        let _ = Duration::from_millis(1);
    }
}

use crate::error::{AppError, AppResult};
use chrono::Utc;
use parking_lot::Mutex;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(app_data: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(app_data)?;
    let path = app_data.join("sectora.log");
    {
        let mut guard = LOG_PATH.lock();
        *guard = Some(path.clone());
    }
    append("INFO", "Sectora logging initialized")?;
    Ok(path)
}

pub fn path() -> Option<PathBuf> {
    LOG_PATH.lock().clone()
}

pub fn append(level: &str, message: &str) -> AppResult<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| AppError::Message(format!("log open: {e}")))?;
    let line = format!("{} [{}] {}\n", Utc::now().to_rfc3339(), level, message);
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn info(message: impl AsRef<str>) {
    let _ = append("INFO", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    let _ = append("ERROR", message.as_ref());
}

pub fn read_tail(max_bytes: usize) -> AppResult<String> {
    let Some(path) = path() else {
        return Ok(String::new());
    };
    if !path.exists() {
        return Ok(String::new());
    }
    let data = std::fs::read(&path)?;
    if data.len() <= max_bytes {
        return Ok(String::from_utf8_lossy(&data).to_string());
    }
    Ok(String::from_utf8_lossy(&data[data.len() - max_bytes..]).to_string())
}

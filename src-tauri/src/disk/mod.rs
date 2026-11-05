use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub model: Option<String>,
    pub is_removable: bool,
    pub is_internal: bool,
    pub device_type: String,
}

pub fn list_disks() -> AppResult<Vec<DiskInfo>> {
    #[cfg(target_os = "macos")]
    {
        list_disks_macos()
    }
    #[cfg(target_os = "linux")]
    {
        list_disks_linux()
    }
    #[cfg(target_os = "windows")]
    {
        list_disks_windows()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(AppError::from("Sistema operativo no soportado"))
    }
}

#[cfg(target_os = "macos")]
fn list_disks_macos() -> AppResult<Vec<DiskInfo>> {
    let output = Command::new("diskutil")
        .args(["list", "-plist"])
        .output()
        .map_err(|e| AppError::Message(format!("No se pudo ejecutar diskutil: {e}")))?;

    if !output.status.success() {
        return Err(AppError::from("diskutil list falló"));
    }

    let plist: plist::Value = plist::from_bytes(&output.stdout)
        .map_err(|e| AppError::Message(format!("plist inválido: {e}")))?;

    let mut disks = Vec::new();
    let Some(arr) = plist
        .as_dictionary()
        .and_then(|d| d.get("AllDisksAndPartitions"))
        .and_then(|v| v.as_array())
    else {
        return Ok(disks);
    };

    for entry in arr {
        let Some(dict) = entry.as_dictionary() else {
            continue;
        };
        let device = dict
            .get("DeviceIdentifier")
            .and_then(|v| v.as_string())
            .unwrap_or("")
            .to_string();
        // Entradas de primer nivel = discos completos (diskN).
        if device.is_empty() {
            continue;
        }
        let size = dict
            .get("Size")
            .and_then(|v| v.as_unsigned_integer())
            .unwrap_or(0);
        if size == 0 {
            continue;
        }
        let path = format!("/dev/{device}");
        let info = diskutil_info(&device).unwrap_or_default();
        disks.push(DiskInfo {
            id: device.clone(),
            name: info
                .volume_name
                .clone()
                .or(info.media_name.clone())
                .unwrap_or_else(|| device.clone()),
            path,
            size_bytes: size,
            model: info.media_name.or(info.device_model),
            is_removable: info.removable.unwrap_or(false),
            is_internal: info.internal.unwrap_or(true),
            device_type: "disk".into(),
        });
    }

    Ok(disks)
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct MacDiskMeta {
    media_name: Option<String>,
    volume_name: Option<String>,
    device_model: Option<String>,
    removable: Option<bool>,
    internal: Option<bool>,
}

#[cfg(target_os = "macos")]
fn diskutil_info(device: &str) -> AppResult<MacDiskMeta> {
    let output = Command::new("diskutil")
        .args(["info", "-plist", device])
        .output()?;
    if !output.status.success() {
        return Ok(MacDiskMeta::default());
    }
    let plist: plist::Value = plist::from_bytes(&output.stdout)
        .map_err(|e| AppError::Message(format!("plist info inválido: {e}")))?;
    let dict = plist.as_dictionary().cloned().unwrap_or_default();
    Ok(MacDiskMeta {
        media_name: dict
            .get("MediaName")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string()),
        volume_name: dict
            .get("VolumeName")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string()),
        device_model: dict
            .get("DeviceModel")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string()),
        removable: dict.get("Removable").and_then(|v| v.as_boolean()),
        internal: dict.get("Internal").and_then(|v| v.as_boolean()),
    })
}

#[cfg(target_os = "linux")]
fn list_disks_linux() -> AppResult<Vec<DiskInfo>> {
    let output = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-o",
            "NAME,PATH,SIZE,MODEL,TYPE,RM,ROTA,TRAN",
        ])
        .output()
        .map_err(|e| AppError::Message(format!("No se pudo ejecutar lsblk: {e}")))?;

    if !output.status.success() {
        return Err(AppError::from("lsblk falló"));
    }

    #[derive(serde::Deserialize)]
    struct Lsblk {
        blockdevices: Vec<LsblkDev>,
    }
    #[derive(serde::Deserialize)]
    struct LsblkDev {
        name: String,
        path: Option<String>,
        size: Option<u64>,
        model: Option<String>,
        #[serde(rename = "type")]
        dev_type: Option<String>,
        rm: Option<bool>,
        #[serde(default)]
        children: Vec<LsblkDev>,
    }

    let parsed: Lsblk = serde_json::from_slice(&output.stdout)?;
    let mut disks = Vec::new();
    for d in parsed.blockdevices {
        let ty = d.dev_type.unwrap_or_default();
        if ty != "disk" {
            continue;
        }
        let path = d
            .path
            .unwrap_or_else(|| format!("/dev/{}", d.name));
        disks.push(DiskInfo {
            id: d.name.clone(),
            name: d
                .model
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| d.name.clone()),
            path,
            size_bytes: d.size.unwrap_or(0),
            model: d.model,
            is_removable: d.rm.unwrap_or(false),
            is_internal: !d.rm.unwrap_or(false),
            device_type: "disk".into(),
        });
    }
    Ok(disks)
}

#[cfg(target_os = "windows")]
fn list_disks_windows() -> AppResult<Vec<DiskInfo>> {
    let script = r#"
Get-CimInstance Win32_DiskDrive | Select-Object DeviceID, Model, Size, MediaType, InterfaceType, Index |
ConvertTo-Json -Compress
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| AppError::Message(format!("PowerShell falló: {e}")))?;

    if !output.status.success() {
        return Err(AppError::from("No se pudieron listar discos en Windows"));
    }

    #[derive(serde::Deserialize)]
    struct WinDisk {
        #[serde(rename = "DeviceID")]
        device_id: String,
        #[serde(rename = "Model")]
        model: Option<String>,
        #[serde(rename = "Size")]
        size: Option<u64>,
        #[serde(rename = "MediaType")]
        media_type: Option<String>,
        #[serde(rename = "Index")]
        index: Option<u32>,
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(vec![]);
    }

    let disks: Vec<WinDisk> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed)?
    } else {
        vec![serde_json::from_str(trimmed)?]
    };

    Ok(disks
        .into_iter()
        .map(|d| {
            let idx = d.index.unwrap_or(0);
            let path = format!(r"\\.\PhysicalDrive{idx}");
            let removable = d
                .media_type
                .as_deref()
                .map(|m| m.to_lowercase().contains("removable"))
                .unwrap_or(false);
            DiskInfo {
                id: format!("PhysicalDrive{idx}"),
                name: d
                    .model
                    .clone()
                    .unwrap_or_else(|| format!("Disco {idx}")),
                path,
                size_bytes: d.size.unwrap_or(0),
                model: d.model,
                is_removable: removable,
                is_internal: !removable,
                device_type: "disk".into(),
            }
        })
        .collect())
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

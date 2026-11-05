mod carve;
mod fat;
mod exfat;
mod ntfs_fs;
#[cfg(test)]
mod fs_detect_tests;

use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub use carve::{carve_scan, CarveHit};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub fs_type: String,
    pub deleted: bool,
    /// Opaque locator for export (FS-specific)
    pub locator: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableListing {
    pub fs_type: String,
    pub entries: Vec<RecoverableEntry>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FsKind {
    Fat32,
    ExFat,
    Ntfs,
    Unknown,
}

pub fn detect_fs(image_path: &str) -> AppResult<FsKind> {
    let mut f = File::open(image_path)?;
    let mut boot = [0u8; 512];
    f.read_exact(&mut boot)?;

    // NTFS: OEM "NTFS    "
    if &boot[3..11] == b"NTFS    " {
        return Ok(FsKind::Ntfs);
    }
    // exFAT
    if &boot[3..11] == b"EXFAT   " {
        return Ok(FsKind::ExFat);
    }
    // FAT32: often has "FAT32   " at 0x52
    if boot.len() >= 0x5A && &boot[0x52..0x5A] == b"FAT32   " {
        return Ok(FsKind::Fat32);
    }
    // Also check partition table / scan first MBs for FS signatures
    if let Some(kind) = scan_for_fs_signature(image_path)? {
        return Ok(kind);
    }
    Ok(FsKind::Unknown)
}

fn scan_for_fs_signature(image_path: &str) -> AppResult<Option<FsKind>> {
    let mut f = File::open(image_path)?;
    let meta = f.metadata()?;
    let len = meta.len().min(64 * 1024 * 1024); // scan first 64MB
    let mut offset = 0u64;
    let mut buf = [0u8; 512];
    while offset + 512 <= len {
        f.seek(SeekFrom::Start(offset))?;
        if f.read_exact(&mut buf).is_err() {
            break;
        }
        if &buf[3..11] == b"NTFS    " {
            return Ok(Some(FsKind::Ntfs));
        }
        if &buf[3..11] == b"EXFAT   " {
            return Ok(Some(FsKind::ExFat));
        }
        if buf.len() >= 0x5A && &buf[0x52..0x5A] == b"FAT32   " {
            return Ok(Some(FsKind::Fat32));
        }
        // typical partition starts at 1MB or 2048 sectors
        offset = if offset == 0 {
            2048 * 512
        } else if offset == 2048 * 512 {
            1024 * 1024
        } else {
            offset + 1024 * 1024
        };
    }
    Ok(None)
}

pub fn partition_offset(image_path: &str, kind: FsKind) -> AppResult<u64> {
    let mut f = File::open(image_path)?;
    let mut boot = [0u8; 512];
    f.read_exact(&mut boot)?;
    let matches = match kind {
        FsKind::Ntfs => &boot[3..11] == b"NTFS    ",
        FsKind::ExFat => &boot[3..11] == b"EXFAT   ",
        FsKind::Fat32 => boot.len() >= 0x5A && &boot[0x52..0x5A] == b"FAT32   ",
        FsKind::Unknown => false,
    };
    if matches {
        return Ok(0);
    }

    let meta = f.metadata()?;
    let len = meta.len().min(64 * 1024 * 1024);
    let mut offset = 2048 * 512u64;
    let mut buf = [0u8; 512];
    while offset + 512 <= len {
        f.seek(SeekFrom::Start(offset))?;
        if f.read_exact(&mut buf).is_err() {
            break;
        }
        let ok = match kind {
            FsKind::Ntfs => &buf[3..11] == b"NTFS    ",
            FsKind::ExFat => &buf[3..11] == b"EXFAT   ",
            FsKind::Fat32 => buf.len() >= 0x5A && &buf[0x52..0x5A] == b"FAT32   ",
            FsKind::Unknown => false,
        };
        if ok {
            return Ok(offset);
        }
        offset += 1024 * 1024;
    }
    Ok(0)
}

pub fn list_recoverable(image_path: &str) -> AppResult<RecoverableListing> {
    if !Path::new(image_path).exists() {
        return Err(AppError::from("La imagen no existe"));
    }

    let kind = detect_fs(image_path)?;
    let offset = partition_offset(image_path, kind)?;

    match kind {
        FsKind::Fat32 => {
            let entries = fat::list_fat32(image_path, offset)?;
            Ok(RecoverableListing {
                fs_type: "FAT32".into(),
                entries,
                message: "Listado FAT32".into(),
            })
        }
        FsKind::ExFat => {
            let entries = exfat::list_exfat(image_path, offset)?;
            Ok(RecoverableListing {
                fs_type: "exFAT".into(),
                entries,
                message: "Listado exFAT".into(),
            })
        }
        FsKind::Ntfs => {
            let entries = ntfs_fs::list_ntfs(image_path, offset)?;
            Ok(RecoverableListing {
                fs_type: "NTFS".into(),
                entries,
                message: "Listado NTFS".into(),
            })
        }
        FsKind::Unknown => Ok(RecoverableListing {
            fs_type: "desconocido".into(),
            entries: vec![],
            message: "No se detectó FAT32/exFAT/NTFS. Usa carving por firmas.".into(),
        }),
    }
}

pub fn export_entry(image_path: &str, locator: &str, dest_path: &str) -> AppResult<u64> {
    // locator formats:
    // fat32:<offset>:<cluster>:<size>:<name>
    // exfat:<offset>:<cluster>:<size>:<name>
    // ntfs:<offset>:<mft_ref>:<size>:<name>
    let parts: Vec<&str> = locator.splitn(5, ':').collect();
    if parts.len() < 4 {
        return Err(AppError::from("Locator inválido"));
    }
    match parts[0] {
        "fat32" => fat::export_fat32(image_path, locator, dest_path),
        "exfat" => exfat::export_exfat(image_path, locator, dest_path),
        "ntfs" => ntfs_fs::export_ntfs(image_path, locator, dest_path),
        "carve" => carve::export_carved(image_path, locator, dest_path),
        _ => Err(AppError::from("Tipo de locator desconocido")),
    }
}

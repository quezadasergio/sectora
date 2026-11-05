use super::RecoverableEntry;
use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CarveHit {
    pub id: String,
    pub kind: String,
    pub offset: u64,
    pub size: u64,
    pub locator: String,
}

struct Signature {
    kind: &'static str,
    header: &'static [u8],
    ext: &'static str,
    max_size: u64,
    footer: Option<&'static [u8]>,
}

const SIGNATURES: &[Signature] = &[
    Signature {
        kind: "JPEG",
        header: &[0xFF, 0xD8, 0xFF],
        ext: "jpg",
        max_size: 40 * 1024 * 1024,
        footer: Some(&[0xFF, 0xD9]),
    },
    Signature {
        kind: "PNG",
        header: &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        ext: "png",
        max_size: 40 * 1024 * 1024,
        footer: Some(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]),
    },
    Signature {
        kind: "PDF",
        header: b"%PDF-",
        ext: "pdf",
        max_size: 80 * 1024 * 1024,
        footer: Some(b"%%EOF"),
    },
    Signature {
        kind: "ZIP",
        header: &[0x50, 0x4B, 0x03, 0x04],
        ext: "zip",
        max_size: 200 * 1024 * 1024,
        footer: Some(&[0x50, 0x4B, 0x05, 0x06]),
    },
    Signature {
        kind: "GIF",
        header: b"GIF89a",
        ext: "gif",
        max_size: 20 * 1024 * 1024,
        footer: Some(&[0x00, 0x3B]),
    },
];

pub fn carve_scan(image_path: &str, max_hits: usize) -> AppResult<Vec<RecoverableEntry>> {
    let mut file = File::open(image_path)?;
    let len = file.metadata()?.len();
    let mut hits = Vec::new();
    let chunk = 1024 * 1024usize;
    let mut buf = vec![0u8; chunk + 64];
    let mut offset = 0u64;
    let overlap = 64u64;

    while offset < len && hits.len() < max_hits {
        file.seek(SeekFrom::Start(offset))?;
        let to_read = ((len - offset) as usize).min(buf.len());
        let n = file.read(&mut buf[..to_read])?;
        if n == 0 {
            break;
        }
        for sig in SIGNATURES {
            let mut start = 0usize;
            while start + sig.header.len() <= n && hits.len() < max_hits {
                if let Some(rel) = find_bytes(&buf[start..n], sig.header) {
                    let abs = offset + (start + rel) as u64;
                    let size = estimate_size(&mut file, abs, sig, len)?;
                    if size > 16 {
                        let name = format!("carved_{}_{}.{}", sig.kind.to_lowercase(), abs, sig.ext);
                        let locator = format!("carve:{}:{}:{}:{}", abs, size, sig.ext, name);
                        hits.push(RecoverableEntry {
                            id: locator.clone(),
                            name,
                            path: format!("/carved/{}", locator),
                            size,
                            is_dir: false,
                            fs_type: format!("carve:{}", sig.kind),
                            deleted: true,
                            locator,
                        });
                    }
                    start += rel + sig.header.len();
                } else {
                    break;
                }
            }
        }
        if (n as u64) < chunk as u64 {
            break;
        }
        offset = offset + chunk as u64 - overlap;
    }

    Ok(hits)
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn estimate_size(file: &mut File, start: u64, sig: &Signature, image_len: u64) -> AppResult<u64> {
    let scan_limit = (start + sig.max_size).min(image_len);
    let Some(footer) = sig.footer else {
        return Ok((scan_limit - start).min(sig.max_size));
    };
    let mut pos = start + sig.header.len() as u64;
    let mut buf = vec![0u8; 64 * 1024];
    while pos < scan_limit {
        file.seek(SeekFrom::Start(pos))?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(rel) = find_bytes(&buf[..n], footer) {
            let end = pos + rel as u64 + footer.len() as u64;
            return Ok(end - start);
        }
        if n <= footer.len() {
            break;
        }
        pos += (n - footer.len()) as u64;
    }
    // Fallback: fixed window
    Ok((scan_limit - start).min(sig.max_size.min(4 * 1024 * 1024)))
}

pub fn export_carved(image_path: &str, locator: &str, dest_path: &str) -> AppResult<u64> {
    // carve:<offset>:<size>:<ext>:<name>
    let parts: Vec<&str> = locator.splitn(5, ':').collect();
    if parts.len() < 3 || parts[0] != "carve" {
        return Err(AppError::from("Locator carve inválido"));
    }
    let offset: u64 = parts[1].parse().map_err(|_| AppError::from("offset inválido"))?;
    let size: u64 = parts[2].parse().map_err(|_| AppError::from("tamaño inválido"))?;

    let mut file = File::open(image_path)?;
    file.seek(SeekFrom::Start(offset))?;
    if let Some(parent) = Path::new(dest_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest_path)?;
    let mut remaining = size;
    let mut buf = vec![0u8; 64 * 1024];
    let mut written = 0u64;
    while remaining > 0 {
        let n = remaining.min(buf.len() as u64) as usize;
        let read = file.read(&mut buf[..n])?;
        if read == 0 {
            break;
        }
        out.write_all(&buf[..read])?;
        written += read as u64;
        remaining -= read as u64;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_jpeg_signature() {
        let dir = std::env::temp_dir().join("sectora-carve-unit");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("sample.bin");
        let mut f = File::create(&path).unwrap();
        f.write_all(&[0u8; 64]).unwrap();
        f.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        f.write_all(&[1u8; 32]).unwrap();
        f.write_all(&[0xFF, 0xD9]).unwrap();
        f.write_all(&[0u8; 16]).unwrap();
        drop(f);

        let hits = carve_scan(path.to_str().unwrap(), 10).unwrap();
        assert!(hits.iter().any(|h| h.fs_type.contains("JPEG")));
    }
}

#[cfg(test)]
mod demo_img {
    use super::*;
    #[test]
    fn carve_demo_img() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/clonado.img");
        let hits = carve_scan(path, 20).expect("carve");
        println!("hits={}", hits.len());
        for h in &hits {
            println!("{} @{} size={}", h.fs_type, /*offset from locator*/ h.locator, h.size);
        }
        assert!(hits.iter().any(|h| h.fs_type.contains("JPEG")));
        assert!(hits.iter().any(|h| h.fs_type.contains("PDF")));
    }
}

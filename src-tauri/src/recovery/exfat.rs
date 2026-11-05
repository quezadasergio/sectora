use super::RecoverableEntry;
use crate::error::{AppError, AppResult};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

struct ExFatFs {
    partition_offset: u64,
    bytes_per_sector_shift: u8,
    sectors_per_cluster_shift: u8,
    fat_offset_sectors: u32,
    cluster_heap_offset: u32,
    root_cluster: u32,
}

impl ExFatFs {
    fn bytes_per_sector(&self) -> u64 {
        1u64 << self.bytes_per_sector_shift
    }
    fn cluster_size(&self) -> u64 {
        self.bytes_per_sector() << self.sectors_per_cluster_shift
    }
    fn fat_offset(&self) -> u64 {
        self.partition_offset + self.fat_offset_sectors as u64 * self.bytes_per_sector()
    }
    fn cluster_offset(&self, cluster: u32) -> u64 {
        self.partition_offset
            + self.cluster_heap_offset as u64 * self.bytes_per_sector()
            + (cluster as u64 - 2) * self.cluster_size()
    }
}

pub fn list_exfat(image_path: &str, partition_offset: u64) -> AppResult<Vec<RecoverableEntry>> {
    let mut file = File::open(image_path)?;
    let fs = read_boot(&mut file, partition_offset)?;
    let mut entries = Vec::new();
    walk(&mut file, &fs, fs.root_cluster, "/", &mut entries, 0)?;
    Ok(entries)
}

fn read_boot(file: &mut File, partition_offset: u64) -> AppResult<ExFatFs> {
    let mut boot = [0u8; 512];
    file.seek(SeekFrom::Start(partition_offset))?;
    file.read_exact(&mut boot)?;
    if &boot[3..11] != b"EXFAT   " {
        return Err(AppError::from("No es un volumen exFAT"));
    }
    Ok(ExFatFs {
        partition_offset,
        bytes_per_sector_shift: boot[108],
        sectors_per_cluster_shift: boot[109],
        fat_offset_sectors: u32::from_le_bytes([boot[80], boot[81], boot[82], boot[83]]),
        cluster_heap_offset: u32::from_le_bytes([boot[88], boot[89], boot[90], boot[91]]),
        root_cluster: u32::from_le_bytes([boot[96], boot[97], boot[98], boot[99]]),
    })
}

fn next_cluster(file: &mut File, fs: &ExFatFs, cluster: u32) -> AppResult<u32> {
    let off = fs.fat_offset() + cluster as u64 * 4;
    file.seek(SeekFrom::Start(off))?;
    let mut b = [0u8; 4];
    file.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_cluster_chain(file: &mut File, fs: &ExFatFs, start: u32) -> AppResult<Vec<u8>> {
    let mut data = Vec::new();
    let mut cluster = start;
    let mut hops = 0u32;
    while cluster >= 2 && cluster < 0xFFFF_FFF8 && hops < 8192 {
        hops += 1;
        let off = fs.cluster_offset(cluster);
        file.seek(SeekFrom::Start(off))?;
        let mut buf = vec![0u8; fs.cluster_size() as usize];
        file.read_exact(&mut buf)?;
        data.extend_from_slice(&buf);
        cluster = next_cluster(file, fs, cluster)?;
    }
    Ok(data)
}

fn walk(
    file: &mut File,
    fs: &ExFatFs,
    start: u32,
    prefix: &str,
    out: &mut Vec<RecoverableEntry>,
    depth: usize,
) -> AppResult<()> {
    if depth > 24 {
        return Ok(());
    }
    let data = read_cluster_chain(file, fs, start)?;
    let mut i = 0usize;
    while i + 32 <= data.len() {
        let entry_type = data[i];
        if entry_type == 0x00 {
            break;
        }
        // File directory entry 0x85
        if entry_type == 0x85 || entry_type == 0x05 {
            let secondary_count = data[i + 1] as usize;
            let deleted = entry_type == 0x05;
            if i + 32 * (1 + secondary_count) > data.len() {
                break;
            }
            let stream = &data[i + 32..i + 64];
            // Stream extension should be 0xC0 / 0x40
            let name_len = stream[3] as usize;
            let first_cluster = u32::from_le_bytes([stream[20], stream[21], stream[22], stream[23]]);
            let data_length = u64::from_le_bytes([
                stream[24], stream[25], stream[26], stream[27], stream[28], stream[29], stream[30],
                stream[31],
            ]);
            let mut name = String::new();
            let mut remaining = name_len;
            for s in 0..secondary_count.saturating_sub(1) {
                let ne = &data[i + 64 + s * 32..i + 96 + s * 32];
                if ne[0] != 0xC1 && ne[0] != 0x41 {
                    continue;
                }
                let mut chars = Vec::new();
                for off in (2..32).step_by(2) {
                    if remaining == 0 {
                        break;
                    }
                    let c = u16::from_le_bytes([ne[off], ne[off + 1]]);
                    if c == 0 {
                        break;
                    }
                    chars.push(c);
                    remaining = remaining.saturating_sub(1);
                }
                name.push_str(&String::from_utf16_lossy(&chars));
            }
            let attr = u16::from_le_bytes([data[i + 4], data[i + 5]]);
            let is_dir = attr & 0x10 != 0;
            if !name.is_empty() {
                let path = if prefix.ends_with('/') {
                    format!("{prefix}{name}")
                } else {
                    format!("{prefix}/{name}")
                };
                let locator = format!(
                    "exfat:{}:{}:{}:{}",
                    fs.partition_offset, first_cluster, data_length, name
                );
                out.push(RecoverableEntry {
                    id: locator.clone(),
                    name: name.clone(),
                    path: path.clone(),
                    size: data_length,
                    is_dir,
                    fs_type: "exFAT".into(),
                    deleted,
                    locator,
                });
                if is_dir && first_cluster >= 2 {
                    walk(file, fs, first_cluster, &path, out, depth + 1)?;
                }
            }
            i += 32 * (1 + secondary_count);
            continue;
        }
        i += 32;
    }
    Ok(())
}

pub fn export_exfat(image_path: &str, locator: &str, dest_path: &str) -> AppResult<u64> {
    let parts: Vec<&str> = locator.splitn(5, ':').collect();
    if parts.len() < 4 {
        return Err(AppError::from("Locator exFAT inválido"));
    }
    let partition_offset: u64 = parts[1].parse().map_err(|_| AppError::from("offset inválido"))?;
    let cluster: u32 = parts[2].parse().map_err(|_| AppError::from("cluster inválido"))?;
    let size: u64 = parts[3].parse().map_err(|_| AppError::from("tamaño inválido"))?;

    let mut file = File::open(image_path)?;
    let fs = read_boot(&mut file, partition_offset)?;
    if let Some(parent) = Path::new(dest_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest_path)?;
    let mut remaining = size;
    let mut cur = cluster;
    let mut written = 0u64;
    while remaining > 0 && cur >= 2 && cur < 0xFFFF_FFF8 {
        let off = fs.cluster_offset(cur);
        file.seek(SeekFrom::Start(off))?;
        let to_read = remaining.min(fs.cluster_size()) as usize;
        let mut buf = vec![0u8; to_read];
        file.read_exact(&mut buf)?;
        out.write_all(&buf)?;
        written += to_read as u64;
        remaining -= to_read as u64;
        cur = next_cluster(&mut file, &fs, cur)?;
    }
    Ok(written)
}

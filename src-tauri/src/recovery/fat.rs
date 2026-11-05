use super::RecoverableEntry;
use crate::error::{AppError, AppResult};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone)]
struct Fat32Fs {
    partition_offset: u64,
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    fat_size_sectors: u32,
    root_cluster: u32,
    total_sectors: u32,
}

impl Fat32Fs {
    fn cluster_size(&self) -> u64 {
        self.bytes_per_sector as u64 * self.sectors_per_cluster as u64
    }

    fn fat_offset(&self) -> u64 {
        self.partition_offset + self.reserved_sectors as u64 * self.bytes_per_sector as u64
    }

    fn data_offset(&self) -> u64 {
        self.fat_offset()
            + self.num_fats as u64 * self.fat_size_sectors as u64 * self.bytes_per_sector as u64
    }

    fn cluster_offset(&self, cluster: u32) -> u64 {
        self.data_offset() + (cluster as u64 - 2) * self.cluster_size()
    }
}

pub fn list_fat32(image_path: &str, partition_offset: u64) -> AppResult<Vec<RecoverableEntry>> {
    let mut file = File::open(image_path)?;
    let fs = read_bpb(&mut file, partition_offset)?;
    let mut entries = Vec::new();
    walk_dir(
        &mut file,
        &fs,
        fs.root_cluster,
        "/",
        &mut entries,
        0,
    )?;
    Ok(entries)
}

fn read_bpb(file: &mut File, partition_offset: u64) -> AppResult<Fat32Fs> {
    let mut boot = [0u8; 512];
    file.seek(SeekFrom::Start(partition_offset))?;
    file.read_exact(&mut boot)?;

    let bytes_per_sector = u16::from_le_bytes([boot[11], boot[12]]);
    let sectors_per_cluster = boot[13];
    let reserved_sectors = u16::from_le_bytes([boot[14], boot[15]]);
    let num_fats = boot[16];
    let fat_size_sectors = u32::from_le_bytes([boot[36], boot[37], boot[38], boot[39]]);
    let root_cluster = u32::from_le_bytes([boot[44], boot[45], boot[46], boot[47]]);
    let mut total_sectors = u16::from_le_bytes([boot[19], boot[20]]) as u32;
    if total_sectors == 0 {
        total_sectors = u32::from_le_bytes([boot[32], boot[33], boot[34], boot[35]]);
    }

    if bytes_per_sector == 0 || sectors_per_cluster == 0 || fat_size_sectors == 0 {
        return Err(AppError::from("BPB FAT32 inválido"));
    }

    Ok(Fat32Fs {
        partition_offset,
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sectors,
        num_fats,
        fat_size_sectors,
        root_cluster,
        total_sectors,
    })
}

fn next_cluster(file: &mut File, fs: &Fat32Fs, cluster: u32) -> AppResult<u32> {
    let fat_off = fs.fat_offset() + cluster as u64 * 4;
    file.seek(SeekFrom::Start(fat_off))?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf) & 0x0FFF_FFFF)
}

fn walk_dir(
    file: &mut File,
    fs: &Fat32Fs,
    start_cluster: u32,
    prefix: &str,
    out: &mut Vec<RecoverableEntry>,
    depth: usize,
) -> AppResult<()> {
    if depth > 24 {
        return Ok(());
    }
    let records = read_dir_records(file, fs, start_cluster)?;
    let mut i = 0;
    while i < records.len() {
        let rec = &records[i];
        if rec[0] == 0x00 {
            break;
        }
        // LFN sequences
        let mut name = String::new();
        let mut deleted = false;
        while i < records.len() && records[i][11] == 0x0F {
            let lfn = &records[i];
            if lfn[0] == 0xE5 {
                deleted = true;
            }
            name = parse_lfn(lfn) + &name;
            i += 1;
        }
        if i >= records.len() {
            break;
        }
        let short = &records[i];
        i += 1;
        if short[0] == 0x00 {
            break;
        }
        if short[0] == 0xE5 {
            deleted = true;
        }
        let attr = short[11];
        if attr & 0x08 != 0 {
            continue; // volume label
        }
        if name.is_empty() {
            name = parse_sfn(short);
        }
        if name == "." || name == ".." || name.is_empty() {
            continue;
        }
        let cluster =
            u16::from_le_bytes([short[26], short[27]]) as u32
                | ((u16::from_le_bytes([short[20], short[21]]) as u32) << 16);
        let size = u32::from_le_bytes([short[28], short[29], short[30], short[31]]) as u64;
        let is_dir = attr & 0x10 != 0;
        let path = if prefix.ends_with('/') {
            format!("{prefix}{name}")
        } else {
            format!("{prefix}/{name}")
        };
        let locator = format!(
            "fat32:{}:{}:{}:{}",
            fs.partition_offset, cluster, size, name
        );
        out.push(RecoverableEntry {
            id: locator.clone(),
            name: name.clone(),
            path: path.clone(),
            size,
            is_dir,
            fs_type: "FAT32".into(),
            deleted,
            locator,
        });
        if is_dir && cluster >= 2 {
            walk_dir(file, fs, cluster, &path, out, depth + 1)?;
        }
    }
    Ok(())
}

fn read_dir_records(file: &mut File, fs: &Fat32Fs, start: u32) -> AppResult<Vec<[u8; 32]>> {
    let mut cluster = start;
    let mut records = Vec::new();
    let mut seen = 0u32;
    while cluster >= 2 && cluster < 0x0FFF_FFF8 && seen < 4096 {
        seen += 1;
        let off = fs.cluster_offset(cluster);
        file.seek(SeekFrom::Start(off))?;
        let mut buf = vec![0u8; fs.cluster_size() as usize];
        file.read_exact(&mut buf)?;
        for chunk in buf.chunks_exact(32) {
            let mut rec = [0u8; 32];
            rec.copy_from_slice(chunk);
            records.push(rec);
        }
        cluster = next_cluster(file, fs, cluster)?;
    }
    Ok(records)
}

fn parse_sfn(rec: &[u8]) -> String {
    let name = String::from_utf8_lossy(&rec[0..8]).trim_end().to_string();
    let ext = String::from_utf8_lossy(&rec[8..11]).trim_end().to_string();
    if ext.is_empty() {
        name
    } else {
        format!("{name}.{ext}")
    }
}

fn parse_lfn(rec: &[u8]) -> String {
    let mut chars = Vec::new();
    for range in [1..11, 14..26, 28..32] {
        for i in range.step_by(2) {
            if i + 1 >= rec.len() {
                break;
            }
            let c = u16::from_le_bytes([rec[i], rec[i + 1]]);
            if c == 0x0000 || c == 0xFFFF {
                return String::from_utf16_lossy(&chars);
            }
            chars.push(c);
        }
    }
    String::from_utf16_lossy(&chars)
}

pub fn export_fat32(image_path: &str, locator: &str, dest_path: &str) -> AppResult<u64> {
    let parts: Vec<&str> = locator.splitn(5, ':').collect();
    if parts.len() < 4 {
        return Err(AppError::from("Locator FAT32 inválido"));
    }
    let partition_offset: u64 = parts[1]
        .parse()
        .map_err(|_| AppError::from("offset inválido"))?;
    let cluster: u32 = parts[2]
        .parse()
        .map_err(|_| AppError::from("cluster inválido"))?;
    let size: u64 = parts[3]
        .parse()
        .map_err(|_| AppError::from("tamaño inválido"))?;

    let mut file = File::open(image_path)?;
    let fs = read_bpb(&mut file, partition_offset)?;
    if let Some(parent) = Path::new(dest_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest_path)?;
    let mut remaining = size;
    let mut cur = cluster;
    let mut written = 0u64;
    while remaining > 0 && cur >= 2 && cur < 0x0FFF_FFF8 {
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
    let _ = fs.total_sectors;
    Ok(written)
}

use super::RecoverableEntry;
use crate::error::{AppError, AppResult};
use ntfs::{Ntfs, NtfsFile, NtfsReadSeek};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Reader that presents a partition slice as a standalone filesystem stream.
struct OffsetReader {
    file: File,
    offset: u64,
    pos: u64,
    len: u64,
}

impl OffsetReader {
    fn open(path: &str, offset: u64) -> AppResult<Self> {
        let file = File::open(path)?;
        let meta_len = file.metadata()?.len().saturating_sub(offset);
        Ok(Self {
            file,
            offset,
            pos: 0,
            len: meta_len,
        })
    }
}

impl Read for OffsetReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.seek(SeekFrom::Start(self.offset + self.pos))?;
        let max = (self.len.saturating_sub(self.pos) as usize).min(buf.len());
        if max == 0 {
            return Ok(0);
        }
        let n = self.file.read(&mut buf[..max])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for OffsetReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => ((self.len as i64) + p).max(0) as u64,
            SeekFrom::Current(p) => ((self.pos as i64) + p).max(0) as u64,
        };
        self.pos = new.min(self.len);
        Ok(self.pos)
    }
}

pub fn list_ntfs(image_path: &str, partition_offset: u64) -> AppResult<Vec<RecoverableEntry>> {
    let mut reader = OffsetReader::open(image_path, partition_offset)?;
    let ntfs = Ntfs::new(&mut reader).map_err(|e| AppError::Message(format!("NTFS: {e}")))?;
    let root = ntfs
        .root_directory(&mut reader)
        .map_err(|e| AppError::Message(format!("NTFS root: {e}")))?;
    let mut entries = Vec::new();
    walk_dir(
        &ntfs,
        &mut reader,
        &root,
        "/",
        partition_offset,
        &mut entries,
        0,
    )?;
    Ok(entries)
}

fn walk_dir<'n, T: Read + Seek>(
    ntfs: &'n Ntfs,
    fs: &mut T,
    dir: &NtfsFile<'n>,
    prefix: &str,
    partition_offset: u64,
    out: &mut Vec<RecoverableEntry>,
    depth: usize,
) -> AppResult<()> {
    if depth > 20 || out.len() > 20_000 {
        return Ok(());
    }

    let index = match dir.directory_index(fs) {
        Ok(i) => i,
        Err(_) => return Ok(()),
    };
    let mut iter = index.entries();
    while let Some(entry) = iter.next(fs) {
        let Ok(entry) = entry else { continue };
        let Some(Ok(name_val)) = entry.key() else {
            continue;
        };
        let name = name_val.name().to_string_lossy().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let is_dir = name_val.is_directory();
        let size = name_val.data_size();
        let record_number = entry.file_reference().file_record_number();
        let path = if prefix.ends_with('/') {
            format!("{prefix}{name}")
        } else {
            format!("{prefix}/{name}")
        };
        let locator = format!("ntfs:{partition_offset}:{record_number}:{size}:{name}");
        out.push(RecoverableEntry {
            id: locator.clone(),
            name: name.clone(),
            path: path.clone(),
            size,
            is_dir,
            fs_type: "NTFS".into(),
            deleted: false,
            locator,
        });

        if is_dir {
            if let Ok(child) = entry.to_file(ntfs, fs) {
                let _ = walk_dir(ntfs, fs, &child, &path, partition_offset, out, depth + 1);
            }
        }
    }
    Ok(())
}

pub fn export_ntfs(image_path: &str, locator: &str, dest_path: &str) -> AppResult<u64> {
    let parts: Vec<&str> = locator.splitn(5, ':').collect();
    if parts.len() < 4 {
        return Err(AppError::from("Locator NTFS inválido"));
    }
    let partition_offset: u64 = parts[1].parse().map_err(|_| AppError::from("offset inválido"))?;
    let record_num: u64 = parts[2]
        .parse()
        .map_err(|_| AppError::from("MFT inválido"))?;

    let mut reader = OffsetReader::open(image_path, partition_offset)?;
    let ntfs = Ntfs::new(&mut reader).map_err(|e| AppError::Message(format!("NTFS: {e}")))?;
    let file = ntfs
        .file(&mut reader, record_num)
        .map_err(|e| AppError::Message(format!("MFT {record_num}: {e}")))?;

    let data_item = file
        .data(&mut reader, "")
        .ok_or_else(|| AppError::from("Sin atributo $DATA"))?
        .map_err(|e| AppError::Message(format!("$DATA: {e}")))?;
    let data_attribute = data_item
        .to_attribute()
        .map_err(|e| AppError::Message(format!("attr: {e}")))?;
    let mut data_value = data_attribute
        .value(&mut reader)
        .map_err(|e| AppError::Message(format!("data value: {e}")))?;

    if let Some(parent) = Path::new(dest_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest_path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = NtfsReadSeek::read(&mut data_value, &mut reader, &mut buf)
            .map_err(|e| AppError::Message(format!("lectura NTFS: {e}")))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        written += n as u64;
    }
    Ok(written)
}

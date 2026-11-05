#[cfg(test)]
mod tests {
    use crate::recovery::{detect_fs, FsKind};
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_img(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("sectora-fs-detect");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(name)
    }

    #[test]
    fn detects_ntfs_oem() {
        let path = temp_img("ntfs.bin");
        let mut boot = vec![0u8; 512];
        boot[3..11].copy_from_slice(b"NTFS    ");
        File::create(&path).unwrap().write_all(&boot).unwrap();
        assert_eq!(detect_fs(path.to_str().unwrap()).unwrap(), FsKind::Ntfs);
    }

    #[test]
    fn detects_exfat_oem() {
        let path = temp_img("exfat.bin");
        let mut boot = vec![0u8; 512];
        boot[3..11].copy_from_slice(b"EXFAT   ");
        File::create(&path).unwrap().write_all(&boot).unwrap();
        assert_eq!(detect_fs(path.to_str().unwrap()).unwrap(), FsKind::ExFat);
    }

    #[test]
    fn detects_fat32_label() {
        let path = temp_img("fat32.bin");
        let mut boot = vec![0u8; 512];
        boot[0x52..0x5A].copy_from_slice(b"FAT32   ");
        File::create(&path).unwrap().write_all(&boot).unwrap();
        assert_eq!(detect_fs(path.to_str().unwrap()).unwrap(), FsKind::Fat32);
    }
}

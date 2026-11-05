use crate::disk::DiskInfo;
use crate::error::AppResult;
use fs2::available_space;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneValidation {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub free_bytes: Option<u64>,
    pub required_bytes: u64,
    pub needs_internal_confirm: bool,
    pub needs_force_confirm: bool,
    pub summary: String,
}

const MARGIN_BYTES: u64 = 64 * 1024 * 1024;

pub fn validate_clone_plan(
    source: &DiskInfo,
    image_path: &str,
    force_overwrite: bool,
    confirm_internal: bool,
    confirm_force: bool,
) -> AppResult<CloneValidation> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let required = source.size_bytes.saturating_add(MARGIN_BYTES);

    let image = PathBuf::from(image_path);
    if image_path.trim().is_empty() {
        errors.push("La ruta de imagen está vacía.".into());
    }

    if paths_conflict(&source.path, image_path) {
        errors.push(
            "El destino no puede ser el mismo dispositivo/ruta que el origen. Elige un disco sano."
                .into(),
        );
    }

    if source.is_internal {
        warnings.push(
            "El origen parece un disco interno del sistema. Clonar el disco del sistema es peligroso y suele requerir arrancar desde otro medio."
                .into(),
        );
    }

    if force_overwrite {
        warnings.push(
            "Sobrescritura forzada (-f): se destruirá el contenido existente del archivo destino si existe."
                .into(),
        );
    } else if image.exists() {
        errors.push(
            "El archivo destino ya existe. Activa sobrescritura forzada (-f) solo si estás seguro."
                .into(),
        );
    }

    let free_bytes = free_space_for_image(&image);
    if let Some(free) = free_bytes {
        if free < required {
            errors.push(format!(
                "Espacio insuficiente en el destino: libre {}, se necesitan al menos {} (tamaño origen + margen).",
                crate::disk::format_size(free),
                crate::disk::format_size(required)
            ));
        }
    } else {
        warnings.push(
            "No se pudo medir el espacio libre del destino; verifica manualmente que quepa la imagen."
                .into(),
        );
    }

    let needs_internal_confirm = source.is_internal;
    let needs_force_confirm = force_overwrite;

    if needs_internal_confirm && !confirm_internal {
        errors.push(
            "Confirma explícitamente que quieres clonar un disco marcado como interno.".into(),
        );
    }
    if needs_force_confirm && !confirm_force {
        errors.push("Confirma explícitamente la sobrescritura forzada (-f).".into());
    }

    let ok = errors.is_empty();
    let summary = format!(
        "Origen: {} ({}) · Destino: {} · Requerido: {} · Libre: {}",
        source.name,
        crate::disk::format_size(source.size_bytes),
        image_path,
        crate::disk::format_size(required),
        free_bytes
            .map(crate::disk::format_size)
            .unwrap_or_else(|| "desconocido".into())
    );

    Ok(CloneValidation {
        ok,
        errors,
        warnings,
        free_bytes,
        required_bytes: required,
        needs_internal_confirm,
        needs_force_confirm,
        summary,
    })
}

fn paths_conflict(source: &str, image_path: &str) -> bool {
    let s = source.trim_end_matches('/');
    let i = image_path.trim_end_matches('/');
    if s.eq_ignore_ascii_case(i) {
        return true;
    }
    // Image path must not live on the raw device path prefix (e.g. writing into /dev/…)
    if i.starts_with(s) {
        return true;
    }
    // Windows physical drive vs path
    let s_l = s.to_ascii_lowercase();
    let i_l = i.to_ascii_lowercase();
    if s_l.contains("physicaldrive") && i_l.starts_with(&s_l) {
        return true;
    }
    false
}

fn free_space_for_image(image: &Path) -> Option<u64> {
    let probe = if image.exists() {
        image.to_path_buf()
    } else {
        image
            .parent()
            .filter(|p| p.as_os_str().len() > 0)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    available_space(&probe).ok()
}

pub fn map_io_error(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("permission") || e.contains("not permitted") || e.contains("access is denied") {
        return format!(
            "{err}\n\nAcción sugerida: concede privilegios de lectura de disco (Full Disk Access en macOS, ejecutar como administrador en Windows, grupo disk/sudo en Linux)."
        );
    }
    if e.contains("resource busy") || e.contains("device or resource busy") {
        return format!(
            "{err}\n\nAcción sugerida: desmonta los volúmenes del disco de origen e inténtalo de nuevo."
        );
    }
    if e.contains("no such file") || e.contains("not found") {
        return format!(
            "{err}\n\nAcción sugerida: verifica que el dispositivo sigue conectado y que la ruta es correcta."
        );
    }
    if e.contains("ddrescue") {
        return format!(
            "{err}\n\nAcción sugerida: ejecuta scripts/fetch-ddrescue.sh o reinstala el sidecar embebido."
        );
    }
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::DiskInfo;

    fn disk(path: &str, size: u64, internal: bool) -> DiskInfo {
        DiskInfo {
            id: "d".into(),
            name: "Test".into(),
            path: path.into(),
            size_bytes: size,
            model: None,
            is_removable: !internal,
            is_internal: internal,
            device_type: "disk".into(),
        }
    }

    #[test]
    fn rejects_same_path() {
        let d = disk("/dev/disk2", 1000, false);
        let v = validate_clone_plan(&d, "/dev/disk2", false, false, false).unwrap();
        assert!(!v.ok);
        assert!(v.errors.iter().any(|e| e.contains("mismo")));
    }

    #[test]
    fn requires_internal_confirm() {
        let d = disk("/dev/disk0", 1000, true);
        let tmp = std::env::temp_dir().join("sectora-safety-test.img");
        let _ = std::fs::remove_file(&tmp);
        let v = validate_clone_plan(&d, tmp.to_str().unwrap(), false, false, false).unwrap();
        assert!(!v.ok);
        assert!(v.needs_internal_confirm);
    }

    #[test]
    fn requires_force_confirm() {
        let d = disk("/dev/disk2", 1000, false);
        let tmp = std::env::temp_dir().join("sectora-safety-force.img");
        let _ = std::fs::remove_file(&tmp);
        let v =
            validate_clone_plan(&d, tmp.to_str().unwrap(), true, true, false).unwrap();
        assert!(!v.ok);
        assert!(v.needs_force_confirm);
    }
}

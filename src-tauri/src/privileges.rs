use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegeChecklist {
    pub os: String,
    pub title: String,
    pub steps: Vec<String>,
    pub notes: Vec<String>,
}

pub fn checklist() -> PrivilegeChecklist {
    #[cfg(target_os = "macos")]
    {
        PrivilegeChecklist {
            os: "macos".into(),
            title: "Privilegios en macOS (Apple Silicon)".into(),
            steps: vec![
                "Abre Ajustes del Sistema → Privacidad y seguridad → Acceso completo al disco.".into(),
                "Añade Sectora (o Terminal / Cursor si usas `tauri dev`) y actívalo.".into(),
                "Desmonta volúmenes del disco de origen en Finder o con `diskutil unmountDisk /dev/diskN`.".into(),
                "No escribas en el disco dañado; el destino .img debe estar en otro disco sano.".into(),
            ],
            notes: vec![
                "Sin Full Disk Access, la lectura de /dev/disk* suele fallar con 'Operation not permitted'.".into(),
                "Sectora solo soporta Apple Silicon (aarch64).".into(),
            ],
        }
    }
    #[cfg(target_os = "windows")]
    {
        PrivilegeChecklist {
            os: "windows".into(),
            title: "Privilegios en Windows".into(),
            steps: vec![
                "Ejecuta Sectora como administrador (clic derecho → Ejecutar como administrador).".into(),
                "Acepta el aviso UAC cuando Windows lo solicite.".into(),
                "Asegúrate de que el destino .img esté en otro disco sano con espacio suficiente.".into(),
                "Cierra exploradores o herramientas que tengan bloqueado el volumen de origen si aparece 'access denied'.".into(),
            ],
            notes: vec![
                "El acceso raw usa \\\\.\\PhysicalDriveN y requiere elevación.".into(),
                "SmartScreen puede avisar porque el instalador no está firmado.".into(),
            ],
        }
    }
    #[cfg(target_os = "linux")]
    {
        PrivilegeChecklist {
            os: "linux".into(),
            title: "Privilegios en Linux".into(),
            steps: vec![
                "Añade tu usuario al grupo `disk` (`sudo usermod -aG disk $USER`) y vuelve a iniciar sesión.".into(),
                "O ejecuta con los permisos necesarios vía polkit/sudo según tu distribución.".into(),
                "Desmonta particiones del disco de origen (`umount`, o desde tu gestor de discos).".into(),
                "Verifica que el destino .img esté en un sistema de archivos con espacio suficiente.".into(),
            ],
            notes: vec![
                "Leer /dev/sd* o /dev/nvme* sin permisos produce EACCES.".into(),
                "Instala gddrescue en el sistema si desarrollas sin sidecar embebido.".into(),
            ],
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        PrivilegeChecklist {
            os: "unknown".into(),
            title: "Privilegios".into(),
            steps: vec!["Sistema no soportado.".into()],
            notes: vec![],
        }
    }
}

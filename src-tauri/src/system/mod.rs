//! Informações do PC (pra recomendar perfil), ponto de restauração e utilidades.

use serde::Serialize;

#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub windows: String,
    pub build: u32,
    pub is_laptop: bool,
    pub ram_gb: u32,
    pub system_disk: DiskKind,
    pub is_admin: bool,
    pub recommended_profile: &'static str,
    /// Por que esse perfil foi recomendado, em uma frase.
    pub reason: String,
}

/// Regra de recomendação. Pura, pra dar pra testar.
pub fn recommend(is_laptop: bool, ram_gb: u32, disk: DiskKind) -> (&'static str, String) {
    if is_laptop {
        return ("notebook", "Notebook detectado: nada que esquente ou gaste bateria.".into());
    }
    if ram_gb > 0 && ram_gb < 12 {
        return ("pc_modesto", format!("{ram_gb} GB de RAM: vale tirar o peso visual da interface."));
    }
    if disk == DiskKind::Hdd {
        return ("pc_modesto", "Windows num HD mecânico: menos coisa carregando ajuda.".into());
    }
    ("competitivo", format!("Desktop com {ram_gb} GB de RAM e SSD: dá pra ir no máximo."))
}

/// Interpreta o MediaType do Get-PhysicalDisk ("SSD", "HDD", "Unspecified").
pub fn parse_media_type(s: &str) -> DiskKind {
    match s.trim().to_ascii_uppercase().as_str() {
        "SSD" => DiskKind::Ssd,
        "HDD" => DiskKind::Hdd,
        _ => DiskKind::Unknown,
    }
}

#[cfg(windows)]
pub use windows::{create_restore_point, info, is_admin, is_laptop, powershell, restart_explorer};

#[cfg(not(windows))]
pub fn info() -> SystemInfo {
    let (recommended_profile, reason) = recommend(false, 0, DiskKind::Unknown);
    SystemInfo {
        windows: "—".into(),
        build: 0,
        is_laptop: false,
        ram_gb: 0,
        system_disk: DiskKind::Unknown,
        is_admin: false,
        recommended_profile,
        reason,
    }
}

#[cfg(not(windows))]
pub fn is_admin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn is_laptop() -> bool {
    false
}

#[cfg(not(windows))]
pub fn create_restore_point() -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn restart_explorer() -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recomendacao() {
        assert_eq!(recommend(true, 32, DiskKind::Ssd).0, "notebook");
        assert_eq!(recommend(false, 8, DiskKind::Ssd).0, "pc_modesto");
        assert_eq!(recommend(false, 32, DiskKind::Hdd).0, "pc_modesto");
        assert_eq!(recommend(false, 32, DiskKind::Ssd).0, "competitivo");
        // RAM desconhecida (0) não deve rebaixar o perfil.
        assert_eq!(recommend(false, 0, DiskKind::Unknown).0, "competitivo");
    }

    #[test]
    fn tipo_de_disco() {
        assert_eq!(parse_media_type("SSD\r\n"), DiskKind::Ssd);
        assert_eq!(parse_media_type("hdd"), DiskKind::Hdd);
        assert_eq!(parse_media_type("Unspecified"), DiskKind::Unknown);
        assert_eq!(parse_media_type(""), DiskKind::Unknown);
    }
}

//! Informações do PC (pra recomendar perfil), ponto de restauração e utilidades.

use serde::Serialize;

#[cfg(windows)]
mod windows;

// ---------------- Texto que sai dos comandos do Windows ----------------

/// Converte a saída de um programa de console em texto.
///
/// Ferramentas como `ipconfig`, `powercfg`, `netsh` e o próprio PowerShell, quando a
/// saída vai pra um pipe, escrevem na *code page OEM* do console (CP850 no Windows em
/// português), não em UTF-8. Ler como UTF-8 vira "opera��o". Ordem de tentativa:
/// 1. UTF-16 (o `sfc` muda pra UTF-16 quando redirecionado: metade dos bytes é 0);
/// 2. UTF-8 válido (ASCII puro também cai aqui);
/// 3. code page OEM (`oem` é quem sabe converter; no Windows, a API do sistema).
pub fn decode_console_with(bytes: &[u8], oem: impl FnOnce(&[u8]) -> String) -> String {
    let zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 4 && zeros * 2 >= bytes.len() / 2 {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units).trim_start_matches('\u{feff}').to_string();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => oem(bytes),
    }
}

pub fn decode_console(bytes: &[u8]) -> String {
    #[cfg(windows)]
    {
        decode_console_with(bytes, windows::oem_to_string)
    }
    #[cfg(not(windows))]
    {
        decode_console_with(bytes, |b| String::from_utf8_lossy(b).into_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    Ssd,
    Hdd,
    Unknown,
}

/// Fatos do hardware que não mudam com o PC ligado. Lidos uma vez e guardados em
/// cache (`SystemCache`): o tipo do disco custa um PowerShell (~0,5–1 s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProfile {
    pub windows: String,
    /// 0 = não deu pra ler.
    pub build: u32,
    pub is_laptop: bool,
    /// RAM INSTALADA em GB (0 = não deu pra ler).
    pub ram_gb: u32,
    pub system_disk: DiskKind,
}

#[derive(Default)]
pub struct SystemCache(std::sync::Mutex<Option<SystemProfile>>);

impl SystemCache {
    /// `refresh`: botão "Reanalisar" (ex.: trocou o disco do sistema).
    pub fn get(&self, refresh: bool) -> SystemProfile {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if refresh || slot.is_none() {
            *slot = Some(read_profile());
        }
        slot.clone().expect("preenchido acima")
    }
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
    /// Monitor principal: taxa atual e a maior disponível na resolução atual. Lido na
    /// hora (fora do cache): muda quando se troca de monitor.
    pub display_hz: Option<u32>,
    pub max_display_hz: Option<u32>,
}

/// Junta o perfil (cache) com o que é lido na hora.
pub fn build_info(p: &SystemProfile, is_admin: bool, display: Option<(u32, u32)>) -> SystemInfo {
    let (recommended_profile, reason) = recommend(p.is_laptop, p.ram_gb, p.system_disk);
    SystemInfo {
        windows: p.windows.clone(),
        build: p.build,
        is_laptop: p.is_laptop,
        ram_gb: p.ram_gb,
        system_disk: p.system_disk,
        is_admin,
        recommended_profile,
        reason,
        display_hz: display.map(|d| d.0),
        max_display_hz: display.map(|d| d.1),
    }
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
pub use windows::{is_admin, powershell, ram_used, read_profile, restart_explorer, running_services};

#[cfg(not(windows))]
pub fn ram_used() -> Option<u64> {
    None
}
#[cfg(not(windows))]
pub fn running_services() -> Option<usize> {
    None
}

/// hiberfil.sys existe no disco do sistema? O arquivo fica bloqueado pelo Windows:
/// "acesso negado" também significa que ele existe.
pub fn hiberfil_present() -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    match std::fs::metadata(format!(r"{drive}\hiberfil.sys")) {
        Ok(_) => Some(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => Some(true),
    }
}

#[cfg(not(windows))]
pub fn read_profile() -> SystemProfile {
    SystemProfile { windows: "—".into(), build: 0, is_laptop: false, ram_gb: 0, system_disk: DiskKind::Unknown }
}

#[cfg(not(windows))]
pub fn is_admin() -> bool {
    false
}

#[cfg(not(windows))]
pub fn restart_explorer() -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    /// Pedaço da CP850 suficiente pro teste (a real vem do Windows).
    fn cp850(b: &[u8]) -> String {
        b.iter()
            .map(|&c| match c {
                0x87 => 'ç',
                0xC6 => 'ã',
                0x82 => 'é',
                c if c < 0x80 => c as char,
                _ => '?',
            })
            .collect()
    }

    #[test]
    fn saida_do_console_em_cp850() {
        // "A operação solicitada requer elevação." como o ipconfig escreve no pipe.
        let mut raw = b"A opera".to_vec();
        raw.extend([0x87, 0xC6]);
        raw.extend(b"o solicitada requer eleva");
        raw.extend([0x87, 0xC6]);
        raw.extend(b"o.");
        assert_eq!(super::decode_console_with(&raw, cp850), "A operação solicitada requer elevação.");
        // UTF-8 válido não passa pela code page.
        assert_eq!(super::decode_console_with("Máximo".as_bytes(), |_| unreachable!()), "Máximo");
        // UTF-16 do sfc.
        let utf16: Vec<u8> = "Verificação".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(super::decode_console_with(&utf16, |_| unreachable!()), "Verificação");
    }

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

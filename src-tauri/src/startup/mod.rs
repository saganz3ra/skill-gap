//! Programas de inicialização.
//!
//! Decisão de projeto: para desativar um item NÃO apagamos a entrada `Run` nem o atalho.
//! Fazemos o mesmo que o Gerenciador de Tarefas: gravamos um marcador em
//! `...\Explorer\StartupApproved`. Vantagens:
//! - é reversível sem precisar guardar backup do comando original;
//! - o Windows e o Gerenciador de Tarefas enxergam o mesmo estado que o app;
//! - se o programa se reinstalar e recriar a entrada, o marcador continua valendo.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod windows;

/// De onde o item é carregado no boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupSource {
    /// HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    UserRun,
    /// HKLM\Software\Microsoft\Windows\CurrentVersion\Run
    MachineRun,
    /// HKLM\Software\WOW6432Node\...\Run (programas 32 bits)
    MachineRun32,
    /// %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
    UserFolder,
    /// %ProgramData%\Microsoft\Windows\Start Menu\Programs\StartUp
    CommonFolder,
}

impl StartupSource {
    #[allow(dead_code)] // usado pela implementação Windows
    pub const ALL: [StartupSource; 5] = [
        StartupSource::UserRun,
        StartupSource::MachineRun,
        StartupSource::MachineRun32,
        StartupSource::UserFolder,
        StartupSource::CommonFolder,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            StartupSource::UserRun => "userRun",
            StartupSource::MachineRun => "machineRun",
            StartupSource::MachineRun32 => "machineRun32",
            StartupSource::UserFolder => "userFolder",
            StartupSource::CommonFolder => "commonFolder",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL_KEYS.iter().find(|(k, _)| *k == key).map(|(_, s)| *s)
    }

    const ALL_KEYS: [(&'static str, StartupSource); 5] = [
        ("userRun", StartupSource::UserRun),
        ("machineRun", StartupSource::MachineRun),
        ("machineRun32", StartupSource::MachineRun32),
        ("userFolder", StartupSource::UserFolder),
        ("commonFolder", StartupSource::CommonFolder),
    ];

    pub const fn label(self) -> &'static str {
        match self {
            StartupSource::UserRun => r"HKCU\…\Run",
            StartupSource::MachineRun => r"HKLM\…\Run",
            StartupSource::MachineRun32 => r"HKLM\…\Run (32 bits)",
            StartupSource::UserFolder => "Pasta Startup",
            StartupSource::CommonFolder => "Pasta Startup (todos)",
        }
    }

    /// Itens em HKLM e na pasta comum valem para todos os usuários: alterar exige admin.
    pub const fn requires_admin(self) -> bool {
        matches!(
            self,
            StartupSource::MachineRun | StartupSource::MachineRun32 | StartupSource::CommonFolder
        )
    }

    pub const fn is_folder(self) -> bool {
        matches!(self, StartupSource::UserFolder | StartupSource::CommonFolder)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    /// Identificador estável: "<origem>::<nome do valor ou arquivo>".
    pub id: String,
    pub name: String,
    pub command: String,
    pub source: StartupSource,
    pub source_label: &'static str,
    pub enabled: bool,
    pub requires_admin: bool,
}

impl StartupItem {
    #[allow(dead_code)] // usado pela implementação Windows
    pub fn new(source: StartupSource, raw_name: &str, command: String, approved: Option<&[u8]>) -> Self {
        Self {
            id: make_id(source, raw_name),
            name: display_name(source, raw_name),
            command,
            source,
            source_label: source.label(),
            enabled: is_enabled(approved),
            requires_admin: source.requires_admin(),
        }
    }
}

pub fn make_id(source: StartupSource, raw_name: &str) -> String {
    format!("{}::{}", source.key(), raw_name)
}

pub fn parse_id(id: &str) -> Option<(StartupSource, &str)> {
    let (key, name) = id.split_once("::")?;
    if name.is_empty() {
        return None;
    }
    Some((StartupSource::from_key(key)?, name))
}

/// Na pasta Startup o nome é o arquivo ("Discord.lnk"); mostramos sem a extensão.
pub fn display_name(source: StartupSource, raw_name: &str) -> String {
    if source.is_folder() {
        if let Some((stem, _ext)) = raw_name.rsplit_once('.') {
            if !stem.is_empty() {
                return stem.to_string();
            }
        }
    }
    raw_name.to_string()
}

/// O valor em StartupApproved tem 12 bytes. O primeiro byte indica o estado:
/// par (0x02, 0x06) = ativado, ímpar (0x03, 0x07) = desativado.
/// Sem valor gravado = ativado (o Windows só cria o marcador quando alguém mexe).
pub fn is_enabled(approved: Option<&[u8]>) -> bool {
    match approved.and_then(|b| b.first()) {
        None => true,
        Some(first) => first & 1 == 0,
    }
}

/// Monta o valor de 12 bytes. Ao desativar, os bytes 4..12 guardam o FILETIME do
/// momento, que é o que o Gerenciador de Tarefas mostra como "desativado em".
pub fn approved_value(enabled: bool, filetime: u64) -> [u8; 12] {
    let mut bytes = [0u8; 12];
    if enabled {
        bytes[0] = 0x02;
    } else {
        bytes[0] = 0x03;
        bytes[4..12].copy_from_slice(&filetime.to_le_bytes());
    }
    bytes
}

/// FILETIME = intervalos de 100 ns desde 01/01/1601 (UTC).
pub fn filetime_from_unix(secs: u64, subsec_nanos: u32) -> u64 {
    const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
    (secs + EPOCH_DIFF_SECS) * 10_000_000 + u64::from(subsec_nanos) / 100
}

#[cfg(windows)]
pub use windows::{list, set_enabled};

#[cfg(not(windows))]
pub fn list() -> crate::error::AppResult<Vec<StartupItem>> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn set_enabled(_id: &str, _enabled: bool) -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_marcador_e_ativado() {
        assert!(is_enabled(None));
        assert!(is_enabled(Some(&[])));
    }

    #[test]
    fn byte_par_ativado_impar_desativado() {
        assert!(is_enabled(Some(&[0x02, 0, 0, 0])));
        assert!(is_enabled(Some(&[0x06, 0, 0, 0])));
        assert!(!is_enabled(Some(&[0x03, 0, 0, 0])));
        assert!(!is_enabled(Some(&[0x07, 0, 0, 0])));
    }

    #[test]
    fn valor_gerado_e_lido_de_volta() {
        let ft = filetime_from_unix(1_763_380_800, 0);
        let off = approved_value(false, ft);
        assert_eq!(off.len(), 12);
        assert!(!is_enabled(Some(&off)));
        assert_eq!(u64::from_le_bytes(off[4..12].try_into().unwrap()), ft);

        let on = approved_value(true, ft);
        assert!(is_enabled(Some(&on)));
        assert_eq!(&on[1..], &[0u8; 11]);
    }

    #[test]
    fn filetime_da_epoch_unix() {
        // 01/01/1970 em FILETIME é um valor conhecido.
        assert_eq!(filetime_from_unix(0, 0), 116_444_736_000_000_000);
    }

    #[test]
    fn id_ida_e_volta() {
        let id = make_id(StartupSource::UserRun, "Steam");
        assert_eq!(id, "userRun::Steam");
        assert_eq!(parse_id(&id), Some((StartupSource::UserRun, "Steam")));
        // nome com "::" dentro continua inteiro
        assert_eq!(parse_id("userRun::a::b"), Some((StartupSource::UserRun, "a::b")));
        assert_eq!(parse_id("desconhecido::x"), None);
        assert_eq!(parse_id("userRun::"), None);
        assert_eq!(parse_id("semseparador"), None);
    }

    #[test]
    fn nome_de_atalho_sem_extensao() {
        assert_eq!(display_name(StartupSource::UserFolder, "Battle.net.lnk"), "Battle.net");
        assert_eq!(display_name(StartupSource::UserRun, "Battle.net"), "Battle.net");
        assert_eq!(display_name(StartupSource::UserFolder, ".lnk"), ".lnk");
    }

    #[test]
    fn todas_as_origens_tem_chave_reversivel() {
        for s in StartupSource::ALL {
            assert_eq!(StartupSource::from_key(s.key()), Some(s));
        }
    }
}

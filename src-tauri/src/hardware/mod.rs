//! Ajustes por dispositivo e por jogo (Fase 6). Cada um guarda o valor anterior no
//! diário `hardware.json` ANTES de mexer, inclusive "não existia".
//!
//! - Modo MSI por dispositivo PCI (vídeo, rede, armazenamento, USB xHCI, áudio HD).
//! - Tela cheia clássica por jogo (flag de compatibilidade do Windows).
//! - Prioridade de rede por jogo (política QoS com DSCP 46).
//! - Propriedades avançadas da placa de rede pelo `RegistryKeyword` padrão.
//!
//! Identificadores que vão pro Windows (InstanceId, nome do adaptador, exe) SEMPRE vêm
//! de uma listagem feita pelo backend na hora, nunca do texto que o front mandou.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

// ---------------- Modo MSI ----------------

/// Classes de dispositivo onde o MSI costuma fazer diferença.
pub const MSI_CLASSES: &[&str] = &["Display", "Net", "SCSIAdapter", "HDC", "USB", "MEDIA"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsiDevice {
    pub instance_id: String,
    pub name: String,
    pub class: String,
    /// `MSISupported` atual (`None` = valor não existe: o driver decide).
    pub msi: Option<u32>,
}

/// `PCI\VEN_10DE&DEV_2803&...\4&...`: só caracteres que o Windows usa em InstanceId.
pub fn is_pci_instance(id: &str) -> bool {
    id.len() < 200
        && id.to_ascii_uppercase().starts_with(r"PCI\")
        && !id.contains("..")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || r"\&_-.".contains(c))
}

/// USB só conta se for controladora xHCI; áudio só HD Audio (os outros não são PCI MSI).
pub fn msi_relevant(class: &str, name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    match class {
        "USB" => n.contains("xhci") || n.contains("usb 3") || n.contains("extensible host"),
        "MEDIA" => n.contains("high definition audio") || n.contains("áudio de alta definição"),
        c => MSI_CLASSES.contains(&c),
    }
}

pub fn msi_path(instance_id: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Enum\{instance_id}\Device Parameters\Interrupt Management\MessageSignaledInterruptProperties")
}

// ---------------- Tela cheia clássica por jogo ----------------

pub const FSE_FLAG: &str = "DISABLEDXMAXIMIZEDWINDOWEDMODE";
/// Preferência de GPU por app (Configurações › Vídeo › Gráficos). 2 = alto desempenho.
pub const GPU_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
pub const GPU_HIGH: &str = "GpuPreference=2;";
pub const LAYERS: &str = r"Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers";

/// Junta a flag às que já existem (ex.: `~ RUNASADMIN`) sem duplicar.
pub fn add_flag(existing: Option<&str>) -> String {
    let mut flags: Vec<&str> = existing.unwrap_or("").split_whitespace().filter(|f| *f != "~").collect();
    if !flags.iter().any(|f| f.eq_ignore_ascii_case(FSE_FLAG)) {
        flags.push(FSE_FLAG);
    }
    format!("~ {}", flags.join(" "))
}

pub fn has_flag(existing: Option<&str>) -> bool {
    existing.is_some_and(|v| v.split_whitespace().any(|f| f.eq_ignore_ascii_case(FSE_FLAG)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: String,
    pub name: String,
    pub install_dir: String,
    /// Executável principal escolhido (caminho completo), se achado.
    pub exe: Option<String>,
    pub classic_fullscreen: bool,
    pub qos: bool,
    /// Windows usa a GPU de alto desempenho neste jogo (útil em notebook com 2 GPUs).
    pub high_perf_gpu: bool,
}

/// Lê `"chave" "valor"` de arquivos VDF/ACF da Steam (o suficiente para path/name/installdir).
pub fn vdf_values<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    text.lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split('"').collect();
            // linha `"chave"\t\t"valor"` → ["", chave, "\t\t", valor, ""]
            (parts.len() >= 5 && parts[1].eq_ignore_ascii_case(key)).then(|| parts[3])
        })
        .collect()
}

/// Ferramentas da Steam que não são jogos.
pub fn is_steam_tool(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["redistributable", "proton", "steamvr", "steamworks", "steam linux runtime", "dedicated server", "sdk"].iter().any(|t| n.contains(t))
}

/// Pontua um .exe candidato a principal: pastas típicas de binário ganham; maior vence o empate.
pub fn exe_score(rel_path: &str, size: u64) -> Option<(u32, u64)> {
    let lower = rel_path.to_ascii_lowercase().replace('/', "\\");
    let file = lower.rsplit('\\').next().unwrap_or(&lower);
    if !file.ends_with(".exe") || crate::gaming::detect::is_ignored(file) {
        return None;
    }
    let mut score = 0;
    for hint in ["win64", "binaries", "shipping", "bin\\x64", "x64"] {
        if lower.contains(hint) {
            score += 1;
        }
    }
    Some((score, size))
}

pub fn pick_main_exe(candidates: &[(String, u64)]) -> Option<String> {
    candidates.iter().filter_map(|(p, s)| exe_score(p, *s).map(|sc| (sc, p))).max_by(|a, b| a.0.cmp(&b.0)).map(|(_, p)| p.clone())
}

// ---------------- QoS e placa de rede ----------------

/// Nome da política QoS do app para um exe (só nomes de arquivo simples).
pub fn qos_name(exe: &str) -> Option<String> {
    let ok = exe.to_ascii_lowercase().ends_with(".exe")
        && exe.len() < 100
        && exe.chars().all(|c| c.is_ascii_alphanumeric() || "._- ".contains(c));
    ok.then(|| format!("SkillGap-{exe}"))
}

/// Propriedades avançadas que o app mexe (keyword padrão da Microsoft, igual em
/// qualquer idioma do driver).
pub const NIC_KEYWORDS: &[(&str, &str)] = &[
    ("*InterruptModeration", "Moderação de interrupções"),
    ("*EEE", "Ethernet com eficiência energética"),
    ("*FlowControl", "Controle de fluxo"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NicProperty {
    pub adapter: String,
    pub keyword: String,
    pub label: String,
    pub value: String,
    /// Valores que ESTA placa aceita (`ValidRegistryValues`).
    pub valid: Vec<String>,
    /// Nome de cada valor no idioma do driver (só exibição).
    pub display: Vec<String>,
}

pub fn nic_value_ok(prop: &NicProperty, value: &str) -> bool {
    NIC_KEYWORDS.iter().any(|(k, _)| *k == prop.keyword) && prop.valid.iter().any(|v| v == value)
}

// ---------------- Diário ----------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Journal {
    /// InstanceId → `MSISupported` anterior (`None` = não existia).
    pub msi: BTreeMap<String, Option<u32>>,
    /// Caminho do exe → valor anterior em AppCompatFlags\Layers.
    pub fse: BTreeMap<String, Option<String>>,
    /// exe → criamos a política QoS.
    pub qos: BTreeMap<String, bool>,
    /// "adaptador|keyword" → valor anterior.
    pub nic: BTreeMap<String, String>,
    /// Caminho do exe → preferência de GPU anterior.
    pub gpu: BTreeMap<String, Option<String>>,
    /// O app colocou o usuário no grupo "Usuários de log de desempenho".
    pub perf_log_users: bool,
    /// "Do not use NLA" antes da primeira política QoS (`Some(None)` = não existia).
    pub nla: Option<Option<String>>,
}

pub struct HardwareState {
    path: PathBuf,
    journal: Mutex<Journal>,
}

impl HardwareState {
    pub fn load(path: PathBuf) -> Self {
        let journal = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, journal: Mutex::new(journal) }
    }

    pub fn snapshot(&self) -> Journal {
        self.journal.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Anota (só se ainda não houver foto: a primeira é o valor original) e grava ANTES de mexer.
    pub fn record(&self, change: impl FnOnce(&mut Journal)) -> AppResult<()> {
        let mut j = self.journal.lock().unwrap_or_else(|e| e.into_inner());
        change(&mut j);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&*j).expect("serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

pub fn invalid(msg: &str) -> AppError {
    AppError::InvalidInput(msg.into())
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod stubs {
    use super::*;
    pub fn msi_devices() -> AppResult<Vec<MsiDevice>> {
        Err(AppError::Unsupported)
    }
    pub fn msi_write(_id: &str, _v: Option<u32>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn steam_games() -> Vec<SteamGame> {
        Vec::new()
    }
    pub fn gpu_read(_exe: &str) -> Option<String> {
        None
    }
    pub fn gpu_write(_exe: &str, _v: Option<&str>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn fse_read(_exe: &str) -> Option<String> {
        None
    }
    pub fn fse_write(_exe: &str, _v: Option<&str>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn qos_set(_exe: &str, _on: bool) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn qos_exists(_exe: &str) -> bool {
        false
    }
    pub fn nla_read() -> Option<String> {
        None
    }
    pub fn nla_write(_v: Option<&str>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn nic_properties() -> AppResult<Vec<NicProperty>> {
        Err(AppError::Unsupported)
    }
    pub fn nic_write(_adapter: &str, _keyword: &str, _value: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
}
#[cfg(not(windows))]
pub use stubs::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_id_so_pci_e_sem_truques() {
        assert!(is_pci_instance(r"PCI\VEN_10DE&DEV_2803&SUBSYS_40F11458&REV_A1\4&2f2b1d6f&0&0008"));
        assert!(!is_pci_instance(r"USB\VID_046D&PID_C539\5&1"));
        assert!(!is_pci_instance(r"PCI\VEN_1\..\..\Services"));
        assert!(!is_pci_instance(r"PCI\VEN_1 & del"));
    }

    #[test]
    fn so_dispositivos_que_importam() {
        assert!(msi_relevant("Display", "NVIDIA GeForce RTX 4060 Ti"));
        assert!(msi_relevant("USB", "USB xHCI Compliant Host Controller"));
        assert!(!msi_relevant("MEDIA", "Steam Streaming Speakers"));
        assert!(!msi_relevant("Keyboard", "HID Keyboard"));
    }

    #[test]
    fn flag_de_compatibilidade_junta_sem_duplicar() {
        assert_eq!(add_flag(None), "~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(add_flag(Some("~ RUNASADMIN")), "~ RUNASADMIN DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert_eq!(add_flag(Some("~ DISABLEDXMAXIMIZEDWINDOWEDMODE")), "~ DISABLEDXMAXIMIZEDWINDOWEDMODE");
        assert!(has_flag(Some("~ RUNASADMIN DISABLEDXMAXIMIZEDWINDOWEDMODE")));
        assert!(!has_flag(Some("~ RUNASADMIN")));
    }

    #[test]
    fn vdf_e_acf_da_steam() {
        let lib = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}";
        assert_eq!(vdf_values(lib, "path"), [r"C:\\Program Files (x86)\\Steam", r"D:\\SteamLibrary"]);
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n}";
        assert_eq!(vdf_values(acf, "name"), ["Counter-Strike 2"]);
        assert!(is_steam_tool("Steamworks Common Redistributables"));
        assert!(is_steam_tool("Proton 9.0"));
        assert!(!is_steam_tool("Hades"));
    }

    #[test]
    fn escolhe_o_exe_principal() {
        let c = vec![
            (r"UnrealEngine\Engine\Binaries\Win64\CrashReportClient.exe".to_string(), 30_000_000u64),
            (r"Game\Binaries\Win64\Game-Win64-Shipping.exe".to_string(), 90_000_000),
            (r"Game.exe".to_string(), 500_000),
            (r"_CommonRedist\vcredist\vc_redist.x64.exe".to_string(), 25_000_000),
        ];
        assert_eq!(pick_main_exe(&c).as_deref(), Some(r"Game\Binaries\Win64\Game-Win64-Shipping.exe"));
        assert_eq!(pick_main_exe(&[(r"Hades.exe".into(), 1), (r"Hades2.exe".into(), 2)]).as_deref(), Some("Hades2.exe"), "empate: o maior");
    }

    #[test]
    fn qos_e_placa_de_rede_so_com_valores_conhecidos() {
        assert_eq!(qos_name("cs2.exe").as_deref(), Some("SkillGap-cs2.exe"));
        assert!(qos_name("cs2.exe; Remove-NetQosPolicy").is_none());
        let p = NicProperty {
            adapter: "Ethernet".into(),
            keyword: "*EEE".into(),
            label: "x".into(),
            value: "1".into(),
            valid: vec!["0".into(), "1".into()],
            display: vec![],
        };
        assert!(nic_value_ok(&p, "0"));
        assert!(!nic_value_ok(&p, "2"), "valor que a placa não aceita");
        assert!(!nic_value_ok(&NicProperty { keyword: "*SpeedDuplex".into(), ..p }, "0"), "keyword fora da lista");
    }
}

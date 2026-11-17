//! Drivers: inventário, dispositivos com problema, links oficiais e backup.
//!
//! Regra de ouro: o Skill Gap NÃO baixa driver. Ele aponta pro site oficial do
//! fabricante ou pro Windows Update, e só abre URLs de uma lista fixa.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfo {
    pub name: String,
    pub version: String,
    /// AAAA-MM-DD
    pub date: String,
    pub provider: String,
    /// Classe do dispositivo: DISPLAY, NET, MEDIA...
    pub class: String,
    /// Página oficial pra baixar, quando o fabricante é conhecido.
    pub download_url: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDevice {
    pub name: String,
    pub code: u32,
    pub class: String,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub drivers: Vec<DriverInfo>,
    pub problems: Vec<ProblemDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub path: String,
    pub count: u32,
}

/// Únicas URLs que o app abre. Qualquer outra é recusada.
pub const WINDOWS_UPDATE: &str = "ms-settings:windowsupdate";
const VENDORS: &[(&str, &str)] = &[
    ("nvidia", "https://www.nvidia.com/Download/index.aspx"),
    ("amd", "https://www.amd.com/en/support/download/drivers.html"),
    ("advanced micro devices", "https://www.amd.com/en/support/download/drivers.html"),
    ("intel", "https://www.intel.com/content/www/us/en/support/detect.html"),
    ("realtek", "https://www.realtek.com/Download/Index"),
];

pub fn vendor_url(provider: &str) -> Option<&'static str> {
    let p = provider.to_lowercase();
    VENDORS.iter().find(|(key, _)| p.contains(key)).map(|(_, url)| *url)
}

pub fn is_allowed_url(url: &str) -> bool {
    url == WINDOWS_UPDATE || VENDORS.iter().any(|(_, u)| *u == url) || crate::apps::is_store_url(url)
}

/// Código do Gerenciador de Dispositivos → explicação curta.
pub fn problem_text(code: u32) -> &'static str {
    match code {
        1 => "Configurado incorretamente",
        3 => "Driver corrompido ou sem memória",
        10 => "O dispositivo não consegue iniciar",
        12 => "Sem recursos livres (conflito)",
        18 | 28 => "Driver não instalado",
        19 => "Configuração do registro corrompida",
        22 => "Desativado manualmente",
        24 => "Dispositivo ausente ou com defeito",
        31 => "Driver não carregou",
        39 => "Driver corrompido ou ausente",
        43 => "O Windows parou o dispositivo após um erro",
        45 => "Dispositivo desconectado",
        52 => "Assinatura digital do driver inválida",
        _ => "Problema no dispositivo",
    }
}

// ---------------- Leitura da saída do PowerShell ----------------

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawDriver {
    device_name: Option<String>,
    driver_version: Option<String>,
    date: Option<String>,
    driver_provider_name: Option<String>,
    device_class: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawProblem {
    name: Option<String>,
    config_manager_error_code: Option<u32>,
    #[serde(rename = "PNPClass")]
    pnp_class: Option<String>,
}

/// O ConvertTo-Json do PowerShell devolve um OBJETO quando há 1 item, uma LISTA quando
/// há vários e nada quando não há nenhum. Aceita as três formas.
fn parse_list<T: for<'de> Deserialize<'de>>(json: &str) -> AppResult<Vec<T>> {
    let json = json.trim();
    if json.is_empty() || json == "null" {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| AppError::CommandFailed { command: "inventário".into(), detail: e.to_string() })?;
    let items = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

pub fn parse_drivers(json: &str) -> AppResult<Vec<DriverInfo>> {
    let mut list: Vec<DriverInfo> = parse_list::<RawDriver>(json)?
        .into_iter()
        .filter_map(|r| {
            let name = r.device_name?.trim().to_string();
            let provider = r.driver_provider_name.unwrap_or_default();
            Some(DriverInfo {
                download_url: vendor_url(&provider),
                name,
                version: r.driver_version.unwrap_or_default(),
                date: r.date.unwrap_or_default(),
                provider,
                class: r.device_class.unwrap_or_default().to_uppercase(),
            })
        })
        .collect();
    list.sort_by(|a, b| a.class.cmp(&b.class).then(a.name.cmp(&b.name)));
    Ok(list)
}

pub fn parse_problems(json: &str) -> AppResult<Vec<ProblemDevice>> {
    Ok(parse_list::<RawProblem>(json)?
        .into_iter()
        .filter_map(|r| {
            let code = r.config_manager_error_code?;
            (code != 0).then(|| ProblemDevice {
                name: r.name.unwrap_or_else(|| "Dispositivo desconhecido".into()),
                code,
                class: r.pnp_class.unwrap_or_default(),
                description: problem_text(code),
            })
        })
        .collect())
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{backup, inventory, open_backup_folder, open_url};

#[cfg(not(windows))]
pub fn inventory() -> AppResult<Inventory> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn backup() -> AppResult<BackupResult> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn open_url(_url: &str) -> AppResult<()> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn open_backup_folder() -> AppResult<()> {
    Err(AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_do_fabricante() {
        assert_eq!(vendor_url("NVIDIA"), Some("https://www.nvidia.com/Download/index.aspx"));
        assert_eq!(vendor_url("Advanced Micro Devices, Inc."), vendor_url("AMD"));
        assert_eq!(vendor_url("Realtek Semiconductor Corp."), Some("https://www.realtek.com/Download/Index"));
        assert_eq!(vendor_url("Fabricante Qualquer"), None);
    }

    #[test]
    fn so_abre_urls_da_lista() {
        assert!(is_allowed_url("https://www.nvidia.com/Download/index.aspx"));
        assert!(is_allowed_url(WINDOWS_UPDATE));
        assert!(!is_allowed_url("https://nvidia.com.site-falso.com/driver.exe"));
        assert!(!is_allowed_url("file:///C:/Windows/System32/cmd.exe"));
    }

    #[test]
    fn json_com_um_item_ou_varios() {
        let one = r#"{"DeviceName":"NVIDIA GeForce RTX 4060 Ti","DriverVersion":"32.0.15.6094","Date":"2024-09-17","DriverProviderName":"NVIDIA","DeviceClass":"DISPLAY"}"#;
        let d = parse_drivers(one).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].download_url, vendor_url("nvidia"));

        let many = format!(r#"[{one}, {{"DeviceName":"Realtek PCIe GbE","DriverVersion":"10.70","Date":null,"DriverProviderName":"Realtek","DeviceClass":"net"}}, {{"DeviceName":null}}]"#);
        let d = parse_drivers(&many).unwrap();
        assert_eq!(d.len(), 2, "item sem nome é ignorado");
        assert_eq!(d[1].class, "NET", "classe normalizada");
        assert_eq!(d[1].date, "");

        assert!(parse_drivers("").unwrap().is_empty());
        assert!(parse_drivers("null").unwrap().is_empty());
    }

    #[test]
    fn dispositivos_com_problema() {
        let json = r#"[{"Name":"Controlador PCI","ConfigManagerErrorCode":28,"PNPClass":null},{"Name":"Ok","ConfigManagerErrorCode":0,"PNPClass":"USB"}]"#;
        let p = parse_problems(json).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].description, "Driver não instalado");
    }
}

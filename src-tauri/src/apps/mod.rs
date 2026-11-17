//! Apps de fábrica (debloat), com lista curta e conservadora.
//!
//! Só entram apps que o Windows instala sozinho e que dá pra reinstalar pela Microsoft
//! Store. Ficam DE FORA de propósito: Xbox e Game Bar (Game Pass e captura dependem
//! deles), Store, Fotos, Calculadora, Bloco de Notas, Terminal e Assistência Rápida.
//! A remoção vale só pro usuário atual e não precisa de admin.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy)]
pub struct Curated {
    /// Nome do pacote (Get-AppxPackage -Name).
    pub name: &'static str,
    /// Nome da família, usado pra abrir a página do app na Store.
    pub family: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

macro_rules! ms {
    ($name:literal, $label:literal, $desc:literal) => {
        Curated { name: $name, family: concat!($name, "_8wekyb3d8bbwe"), label: $label, description: $desc }
    };
}

pub static CURATED: &[Curated] = &[
    ms!("Microsoft.BingNews", "Notícias", "Feed de notícias do MSN."),
    ms!("Microsoft.BingWeather", "Clima", "Previsão do tempo do MSN."),
    ms!("Microsoft.GetHelp", "Obter Ajuda", "Chat de suporte da Microsoft."),
    ms!("Microsoft.Getstarted", "Dicas", "Tutoriais do Windows."),
    ms!("Microsoft.MicrosoftSolitaireCollection", "Solitaire Collection", "Jogos de cartas com anúncios."),
    ms!("Microsoft.WindowsFeedbackHub", "Hub de Comentários", "Envio de sugestões e bugs pra Microsoft."),
    ms!("Microsoft.ZuneVideo", "Filmes e TV", "Player de vídeo antigo; o Media Player continua."),
    ms!("Microsoft.MicrosoftOfficeHub", "Microsoft 365 (atalho)", "Atalho pro Office na web; não remove o Office instalado."),
    ms!("Microsoft.People", "Pessoas", "Agenda de contatos."),
    ms!("Microsoft.PowerAutomateDesktop", "Power Automate", "Automação de tarefas."),
    ms!("Microsoft.Todos", "Microsoft To Do", "Lista de tarefas."),
    ms!("Microsoft.WindowsMaps", "Mapas", "Mapas offline."),
    ms!("Microsoft.549981C3F5F10", "Cortana", "Assistente de voz descontinuada."),
    ms!("Microsoft.Copilot", "Copilot", "App do assistente de IA da Microsoft."),
    Curated {
        name: "Clipchamp.Clipchamp",
        family: "Clipchamp.Clipchamp_yxz26nhyzhsrt",
        label: "Clipchamp",
        description: "Editor de vídeo online.",
    },
    ms!("MicrosoftCorporationII.MicrosoftFamily", "Microsoft Family", "Controle parental da conta Microsoft."),
];

pub fn find(name: &str) -> Option<&'static Curated> {
    CURATED.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

pub fn store_url(family: &str) -> String {
    format!("ms-windows-store://pdp/?PFN={family}")
}

/// Página da Store de um app da lista (pra reinstalar).
pub fn is_store_url(url: &str) -> bool {
    CURATED.iter().any(|c| store_url(c.family) == url)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BloatApp {
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub installed: bool,
    pub store_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawPackage {
    name: Option<String>,
}

/// Nomes de pacotes instalados (JSON do Get-AppxPackage; objeto ou lista).
pub fn parse_installed(json: &str) -> Vec<String> {
    let json = json.trim();
    let values: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::Array(a)) => a,
        Ok(other) => vec![other],
        Err(_) => Vec::new(),
    };
    values
        .into_iter()
        .filter_map(|v| serde_json::from_value::<RawPackage>(v).ok()?.name)
        .collect()
}

pub fn view(installed: &[String]) -> Vec<BloatApp> {
    CURATED
        .iter()
        .map(|c| BloatApp {
            name: c.name,
            label: c.label,
            description: c.description,
            installed: installed.iter().any(|n| n.eq_ignore_ascii_case(c.name)),
            store_url: store_url(c.family),
        })
        .collect()
}

#[cfg(windows)]
pub fn list() -> AppResult<Vec<BloatApp>> {
    let out = crate::system::powershell("ConvertTo-Json -Compress -InputObject @(Get-AppxPackage | Select-Object Name)")?;
    Ok(view(&parse_installed(&out)))
}

/// Remove pelo NOME (só da lista curada); o PowerShell resolve o pacote exato. O texto
/// vindo do front nunca entra no comando: usamos o nome da lista estática.
#[cfg(windows)]
pub fn remove(name: &str) -> AppResult<()> {
    let app = find(name).ok_or_else(|| AppError::InvalidInput(format!("{name} não está na lista de apps removíveis")))?;
    crate::system::powershell(&format!("Get-AppxPackage -Name '{}' | Remove-AppxPackage -ErrorAction Stop", app.name)).map(|_| ())
}

/// Manifesto da cópia provisionada (o "molde" que o Windows usa pra instalar o app em
/// usuários novos). Fica em WindowsApps mesmo depois de remover o app do seu usuário.
/// Caminho validado: dentro de `\WindowsApps\` e terminando no AppxManifest.xml.
pub fn manifest_from_location(location: &str) -> Option<String> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let loc = location.trim().replace("%SYSTEMDRIVE%", &drive).replace("%SystemDrive%", &drive);
    if loc.is_empty() || loc.contains("..") || !loc.to_ascii_lowercase().contains(r"\windowsapps\") {
        return None;
    }
    let manifest = if loc.to_ascii_lowercase().ends_with("appxmanifest.xml") { loc } else { format!(r"{}\AppxManifest.xml", loc.trim_end_matches('\\')) };
    Some(manifest)
}

/// Lê (no auxiliar elevado: a lista de provisionados exige admin) onde está a cópia
/// provisionada do app. Só para apps da lista curada.
#[cfg(windows)]
pub fn provisioned_manifest(name: &str) -> AppResult<String> {
    let app = find(name).ok_or_else(|| AppError::InvalidInput(format!("{name} não está na lista")))?;
    let out = crate::system::powershell(&format!(
        "(Get-AppxProvisionedPackage -Online | Where-Object DisplayName -eq '{}' | Select-Object -First 1).InstallLocation",
        app.name
    ))?;
    let manifest = manifest_from_location(&out).ok_or_else(|| AppError::NotFound("não há cópia do app neste PC: reinstale pela Store".into()))?;
    if !std::path::Path::new(&manifest).is_file() {
        return Err(AppError::NotFound("não há cópia do app neste PC: reinstale pela Store".into()));
    }
    Ok(manifest)
}

/// Reinstala para o usuário atual a partir da cópia do próprio PC (sem Store, sem internet).
#[cfg(windows)]
pub fn register_manifest(manifest: &str) -> AppResult<()> {
    if manifest_from_location(manifest).as_deref() != Some(manifest) || manifest.contains('\'') {
        return Err(AppError::InvalidInput("caminho de manifesto inválido".into()));
    }
    crate::system::powershell(&format!("Add-AppxPackage -Register '{manifest}' -DisableDevelopmentMode -ErrorAction Stop")).map(|_| ())
}

#[cfg(not(windows))]
pub fn provisioned_manifest(_name: &str) -> AppResult<String> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn register_manifest(_m: &str) -> AppResult<()> {
    Err(AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn list() -> AppResult<Vec<BloatApp>> {
    Err(AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn remove(_name: &str) -> AppResult<()> {
    Err(AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifesto_so_dentro_de_windowsapps() {
        assert_eq!(
            manifest_from_location(r"C:\Program Files\WindowsApps\Microsoft.BingNews_4.1_x64__8wekyb3d8bbwe").as_deref(),
            Some(r"C:\Program Files\WindowsApps\Microsoft.BingNews_4.1_x64__8wekyb3d8bbwe\AppxManifest.xml")
        );
        assert!(manifest_from_location(r"%SYSTEMDRIVE%\Program Files\WindowsApps\X\AppxManifest.xml").is_some_and(|m| !m.contains('%')));
        assert!(manifest_from_location(r"C:\Users\x\evil").is_none());
        assert!(manifest_from_location(r"C:\Program Files\WindowsApps\..\..\evil").is_none());
        assert!(manifest_from_location("").is_none());
    }

    #[test]
    fn lista_nao_tem_apps_essenciais_nem_de_jogo() {
        for c in CURATED {
            let n = c.name.to_lowercase();
            for proibido in ["xbox", "gaming", "store", "photos", "calculator", "notepad", "terminal", "quickassist"] {
                assert!(!n.contains(proibido), "{} não pode ser removível", c.name);
            }
            // O nome vai pro PowerShell entre aspas simples: não pode ter aspas.
            assert!(c.name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.'));
        }
    }

    #[test]
    fn instalados_e_links_da_store() {
        let json = r#"[{"Name":"Microsoft.BingNews"},{"Name":"Microsoft.WindowsCalculator"},{"Name":null}]"#;
        let installed = parse_installed(json);
        let v = view(&installed);
        assert!(v.iter().find(|a| a.name == "Microsoft.BingNews").unwrap().installed);
        assert!(!v.iter().find(|a| a.name == "Microsoft.Todos").unwrap().installed);
        assert_eq!(v[0].store_url, "ms-windows-store://pdp/?PFN=Microsoft.BingNews_8wekyb3d8bbwe");
        assert!(is_store_url(&v[0].store_url));
        assert!(!is_store_url("ms-windows-store://pdp/?PFN=Qualquer.Coisa_123"));
        assert_eq!(parse_installed(r#"{"Name":"Microsoft.People"}"#), vec!["Microsoft.People".to_string()]);
    }
}

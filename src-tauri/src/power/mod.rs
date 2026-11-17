//! Plano de energia via `powercfg`.
//!
//! Usamos os aliases do próprio Windows (SCHEME_MIN etc.) em vez de editar o registro:
//! o powercfg valida a troca e notifica o sistema na hora.

use serde::{Deserialize, Serialize};

pub mod plans;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PowerPlan {
    PowerSaver,
    Balanced,
    HighPerformance,
}

impl PowerPlan {
    pub const ALL: [PowerPlan; 3] = [PowerPlan::PowerSaver, PowerPlan::Balanced, PowerPlan::HighPerformance];

    /// GUIDs fixos dos planos padrão do Windows.
    pub const fn guid(self) -> &'static str {
        match self {
            PowerPlan::PowerSaver => "a1841308-3541-4fab-bc81-f71556f20b4a",
            PowerPlan::Balanced => "381b4222-f694-41f0-9685-ff5bb260df2e",
            PowerPlan::HighPerformance => "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c",
        }
    }

    /// Alias aceito por `powercfg /setactive`. Atenção: SCHEME_MIN = "mínima economia" = Alto desempenho.
    #[allow(dead_code)] // usado pela implementação Windows
    pub const fn alias(self) -> &'static str {
        match self {
            PowerPlan::PowerSaver => "SCHEME_MAX",
            PowerPlan::Balanced => "SCHEME_BALANCED",
            PowerPlan::HighPerformance => "SCHEME_MIN",
        }
    }

    pub fn from_guid(guid: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.guid().eq_ignore_ascii_case(guid))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivePlan {
    /// `None` quando é um plano personalizado (criado pelo usuário ou pelo fabricante).
    pub plan: Option<PowerPlan>,
    pub guid: String,
    /// Nome como o Windows informa (no idioma do sistema).
    pub name: String,
}

pub fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Interpreta a saída de `powercfg /getactivescheme`. O texto muda com o idioma do Windows
/// ("Power Scheme GUID: ..." / "GUID do Esquema de Energia: ..."), então não dependemos
/// das palavras: procuramos o GUID e o nome entre parênteses.
pub fn parse_active_scheme(output: &str) -> Option<ActivePlan> {
    let guid = output.split_whitespace().find(|t| is_guid(t))?.to_ascii_lowercase();
    let name = match (output.rfind('('), output.rfind(')')) {
        (Some(start), Some(end)) if end > start => output[start + 1..end].trim().to_string(),
        _ => String::new(),
    };
    Some(ActivePlan { plan: PowerPlan::from_guid(&guid), guid, name })
}

#[cfg(windows)]
pub use windows::{active, apply_custom, energy_status, revert_custom, set_active, set_active_guid};

#[cfg(not(windows))]
pub fn active() -> crate::error::AppResult<ActivePlan> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn set_active(_plan: PowerPlan) -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn energy_status(_s: &plans::EnergyState) -> crate::error::AppResult<plans::EnergyStatus> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn apply_custom(_s: &plans::EnergyState, _p: plans::CustomPlan) -> crate::error::AppResult<plans::EnergyStatus> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn revert_custom(_s: &plans::EnergyState) -> crate::error::AppResult<plans::EnergyStatus> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn set_active_guid(_guid: &str) -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saida_em_ingles() {
        let out = "Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\r\n";
        let p = parse_active_scheme(out).unwrap();
        assert_eq!(p.plan, Some(PowerPlan::Balanced));
        assert_eq!(p.name, "Balanced");
    }

    #[test]
    fn saida_em_portugues() {
        let out = "GUID do Esquema de Energia: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Alto desempenho)\r\n";
        let p = parse_active_scheme(out).unwrap();
        assert_eq!(p.plan, Some(PowerPlan::HighPerformance));
        assert_eq!(p.name, "Alto desempenho");
    }

    #[test]
    fn plano_personalizado() {
        let out = "GUID do Esquema de Energia: 11111111-2222-3333-4444-555555555555  (Plano Gamer XPTO)";
        let p = parse_active_scheme(out).unwrap();
        assert_eq!(p.plan, None);
        assert_eq!(p.name, "Plano Gamer XPTO");
    }

    #[test]
    fn guid_em_maiusculas() {
        let out = "Power Scheme GUID: A1841308-3541-4FAB-BC81-F71556F20B4A  (Power saver)";
        assert_eq!(parse_active_scheme(out).unwrap().plan, Some(PowerPlan::PowerSaver));
    }

    #[test]
    fn saida_sem_guid() {
        assert_eq!(parse_active_scheme("Acesso negado."), None);
    }
}

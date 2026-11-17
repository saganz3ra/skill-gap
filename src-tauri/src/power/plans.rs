//! Planos de energia próprios do Skill Gap: um pra desktop e um pra notebook.
//!
//! São cópias de um plano padrão do Windows (powercfg -duplicatescheme) com alguns
//! ajustes por cima. Por serem cópias com GUID fixo, dá pra saber se já existem,
//! atualizar sem duplicar e apagar ao desfazer.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::PowerPlan;
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CustomPlan {
    Desktop,
    Notebook,
}

/// Um valor do plano. `sub`/`setting` aceitam o alias do powercfg ou o GUID.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    pub sub: &'static str,
    pub setting: &'static str,
    /// Na tomada.
    pub ac: u32,
    /// Na bateria.
    pub dc: u32,
    pub what: &'static str,
}

/// Suspensão seletiva de USB não tem alias no powercfg: vai pelo GUID.
const SUB_USB: &str = "2a737441-1930-4402-8d77-b2bebba308a3";
const USB_SUSPEND: &str = "48e6b7a6-50f5-4782-a5d4-53bb8f07e226";

const DESKTOP: &[Setting] = &[
    Setting { sub: "SUB_PROCESSOR", setting: "PROCTHROTTLEMIN", ac: 100, dc: 100, what: "Processador sempre no clock máximo" },
    Setting { sub: "SUB_PROCESSOR", setting: "CPMINCORES", ac: 100, dc: 100, what: "Todos os núcleos acordados (sem core parking)" },
    Setting { sub: "SUB_PCIEXPRESS", setting: "ASPM", ac: 0, dc: 0, what: "Sem economia de energia no PCI Express" },
    Setting { sub: SUB_USB, setting: USB_SUSPEND, ac: 0, dc: 0, what: "USB nunca entra em suspensão" },
    Setting { sub: "SUB_DISK", setting: "DISKIDLE", ac: 0, dc: 0, what: "Disco nunca desliga" },
];

const NOTEBOOK: &[Setting] = &[
    // 2 = turbo agressivo na tomada; 0 = turbo desligado na bateria.
    Setting { sub: "SUB_PROCESSOR", setting: "PERFBOOSTMODE", ac: 2, dc: 0, what: "Turbo agressivo na tomada, sem turbo na bateria" },
    Setting { sub: "SUB_PROCESSOR", setting: "PROCTHROTTLEMIN", ac: 50, dc: 5, what: "Clock mínimo mais alto só na tomada" },
    Setting { sub: SUB_USB, setting: USB_SUSPEND, ac: 0, dc: 1, what: "USB sem suspensão na tomada (mouse e headset não cortam)" },
];

impl CustomPlan {
    pub const ALL: [CustomPlan; 2] = [CustomPlan::Desktop, CustomPlan::Notebook];

    /// GUIDs fixos, gerados uma vez pro Skill Gap.
    pub const fn guid(self) -> &'static str {
        match self {
            CustomPlan::Desktop => "9f3c1e2a-7b4d-4c8e-a1f0-5d2b8c6e4a01",
            CustomPlan::Notebook => "9f3c1e2a-7b4d-4c8e-a1f0-5d2b8c6e4a02",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            CustomPlan::Desktop => "Skill Gap - PC de mesa",
            CustomPlan::Notebook => "Skill Gap - Notebook",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            CustomPlan::Desktop => "Processador sempre no clock máximo e sem economia no PCI Express, USB e disco. Resposta máxima, com mais calor e consumo.",
            CustomPlan::Notebook => "Na tomada, o processador sobe rápido e usa o turbo pra jogar. Na bateria, fica sem turbo: esquenta menos e a carga dura mais.",
        }
    }

    /// Plano padrão usado como base da cópia.
    pub const fn base(self) -> PowerPlan {
        match self {
            CustomPlan::Desktop => PowerPlan::HighPerformance,
            CustomPlan::Notebook => PowerPlan::Balanced,
        }
    }

    pub const fn settings(self) -> &'static [Setting] {
        match self {
            CustomPlan::Desktop => DESKTOP,
            CustomPlan::Notebook => NOTEBOOK,
        }
    }

    pub fn from_guid(guid: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.guid().eq_ignore_ascii_case(guid))
    }
}

/// GUIDs de todos os planos listados por `powercfg /list` (o texto muda com o idioma;
/// os GUIDs não).
pub fn parse_list(output: &str) -> Vec<String> {
    output
        .split_whitespace()
        .filter(|t| super::is_guid(t))
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub plan: CustomPlan,
    pub name: &'static str,
    pub description: &'static str,
    pub changes: Vec<&'static str>,
    pub installed: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnergyStatus {
    pub active_name: String,
    pub active_guid: String,
    pub plans: Vec<PlanView>,
    /// Existe um plano anterior anotado pra "Desfazer".
    pub can_revert: bool,
    /// Ajustes que o hardware não tem (ex.: core parking em alguns CPUs) e foram pulados.
    pub skipped: Vec<&'static str>,
}

/// Anota o plano que estava ativo antes do primeiro "Aplicar", pra voltar nele.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    previous_guid: Option<String>,
}

pub struct EnergyState {
    path: PathBuf,
    saved: Mutex<Saved>,
    skipped: Mutex<Vec<&'static str>>,
}

impl EnergyState {
    pub fn load(path: PathBuf) -> Self {
        let saved = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, saved: Mutex::new(saved), skipped: Mutex::new(Vec::new()) }
    }

    fn lock(&self) -> MutexGuard<'_, Saved> {
        self.saved.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn previous(&self) -> Option<String> {
        self.lock().previous_guid.clone()
    }

    /// Só anota se ainda não houver anotação: trocar entre os dois planos do Skill Gap
    /// não pode fazer o "anterior" virar um plano do próprio Skill Gap.
    pub fn remember(&self, guid: &str) -> AppResult<()> {
        let mut s = self.lock();
        if s.previous_guid.is_none() && CustomPlan::from_guid(guid).is_none() {
            s.previous_guid = Some(guid.to_string());
            self.persist(&s)?;
        }
        Ok(())
    }

    pub fn forget(&self) -> AppResult<()> {
        let mut s = self.lock();
        s.previous_guid = None;
        self.persist(&s)
    }

    fn persist(&self, s: &Saved) -> AppResult<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(s).expect("serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn set_skipped(&self, v: Vec<&'static str>) {
        *self.skipped.lock().unwrap_or_else(|e| e.into_inner()) = v;
    }

    pub fn skipped(&self) -> Vec<&'static str> {
        self.skipped.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_validos_e_unicos() {
        for p in CustomPlan::ALL {
            assert!(super::super::is_guid(p.guid()));
            assert_eq!(CustomPlan::from_guid(&p.guid().to_uppercase()), Some(p));
        }
        assert_ne!(CustomPlan::Desktop.guid(), CustomPlan::Notebook.guid());
    }

    #[test]
    fn lista_do_powercfg_em_portugues() {
        let out = "Esquemas de Energia Existentes (* Ativo)\r\n-----------------------------------\r\n\
                   GUID do Esquema de Energia: 381b4222-f694-41f0-9685-ff5bb260df2e  (Equilibrado) *\r\n\
                   GUID do Esquema de Energia: 9F3C1E2A-7B4D-4C8E-A1F0-5D2B8C6E4A01  (Skill Gap - PC de mesa)\r\n";
        let guids = parse_list(out);
        assert_eq!(guids.len(), 2);
        assert!(guids.contains(&CustomPlan::Desktop.guid().to_string()));
    }

    #[test]
    fn anota_o_plano_anterior_uma_vez_so() {
        let path = std::env::temp_dir().join(format!("skillgap-energia-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        let s = EnergyState::load(path.clone());
        s.remember(CustomPlan::Desktop.guid()).unwrap();
        assert_eq!(s.previous(), None, "plano do Skill Gap nunca vira o 'anterior'");
        s.remember("381b4222-f694-41f0-9685-ff5bb260df2e").unwrap();
        s.remember("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c").unwrap();
        assert_eq!(EnergyState::load(path.clone()).previous().as_deref(), Some("381b4222-f694-41f0-9685-ff5bb260df2e"));
        s.forget().unwrap();
        assert_eq!(EnergyState::load(path).previous(), None);
    }
}

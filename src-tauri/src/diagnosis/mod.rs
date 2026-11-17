//! Diagnóstico ("potencial de otimização") e comparação com a primeira leitura.
//!
//! Regras:
//! - A nota sai SÓ do estado real do Windows (o mesmo `is_applied` do catálogo + alguns
//!   sinais extras). Não depende do histórico do app: desfazer tudo pelo Windows à mão
//!   faz a nota voltar a subir.
//! - Não é promessa de FPS. Mede quantos itens conhecidos ainda estão no padrão.
//! - Na comparação, números FIRMES (configuração) ficam separados dos OSCILANTES (RAM em
//!   uso, processos), que mudam a cada segundo e só valem como leitura do momento.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::tweaks::TweakState;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: &'static str,
    pub label: &'static str,
    pub weight: u32,
    /// `None` = não deu pra ler (não entra na conta).
    pub optimized: Option<bool>,
    /// O que fazer, em uma frase.
    pub hint: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// 0 = nada a otimizar; 100 = tudo no padrão.
    pub potential: u8,
    pub findings: Vec<Finding>,
}

/// Ajustes que entram na nota: (id do ajuste, peso, rótulo, dica).
const TWEAK_SIGNALS: &[(&str, u32, &str, &str)] = &[
    ("game_mode", 3, "Modo de Jogo do Windows", "Ligue em Todos os ajustes › Jogos."),
    ("game_dvr_off", 3, "Gravação da Game Bar", "Desligue se você não grava com a Game Bar."),
    ("mouse_accel_off", 2, "Aceleração do mouse", "Desligue pra mira consistente."),
    ("hags", 2, "Agendamento de GPU por hardware", "Ligue se sua GPU suporta (exige reiniciar)."),
    ("telemetry_min", 2, "Nível de telemetria", "Coloque no mínimo em Privacidade."),
    ("transparency_off", 1, "Transparência", "Desligue em PCs mais fracos."),
    ("animations_off", 1, "Animações de janelas", "Desligue em PCs mais fracos."),
    ("startup_delay_off", 1, "Espera dos apps de inicialização", "Tire a espera em Todos os ajustes."),
    ("widgets_off", 1, "Widgets", "Desligue se não usa."),
    ("edge_preload_off", 1, "Edge pré-carregado", "Desligue se não usa o Edge."),
];

/// Sinais lidos fora do catálogo.
#[derive(Debug, Clone, Default)]
pub struct Extras {
    pub diagtrack_running: Option<bool>,
    pub sysmain_running: Option<bool>,
    pub wsearch_running: Option<bool>,
    /// hiberfil.sys presente no disco do sistema.
    pub hiberfil: Option<bool>,
    /// Plano ativo é de desempenho (Alto/Máximo ou o plano do Skill Gap).
    pub performance_plan: Option<bool>,
    pub startup_enabled: Option<usize>,
    pub is_laptop: bool,
    pub hdd: bool,
}

/// Monta a lista de sinais e a nota. Pura: recebe o estado lido.
pub fn build(states: &HashMap<&str, TweakState>, x: &Extras) -> Diagnosis {
    let mut f: Vec<Finding> = TWEAK_SIGNALS
        .iter()
        .filter(|(id, ..)| states.contains_key(id))
        .map(|&(id, weight, label, hint)| Finding {
            id,
            label,
            weight,
            optimized: match states[id] {
                TweakState::Active => Some(true),
                TweakState::Inactive | TweakState::Partial => Some(false),
                TweakState::Unknown => None,
            },
            hint,
        })
        .collect();
    f.push(Finding {
        id: "diagtrack",
        label: "Serviço de telemetria (DiagTrack) rodando",
        weight: 2,
        optimized: x.diagtrack_running.map(|r| !r),
        hint: "Desligue em Privacidade › DiagTrack.",
    });
    if !x.hdd {
        // Em HD o SysMain AJUDA: não conta contra.
        f.push(Finding {
            id: "sysmain",
            label: "SysMain (SuperFetch) rodando",
            weight: 1,
            optimized: x.sysmain_running.map(|r| !r),
            hint: "Em SSD o ganho dele é mínimo; o Modo Jogo já o pausa durante a partida.",
        });
    }
    f.push(Finding {
        id: "wsearch",
        label: "Indexação de arquivos rodando",
        weight: 1,
        optimized: x.wsearch_running.map(|r| !r),
        hint: "O Modo Jogo pausa durante a partida; desligar de vez deixa a busca lenta.",
    });
    if !x.is_laptop {
        f.push(Finding {
            id: "hiberfil",
            label: "Arquivo de hibernação ocupando disco",
            weight: 1,
            optimized: x.hiberfil.map(|h| !h),
            hint: "Desligue a hibernação pra liberar alguns GB (desktop).",
        });
        f.push(Finding {
            id: "power_plan",
            label: "Plano de energia de desempenho",
            weight: 1,
            optimized: x.performance_plan,
            hint: "Em CPUs modernas o Equilibrado já é bom; veja a tela Energia.",
        });
    }
    f.push(Finding {
        id: "startup",
        label: "Até 8 programas na inicialização",
        weight: 2,
        optimized: x.startup_enabled.map(|n| n <= 8),
        hint: "Desative o que não precisa abrir com o Windows em Inicialização.",
    });
    Diagnosis { potential: potential(&f), findings: f }
}

pub fn potential(findings: &[Finding]) -> u8 {
    let known: Vec<&Finding> = findings.iter().filter(|f| f.optimized.is_some()).collect();
    let total: u32 = known.iter().map(|f| f.weight).sum();
    if total == 0 {
        return 0;
    }
    let pending: u32 = known.iter().filter(|f| f.optimized == Some(false)).map(|f| f.weight).sum();
    ((pending * 100 + total / 2) / total) as u8
}

// ---------------- Leituras e linha de base ----------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub at: i64,
    pub potential: u8,
    pub startup_enabled: Option<usize>,
    pub services_running: Option<usize>,
    pub power_plan: Option<String>,
    pub tweaks_applied: usize,
    pub ram_used: Option<u64>,
    pub processes: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    baseline: Option<Reading>,
    /// A primeira leitura foi feita com ajustes já aplicados (versão antiga do app).
    after_changes: bool,
    /// Espaço liberado pela Limpeza desde a primeira leitura.
    cleaned_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Count,
    Bytes,
    Score,
    Text,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub label: &'static str,
    pub kind: Kind,
    pub before: Option<serde_json::Value>,
    pub now: Option<serde_json::Value>,
    /// Firme (configuração) ou oscilante (leitura do momento).
    pub firm: bool,
    /// Menor é melhor? (`None` = sem direção, ex.: nome do plano)
    pub lower_is_better: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub baseline_at: Option<i64>,
    pub after_changes: bool,
    pub rows: Vec<Row>,
}

fn v<T: Serialize>(x: T) -> Option<serde_json::Value> {
    serde_json::to_value(x).ok().filter(|v| !v.is_null())
}

pub fn compare(base: Option<&Reading>, now: &Reading, cleaned: u64, after_changes: bool) -> Comparison {
    let b = |f: fn(&Reading) -> Option<serde_json::Value>| base.and_then(f);
    let rows = vec![
        Row { label: "Potencial de otimização", kind: Kind::Score, before: b(|r| v(r.potential)), now: v(now.potential), firm: true, lower_is_better: Some(true) },
        Row { label: "Ajustes ativos", kind: Kind::Count, before: b(|r| v(r.tweaks_applied)), now: v(now.tweaks_applied), firm: true, lower_is_better: Some(false) },
        Row { label: "Programas na inicialização", kind: Kind::Count, before: b(|r| v(r.startup_enabled)), now: v(now.startup_enabled), firm: true, lower_is_better: Some(true) },
        Row { label: "Serviços rodando", kind: Kind::Count, before: b(|r| v(r.services_running)), now: v(now.services_running), firm: true, lower_is_better: Some(true) },
        Row { label: "Plano de energia", kind: Kind::Text, before: b(|r| v(r.power_plan.clone())), now: v(now.power_plan.clone()), firm: true, lower_is_better: None },
        Row { label: "Espaço liberado pela Limpeza", kind: Kind::Bytes, before: v(0u64), now: v(cleaned), firm: true, lower_is_better: Some(false) },
        Row { label: "RAM em uso", kind: Kind::Bytes, before: b(|r| v(r.ram_used)), now: v(now.ram_used), firm: false, lower_is_better: Some(true) },
        Row { label: "Processos abertos", kind: Kind::Count, before: b(|r| v(r.processes)), now: v(now.processes), firm: false, lower_is_better: Some(true) },
    ];
    Comparison { baseline_at: base.map(|r| r.at), after_changes, rows }
}

pub struct BaselineState {
    path: PathBuf,
    saved: Mutex<Saved>,
}

impl BaselineState {
    pub fn load(path: PathBuf) -> Self {
        let saved = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, saved: Mutex::new(saved) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Saved> {
        self.saved.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, s: &Saved) -> AppResult<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(s).expect("serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Grava a primeira leitura. Nunca sobrescreve.
    pub fn ensure(&self, reading: Reading, has_changes: bool) -> AppResult<()> {
        let mut s = self.lock();
        if s.baseline.is_some() {
            return Ok(());
        }
        s.baseline = Some(reading);
        s.after_changes = has_changes;
        self.persist(&s)
    }

    pub fn add_cleaned(&self, bytes: u64) {
        let mut s = self.lock();
        s.cleaned_bytes = s.cleaned_bytes.saturating_add(bytes);
        let _ = self.persist(&s);
    }

    pub fn comparison(&self, now: &Reading) -> Comparison {
        let s = self.lock();
        compare(s.baseline.as_ref(), now, s.cleaned_bytes, s.after_changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(active: &[&'static str]) -> HashMap<&'static str, TweakState> {
        TWEAK_SIGNALS
            .iter()
            .map(|(id, ..)| (*id, if active.contains(id) { TweakState::Active } else { TweakState::Inactive }))
            .collect()
    }

    fn clean_pc() -> Extras {
        Extras {
            diagtrack_running: Some(false),
            sysmain_running: Some(false),
            wsearch_running: Some(false),
            hiberfil: Some(false),
            performance_plan: Some(true),
            startup_enabled: Some(5),
            is_laptop: false,
            hdd: false,
        }
    }

    #[test]
    fn tudo_otimizado_da_zero_e_nada_da_cem() {
        let all: Vec<&str> = TWEAK_SIGNALS.iter().map(|t| t.0).collect();
        assert_eq!(build(&states(&all), &clean_pc()).potential, 0);
        let worst = Extras {
            diagtrack_running: Some(true),
            sysmain_running: Some(true),
            wsearch_running: Some(true),
            hiberfil: Some(true),
            performance_plan: Some(false),
            startup_enabled: Some(30),
            ..clean_pc()
        };
        assert_eq!(build(&states(&[]), &worst).potential, 100);
    }

    #[test]
    fn nao_lido_nao_entra_na_conta() {
        let mut st = states(&[]);
        for v in st.values_mut() {
            *v = TweakState::Unknown;
        }
        let x = Extras { startup_enabled: Some(30), ..Extras::default() };
        let d = build(&st, &x);
        assert_eq!(d.potential, 100, "só a inicialização foi lida, e está ruim");
        assert_eq!(build(&st, &Extras::default()).potential, 0, "nada lido: 0, não 100");
    }

    #[test]
    fn hd_e_notebook_mudam_os_sinais() {
        let d = build(&states(&[]), &Extras { hdd: true, is_laptop: true, ..clean_pc() });
        assert!(d.findings.iter().all(|f| f.id != "sysmain" && f.id != "hiberfil" && f.id != "power_plan"));
    }

    fn reading(potential: u8) -> Reading {
        Reading { at: 1, potential, startup_enabled: Some(12), services_running: Some(140), power_plan: Some("Equilibrado".into()), tweaks_applied: 0, ram_used: Some(8 << 30), processes: 250 }
    }

    #[test]
    fn linha_de_base_gravada_uma_vez_e_limpeza_acumula() {
        let dir = std::env::temp_dir().join(format!("sg-diag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let s = BaselineState::load(dir.join("base.json"));
        s.ensure(reading(70), false).unwrap();
        s.ensure(reading(10), true).unwrap();
        s.add_cleaned(1000);
        s.add_cleaned(500);
        let s = BaselineState::load(dir.join("base.json"));
        let c = s.comparison(&reading(30));
        assert!(!c.after_changes);
        assert_eq!(c.rows[0].before, v(70u8), "a primeira leitura não é sobrescrita");
        assert_eq!(c.rows[0].now, v(30u8));
        assert_eq!(c.rows.iter().find(|r| r.kind == Kind::Bytes && r.firm).unwrap().now, v(1500u64));
        assert!(c.rows.iter().filter(|r| !r.firm).all(|r| r.label.contains("RAM") || r.label.contains("Processos")));
        let _ = fs::remove_dir_all(dir);
    }
}

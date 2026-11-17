//! Novidades: changelog embutido no executável e selo "novo" nos ajustes.
//!
//! O "novo" compara com a ÚLTIMA VERSÃO QUE O USUÁRIO ABRIU (não com a versão atual):
//! assim o selo some depois de visto e quem pulou uma versão vê tudo o que perdeu.
//! Na primeira execução não há referência: marca a versão atual como vista, pra não
//! encher de "novo" quem acabou de instalar.

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Versão do app (única fonte: Cargo.toml; um teste garante que package.json e
/// tauri.conf.json batem com ela).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Novo,
    Melhoria,
    Correcao,
    Mudanca,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: Kind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    /// `None` = ainda não lançada (em desenvolvimento).
    pub date: Option<String>,
    pub items: Vec<Item>,
}

static CHANGELOG: LazyLock<Vec<Release>> =
    LazyLock::new(|| serde_json::from_str(include_str!("../../../CHANGELOG.json")).expect("CHANGELOG.json válido (testado)"));

pub fn changelog() -> &'static [Release] {
    &CHANGELOG
}

/// "1.2.3" → (1, 2, 3). Só o formato que o projeto usa; nada de pré-release.
pub fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().split('.').map(|p| p.parse::<u32>().ok());
    let parsed = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(parsed)
}

fn newer(v: &str, than: &str) -> bool {
    matches!((parse_version(v), parse_version(than)), (Some(a), Some(b)) if a > b)
}

/// Versões entre a última vista (exclusive) e a atual (inclusive).
pub fn unseen<'a>(log: &'a [Release], last_seen: &str, current: &str) -> Vec<&'a Release> {
    log.iter().filter(|r| newer(&r.version, last_seen) && !newer(&r.version, current)).collect()
}

/// Ajustes que entraram depois da última versão vista.
pub fn new_tweaks(last_seen: &str) -> Vec<&'static str> {
    crate::tweaks::catalog::TWEAKS.iter().filter(|t| newer(t.since, last_seen)).map(|t| t.id).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub current: &'static str,
    pub last_seen: String,
    pub unseen: Vec<Release>,
    pub all: &'static [Release],
    pub new_tweaks: Vec<&'static str>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    last_seen: Option<String>,
}

pub struct NewsState {
    path: PathBuf,
    last_seen: Mutex<String>,
}

impl NewsState {
    pub fn load(path: PathBuf) -> Self {
        let saved: Option<Saved> = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let state = Self { path, last_seen: Mutex::new(CURRENT.to_string()) };
        match saved.and_then(|s| s.last_seen).filter(|v| parse_version(v).is_some()) {
            Some(v) => *state.lock() = v,
            // Primeira execução (ou arquivo estragado): começa "em dia", sem selos.
            None => {
                let _ = state.persist(CURRENT);
            }
        }
        state
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, String> {
        self.last_seen.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, version: &str) -> AppResult<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let saved = Saved { last_seen: Some(version.to_string()) };
        fs::write(&tmp, serde_json::to_vec_pretty(&saved).expect("serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn whats_new(&self) -> WhatsNew {
        let last_seen = self.lock().clone();
        WhatsNew {
            current: CURRENT,
            unseen: unseen(changelog(), &last_seen, CURRENT).into_iter().cloned().collect(),
            all: changelog(),
            new_tweaks: new_tweaks(&last_seen),
            last_seen,
        }
    }

    pub fn mark_seen(&self) -> AppResult<()> {
        self.persist(CURRENT)?;
        *self.lock() = CURRENT.to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versao_igual_nos_tres_arquivos() {
        // Se divergirem, o instalador e a tela de novidades mentem sobre a versão.
        let pkg: serde_json::Value = serde_json::from_str(include_str!("../../../package.json")).unwrap();
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(pkg["version"], CURRENT, "package.json diferente do Cargo.toml");
        assert_eq!(conf["version"], CURRENT, "tauri.conf.json diferente do Cargo.toml");
    }

    #[test]
    fn changelog_bem_formado() {
        let log = changelog();
        assert_eq!(log.first().map(|r| r.version.as_str()), Some(CURRENT), "a versão atual precisa ser a primeira do CHANGELOG.json");
        for pair in log.windows(2) {
            assert!(newer(&pair[0].version, &pair[1].version), "{} deveria vir antes de {}", pair[0].version, pair[1].version);
        }
        for (i, r) in log.iter().enumerate() {
            assert!(parse_version(&r.version).is_some(), "versão inválida: {}", r.version);
            assert!(!r.items.is_empty(), "{} sem itens", r.version);
            assert!(r.items.iter().all(|it| !it.text.trim().is_empty()));
            assert!(i == 0 || r.date.is_some(), "só a versão em desenvolvimento pode ficar sem data ({})", r.version);
        }
    }

    #[test]
    fn todo_ajuste_tem_since_valido() {
        let versions: Vec<&str> = changelog().iter().map(|r| r.version.as_str()).collect();
        for t in crate::tweaks::catalog::TWEAKS {
            assert!(versions.contains(&t.since), "{}: since {} não está no CHANGELOG.json", t.id, t.since);
            assert!(!newer(t.since, CURRENT), "{}: since {} é mais nova que o app", t.id, t.since);
        }
    }

    #[test]
    fn versoes() {
        assert_eq!(parse_version("0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(newer("0.10.0", "0.9.9"), "compara número, não texto");
    }

    fn rel(v: &str) -> Release {
        Release { version: v.into(), date: Some("x".into()), items: vec![] }
    }

    #[test]
    fn so_o_que_ainda_nao_foi_visto() {
        let log = [rel("0.4.0"), rel("0.3.0"), rel("0.2.0"), rel("0.1.0")];
        let v: Vec<_> = unseen(&log, "0.1.0", "0.3.0").into_iter().map(|r| r.version.as_str()).collect();
        assert_eq!(v, ["0.3.0", "0.2.0"], "pulou uma versão: vê as duas; nada além da atual");
        assert!(unseen(&log, "0.3.0", "0.3.0").is_empty());
    }

    #[test]
    fn primeira_execucao_comeca_em_dia_e_marcar_visto_persiste() {
        let dir = std::env::temp_dir().join(format!("sg-news-{}", std::process::id()));
        let path = dir.join("novidades.json");
        let _ = fs::remove_dir_all(&dir);
        let s = NewsState::load(path.clone());
        assert_eq!(s.whats_new().last_seen, CURRENT);
        assert!(s.whats_new().unseen.is_empty());

        fs::write(&path, r#"{"lastSeen":"0.0.1"}"#).unwrap();
        let s = NewsState::load(path.clone());
        assert!(!s.whats_new().unseen.is_empty(), "atualizou de uma versão antiga");
        s.mark_seen().unwrap();
        assert!(NewsState::load(path).whats_new().unseen.is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}

//! Ponto de restauração com resultado HONESTO.
//!
//! O problema: `Checkpoint-Computer` "dá certo" mesmo quando o Windows não cria nada.
//! Se já existe um ponto das últimas 24 h, o Windows só emite um aviso e ignora o pedido.
//! Tratar isso como sucesso fazia o app dizer "rede de segurança criada" sem ter criada.
//!
//! A solução: ler o maior `SequenceNumber` antes e depois do pedido e decidir pelo que
//! o Windows de fato tem (`decide`). O limite de 24 h do Windows
//! (`SystemRestorePointCreationFrequency`) NÃO é alterado: seria uma mudança escondida
//! no sistema só pra o app parecer melhor.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Janela em que o Windows reaproveita o último ponto (padrão de fábrica: 24 h).
pub const WINDOW_SECS: i64 = 24 * 60 * 60;

/// Um ponto de restauração, como o Windows lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub seq: u64,
    /// Criação, em segundos Unix.
    pub at: i64,
}

/// O que o Windows tinha antes e depois do pedido.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Probe {
    pub before: Option<Point>,
    pub after: Option<Point>,
    /// Aviso do `Checkpoint-Computer` (ex.: "um ponto já foi criado nas últimas 24 h").
    #[serde(default)]
    pub warning: String,
}

/// Resultado tipado. O caso de erro vai pelo `AppError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Outcome {
    /// Um ponto novo foi criado agora.
    Created { at: i64 },
    /// O Windows não criou outro porque o último tem menos de 24 h. Ele continua valendo
    /// como rede de segurança: desfazer volta ao estado DAQUELA data.
    RecentValid { at: i64 },
    /// A Proteção do Sistema está desligada: não existe ponto e não dá pra criar.
    ProtectionOff,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Decide pelo que o Windows tem, nunca pelo "não deu erro".
pub fn decide(probe: &Probe, now: i64) -> AppResult<Outcome> {
    let Some(after) = probe.after else {
        // Pediu, não deu erro e mesmo assim não existe nenhum ponto: proteção desligada no disco.
        return Ok(Outcome::ProtectionOff);
    };
    if probe.before.is_none_or(|b| after.seq > b.seq) {
        return Ok(Outcome::Created { at: after.at });
    }
    if now - after.at < WINDOW_SECS {
        return Ok(Outcome::RecentValid { at: after.at });
    }
    let warning = probe.warning.trim();
    Err(AppError::CommandFailed {
        command: "Ponto de restauração".into(),
        detail: format!(
            "O Windows não criou um ponto novo e o último tem mais de 24 h.{}",
            if warning.is_empty() { String::new() } else { format!(" Aviso do Windows: {warning}") }
        ),
    })
}

/// Proteção do Sistema desligada pela política ou no computador todo.
/// `disable_sr`: `HKLM\SOFTWARE\Policies\Microsoft\Windows NT\SystemRestore\DisableSR`.
/// `session_interval`: `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore\RPSessionInterval`.
pub fn protection_off(disable_sr: Option<u32>, session_interval: Option<u32>) -> bool {
    disable_sr == Some(1) || session_interval == Some(0)
}

// ---------------- Memória do último ponto criado pelo app ----------------

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    last_created: Option<i64>,
}

/// Lembra quando o APP criou o último ponto, pra não pedir outro (UAC + 10–30 s de
/// espera) dentro de 24 h quando ninguém pediu explicitamente.
pub struct RestoreState {
    path: PathBuf,
    saved: Mutex<Saved>,
}

impl RestoreState {
    pub fn load(path: PathBuf) -> Self {
        let saved = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, saved: Mutex::new(saved) }
    }

    fn last_created(&self) -> Option<i64> {
        self.saved.lock().unwrap_or_else(|e| e.into_inner()).last_created
    }

    fn record(&self, at: i64) -> AppResult<()> {
        let mut saved = self.saved.lock().unwrap_or_else(|e| e.into_inner());
        saved.last_created = Some(at);
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        // Escrita atômica: arquivo temporário + rename.
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&*saved).expect("serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Fluxo completo. `manual`: o usuário clicou em "Criar ponto" (sempre pede ao Windows).
    /// `create` é quem conversa com o Windows (local se admin, senão pelo auxiliar elevado).
    pub fn ensure(&self, manual: bool, protection_is_off: bool, create: impl FnOnce() -> AppResult<Outcome>) -> AppResult<Outcome> {
        if protection_is_off {
            return Ok(Outcome::ProtectionOff);
        }
        if let Some(at) = skip_automatic(self.last_created(), now(), manual) {
            return Ok(Outcome::RecentValid { at });
        }
        let outcome = create()?;
        if let Outcome::Created { at } = outcome {
            // Falhar ao lembrar não invalida o ponto que o Windows criou.
            let _ = self.record(at);
        }
        Ok(outcome)
    }
}

/// Pedido automático (antes de um perfil) com ponto do app há menos de 24 h: reaproveita.
pub fn skip_automatic(last_created: Option<i64>, now: i64, manual: bool) -> Option<i64> {
    match last_created {
        Some(at) if !manual && (0..WINDOW_SECS).contains(&(now - at)) => Some(at),
        _ => None,
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{create, open_protection_settings, protection_is_off};

#[cfg(not(windows))]
pub fn create() -> AppResult<Outcome> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn protection_is_off() -> bool {
    false
}
#[cfg(not(windows))]
pub fn open_protection_settings() -> AppResult<()> {
    Err(AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const H: i64 = 60 * 60;

    fn p(seq: u64, hours_ago: i64) -> Option<Point> {
        Some(Point { seq, at: NOW - hours_ago * H })
    }

    #[test]
    fn ponto_novo_e_criado() {
        let probe = Probe { before: p(41, 30), after: p(42, 0), warning: String::new() };
        assert_eq!(decide(&probe, NOW).unwrap(), Outcome::Created { at: NOW });
        // Primeiro ponto da máquina.
        let probe = Probe { before: None, after: p(1, 0), warning: String::new() };
        assert_eq!(decide(&probe, NOW).unwrap(), Outcome::Created { at: NOW });
    }

    #[test]
    fn windows_ignorou_mas_ha_ponto_recente() {
        // O bug corrigido: sem erro, sem ponto novo. Antes isso virava "criado".
        let probe = Probe { before: p(42, 5), after: p(42, 5), warning: "já existe um ponto".into() };
        assert_eq!(decide(&probe, NOW).unwrap(), Outcome::RecentValid { at: NOW - 5 * H });
    }

    #[test]
    fn nao_criou_e_o_ultimo_e_velho_vira_erro() {
        let probe = Probe { before: p(42, 30), after: p(42, 30), warning: "aviso X".into() };
        let err = decide(&probe, NOW).unwrap_err().to_string();
        assert!(err.contains("mais de 24 h") && err.contains("aviso X"), "{err}");
    }

    #[test]
    fn nenhum_ponto_e_protecao_desligada() {
        assert_eq!(decide(&Probe::default(), NOW).unwrap(), Outcome::ProtectionOff);
    }

    #[test]
    fn deteccao_da_protecao_desligada() {
        assert!(protection_off(Some(1), None));
        assert!(protection_off(None, Some(0)));
        assert!(!protection_off(Some(0), Some(1)));
        assert!(!protection_off(None, None));
    }

    #[test]
    fn automatico_nao_repete_em_24h_mas_manual_sim() {
        assert_eq!(skip_automatic(Some(NOW - 2 * H), NOW, false), Some(NOW - 2 * H));
        assert_eq!(skip_automatic(Some(NOW - 2 * H), NOW, true), None, "clique manual sempre pede");
        assert_eq!(skip_automatic(Some(NOW - 25 * H), NOW, false), None);
        assert_eq!(skip_automatic(None, NOW, false), None);
        assert_eq!(skip_automatic(Some(NOW + 5 * H), NOW, false), None, "relógio voltou: não confia");
    }

    #[test]
    fn ensure_lembra_so_o_que_foi_criado() {
        let dir = std::env::temp_dir().join(format!("sg-restore-{}", std::process::id()));
        let path = dir.join("restore.json");
        let _ = fs::remove_file(&path);

        let state = RestoreState::load(path.clone());
        assert_eq!(state.ensure(false, true, || unreachable!()).unwrap(), Outcome::ProtectionOff);

        // Ponto antigo reaproveitado pelo Windows não conta como "criado pelo app".
        let old = Outcome::RecentValid { at: now() - H };
        assert_eq!(state.ensure(false, false, || Ok(old)).unwrap(), old);
        assert_eq!(state.last_created(), None);

        let created = Outcome::Created { at: now() };
        assert_eq!(state.ensure(false, false, || Ok(created)).unwrap(), created);
        // Persistiu: um novo carregamento lembra, e o automático não chama o Windows de novo.
        let state = RestoreState::load(path.clone());
        assert!(matches!(state.ensure(false, false, || unreachable!()).unwrap(), Outcome::RecentValid { .. }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn formato_para_o_front() {
        let v = serde_json::to_value(Outcome::RecentValid { at: 5 }).unwrap();
        assert_eq!(v, serde_json::json!({ "status": "recentValid", "at": 5 }));
        assert_eq!(serde_json::to_value(Outcome::ProtectionOff).unwrap(), serde_json::json!({ "status": "protectionOff" }));
    }
}

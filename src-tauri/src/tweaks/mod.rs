//! Motor de ajustes reversíveis.
//!
//! Fluxo de "aplicar" (a ordem importa):
//! 1. tira a foto dos valores atuais;
//! 2. GRAVA a foto em disco (diário) — "anota antes de mexer";
//! 3. aplica as operações; se uma falhar, desfaz as que já foram e devolve o erro.
//!
//! Se o PC desligar entre 2 e 3, a foto já está salva e o "Desfazer" continua
//! disponível ao abrir o app de novo.

pub mod catalog;
pub mod jsonfile;
pub mod net;
pub mod ops;
#[cfg(windows)]
pub mod elevated;
#[cfg(windows)]
pub mod windows;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::system::SystemProfile;
use catalog::{Category, Gate, Impact, Restart, Risk, Tweak, PROFILES, TWEAKS};
use ops::{Snap, Sys};

// ---------------- Dados persistidos ----------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakSnapshot {
    /// Uma foto por operação, na mesma ordem de `Tweak::ops`.
    pub ops: Vec<Snap>,
    /// Segundos desde 1970.
    pub applied_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfile {
    pub id: String,
    pub name: String,
    pub tweaks: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub snapshots: BTreeMap<String, TweakSnapshot>,
    pub favorites: BTreeSet<String>,
    pub profiles: Vec<CustomProfile>,
}

// ---------------- O que o front recebe ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TweakState {
    Active,
    Inactive,
    /// Parte das operações aplicada (ex.: alguém mudou um dos valores à mão).
    Partial,
    /// Não deu pra ler o estado.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TweakView {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub details: &'static str,
    pub category: Category,
    pub impact: Impact,
    pub restart: Restart,
    pub requires_admin: bool,
    pub risk: Risk,
    pub breaks: Option<&'static str>,
    pub heats: bool,
    /// Se serve para ESTE PC (calculado aqui: o front só exibe).
    pub gate: Gate,
    pub state: TweakState,
    pub can_revert: bool,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tweaks: Vec<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchReport {
    pub applied: Vec<String>,
    /// Pulados de propósito, com o motivo (ex.: esquenta, exige Windows 11).
    pub skipped: Vec<Skipped>,
    pub failed: Vec<Failure>,
    /// A reinicialização mais pesada exigida pelo que foi aplicado.
    pub restart: Restart,
}

// ---------------- Motor ----------------

pub struct TweakEngine {
    sys: Box<dyn Sys>,
    path: PathBuf,
    store: Mutex<Store>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Motivo legível de "indisponível" vindo do próprio Windows.
fn unsupported_reason(t: &Tweak) -> String {
    let op = t.ops.iter().find(|op| !matches!(op, ops::Op::Dword { .. } | ops::Op::Str { .. }));
    match op {
        Some(ops::Op::PowerSetting { .. }) => "este PC não tem essa opção de energia".into(),
        Some(ops::Op::ScheduledTaskOff { .. }) => "a tarefa não existe neste Windows".into(),
        Some(ops::Op::Net { .. }) => "este Windows não tem essa configuração de rede".into(),
        Some(ops::Op::JsonFileKey { .. }) => "o app não está instalado (ou nunca foi aberto)".into(),
        Some(ops::Op::Service { .. }) => "o serviço não existe neste Windows".into(),
        _ => "este PC não tem o que o ajuste precisa".into(),
    }
}

fn find(id: &str) -> AppResult<&'static Tweak> {
    catalog::find(id).ok_or_else(|| AppError::NotFound(format!("ajuste {id}")))
}

impl TweakEngine {
    /// Diário corrompido não impede o app de abrir: começa vazio.
    pub fn load(sys: Box<dyn Sys>, path: PathBuf) -> Self {
        let store = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { sys, path, store: Mutex::new(store) }
    }

    fn lock(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Escrita atômica: arquivo temporário + rename.
    fn persist(&self, store: &Store) -> AppResult<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(store).expect("store serializável"))?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn state(&self, tweak: &Tweak) -> TweakState {
        let results: Vec<Option<bool>> = tweak.ops.iter().map(|op| ops::is_applied(self.sys.as_ref(), op)).collect();
        if results.iter().any(Option::is_none) {
            TweakState::Unknown
        } else if results.iter().all(|r| *r == Some(true)) {
            TweakState::Active
        } else if results.iter().all(|r| *r == Some(false)) {
            TweakState::Inactive
        } else {
            TweakState::Partial
        }
    }

    /// (estado, tem foto pra desfazer) de um ajuste.
    pub fn status_of(&self, id: &str) -> AppResult<(TweakState, bool)> {
        let tweak = find(id)?;
        let can_revert = self.lock().snapshots.contains_key(id);
        Ok((self.state(tweak), can_revert))
    }

    pub fn list(&self, sys: &SystemProfile) -> Vec<TweakView> {
        let store = self.lock().clone();
        TWEAKS
            .iter()
            .map(|t| TweakView {
                id: t.id,
                name: t.name,
                description: t.description,
                details: t.details,
                category: t.category,
                impact: t.impact,
                restart: t.restart,
                requires_admin: t.requires_admin(),
                risk: t.risk,
                breaks: t.breaks,
                heats: t.heats,
                gate: self.gate(t, sys),
                state: self.state(t),
                can_revert: store.snapshots.contains_key(t.id),
                favorite: store.favorites.contains(t.id),
            })
            .collect()
    }

    /// Requisitos do catálogo + o que só dá pra saber perguntando ao Windows (a opção
    /// de energia existe? a tarefa existe? o app está instalado?).
    fn gate(&self, t: &Tweak, sys: &SystemProfile) -> Gate {
        match catalog::gate(t, sys) {
            Gate::Unavailable(r) => Gate::Unavailable(r),
            other if t.ops.iter().all(|op| ops::supported(self.sys.as_ref(), op)) => other,
            _ => Gate::Unavailable(unsupported_reason(t)),
        }
    }

    /// Aplicar pedido pelo usuário: confere requisitos e confirmação de risco alto ANTES.
    pub fn apply_checked(&self, id: &str, sys: &SystemProfile, confirmed: bool) -> AppResult<()> {
        let t = find(id)?;
        catalog::check_apply(t, sys, confirmed)?;
        if let Gate::Unavailable(r) = self.gate(t, sys) {
            return Err(AppError::Unavailable(r));
        }
        self.apply(id)
    }

    /// Núcleo do aplicar, sem as checagens de requisito (usado depois delas).
    pub fn apply(&self, id: &str) -> AppResult<()> {
        let tweak = find(id)?;
        if self.state(tweak) == TweakState::Active {
            return Ok(());
        }

        // 1-2) foto + diário. Mantém a foto mais antiga: aplicar de novo não pode
        // sobrescrever o valor original do usuário.
        let mut store = self.lock();
        let created = !store.snapshots.contains_key(id);
        let snaps = match store.snapshots.get(id) {
            Some(existing) => existing.ops.clone(),
            None => {
                let snaps = tweak.ops.iter().map(|op| ops::snapshot(self.sys.as_ref(), op)).collect::<AppResult<Vec<_>>>()?;
                store.snapshots.insert(id.to_string(), TweakSnapshot { ops: snaps.clone(), applied_at: now() });
                self.persist(&store)?;
                snaps
            }
        };

        // 3) aplica; em caso de falha, volta o que já mudou.
        for (i, op) in tweak.ops.iter().enumerate() {
            if let Err(e) = ops::apply(self.sys.as_ref(), op) {
                let mut all_restored = true;
                for (done, snap) in tweak.ops[..i].iter().zip(&snaps).rev() {
                    all_restored &= ops::restore(self.sys.as_ref(), done, snap).is_ok();
                }
                // Só apaga a foto se ela nasceu nesta chamada: uma foto antiga guarda
                // o valor original do usuário e não pode se perder.
                if all_restored && created {
                    store.snapshots.remove(id);
                    let _ = self.persist(&store);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    pub fn revert(&self, id: &str) -> AppResult<()> {
        let tweak = find(id)?;
        let mut store = self.lock();
        let snap = store
            .snapshots
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("nada para desfazer neste ajuste".into()))?;

        // Restaura de trás pra frente, tentando todas mesmo se uma falhar.
        let mut first_err = None;
        for (op, s) in tweak.ops.iter().zip(&snap.ops).rev() {
            if let Err(e) = ops::restore(self.sys.as_ref(), op, s) {
                first_err.get_or_insert(e);
            }
        }
        match first_err {
            Some(e) => Err(e), // mantém a foto pra tentar de novo (ex.: depois de abrir como admin)
            None => {
                store.snapshots.remove(id);
                self.persist(&store)
            }
        }
    }

    /// Ajustes que o APP aplicou (têm foto) e que o Windows desfez depois (grande
    /// atualização, política, outro programa). Lido do estado real.
    pub fn undone_by_windows(&self) -> Vec<&'static str> {
        let store = self.lock().clone();
        TWEAKS
            .iter()
            .filter(|t| store.snapshots.contains_key(t.id))
            .filter(|t| matches!(self.state(t), TweakState::Inactive | TweakState::Partial))
            .map(|t| t.id)
            .collect()
    }

    pub fn revert_all(&self) -> BatchReport {
        let ids: Vec<String> = self.lock().snapshots.keys().cloned().collect();
        let mut report = BatchReport { applied: vec![], skipped: vec![], failed: vec![], restart: Restart::None };
        for id in ids {
            match self.revert(&id) {
                Ok(()) => {
                    if let Some(t) = catalog::find(&id) {
                        report.restart = report.restart.max(t.restart);
                    }
                    report.applied.push(id);
                }
                Err(e) => report.failed.push(Failure { id, error: e.to_string() }),
            }
        }
        report
    }

    pub fn set_favorite(&self, id: &str, favorite: bool) -> AppResult<()> {
        find(id)?;
        let mut store = self.lock();
        if favorite {
            store.favorites.insert(id.to_string());
        } else {
            store.favorites.remove(id);
        }
        self.persist(&store)
    }

    // ---------------- Perfis ----------------

    pub fn profiles(&self) -> Vec<ProfileView> {
        let builtin = PROFILES.iter().map(|p| ProfileView {
            id: p.id.into(),
            name: p.name.into(),
            description: p.description.into(),
            tweaks: p.tweaks.iter().map(|s| s.to_string()).collect(),
            builtin: true,
        });
        let custom = self.lock().profiles.clone().into_iter().map(|p| ProfileView {
            description: format!("Perfil salvo com {} ajustes.", p.tweaks.len()),
            id: p.id,
            name: p.name,
            tweaks: p.tweaks,
            builtin: false,
        });
        builtin.chain(custom).collect()
    }

    fn profile_tweaks(&self, id: &str) -> AppResult<Vec<String>> {
        if let Some(p) = catalog::find_profile(id) {
            return Ok(p.tweaks.iter().map(|s| s.to_string()).collect());
        }
        self.lock()
            .profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.tweaks.clone())
            .ok_or_else(|| AppError::NotFound(format!("perfil {id}")))
    }

    /// Aplica um perfil (aplicar em um clique). Pula, com o motivo, o que não serve pra
    /// este PC, o que esquenta e o que tem risco alto.
    pub fn apply_profile(&self, id: &str, sys: &SystemProfile) -> AppResult<BatchReport> {
        let mut report = BatchReport { applied: vec![], skipped: vec![], failed: vec![], restart: Restart::None };
        for tid in self.profile_tweaks(id)? {
            let Some(t) = catalog::find(&tid) else { continue };
            let unsupported = match self.gate(t, sys) {
                Gate::Unavailable(r) => Some(r),
                _ => None,
            };
            if let Some(reason) = unsupported.or_else(|| catalog::profile_skip_reason(t, sys)) {
                report.skipped.push(Skipped { id: tid, reason });
                continue;
            }
            if self.state(t) == TweakState::Active {
                continue;
            }
            match self.apply(&tid) {
                Ok(()) => {
                    report.restart = report.restart.max(t.restart);
                    report.applied.push(tid);
                }
                Err(e) => report.failed.push(Failure { id: tid, error: e.to_string() }),
            }
        }
        Ok(report)
    }

    /// Salva os ajustes ativos agora como um perfil novo.
    pub fn save_profile(&self, name: &str) -> AppResult<ProfileView> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err(AppError::InvalidInput("o nome do perfil precisa ter de 1 a 40 caracteres".into()));
        }
        let tweaks: Vec<String> = TWEAKS
            .iter()
            .filter(|t| self.state(t) == TweakState::Active)
            .map(|t| t.id.to_string())
            .collect();
        if tweaks.is_empty() {
            return Err(AppError::InvalidInput("nenhum ajuste ativo pra salvar".into()));
        }
        let profile = CustomProfile { id: format!("custom-{}", now()), name: name.to_string(), tweaks };
        let mut store = self.lock();
        store.profiles.push(profile.clone());
        self.persist(&store)?;
        Ok(ProfileView {
            description: format!("Perfil salvo com {} ajustes.", profile.tweaks.len()),
            id: profile.id,
            name: profile.name,
            tweaks: profile.tweaks,
            builtin: false,
        })
    }

    pub fn delete_profile(&self, id: &str) -> AppResult<()> {
        let mut store = self.lock();
        let before = store.profiles.len();
        store.profiles.retain(|p| p.id != id);
        if store.profiles.len() == before {
            return Err(AppError::NotFound(format!("perfil {id}")));
        }
        self.persist(&store)
    }
}

/// Sistema real da plataforma.
/// No Windows, o `Sys` do app usa o auxiliar elevado quando precisa de admin.
#[cfg(windows)]
pub fn platform_sys() -> Box<dyn Sys> {
    Box::new(elevated::ElevatedSys)
}

#[cfg(not(windows))]
pub fn platform_sys() -> Box<dyn Sys> {
    Box::new(NoSys)
}

#[cfg(windows)]
pub fn platform_game_sys() -> Box<dyn ops::GameSys> {
    Box::new(elevated::ElevatedSys)
}

#[cfg(not(windows))]
pub fn platform_game_sys() -> Box<dyn ops::GameSys> {
    Box::new(NoSys)
}

#[cfg(not(windows))]
impl ops::GameSys for NoSys {
    fn processes(&self) -> Vec<(u32, String)> {
        Vec::new()
    }
    fn priority(&self, _: u32) -> Option<u32> {
        None
    }
    fn set_priority(&self, _: u32, _: u32) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn service_running(&self, _: &str) -> Option<bool> {
        None
    }
    fn pause_service(&self, _: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn resume_service(&self, _: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
}

/// Fora do Windows nada é suportado: leitura devolve "desconhecido", escrita dá erro.
#[cfg(not(windows))]
struct NoSys;

#[cfg(not(windows))]
impl Sys for NoSys {
    fn dword(&self, _: ops::Hive, _: &str, _: &str) -> Option<u32> {
        None
    }
    fn set_dword(&self, _: ops::Hive, _: &str, _: &str, _: Option<u32>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn string(&self, _: ops::Hive, _: &str, _: &str) -> Option<String> {
        None
    }
    fn set_string(&self, _: ops::Hive, _: &str, _: &str, _: Option<&str>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn key_exists(&self, _: ops::Hive, _: &str) -> bool {
        false
    }
    fn delete_key_tree(&self, _: ops::Hive, _: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn stop_service(&self, _: &str) {}
    fn start_service(&self, _: &str) {}
    fn active_plan(&self) -> AppResult<(String, Option<crate::power::PowerPlan>)> {
        Err(AppError::Unsupported)
    }
    fn set_plan(&self, _: crate::power::PowerPlan) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn set_plan_guid(&self, _: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn mouse(&self) -> AppResult<[i32; 3]> {
        Err(AppError::Unsupported)
    }
    fn set_mouse(&self, _: [i32; 3]) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn mouse_speed(&self) -> AppResult<u32> {
        Err(AppError::Unsupported)
    }
    fn set_mouse_speed(&self, _: u32) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn memory_compression(&self) -> AppResult<bool> {
        Err(AppError::Unsupported)
    }
    fn set_memory_compression(&self, _: bool) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn power_value(&self, _: &str, _: &str) -> AppResult<Option<(String, u32)>> {
        Ok(None)
    }
    fn set_power_value(&self, _: &str, _: &str, _: &str, _: u32) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn task_enabled(&self, _: &str) -> Option<bool> {
        None
    }
    fn set_task_enabled(&self, _: &str, _: bool) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn page_combining(&self) -> AppResult<bool> {
        Err(AppError::Unsupported)
    }
    fn set_page_combining(&self, _: bool) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn net_setting(&self, _: ops::NetKey) -> AppResult<String> {
        Err(AppError::Unsupported)
    }
    fn set_net_setting(&self, _: ops::NetKey, _: &str) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    fn json_file_exists(&self, _: &str, _: &str) -> bool {
        false
    }
    fn json_key(&self, _: &str, _: &str, _: &str) -> AppResult<Option<String>> {
        Err(AppError::Unsupported)
    }
    fn set_json_key(&self, _: &str, _: &str, _: &str, _: Option<&str>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests {
    use super::fake::FakeSys;
    use super::ops::Hive;
    use super::*;

    fn engine(name: &str) -> (TweakEngine, PathBuf, FakeSys) {
        let dir = std::env::temp_dir().join(format!("skillgap-tweaks-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("tweaks.json");
        let fake = FakeSys::default();
        (TweakEngine::load(Box::new(fake.clone()), path.clone()), path, fake)
    }

    fn view(e: &TweakEngine, id: &str) -> TweakView {
        e.list(&desktop()).into_iter().find(|t| t.id == id).unwrap()
    }

    #[test]
    fn aplicar_e_desfazer_volta_ao_original() {
        let (e, _, fake) = engine("roundtrip");
        // Valor original "estranho" do usuário: deve voltar exatamente ele.
        fake.set_dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled", Some(7)).unwrap();
        assert_eq!(view(&e, "game_mode").state, TweakState::Inactive);

        e.apply("game_mode").unwrap();
        let v = view(&e, "game_mode");
        assert_eq!(v.state, TweakState::Active);
        assert!(v.can_revert);

        e.revert("game_mode").unwrap();
        assert_eq!(fake.dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled"), Some(7));
        assert!(!view(&e, "game_mode").can_revert);
    }

    #[test]
    fn valor_inexistente_e_apagado_ao_desfazer() {
        let (e, _, fake) = engine("missing");
        e.apply("ad_id_off").unwrap();
        e.revert("ad_id_off").unwrap();
        assert_eq!(fake.dword(Hive::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled"), None);
    }

    #[test]
    fn diario_e_gravado_antes_de_mexer_e_sobrevive_ao_reinicio() {
        let (e, path, _fake) = engine("journal");
        e.apply("suggestions_off").unwrap();
        // "Reabre o app": novo motor lendo o mesmo arquivo.
        let reopened = TweakEngine::load(Box::new(FakeSys::default()), path);
        assert!(reopened.lock().snapshots.contains_key("suggestions_off"));
    }

    #[test]
    fn aplicar_duas_vezes_mantem_a_foto_original() {
        let (e, _, fake) = engine("twice");
        fake.set_dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled", Some(0)).unwrap();
        e.apply("game_mode").unwrap();
        // Alguém volta o valor e o usuário aplica de novo.
        fake.set_dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled", Some(5)).unwrap();
        e.apply("game_mode").unwrap();
        e.revert("game_mode").unwrap();
        assert_eq!(fake.dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled"), Some(0));
    }

    #[test]
    fn falha_no_meio_desfaz_o_que_ja_mudou() {
        let (e, _, fake) = engine("rollback");
        // O 2º valor do game_dvr_off vai falhar.
        fake.fail_on("AppCaptureEnabled");
        assert!(e.apply("game_dvr_off").is_err());
        assert_eq!(fake.dword(Hive::Hkcu, r"System\GameConfigStore", "GameDVR_Enabled"), None, "1º valor deveria ter voltado");
        assert!(!view(&e, "game_dvr_off").can_revert);
    }

    #[test]
    fn sem_admin_vira_erro_e_nao_suja_o_registro() {
        let (e, _, fake) = engine("admin");
        fake.deny_hklm();
        assert!(matches!(e.apply("hags"), Err(AppError::RequiresAdmin)));
        assert!(!view(&e, "hags").can_revert);
    }

    #[test]
    fn menu_classico_apaga_a_chave_que_criou() {
        let (e, _, fake) = engine("classic");
        e.apply("classic_context_menu").unwrap();
        assert_eq!(view(&e, "classic_context_menu").state, TweakState::Active);
        e.revert("classic_context_menu").unwrap();
        assert!(!fake.key_exists(Hive::Hkcu, r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}"));
    }

    #[test]
    fn servico_e_plano_e_mouse_e_memoria() {
        let (e, _, fake) = engine("misc");
        fake.set_dword(Hive::Hklm, &ops::service_path("SysMain"), "Start", Some(2)).unwrap();
        for id in ["sysmain_off", "power_high", "mouse_accel_off", "mouse_speed_default", "memory_compression_off"] {
            e.apply(id).unwrap();
            assert_eq!(view(&e, id).state, TweakState::Active, "{id}");
        }
        let report = e.revert_all();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(fake.dword(Hive::Hklm, &ops::service_path("SysMain"), "Start"), Some(2));
        assert_eq!(fake.active_plan().unwrap().1, Some(crate::power::PowerPlan::Balanced));
        assert_eq!(fake.mouse().unwrap(), [6, 10, 1]);
        assert_eq!(fake.mouse_speed().unwrap(), 14);
        assert!(fake.memory_compression().unwrap());
    }

    fn desktop() -> SystemProfile {
        SystemProfile { windows: String::new(), build: 26100, is_laptop: false, ram_gb: 32, system_disk: crate::system::DiskKind::Ssd }
    }

    #[test]
    fn guardiao_ve_o_que_o_windows_desfez() {
        let (e, _, fake) = engine("guardiao");
        e.apply("game_mode").unwrap();
        assert!(e.undone_by_windows().is_empty());
        // Uma grande atualização volta o valor de fábrica:
        fake.set_dword(Hive::Hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled", Some(0)).unwrap();
        assert_eq!(e.undone_by_windows(), ["game_mode"]);
        e.apply_checked("game_mode", &desktop(), true).unwrap();
        assert!(e.undone_by_windows().is_empty());
        // Ajuste que já estava assim antes do app (sem foto) não entra.
        assert!(!e.undone_by_windows().contains(&"hags"));
    }

    #[test]
    fn perfil_pronto_aplica_e_pede_reinicio() {
        let (e, _, _fake) = engine("builtin");
        let r = e.apply_profile("competitivo", &desktop()).unwrap();
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert!(r.applied.contains(&"hags".to_string()));
        assert_eq!(r.restart, Restart::Reboot, "HAGS exige reiniciar");
    }

    #[test]
    fn perfil_salvo_pula_o_que_esquenta_com_motivo() {
        let (e, _, _fake) = engine("heats");
        e.apply("game_mode").unwrap();
        e.apply("power_high").unwrap();
        let p = e.save_profile("Meu").unwrap();
        e.revert_all();
        let r = e.apply_profile(&p.id, &desktop()).unwrap();
        assert_eq!(r.applied, vec!["game_mode".to_string()]);
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].id, "power_high");
        assert!(r.skipped[0].reason.contains("calor"));
        // À mão, com o aviso, continua possível.
        e.apply_checked("power_high", &desktop(), false).unwrap();
    }

    #[test]
    fn notebook_mostra_aviso_e_desfazer_nunca_e_bloqueado() {
        let (e, _, _fake) = engine("gate");
        let laptop = SystemProfile { is_laptop: true, ..desktop() };
        let v = e.list(&laptop).into_iter().find(|v| v.id == "power_high").unwrap();
        assert!(matches!(v.gate, Gate::Warn(_)));
        e.apply_checked("power_high", &laptop, false).unwrap();
        e.revert("power_high").unwrap();
    }

    #[test]
    fn salvar_e_apagar_perfil() {
        let (e, _, _fake) = engine("profile");
        assert!(e.save_profile("Meu").is_err(), "sem ajustes ativos");
        e.apply("game_mode").unwrap();
        let p = e.save_profile("  Meu perfil ").unwrap();
        assert_eq!(p.name, "Meu perfil");
        assert!(p.tweaks.contains(&"game_mode".to_string()));
        assert!(e.profiles().iter().any(|x| x.id == p.id && !x.builtin));
        e.delete_profile(&p.id).unwrap();
        assert!(e.delete_profile(&p.id).is_err());
    }

    #[test]
    fn favoritos_persistem() {
        let (e, path, _fake) = engine("fav");
        e.set_favorite("hags", true).unwrap();
        assert!(view(&e, "hags").favorite);
        let reopened = TweakEngine::load(Box::new(FakeSys::default()), path);
        assert!(reopened.lock().favorites.contains("hags"));
        assert!(e.set_favorite("nao_existe", true).is_err());
    }
}

//! Modo Jogo: diagnóstico de latência com correção e desfazer.
//!
//! Cada verificação lê o estado atual do Windows, diz se está ok e, quando dá,
//! oferece uma correção. Antes de corrigir guardamos uma "foto" (snapshot) do valor
//! original no estado do app, então o desfazer volta exatamente ao que era, não a um
//! "padrão" que talvez não fosse o do usuário.
//!
//! Nada aqui toca no processo do jogo: só configurações do Windows.

pub mod watcher;
#[cfg(windows)]
mod windows;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::tweaks::{catalog, TweakEngine, TweakState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckId {
    RefreshRate,
    MouseAcceleration,
    BackgroundRecording,
    Hags,
    GameMode,
    DriverVsync,
}

impl CheckId {
    pub const ALL: [CheckId; 6] = [
        CheckId::RefreshRate,
        CheckId::DriverVsync,
        CheckId::MouseAcceleration,
        CheckId::BackgroundRecording,
        CheckId::Hags,
        CheckId::GameMode,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Ok,
    Warn,
    /// Não dá pra saber ou exige ação manual.
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: CheckId,
    pub status: Status,
    /// Estado atual em texto curto: "60 Hz (máx. 144 Hz)", "Ligada"...
    pub current: String,
    pub fixable: bool,
    pub requires_admin: bool,
    pub requires_restart: bool,
    /// Existe um valor original guardado nesta sessão.
    pub can_revert: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// Nome interno do Windows, ex.: \\.\DISPLAY1
    pub device: String,
    /// Nome do monitor como o driver informa.
    pub name: String,
    pub current: Mode,
    /// Maior taxa disponível NA RESOLUÇÃO ATUAL.
    pub max_hz: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamingReport {
    pub checks: Vec<Check>,
    pub displays: Vec<Display>,
    /// Limite de FPS sugerido para o monitor principal.
    pub fps_cap: Option<u32>,
}

// ---------------- Lógica pura (testável em qualquer SO) ----------------

/// Maior taxa de atualização entre os modos com a mesma resolução do atual.
/// Trocar a resolução pra ganhar Hz não é decisão nossa, então nem consideramos.
pub fn best_refresh(modes: &[Mode], current: Mode) -> u32 {
    modes
        .iter()
        .filter(|m| m.width == current.width && m.height == current.height)
        .map(|m| m.hz)
        .chain(std::iter::once(current.hz))
        .max()
        .unwrap_or(current.hz)
}

/// Limite de FPS sugerido: a mesma fórmula que o NVIDIA Reflex usa no limitador
/// automático (3600·Hz / (3600 + Hz)). Fica um pouco abaixo da taxa do monitor, o que
/// mantém o G-Sync/FreeSync ativo e impede a fila de quadros que causa input lag.
/// 144 Hz → 138 FPS, 240 Hz → 225 FPS.
pub fn fps_cap(hz: u32) -> u32 {
    if hz == 0 {
        return 0;
    }
    (3600 * u64::from(hz) / (3600 + u64::from(hz))) as u32
}

pub fn refresh_status(displays: &[Display]) -> (Status, String) {
    if displays.is_empty() {
        return (Status::Info, "Não detectado".into());
    }
    let low: Vec<_> = displays.iter().filter(|d| d.current.hz < d.max_hz).collect();
    if low.is_empty() {
        let list: Vec<_> = displays.iter().map(|d| format!("{} Hz", d.current.hz)).collect();
        (Status::Ok, list.join(" · "))
    } else {
        let list: Vec<_> = low.iter().map(|d| format!("{} Hz (máx. {} Hz)", d.current.hz, d.max_hz)).collect();
        (Status::Warn, list.join(" · "))
    }
}

// ---------------- Verificações que viram ajustes do catálogo ----------------

/// Verificações que são ajustes do motor (`tweaks`): o diagnóstico só mostra o estado
/// e "Corrigir"/"Desfazer" chamam o motor, que guarda o valor original em disco.
/// (id da verificação, id do ajuste, texto quando ok, texto quando não, status quando não)
const TWEAK_CHECKS: [(CheckId, &str, &str, &str, Status); 4] = [
    (CheckId::MouseAcceleration, "mouse_accel_off", "Desligada", "Ligada", Status::Warn),
    (CheckId::BackgroundRecording, "game_dvr_off", "Desligada", "Ligada", Status::Warn),
    (CheckId::Hags, "hags", "Ativado", "Não ativado", Status::Info),
    (CheckId::GameMode, "game_mode", "Ligado", "Desligado", Status::Warn),
];

fn tweak_for(id: CheckId) -> Option<&'static str> {
    TWEAK_CHECKS.iter().find(|c| c.0 == id).map(|c| c.1)
}

// ---------------- Plataforma ----------------

#[cfg(windows)]
pub(crate) use windows::displays;
#[cfg(windows)]
use windows::set_refresh;
/// Reaproveitados pelo motor de ajustes (tweaks) e pelo monitor de jogos.
#[cfg(windows)]
pub(crate) use windows::{mouse_get, mouse_set, mouse_speed_get, mouse_speed_set, process_list};

#[cfg(not(windows))]
pub(crate) fn displays() -> Vec<Display> {
    Vec::new()
}
#[cfg(not(windows))]
fn set_refresh(_device: &str, _hz: u32) -> AppResult<()> {
    Err(AppError::Unsupported)
}

// ---------------- Estado: valores originais para desfazer ----------------

#[derive(Debug, Clone)]
enum Snapshot {
    /// (dispositivo, Hz original) de cada monitor alterado.
    Displays(Vec<(String, u32)>),
}

/// Guardado pelo Tauri (`app.manage`) e compartilhado entre comandos. O Mutex é
/// necessário porque cada comando roda numa thread diferente.
#[derive(Default)]
pub struct GamingState {
    snapshots: Mutex<HashMap<CheckId, Snapshot>>,
}

impl GamingState {
    fn lock(&self) -> MutexGuard<'_, HashMap<CheckId, Snapshot>> {
        // Se outra thread entrou em pânico segurando o lock, os dados continuam válidos.
        self.snapshots.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// ---------------- Diagnóstico ----------------

pub fn diagnose(state: &GamingState, engine: &TweakEngine) -> GamingReport {
    let displays = displays();
    let snapshots = state.lock();

    let checks = CheckId::ALL
        .into_iter()
        .map(|id| match id {
            CheckId::RefreshRate => {
                let (status, current) = refresh_status(&displays);
                Check {
                    id,
                    fixable: status == Status::Warn,
                    requires_admin: false,
                    requires_restart: false,
                    can_revert: snapshots.contains_key(&id),
                    status,
                    current,
                }
            }
            // Exige a API do driver (NVAPI/ADLX): por enquanto é verificação manual.
            CheckId::DriverVsync => Check {
                id,
                status: Status::Info,
                current: "Verificar no painel da GPU".into(),
                fixable: false,
                requires_admin: false,
                requires_restart: false,
                can_revert: false,
            },
            _ => {
                let &(_, tweak_id, ok_text, bad_text, bad_status) = TWEAK_CHECKS.iter().find(|c| c.0 == id).expect("mapeado");
                let tweak = catalog::find(tweak_id).expect("ajuste no catálogo");
                let (st, can_revert) = engine.status_of(tweak_id).unwrap_or((TweakState::Unknown, false));
                let (status, current) = match st {
                    TweakState::Active => (Status::Ok, ok_text.to_string()),
                    TweakState::Unknown => (Status::Info, "Não foi possível ler".to_string()),
                    _ => (bad_status, bad_text.to_string()),
                };
                Check {
                    id,
                    fixable: matches!(st, TweakState::Inactive | TweakState::Partial),
                    requires_admin: tweak.requires_admin(),
                    requires_restart: tweak.restart == catalog::Restart::Reboot,
                    can_revert,
                    status,
                    current,
                }
            }
        })
        .collect();

    let primary_hz = displays.first().map(|d| d.max_hz);
    GamingReport { checks, fps_cap: primary_hz.map(fps_cap), displays }
}

// ---------------- Corrigir / desfazer ----------------

pub fn fix(state: &GamingState, engine: &TweakEngine, sys: &crate::system::SystemProfile, id: CheckId) -> AppResult<()> {
    if let Some(tweak_id) = tweak_for(id) {
        // Mesma porta do "Todos os ajustes": requisitos conferidos no backend.
        return engine.apply_checked(tweak_id, sys, false);
    }
    match id {
        CheckId::RefreshRate => {
            // Vários monitores: se o 2º falhar, o 1º já mudou e precisa entrar na foto,
            // senão o desfazer não saberia voltá-lo.
            let mut changed = Vec::new();
            let mut result = Ok(());
            for d in displays().into_iter().filter(|d| d.current.hz < d.max_hz) {
                match set_refresh(&d.device, d.max_hz) {
                    Ok(()) => changed.push((d.device, d.current.hz)),
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            if !changed.is_empty() {
                // Mantém a foto MAIS ANTIGA: corrigir duas vezes não apaga o valor original.
                state.lock().entry(id).or_insert(Snapshot::Displays(changed));
            }
            result
        }
        _ => Err(AppError::InvalidInput("VSync do driver é ajustado no painel da GPU".into())),
    }
}

pub fn revert(state: &GamingState, engine: &TweakEngine, id: CheckId) -> AppResult<()> {
    if let Some(tweak_id) = tweak_for(id) {
        return engine.revert(tweak_id);
    }
    let snapshot = state
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| AppError::NotFound("nada para desfazer nesta sessão".into()))?;
    let Snapshot::Displays(list) = snapshot;
    for (device, hz) in list {
        set_refresh(&device, hz)?;
    }
    // Só remove a foto depois de restaurar com sucesso.
    state.lock().remove(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(width: u32, height: u32, hz: u32) -> Mode {
        Mode { width, height, hz }
    }

    #[test]
    fn maior_taxa_so_na_resolucao_atual() {
        let modes = [m(1920, 1080, 60), m(1920, 1080, 144), m(2560, 1440, 165), m(1920, 1080, 120)];
        assert_eq!(best_refresh(&modes, m(1920, 1080, 60)), 144);
        assert_eq!(best_refresh(&[], m(1920, 1080, 75)), 75);
    }

    #[test]
    fn limite_de_fps_igual_ao_reflex() {
        assert_eq!(fps_cap(60), 59);
        assert_eq!(fps_cap(144), 138);
        assert_eq!(fps_cap(165), 157);
        assert_eq!(fps_cap(240), 225);
        assert_eq!(fps_cap(0), 0);
    }

    fn display(hz: u32, max: u32) -> Display {
        Display { device: r"\\.\DISPLAY1".into(), name: "Monitor".into(), current: m(1920, 1080, hz), max_hz: max }
    }

    #[test]
    fn monitor_abaixo_do_maximo_e_alerta() {
        let (s, txt) = refresh_status(&[display(60, 144)]);
        assert_eq!(s, Status::Warn);
        assert_eq!(txt, "60 Hz (máx. 144 Hz)");
        assert_eq!(refresh_status(&[display(144, 144)]).0, Status::Ok);
        assert_eq!(refresh_status(&[]).0, Status::Info);
    }

    fn engine() -> (TweakEngine, crate::tweaks::fake::FakeSys) {
        let fake = crate::tweaks::fake::FakeSys::default();
        let path = std::env::temp_dir().join(format!("skillgap-gaming-{}-{:?}.json", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        (TweakEngine::load(Box::new(fake.clone()), path), fake)
    }

    #[test]
    fn desfazer_taxa_sem_foto_e_erro() {
        let (e, _) = engine();
        assert!(matches!(revert(&GamingState::default(), &e, CheckId::RefreshRate), Err(AppError::NotFound(_))));
    }

    #[test]
    fn diagnostico_usa_o_motor_de_ajustes() {
        let (e, _) = engine();
        let state = GamingState::default();
        let r = diagnose(&state, &e);
        assert_eq!(r.checks.len(), CheckId::ALL.len());
        let mouse = r.checks.iter().find(|c| c.id == CheckId::MouseAcceleration).unwrap();
        assert_eq!(mouse.status, Status::Warn, "o FakeSys começa com aceleração ligada");
        assert!(mouse.fixable);

        fix(&state, &e, &crate::system::read_profile(), CheckId::MouseAcceleration).unwrap();
        let mouse = diagnose(&state, &e).checks.into_iter().find(|c| c.id == CheckId::MouseAcceleration).unwrap();
        assert_eq!(mouse.status, Status::Ok);
        assert!(mouse.can_revert, "o desfazer agora é do motor, salvo em disco");

        revert(&state, &e, CheckId::MouseAcceleration).unwrap();
        let vsync = diagnose(&state, &e).checks.into_iter().find(|c| c.id == CheckId::DriverVsync).unwrap();
        assert!(!vsync.fixable);
    }
}

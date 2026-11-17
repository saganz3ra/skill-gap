//! Timer do Windows: medir de verdade e, opcionalmente, segurar a resolução fina
//! durante o jogo.
//!
//! O que é: o "tique" do agendador do Windows (padrão ~15,6 ms). Um `Sleep(1)` só acorda
//! no próximo tique, então pode durar 15 ms. Jogos que limitam FPS com `Sleep` sofrem
//! com isso. Pedir 0,5 ms (`NtSetTimerResolution`) deixa o `Sleep(1)` perto de 1 ms.
//!
//! Honestidade (muda conforme a versão):
//! - Até o Windows 10 1909: o pedido de UM programa valia pro sistema todo.
//! - Windows 10 2004+: vale só pro programa que pede. O Skill Gap pedir NÃO ajuda o jogo.
//! - Windows 11: volta a valer pro sistema todo SE `GlobalTimerResolutionRequests = 1`
//!   (ajuste "Timer Resolution global"). Sem ele, também não ajuda o jogo.
//!
//! Por que só com o app aberto: a resolução é segurada por um processo vivo e cai quando
//! ele fecha. Uma tarefa agendada também precisaria de um processo residente; preferimos
//! não deixar nada rodando escondido.

use serde::Serialize;

/// 0,5 ms em unidades de 100 ns (o que o `NtSetTimerResolution` usa).
pub const FINE_100NS: u32 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub mean_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub stddev_ms: f64,
}

pub fn stats(samples: &[f64]) -> Option<Stats> {
    if samples.is_empty() {
        return None;
    }
    let n = samples.len() as f64;
    let mean = samples.iter().sum::<f64>() / n;
    let var = samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
    Some(Stats {
        mean_ms: mean,
        min_ms: samples.iter().copied().fold(f64::INFINITY, f64::min),
        max_ms: samples.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        stddev_ms: var.sqrt(),
    })
}

/// Segurar a resolução no app chega até o jogo? (ver a explicação no topo)
pub fn mode_reaches_game(build: u32, global_requests: bool) -> bool {
    match build {
        0 => false,
        b if b < 19041 => true,
        b if b < 22000 => false,
        _ => global_requests,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerReport {
    /// 120 × `Sleep(1)`, medidos com o contador de alta precisão.
    pub sleep: Option<Stats>,
    /// Resolução atual do sistema, em ms (ex.: 15,625 ou 0,5).
    pub current_ms: Option<f64>,
    /// A mais fina que o hardware permite (geralmente 0,5).
    pub finest_ms: Option<f64>,
    /// A padrão/mais grossa (geralmente 15,625).
    pub coarsest_ms: Option<f64>,
    /// `GlobalTimerResolutionRequests = 1`.
    pub global_requests: bool,
    /// O Modo Timer do app ajuda o jogo neste Windows?
    pub mode_reaches_game: bool,
    /// O próprio Skill Gap está segurando a resolução agora (Modo Timer em partida).
    pub held_by_app: bool,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{hold, is_held, measure};

#[cfg(not(windows))]
pub fn measure(_build: u32) -> TimerReport {
    TimerReport {
        sleep: None,
        current_ms: None,
        finest_ms: None,
        coarsest_ms: None,
        global_requests: false,
        mode_reaches_game: false,
        held_by_app: false,
    }
}
#[cfg(not(windows))]
pub fn hold(_on: bool) {}
#[cfg(not(windows))]
pub fn is_held() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estatisticas() {
        let s = stats(&[15.0, 16.0, 17.0]).unwrap();
        assert!((s.mean_ms - 16.0).abs() < 1e-9);
        assert_eq!((s.min_ms, s.max_ms), (15.0, 17.0));
        assert!((s.stddev_ms - (2.0f64 / 3.0).sqrt()).abs() < 1e-9);
        assert!(stats(&[]).is_none());
    }

    #[test]
    fn quando_o_modo_timer_chega_no_jogo() {
        assert!(mode_reaches_game(18363, false), "Win10 1909: global");
        assert!(!mode_reaches_game(19045, true), "Win10 22H2: só o próprio processo");
        assert!(!mode_reaches_game(26100, false), "Win11 sem a chave");
        assert!(mode_reaches_game(26100, true), "Win11 com a chave");
        assert!(!mode_reaches_game(0, true), "versão desconhecida: não promete");
    }
}

//! Telemetria do overlay e da segunda tela. Só roda enquanto alguém está olhando:
//! cada leitura é puxada pela tela (1×/s); nada fica medindo em segundo plano.
//!
//! - Ping/jitter/perda: 1 eco ICMP por leitura, timeout 800 ms, janela de 30;
//!   alvo 1.1.1.1 com troca para 8.8.8.8 depois de 5 falhas seguidas.
//! - GPU: temperatura pelo `nvidia-smi` quando existe (só NVIDIA).
//! - FPS/frametime: PresentMon (Intel, MIT), baixado sob demanda e verificado
//!   (SHA-256 fixo + assinatura Authenticode da Intel) ANTES de executar.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

pub mod presentmon;

pub const WINDOW: usize = 30;
pub const TARGETS: [[u8; 4]; 2] = [[1, 1, 1, 1], [8, 8, 8, 8]];

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PingStats {
    pub last_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    /// Média da diferença entre respostas seguidas.
    pub jitter_ms: Option<f64>,
    /// % de ecos sem resposta na janela.
    pub loss_pct: f64,
    pub target: [u8; 4],
}

/// Estatística da janela (`None` = sem resposta).
pub fn ping_stats(window: &VecDeque<Option<f64>>, target: [u8; 4]) -> PingStats {
    let ok: Vec<f64> = window.iter().flatten().copied().collect();
    let avg = (!ok.is_empty()).then(|| ok.iter().sum::<f64>() / ok.len() as f64);
    let jitter = (ok.len() >= 2).then(|| ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (ok.len() - 1) as f64);
    let loss = if window.is_empty() { 0.0 } else { window.iter().filter(|x| x.is_none()).count() as f64 * 100.0 / window.len() as f64 };
    PingStats { last_ms: window.back().copied().flatten(), avg_ms: avg, jitter_ms: jitter, loss_pct: loss, target }
}

#[derive(Default)]
pub struct PingState {
    window: Mutex<(VecDeque<Option<f64>>, usize, u32)>, // (amostras, alvo, falhas seguidas)
}

impl PingState {
    /// Uma medida nova + estatística. `echo` envia 1 eco e devolve o RTT.
    pub fn tick(&self, echo: impl Fn([u8; 4]) -> Option<f64>) -> PingStats {
        let mut w = self.window.lock().unwrap_or_else(|e| e.into_inner());
        let target = TARGETS[w.1];
        let r = echo(target);
        if r.is_none() {
            w.2 += 1;
            if w.2 >= 5 {
                // Alvo pode estar bloqueado na rede: troca e recomeça a janela.
                w.1 = (w.1 + 1) % TARGETS.len();
                w.2 = 0;
                w.0.clear();
            }
        } else {
            w.2 = 0;
        }
        w.0.push_back(r);
        while w.0.len() > WINDOW {
            w.0.pop_front();
        }
        ping_stats(&w.0, TARGETS[w.1])
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::echo;

#[cfg(not(windows))]
pub fn echo(_t: [u8; 4]) -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_e_perda() {
        let w: VecDeque<Option<f64>> = [Some(10.0), Some(14.0), None, Some(12.0)].into_iter().collect();
        let s = ping_stats(&w, TARGETS[0]);
        assert_eq!(s.avg_ms, Some(12.0));
        assert_eq!(s.jitter_ms, Some(3.0), "|14-10| e |12-14| → 3");
        assert_eq!(s.loss_pct, 25.0);
        assert_eq!(s.last_ms, Some(12.0));
    }

    #[test]
    fn troca_de_alvo_depois_de_5_falhas_e_janela_de_30() {
        let p = PingState::default();
        for _ in 0..5 {
            p.tick(|_| None);
        }
        assert_eq!(p.tick(|t| (t == [8, 8, 8, 8]).then_some(20.0)).target, [8, 8, 8, 8]);
        for _ in 0..40 {
            p.tick(|_| Some(5.0));
        }
        assert_eq!(p.window.lock().unwrap().0.len(), WINDOW);
    }
}

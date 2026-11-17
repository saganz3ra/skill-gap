//! FPS e frametime pelo PresentMon (console, Intel, licença MIT).
//!
//! Segurança do download (nada roda antes de passar nas duas):
//! 1. SHA-256 igual ao publicado no manifesto oficial do winget para esta versão;
//! 2. assinatura Authenticode válida, emitida para a Intel Corporation.
//!
//! Versão fixa: atualizar = trocar URL e hash aqui, conscientemente.
//!
//! Permissão: o PresentMon lê eventos ETW do sistema, o que exige administrador OU o
//! grupo "Usuários de log de desempenho" (S-1-5-32-559). O app oferece colocar o seu
//! usuário nesse grupo uma vez (com desfazer); vale depois de sair e entrar no Windows.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

pub const VERSION: &str = "2.4.1";
pub const URL: &str = "https://github.com/GameTechDev/PresentMon/releases/download/v2.4.1/PresentMon-2.4.1-x64.exe";
pub const SHA256: &str = "D74183E7AE630F72CD3690BE0373ECBFDC6CBB86578148AAB8FA2A7166068F34";
pub const PERF_LOG_USERS: &str = "S-1-5-32-559";

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameStats {
    pub fps: f64,
    pub frametime_ms: f64,
    /// Desvio padrão do frametime no último segundo ("var").
    pub var_ms: f64,
}

/// Coluna de frametime no CSV: v2 usa `FrameTime`, v1 `MsBetweenPresents`.
pub fn frametime_column(header: &str) -> Option<usize> {
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    ["FrameTime", "MsBetweenPresents", "msBetweenPresents"].iter().find_map(|name| cols.iter().position(|c| c == name))
}

pub fn parse_frametime(line: &str, col: usize) -> Option<f64> {
    line.split(',').nth(col)?.trim().parse::<f64>().ok().filter(|v| *v > 0.0 && *v < 5000.0)
}

/// Janela de 1 segundo de frames (soma dos frametimes ≤ 1000 ms).
#[derive(Default)]
pub struct Frames(Mutex<VecDeque<f64>>);

impl Frames {
    pub fn push(&self, ms: f64) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        q.push_back(ms);
        while q.iter().sum::<f64>() > 1000.0 && q.len() > 1 {
            q.pop_front();
        }
    }

    pub fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub fn stats(&self) -> Option<FrameStats> {
        let q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if q.is_empty() {
            return None;
        }
        let n = q.len() as f64;
        let mean = q.iter().sum::<f64>() / n;
        let var = (q.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
        Some(FrameStats { fps: 1000.0 / mean, frametime_ms: mean, var_ms: var })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentMonStatus {
    pub version: &'static str,
    pub installed: bool,
    /// Já pode ler eventos: o app é admin ou o usuário está no grupo (sessão atual).
    pub can_capture: bool,
    /// O app colocou o usuário no grupo; falta sair e entrar no Windows.
    pub needs_logoff: bool,
    /// Medindo agora (nome do exe).
    pub capturing: Option<String>,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod stubs {
    use super::*;
    use crate::error::{AppError, AppResult};
    pub fn installed() -> bool {
        false
    }
    pub fn install() -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn in_perf_group() -> bool {
        false
    }
    pub fn set_perf_group(_sid: &str, _add: bool) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn start(_exe: &str, _frames: std::sync::Arc<Frames>) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
    pub fn stop() {}
    pub fn capturing() -> Option<String> {
        None
    }
}
#[cfg(not(windows))]
pub use stubs::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_v1_e_v2() {
        let v2 = "Application,ProcessID,SwapChainAddress,PresentRuntime,FrameTime,CPUBusy";
        let v1 = "Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,MsBetweenPresents";
        assert_eq!(frametime_column(v2), Some(4));
        assert_eq!(frametime_column(v1), Some(6));
        assert_eq!(parse_frametime("cs2.exe,123,0x1,DXGI,6.94,3.1", 4), Some(6.94));
        assert_eq!(parse_frametime("cs2.exe,123,0x1,DXGI,NA,3.1", 4), None);
    }

    #[test]
    fn janela_de_um_segundo() {
        let f = Frames::default();
        for _ in 0..300 {
            f.push(4.0); // 250 FPS
        }
        let s = f.stats().unwrap();
        assert!((s.fps - 250.0).abs() < 1e-6);
        assert!(s.var_ms < 1e-9);
        assert!(f.0.lock().unwrap().iter().sum::<f64>() <= 1000.0);
    }
}

//! Monitor ao vivo: CPU, memória, GPU, disco e rede.
//!
//! Fonte: os contadores de desempenho do Windows (PDH), os mesmos que o Gerenciador
//! de Tarefas e o Monitor de Desempenho usam. Nada aqui abre processo de jogo.
//!
//! Contadores de taxa (% de uso, bytes/s) precisam de DUAS leituras pra existir: o
//! valor é a diferença entre elas. Por isso a consulta fica aberta no estado do app
//! e cada amostra é uma nova leitura comparada com a anterior.

use std::sync::Mutex;

use serde::Serialize;

#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// % de uso de CPU (0–100).
    pub cpu: f64,
    pub ram_used: u64,
    pub ram_total: u64,
    /// % do motor 3D da GPU (0–100). None = contador indisponível (driver antigo).
    pub gpu: Option<f64>,
    pub vram_used: Option<u64>,
    /// % de tempo ativo do disco (0–100).
    pub disk_active: f64,
    pub disk_read: f64,
    pub disk_write: f64,
    /// Bytes por segundo, somando os adaptadores.
    pub net_down: f64,
    pub net_up: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub cpu_name: String,
    pub cpu_threads: u32,
    pub cpu_mhz: u32,
    pub gpu_name: String,
    pub ram_total: u64,
}

/// "% de tempo ocioso" → "% de tempo ativo", que é o que o Gerenciador de Tarefas mostra.
pub fn active_from_idle(idle: f64) -> f64 {
    (100.0 - idle).clamp(0.0, 100.0)
}

/// Uso da GPU = soma do motor 3D de todos os processos, limitada a 100%.
pub fn gpu_total(values: &[f64]) -> f64 {
    values.iter().filter(|v| v.is_finite() && **v > 0.0).sum::<f64>().min(100.0)
}

/// Estado guardado pelo Tauri: a consulta PDH aberta entre uma amostra e outra.
#[derive(Default)]
pub struct MonitorState {
    #[cfg(windows)]
    query: Mutex<Option<windows::Pdh>>,
    #[cfg(not(windows))]
    _unused: Mutex<()>,
}

#[cfg(windows)]
pub fn sample(state: &MonitorState) -> crate::error::AppResult<Sample> {
    let mut slot = state.query.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(windows::Pdh::open()?);
    }
    Ok(slot.as_mut().expect("aberta").sample())
}

#[cfg(windows)]
pub use windows::hardware_info;

#[cfg(not(windows))]
pub fn sample(_state: &MonitorState) -> crate::error::AppResult<Sample> {
    Err(crate::error::AppError::Unsupported)
}

#[cfg(not(windows))]
pub fn hardware_info() -> HardwareInfo {
    HardwareInfo::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disco_ativo_a_partir_do_ocioso() {
        assert_eq!(active_from_idle(96.0), 4.0);
        assert_eq!(active_from_idle(103.0), 0.0, "PDH às vezes passa de 100");
        assert_eq!(active_from_idle(-2.0), 100.0);
    }

    #[test]
    fn gpu_soma_e_limita() {
        assert_eq!(gpu_total(&[10.0, 20.5]), 30.5);
        assert_eq!(gpu_total(&[80.0, 40.0]), 100.0);
        assert_eq!(gpu_total(&[f64::NAN, -1.0, 5.0]), 5.0);
        assert_eq!(gpu_total(&[]), 0.0);
    }
}

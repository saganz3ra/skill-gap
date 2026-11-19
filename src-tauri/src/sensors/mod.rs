//! Temperaturas e ventoinhas. Só LEITURA: o Windows não tem API de controle de
//! ventoinha, e todo app que controla instala driver de kernel (regra do projeto: não).
//!
//! Fontes, da mais confiável para a menos:
//! - GPU NVIDIA: `nvidia-smi` (vem com o driver), sem permissão especial;
//! - LibreHardwareMonitor, SE já estiver aberto: CPU real (Tctl/Tdie no Ryzen),
//!   placa-mãe e RPM das ventoinhas, lidos do WMI que ele publica. Não instalamos nada;
//! - SSD/HD: o próprio Windows (Get-StorageReliabilityCounter), exige administrador;
//! - CPU sem LibreHardwareMonitor: zona térmica ACPI, aproximada (no AM4 quase sempre vazia).

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{disks, gpu, lhm};

#[cfg(not(windows))]
pub fn gpu() -> Option<GpuStats> {
    None
}
#[cfg(not(windows))]
pub fn lhm() -> Option<Lhm> {
    None
}
#[cfg(not(windows))]
pub fn disks() -> crate::error::AppResult<Vec<Reading>> {
    Err(crate::error::AppError::Unsupported)
}

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// De quanto em quanto tempo os sensores são relidos (abrir PowerShell custa ~0,5 s).
pub const REFRESH: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStats {
    pub name: String,
    pub temp: Option<f64>,
    /// Ventoinha da GPU em % (alvo do driver). Algumas placas param a ventoinha em idle (0%).
    pub fan_pct: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub util_pct: Option<f64>,
    pub mem_used_mb: Option<f64>,
    pub mem_total_mb: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub name: String,
    pub celsius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fan {
    pub name: String,
    pub rpm: Option<f64>,
    /// Quanto a placa-mãe está mandando (PWM/DC), em %.
    pub pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CpuSource {
    /// LibreHardwareMonitor (sensor do próprio processador).
    Lhm,
    /// Zona térmica ACPI do Windows: aproximada.
    Acpi,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensors {
    pub gpu: Option<GpuStats>,
    pub cpu_temp: Option<f64>,
    pub cpu_source: Option<CpuSource>,
    /// Outras temperaturas da placa-mãe (do LibreHardwareMonitor).
    pub board: Vec<Reading>,
    pub disks: Vec<Reading>,
    pub fans: Vec<Fan>,
    /// O LibreHardwareMonitor estava aberto e respondeu.
    pub lhm: bool,
    /// SSD/HD só aparecem com o app como administrador (ou com o auxiliar aberto).
    pub disks_need_admin: bool,
}

/// Faixa plausível: descarta sensor desconectado (0, -128, 255 °C…).
fn plausible(c: f64) -> bool {
    (1.0..=130.0).contains(&c)
}

fn num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.starts_with('[') || s.eq_ignore_ascii_case("N/A") {
        return None;
    }
    s.parse().ok()
}

/// Uma linha de
/// `nvidia-smi --query-gpu=name,temperature.gpu,fan.speed,power.draw,power.limit,clocks.gr,utilization.gpu,memory.used,memory.total --format=csv,noheader,nounits`.
pub fn parse_nvidia_smi(line: &str) -> Option<GpuStats> {
    let f: Vec<&str> = line.split(',').map(str::trim).collect();
    if f.len() < 9 || f[0].is_empty() {
        return None;
    }
    Some(GpuStats {
        name: f[0].to_string(),
        temp: num(f[1]).filter(|c| plausible(*c)),
        fan_pct: num(f[2]).filter(|p| (0.0..=100.0).contains(p)),
        power_w: num(f[3]),
        power_limit_w: num(f[4]),
        clock_mhz: num(f[5]),
        util_pct: num(f[6]),
        mem_used_mb: num(f[7]),
        mem_total_mb: num(f[8]),
    })
}

/// PowerShell `ConvertTo-Json` devolve objeto quando há um item e lista quando há vários.
fn one_or_many<T: for<'de> Deserialize<'de>>(json: &str) -> Vec<T> {
    let json = json.trim();
    if json.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<T>>(json)
        .or_else(|_| serde_json::from_str::<T>(json).map(|x| vec![x]))
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct DiskJson {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Temp")]
    temp: Option<f64>,
}

/// Saída de `Get-PhysicalDisk | … Get-StorageReliabilityCounter` (Name, Temp).
/// Temperatura 0 = o disco não informa (comum em HD antigo e em pendrive).
pub fn parse_disks(json: &str) -> Vec<Reading> {
    one_or_many::<DiskJson>(json)
        .into_iter()
        .filter_map(|d| {
            let c = d.temp.filter(|c| plausible(*c))?;
            Some(Reading { name: d.name.unwrap_or_else(|| "Disco".into()).trim().to_string(), celsius: c })
        })
        .collect()
}

#[derive(Deserialize)]
struct LhmSensor {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "SensorType")]
    kind: String,
    #[serde(rename = "Value")]
    value: Option<f64>,
    #[serde(rename = "Identifier")]
    id: String,
}

/// O que interessa do WMI do LibreHardwareMonitor (`root/LibreHardwareMonitor`, classe Sensor).
#[derive(Debug, Default, PartialEq)]
pub struct Lhm {
    pub cpu_temp: Option<f64>,
    pub board: Vec<Reading>,
    pub fans: Vec<Fan>,
}

/// Identifier: "/amdcpu/0/temperature/2", "/lpc/nct6798d/fan/1", "/lpc/nct6798d/control/1".
/// GPU fica de fora (já vem do nvidia-smi); memória e disco também (vêm de outras fontes).
pub fn parse_lhm(json: &str) -> Option<Lhm> {
    let sensors = one_or_many::<LhmSensor>(json);
    if sensors.is_empty() {
        return None;
    }
    let is_cpu = |id: &str| id.starts_with("/amdcpu") || id.starts_with("/intelcpu");
    let is_board = |id: &str| id.starts_with("/lpc") || id.starts_with("/motherboard");

    // CPU: o sensor "do pacote" quando existe; senão, o mais quente.
    let cpu_temps: Vec<&LhmSensor> = sensors
        .iter()
        .filter(|s| s.kind == "Temperature" && is_cpu(&s.id) && s.value.is_some_and(plausible))
        .collect();
    const PREFERRED: [&str; 4] = ["Core (Tctl/Tdie)", "Tctl/Tdie", "CPU Package", "Core (Tctl)"];
    let cpu_temp = PREFERRED
        .iter()
        .find_map(|p| cpu_temps.iter().find(|s| s.name == *p).and_then(|s| s.value))
        .or_else(|| cpu_temps.iter().filter_map(|s| s.value).reduce(f64::max));

    let board = sensors
        .iter()
        .filter(|s| s.kind == "Temperature" && is_board(&s.id))
        .filter_map(|s| s.value.filter(|c| plausible(*c)).map(|c| Reading { name: s.name.clone(), celsius: c }))
        .collect();

    // Ventoinha: RPM em ".../fan/N" e a % enviada em ".../control/N" (mesmo N, mesmo chip).
    let control = |fan_id: &str| {
        let ctl = fan_id.replacen("/fan/", "/control/", 1);
        sensors.iter().find(|s| s.kind == "Control" && s.id == ctl).and_then(|s| s.value)
    };
    let fans = sensors
        .iter()
        .filter(|s| s.kind == "Fan" && is_board(&s.id))
        // Conector sem ventoinha aparece com 0 RPM e 0%: não é uma ventoinha.
        .filter(|s| s.value.is_some_and(|r| r > 0.0) || control(&s.id).is_some_and(|p| p > 0.0))
        .map(|s| Fan { name: s.name.clone(), rpm: s.value, pct: control(&s.id) })
        .collect();

    Some(Lhm { cpu_temp, board, fans })
}

/// Junta as fontes. A CPU do LibreHardwareMonitor ganha da ACPI.
pub fn combine(gpu: Option<GpuStats>, lhm: Option<Lhm>, acpi_cpu: Option<f64>, disks: Option<Vec<Reading>>) -> Sensors {
    let lhm_on = lhm.is_some();
    let Lhm { cpu_temp: lhm_cpu, board, fans } = lhm.unwrap_or_default();
    let (cpu_temp, cpu_source) = match (lhm_cpu, acpi_cpu.filter(|c| plausible(*c))) {
        (Some(c), _) => (Some(c), Some(CpuSource::Lhm)),
        (None, Some(c)) => (Some(c), Some(CpuSource::Acpi)),
        _ => (None, None),
    };
    Sensors {
        gpu,
        cpu_temp,
        cpu_source,
        board,
        disks_need_admin: disks.is_none(),
        disks: disks.unwrap_or_default(),
        fans,
        lhm: lhm_on,
    }
}

/// Última leitura + releitura em segundo plano. Quem pede (tela, overlay, celular,
/// 1×/s) nunca espera o PowerShell: recebe o último valor e, se estiver velho,
/// dispara uma releitura numa thread.
#[derive(Default)]
pub struct SensorsState {
    last: Mutex<(Option<Instant>, Sensors)>,
    busy: Arc<AtomicBool>,
}

impl SensorsState {
    pub fn get(self: &Arc<Self>, read: impl FnOnce() -> Sensors + Send + 'static) -> Sensors {
        let (at, value) = self.last.lock().map(|g| g.clone()).unwrap_or_default();
        let stale = at.is_none_or(|t| t.elapsed() >= REFRESH);
        if stale && !self.busy.swap(true, Ordering::AcqRel) {
            let me = Arc::clone(self);
            std::thread::spawn(move || {
                let fresh = read();
                if let Ok(mut g) = me.last.lock() {
                    *g = (Some(Instant::now()), fresh);
                }
                me.busy.store(false, Ordering::Release);
            });
        }
        value
    }

    #[cfg(test)]
    fn wait_idle(&self) {
        while self.busy.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_line_with_na_fields() {
        let g = parse_nvidia_smi("NVIDIA GeForce RTX 4060 Ti, 62, 38, 105.40, 165.00, 2610, 97, 6120, 8188").unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 4060 Ti");
        assert_eq!((g.temp, g.fan_pct, g.power_w), (Some(62.0), Some(38.0), Some(105.4)));
        assert_eq!((g.mem_used_mb, g.mem_total_mb), (Some(6120.0), Some(8188.0)));

        let laptop = parse_nvidia_smi("NVIDIA GeForce RTX 3050 Laptop GPU, 48, [N/A], 12.3, [N/A], 210, 3, 400, 4096").unwrap();
        assert_eq!((laptop.fan_pct, laptop.power_limit_w), (None, None));
        assert!(parse_nvidia_smi("").is_none());
        assert!(parse_nvidia_smi("No devices were found").is_none());
    }

    #[test]
    fn disks_single_object_list_and_unsupported() {
        assert_eq!(parse_disks(r#"{"Name":"Samsung SSD 980 1TB","Temp":41}"#), vec![Reading { name: "Samsung SSD 980 1TB".into(), celsius: 41.0 }]);
        let many = parse_disks(r#"[{"Name":"NVMe","Temp":38},{"Name":"HD antigo","Temp":0},{"Name":"Pendrive","Temp":null}]"#);
        assert_eq!(many.len(), 1, "temperatura 0/nula = disco não informa");
        assert!(parse_disks("").is_empty());
        assert!(parse_disks("lixo").is_empty());
    }

    #[test]
    fn lhm_picks_tctl_and_pairs_fans_with_control() {
        let json = r#"[
          {"Name":"Core (Tctl/Tdie)","SensorType":"Temperature","Value":67.25,"Identifier":"/amdcpu/0/temperature/2"},
          {"Name":"CCD1 (Tdie)","SensorType":"Temperature","Value":71.0,"Identifier":"/amdcpu/0/temperature/3"},
          {"Name":"System","SensorType":"Temperature","Value":34.0,"Identifier":"/lpc/nct6798d/temperature/0"},
          {"Name":"Desconectado","SensorType":"Temperature","Value":-128.0,"Identifier":"/lpc/nct6798d/temperature/5"},
          {"Name":"Fan #1","SensorType":"Fan","Value":1180.0,"Identifier":"/lpc/nct6798d/fan/1"},
          {"Name":"Fan #1","SensorType":"Control","Value":45.0,"Identifier":"/lpc/nct6798d/control/1"},
          {"Name":"Fan #4","SensorType":"Fan","Value":0.0,"Identifier":"/lpc/nct6798d/fan/4"},
          {"Name":"Fan #4","SensorType":"Control","Value":0.0,"Identifier":"/lpc/nct6798d/control/4"},
          {"Name":"GPU Core","SensorType":"Temperature","Value":60.0,"Identifier":"/gpu-nvidia/0/temperature/0"},
          {"Name":"GPU Fan","SensorType":"Fan","Value":1500.0,"Identifier":"/gpu-nvidia/0/fan/1"}
        ]"#;
        let l = parse_lhm(json).unwrap();
        assert_eq!(l.cpu_temp, Some(67.25), "prefere Tctl/Tdie ao CCD mais quente");
        assert_eq!(l.board, vec![Reading { name: "System".into(), celsius: 34.0 }]);
        assert_eq!(l.fans, vec![Fan { name: "Fan #1".into(), rpm: Some(1180.0), pct: Some(45.0) }]);
    }

    #[test]
    fn lhm_without_package_sensor_uses_hottest_and_empty_is_none() {
        let json = r#"{"Name":"Core #1","SensorType":"Temperature","Value":55.0,"Identifier":"/intelcpu/0/temperature/1"}"#;
        assert_eq!(parse_lhm(json).unwrap().cpu_temp, Some(55.0));
        assert!(parse_lhm("").is_none(), "LibreHardwareMonitor fechado");
    }

    #[test]
    fn combine_prefers_lhm_cpu_and_flags_missing_disks() {
        let lhm = Lhm { cpu_temp: Some(70.0), ..Default::default() };
        let s = combine(None, Some(lhm), Some(40.0), None);
        assert_eq!((s.cpu_temp, s.cpu_source, s.lhm, s.disks_need_admin), (Some(70.0), Some(CpuSource::Lhm), true, true));

        let s = combine(None, None, Some(27.8), Some(vec![]));
        assert_eq!((s.cpu_temp, s.cpu_source, s.lhm, s.disks_need_admin), (Some(27.8), Some(CpuSource::Acpi), false, false));
        assert_eq!(combine(None, None, Some(0.0), None).cpu_temp, None, "ACPI 0 °C = sem sensor");
    }

    #[test]
    fn state_returns_last_value_and_refreshes_in_background() {
        let st = Arc::new(SensorsState::default());
        let first = st.get(|| Sensors { cpu_temp: Some(50.0), ..Default::default() });
        assert_eq!(first, Sensors::default(), "primeira chamada não espera a leitura");
        st.wait_idle();
        let second = st.get(|| panic!("ainda não está velho: não relê"));
        assert_eq!(second.cpu_temp, Some(50.0));
    }
}

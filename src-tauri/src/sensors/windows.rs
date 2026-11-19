use std::os::windows::process::CommandExt;
use std::process::Command;

use super::{parse_disks, parse_lhm, parse_nvidia_smi, GpuStats, Lhm, Reading};
use crate::error::AppResult;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// GPU NVIDIA pelo `nvidia-smi` do System32 (instalado pelo driver). Outras GPUs: None.
/// Caminho absoluto: nunca um nvidia-smi.exe qualquer que esteja no PATH.
pub fn gpu() -> Option<GpuStats> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let exe = format!(r"{root}\System32\nvidia-smi.exe");
    if !std::path::Path::new(&exe).is_file() {
        return None;
    }
    let out = Command::new(exe)
        .args([
            "--query-gpu=name,temperature.gpu,fan.speed,power.draw,power.limit,clocks.gr,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    // Várias GPUs NVIDIA: a primeira (a principal na quase totalidade dos PCs de jogo).
    String::from_utf8_lossy(&out.stdout).lines().find_map(parse_nvidia_smi)
}

/// Sensores do LibreHardwareMonitor, se ele estiver aberto (publica o namespace WMI
/// `root/LibreHardwareMonitor` enquanto roda). Fechado: o namespace não existe e
/// o comando falha rápido, o que vira None.
pub fn lhm() -> Option<Lhm> {
    const SCRIPT: &str = "Get-CimInstance -Namespace root/LibreHardwareMonitor -ClassName Sensor -ErrorAction Stop | \
        Where-Object { $_.SensorType -in 'Temperature','Fan','Control' } | \
        Select-Object Name,SensorType,Value,Identifier | ConvertTo-Json -Compress";
    crate::system::powershell(SCRIPT).ok().and_then(|out| parse_lhm(&out))
}

/// Temperatura dos discos pelo próprio Windows. Exige administrador
/// (Get-StorageReliabilityCounter): chamado direto se o app já for admin, ou pelo auxiliar.
pub fn disks() -> AppResult<Vec<Reading>> {
    const SCRIPT: &str = "Get-PhysicalDisk | ForEach-Object { \
        $r = $_ | Get-StorageReliabilityCounter -ErrorAction SilentlyContinue; \
        [pscustomobject]@{ Name = $_.FriendlyName; Temp = $r.Temperature } } | ConvertTo-Json -Compress";
    crate::system::powershell(SCRIPT).map(|out| parse_disks(&out))
}

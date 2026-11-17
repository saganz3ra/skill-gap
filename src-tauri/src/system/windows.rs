use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::UI::Shell::IsUserAnAdmin;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{parse_media_type, recommend, SystemInfo};
use crate::error::{AppError, AppResult};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Roda um script no Windows PowerShell 5.1 (vem em todo Windows 10/11) e devolve a saída.
pub fn powershell(script: &str) -> AppResult<String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        // Mensagem de acesso negado do PowerShell vira o erro tipado de admin.
        if detail.contains("Acesso negado") || detail.contains("Access is denied") || detail.contains("0x80070005") {
            return Err(AppError::RequiresAdmin);
        }
        Err(AppError::CommandFailed { command: "PowerShell".into(), detail })
    }
}

/// O processo atual tem token de administrador (UAC elevado)?
pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn is_laptop() -> bool {
    // SAFETY: struct POD zerada, preenchida pela API.
    let mut status: SYSTEM_POWER_STATUS = unsafe { zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return false;
    }
    // BatteryFlag 128 = "sem bateria"; 255 = desconhecido. Qualquer outro valor = tem bateria.
    !matches!(status.BatteryFlag, 128 | 255)
}

fn ram_gb() -> u32 {
    let mut mem: MEMORYSTATUSEX = unsafe { zeroed() };
    mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut mem) } == 0 {
        return 0;
    }
    // O Windows reserva um pouco pra hardware: 31,9 GB visíveis = 32 GB instalados.
    ((mem.ullTotalPhys as f64) / 1024f64.powi(3)).round() as u32
}

fn windows_version() -> (String, u32) {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let Ok(key) = key else { return ("Windows".into(), 0) };
    let build: u32 = key.get_value::<String, _>("CurrentBuild").ok().and_then(|b| b.parse().ok()).unwrap_or(0);
    // O registro ainda diz "Windows 10" no 11; o número do build é o que separa.
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    let display = key.get_value::<String, _>("DisplayVersion").unwrap_or_default();
    (format!("{name} {display}").trim().to_string(), build)
}

fn system_disk() -> super::DiskKind {
    let script = "$n = (Get-Partition -DriveLetter $env:SystemDrive[0]).DiskNumber; \
                  (Get-PhysicalDisk | Where-Object DeviceId -eq \"$n\").MediaType";
    powershell(script).map(|s| parse_media_type(&s)).unwrap_or(super::DiskKind::Unknown)
}

pub fn info() -> SystemInfo {
    let (windows, build) = windows_version();
    let is_laptop = is_laptop();
    let ram_gb = ram_gb();
    let system_disk = system_disk();
    let (recommended_profile, reason) = recommend(is_laptop, ram_gb, system_disk);
    SystemInfo {
        windows,
        build,
        is_laptop,
        ram_gb,
        system_disk,
        is_admin: is_admin(),
        recommended_profile,
        reason,
    }
}

/// Cria um ponto de restauração. Falha se a Proteção do Sistema estiver desligada no C:
/// ou sem admin. O Windows também ignora pedidos repetidos em menos de 24 h.
pub fn create_restore_point() -> AppResult<()> {
    powershell("Checkpoint-Computer -Description 'Skill Gap' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop")
        .map(|_| ())
        .map_err(|e| match e {
            AppError::CommandFailed { detail, .. } => AppError::CommandFailed {
                command: "Ponto de restauração".into(),
                detail: format!(
                    "{detail}\nVerifique se a Proteção do Sistema está ativada no disco C: (Painel de Controle › Sistema › Proteção do Sistema)."
                ),
            },
            other => other,
        })
}

/// Reinicia o Explorer pra aplicar ajustes de barra de tarefas e menus sem sair da conta.
pub fn restart_explorer() -> AppResult<()> {
    let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).creation_flags(CREATE_NO_WINDOW).status();
    Command::new("explorer.exe").spawn()?;
    Ok(())
}

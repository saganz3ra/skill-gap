use std::mem::{size_of, zeroed};
use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::SystemInformation::{GetPhysicallyInstalledSystemMemory, GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::UI::Shell::IsUserAnAdmin;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{decode_console, parse_media_type, SystemProfile};
use crate::error::{AppError, AppResult};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Bytes na code page OEM do console (a mesma que o `chcp` mostra) → texto.
pub fn oem_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let len = bytes.len().min(i32::MAX as usize) as i32;
    unsafe {
        let n = MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0);
        if n <= 0 {
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let mut wide = vec![0u16; n as usize];
        MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), n);
        String::from_utf16_lossy(&wide)
    }
}

/// Roda um script no Windows PowerShell 5.1 (vem em todo Windows 10/11) e devolve a saída.
pub fn powershell(script: &str) -> AppResult<String> {
    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    let stdout = decode_console(&out.stdout).trim().to_string();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = decode_console(&out.stderr).trim().to_string();
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

fn is_laptop() -> bool {
    // SAFETY: struct POD zerada, preenchida pela API.
    let mut status: SYSTEM_POWER_STATUS = unsafe { zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
        return false;
    }
    // BatteryFlag 128 = "sem bateria"; 255 = desconhecido. Qualquer outro valor = tem bateria.
    !matches!(status.BatteryFlag, 128 | 255)
}

/// RAM INSTALADA (32 GB), não a visível ao Windows (31,9 GB, parte fica reservada
/// pro hardware). Se a API falhar (BIOS sem a tabela SMBIOS), usa a visível arredondada.
fn ram_gb() -> u32 {
    let mut kb: u64 = 0;
    if unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) } != 0 && kb > 0 {
        return (kb / (1024 * 1024)) as u32;
    }
    let mut mem: MEMORYSTATUSEX = unsafe { zeroed() };
    mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut mem) } == 0 {
        return 0;
    }
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

/// Bytes de RAM em uso agora.
pub fn ram_used() -> Option<u64> {
    let mut mem: MEMORYSTATUSEX = unsafe { zeroed() };
    mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut mem) } != 0).then(|| mem.ullTotalPhys.saturating_sub(mem.ullAvailPhys))
}

/// Quantos serviços estão rodando (uma leitura de ~1 s).
pub fn running_services() -> Option<usize> {
    powershell("(Get-Service | Where-Object Status -eq 'Running').Count").ok()?.trim().parse().ok()
}

pub fn read_profile() -> SystemProfile {
    let (windows, build) = windows_version();
    SystemProfile { windows, build, is_laptop: is_laptop(), ram_gb: ram_gb(), system_disk: system_disk() }
}


/// Reinicia o Explorer pra aplicar ajustes de barra de tarefas e menus sem sair da conta.
pub fn restart_explorer() -> AppResult<()> {
    let _ = Command::new("taskkill").args(["/f", "/im", "explorer.exe"]).creation_flags(CREATE_NO_WINDOW).status();
    Command::new("explorer.exe").spawn()?;
    Ok(())
}

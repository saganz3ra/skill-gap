//! Lado Windows: um PowerShell só (lê, pede e lê de novo) e a leitura das políticas.

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{decide, now, protection_off, Outcome, Probe};
use crate::error::{AppError, AppResult};

const POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows NT\SystemRestore";
const CONFIG: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore";

fn hklm_dword(path: &str, name: &str) -> Option<u32> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(path).ok()?.get_value::<u32, _>(name).ok()
}

/// Leitura sem admin: dá pra avisar ANTES de pedir o UAC.
pub fn protection_is_off() -> bool {
    protection_off(hklm_dword(POLICY, "DisableSR"), hklm_dword(CONFIG, "RPSessionInterval"))
}

/// Antes e depois numa execução só (cada PowerShell custa ~0,5 s pra abrir).
/// - `ManagementDateTimeConverter`: a data vem no formato WMI ("20261117164300.000000-180").
/// - O erro do `Checkpoint-Computer` também vai pro `warning`: quem decide é o que existe
///   depois, não a mensagem (que muda com o idioma do Windows).
const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
function Last {
  $p = Get-ComputerRestorePoint -ErrorAction SilentlyContinue | Sort-Object SequenceNumber | Select-Object -Last 1
  if ($p) {
    $at = [DateTimeOffset][Management.ManagementDateTimeConverter]::ToDateTime($p.CreationTime)
    @{ seq = [uint64]$p.SequenceNumber; at = $at.ToUnixTimeSeconds() }
  }
}
$before = Last
$w = @()
$err = ''
try {
  Checkpoint-Computer -Description 'Skill Gap' -RestorePointType MODIFY_SETTINGS -WarningAction SilentlyContinue -WarningVariable w
} catch { $err = $_.Exception.Message }
$after = Last
$msg = ((@($w | ForEach-Object { "$_" }) + $err) | Where-Object { $_ }) -join ' '
[pscustomobject]@{ before = $before; after = $after; warning = $msg } | ConvertTo-Json -Compress
"#;

/// Pede o ponto e devolve o que o Windows realmente fez. Exige admin: sem ele o
/// `Get-ComputerRestorePoint` volta vazio em silêncio e pareceria "proteção desligada".
pub fn create() -> AppResult<Outcome> {
    if !crate::system::is_admin() {
        return Err(AppError::RequiresAdmin);
    }
    let out = crate::system::powershell(SCRIPT)?;
    let line = out.lines().rev().find(|l| l.trim_start().starts_with('{')).unwrap_or_default();
    let probe: Probe = serde_json::from_str(line).map_err(|e| AppError::CommandFailed {
        command: "Ponto de restauração".into(),
        detail: format!("resposta inesperada do Windows ({e}): {out}"),
    })?;
    decide(&probe, now())
}

/// Abre "Propriedades do Sistema › Proteção do Sistema". O próprio Windows pede o UAC.
pub fn open_protection_settings() -> AppResult<()> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let exe = format!(r"{root}\System32\SystemPropertiesProtection.exe");
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(&exe));
    // SAFETY: strings terminadas em zero que vivem até o fim da chamada.
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    // ShellExecute devolve um "HINSTANCE" > 32 em caso de sucesso.
    if r as isize > 32 {
        Ok(())
    } else {
        Err(AppError::CommandFailed { command: "Proteção do Sistema".into(), detail: format!("o Windows recusou abrir ({})", r as isize) })
    }
}

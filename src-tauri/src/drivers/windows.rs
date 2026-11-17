use std::os::windows::process::CommandExt;
use std::process::Command;

use super::{is_allowed_url, parse_drivers, parse_problems, BackupResult, Inventory};
use crate::error::{AppError, AppResult};
use crate::system::powershell;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Drivers de terceiros (o que é da própria Microsoft vem pelo Windows Update e não
/// é o que o usuário procura) e dispositivos com erro no Gerenciador de Dispositivos.
pub fn inventory() -> AppResult<Inventory> {
    // Data formatada no próprio PowerShell: o JSON de DateTime do PS 5.1 ("/Date(...)/")
    // é chato de interpretar. -InputObject @() garante lista mesmo com 1 item.
    let drivers = powershell(
        "$d = Get-CimInstance Win32_PnPSignedDriver | Where-Object { $_.DeviceName -and $_.DriverProviderName -and $_.DriverProviderName -notmatch '^Microsoft' } | \
         Select-Object DeviceName, DriverVersion, DriverProviderName, DeviceClass, \
         @{n='Date';e={ if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } }}; \
         ConvertTo-Json -Compress -InputObject @($d)",
    )?;
    let problems = powershell(
        "$p = Get-CimInstance Win32_PnPEntity | Where-Object { $_.ConfigManagerErrorCode -ne 0 } | \
         Select-Object Name, ConfigManagerErrorCode, PNPClass; \
         ConvertTo-Json -Compress -InputObject @($p)",
    )?;
    Ok(Inventory { drivers: parse_drivers(&drivers)?, problems: parse_problems(&problems)? })
}

/// Exporta todos os drivers de terceiros com o pnputil (ferramenta do próprio Windows)
/// pra Documentos\Skill Gap\Drivers AAAA-MM-DD HHmm. Reinstalar depois: pnputil /add-driver
/// "<pasta>\*.inf" /subdirs /install. Exige admin.
pub fn backup() -> AppResult<BackupResult> {
    let out = powershell(
        "$p = Join-Path ([Environment]::GetFolderPath('MyDocuments')) ('Skill Gap\\Drivers ' + (Get-Date -Format 'yyyy-MM-dd HHmm')); \
         New-Item -ItemType Directory -Force -Path $p | Out-Null; \
         $log = pnputil /export-driver * \"$p\" 2>&1; \
         if ($LASTEXITCODE -ne 0) { throw \"pnputil terminou com código $LASTEXITCODE. $($log | Select-Object -Last 3)\" }; \
         ConvertTo-Json -Compress @{ path = $p; count = @(Get-ChildItem -Path $p -Directory).Count }",
    )?;
    serde_json::from_str(&out).map_err(|e| AppError::CommandFailed { command: "backup de drivers".into(), detail: e.to_string() })
}

/// Abre no navegador padrão (ou nas Configurações, pro Windows Update). Só URLs da lista.
pub fn open_url(url: &str) -> AppResult<()> {
    if !is_allowed_url(url) {
        return Err(AppError::InvalidInput("endereço fora da lista de sites oficiais".into()));
    }
    Command::new("explorer.exe").arg(url).creation_flags(CREATE_NO_WINDOW).spawn()?;
    Ok(())
}

pub fn open_backup_folder() -> AppResult<()> {
    let docs = powershell("[Environment]::GetFolderPath('MyDocuments')")?;
    let dir = std::path::Path::new(docs.trim()).join("Skill Gap");
    std::fs::create_dir_all(&dir)?;
    Command::new("explorer.exe").arg(&dir).spawn()?;
    Ok(())
}

//! Implementação Windows: leitura do registro e das pastas Startup.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY};
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

use super::{approved_value, filetime_from_unix, parse_id, StartupItem, StartupSource};
use crate::error::{AppError, AppResult};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

fn hive(source: StartupSource) -> RegKey {
    if source.requires_admin() {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    } else {
        RegKey::predef(HKEY_CURRENT_USER)
    }
}

fn approved_path(source: StartupSource) -> String {
    let sub = match source {
        StartupSource::UserRun | StartupSource::MachineRun => "Run",
        StartupSource::MachineRun32 => "Run32",
        StartupSource::UserFolder | StartupSource::CommonFolder => "StartupFolder",
    };
    format!(r"{APPROVED}\{sub}")
}

fn read_approved(source: StartupSource, raw_name: &str) -> Option<Vec<u8>> {
    hive(source)
        .open_subkey_with_flags(approved_path(source), KEY_READ)
        .ok()?
        .get_raw_value(raw_name)
        .ok()
        .map(|v| v.bytes)
}

fn startup_folder(source: StartupSource) -> Option<PathBuf> {
    let (var, tail) = match source {
        StartupSource::UserFolder => ("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup"),
        StartupSource::CommonFolder => ("ProgramData", r"Microsoft\Windows\Start Menu\Programs\StartUp"),
        _ => return None,
    };
    std::env::var_os(var).map(|base| PathBuf::from(base).join(tail))
}

/// Lista (nome bruto, comando) de uma origem. Origem inexistente = lista vazia, não erro:
/// é normal um PC não ter nada em Run32 ou na pasta comum.
fn entries(source: StartupSource) -> Vec<(String, String)> {
    if source.is_folder() {
        let Some(dir) = startup_folder(source) else { return Vec::new() };
        let Ok(read) = std::fs::read_dir(&dir) else { return Vec::new() };
        read.filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                // desktop.ini é metadado da pasta, não um programa.
                (!name.eq_ignore_ascii_case("desktop.ini"))
                    .then(|| (name, e.path().to_string_lossy().into_owned()))
            })
            .collect()
    } else {
        let path = if source == StartupSource::MachineRun32 { RUN32 } else { RUN };
        let Ok(key) = hive(source).open_subkey_with_flags(path, KEY_READ) else { return Vec::new() };
        key.enum_values()
            .filter_map(Result::ok)
            // Só valores texto (REG_SZ / REG_EXPAND_SZ) são comandos válidos.
            .filter_map(|(name, value)| String::from_reg_value(&value).ok().map(|cmd| (name, cmd)))
            .collect()
    }
}

pub fn list() -> AppResult<Vec<StartupItem>> {
    let mut items: Vec<StartupItem> = StartupSource::ALL
        .into_iter()
        .flat_map(|source| {
            entries(source).into_iter().map(move |(name, command)| {
                let approved = read_approved(source, &name);
                StartupItem::new(source, &name, command, approved.as_deref())
            })
        })
        .collect();
    items.sort_by_key(|i| i.name.to_lowercase());
    Ok(items)
}

fn now_filetime() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    filetime_from_unix(d.as_secs(), d.subsec_nanos())
}

pub fn set_enabled(id: &str, enabled: bool) -> AppResult<()> {
    let (source, raw_name) = parse_id(id).ok_or_else(|| AppError::InvalidInput(id.to_string()))?;

    // Só aceitamos ids que existem de fato: o front não deve conseguir criar
    // marcadores arbitrários no registro.
    if !entries(source).iter().any(|(name, _)| name == raw_name) {
        return Err(AppError::NotFound(raw_name.to_string()));
    }

    let (key, _) = hive(source)
        .create_subkey_with_flags(approved_path(source), KEY_SET_VALUE)
        .map_err(AppError::from_io)?;

    let value = RegValue {
        bytes: approved_value(enabled, now_filetime()).to_vec(),
        vtype: REG_BINARY,
    };
    key.set_raw_value(raw_name, &value).map_err(AppError::from_io)
}

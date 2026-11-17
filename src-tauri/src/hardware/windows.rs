//! Windows: PnP/registro (MSI), Steam (arquivos), AppCompat (HKCU), QoS e placa de rede (cmdlets).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

use super::*;
use crate::system::powershell;

fn json_list(out: &str) -> Vec<Value> {
    match serde_json::from_str::<Value>(out.trim()) {
        Ok(Value::Array(a)) => a,
        Ok(v @ Value::Object(_)) => vec![v],
        _ => Vec::new(),
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn ps_quote(x: &str) -> String {
    format!("'{}'", x.replace('\'', "''"))
}

// ---------------- MSI ----------------

pub fn msi_devices() -> AppResult<Vec<MsiDevice>> {
    let out = powershell(
        "Get-PnpDevice -PresentOnly -Status OK | Where-Object { $_.InstanceId -like 'PCI\\*' -and \
         $_.Class -in @('Display','Net','SCSIAdapter','HDC','USB','MEDIA') } | \
         Select-Object InstanceId,FriendlyName,Class | ConvertTo-Json -Compress",
    )?;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    Ok(json_list(&out)
        .iter()
        .map(|v| (s(v, "InstanceId"), s(v, "FriendlyName"), s(v, "Class")))
        .filter(|(id, name, class)| is_pci_instance(id) && msi_relevant(class, name))
        .map(|(instance_id, name, class)| {
            let msi = hklm.open_subkey(msi_path(&instance_id)).ok().and_then(|k| k.get_value::<u32, _>("MSISupported").ok());
            MsiDevice { instance_id, name, class, msi }
        })
        .collect())
}

/// Grava (ou apaga, com `None`) o `MSISupported`. Só para um InstanceId da listagem atual.
pub fn msi_write(instance_id: &str, value: Option<u32>) -> AppResult<()> {
    if !msi_devices()?.iter().any(|d| d.instance_id.eq_ignore_ascii_case(instance_id)) {
        return Err(invalid("dispositivo fora da lista atual do Windows"));
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    match value {
        Some(v) => {
            let (key, _) = hklm.create_subkey(msi_path(instance_id)).map_err(AppError::from_io)?;
            key.set_value("MSISupported", &v).map_err(AppError::from_io)
        }
        None => match hklm.open_subkey_with_flags(msi_path(instance_id), KEY_SET_VALUE) {
            Ok(key) => match key.delete_value("MSISupported") {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(AppError::from_io(e)),
                _ => Ok(()),
            },
            Err(_) => Ok(()),
        },
    }
}

// ---------------- Steam e tela cheia clássica ----------------

fn steam_root() -> Option<PathBuf> {
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let p: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(p.replace('/', "\\")))
}

fn collect_exes(dir: &Path, base: &Path, depth: u32, out: &mut Vec<(String, u64)>) {
    if depth > 4 || out.len() > 400 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let p = e.path();
        if meta.is_dir() {
            collect_exes(&p, base, depth + 1, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
            if let Ok(rel) = p.strip_prefix(base) {
                out.push((rel.to_string_lossy().into_owned(), meta.len()));
            }
        }
    }
}

pub fn steam_games() -> Vec<SteamGame> {
    let Some(root) = steam_root() else { return Vec::new() };
    let lib_file = fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")).unwrap_or_default();
    let mut libs: Vec<PathBuf> = vdf_values(&lib_file, "path").into_iter().map(|p| PathBuf::from(p.replace(r"\\", r"\"))).collect();
    if libs.is_empty() {
        libs.push(root);
    }
    let mut games = Vec::new();
    for lib in libs {
        let apps = lib.join("steamapps");
        let Ok(entries) = fs::read_dir(&apps) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let acf = fs::read_to_string(e.path()).unwrap_or_default();
            let (Some(app_id), Some(title), Some(dir)) =
                (vdf_values(&acf, "appid").first().copied(), vdf_values(&acf, "name").first().copied(), vdf_values(&acf, "installdir").first().copied())
            else {
                continue;
            };
            if is_steam_tool(title) {
                continue;
            }
            let install = apps.join("common").join(dir);
            let mut exes = Vec::new();
            collect_exes(&install, &install, 0, &mut exes);
            let exe = pick_main_exe(&exes).map(|rel| install.join(rel).to_string_lossy().into_owned());
            let classic = exe.as_deref().is_some_and(|p| has_flag(fse_read(p).as_deref()));
            let qos = exe.as_deref().and_then(|p| Path::new(p).file_name()).is_some_and(|f| qos_exists(&f.to_string_lossy()));
            let high_perf_gpu = exe.as_deref().is_some_and(|p| gpu_read(p).is_some_and(|v| v.contains("GpuPreference=2")));
            games.push(SteamGame {
                app_id: app_id.into(),
                name: title.into(),
                install_dir: install.to_string_lossy().into_owned(),
                exe,
                classic_fullscreen: classic,
                qos,
                high_perf_gpu,
            });
        }
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

fn valid_exe_path(p: &str) -> bool {
    let path = Path::new(p);
    path.is_absolute() && p.to_ascii_lowercase().ends_with(".exe") && path.is_file()
}

pub fn fse_read(exe_path: &str) -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(LAYERS, KEY_READ).ok()?.get_value::<String, _>(exe_path).ok()
}

/// Grava o valor do exe nas flags de compatibilidade do usuário (`None` apaga).
pub fn fse_write(exe_path: &str, value: Option<&str>) -> AppResult<()> {
    if !valid_exe_path(exe_path) {
        return Err(invalid("executável inválido"));
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(LAYERS).map_err(AppError::from_io)?;
    match value {
        Some(v) => key.set_value(exe_path, &v.to_string()).map_err(AppError::from_io),
        None => match key.delete_value(exe_path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(AppError::from_io(e)),
            _ => Ok(()),
        },
    }
}

pub fn gpu_read(exe_path: &str) -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(GPU_PREFS, KEY_READ).ok()?.get_value::<String, _>(exe_path).ok()
}

pub fn gpu_write(exe_path: &str, value: Option<&str>) -> AppResult<()> {
    if !valid_exe_path(exe_path) {
        return Err(invalid("executável inválido"));
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(GPU_PREFS).map_err(AppError::from_io)?;
    match value {
        Some(v) => key.set_value(exe_path, &v.to_string()).map_err(AppError::from_io),
        None => match key.delete_value(exe_path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(AppError::from_io(e)),
            _ => Ok(()),
        },
    }
}

// ---------------- QoS ----------------

pub fn qos_exists(exe: &str) -> bool {
    let Some(name) = qos_name(exe) else { return false };
    powershell(&format!("Get-NetQosPolicy -Name {} -ErrorAction SilentlyContinue | Measure-Object | Select-Object -ExpandProperty Count", ps_quote(&name)))
        .ok()
        .is_some_and(|o| o.trim() != "0" && !o.trim().is_empty())
}

pub fn qos_set(exe: &str, on: bool) -> AppResult<()> {
    let name = qos_name(exe).ok_or_else(|| invalid("nome de executável inválido"))?;
    let script = if on {
        if qos_exists(exe) {
            return Ok(());
        }
        format!(
            "New-NetQosPolicy -Name {} -AppPathNameMatchCondition {} -DSCPAction 46 -NetworkProfile All -IPProtocolMatchCondition Both | Out-Null",
            ps_quote(&name),
            ps_quote(exe)
        )
    } else {
        format!("Remove-NetQosPolicy -Name {} -Confirm:$false -ErrorAction SilentlyContinue", ps_quote(&name))
    };
    powershell(&script).map(|_| ())
}

pub const NLA_PATH: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\QoS";

pub fn nla_read() -> Option<String> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(NLA_PATH).ok()?.get_value::<String, _>("Do not use NLA").ok()
}

/// "Do not use NLA" = "1": sem isso o Windows fora de domínio não marca o DSCP.
pub fn nla_write(value: Option<&str>) -> AppResult<()> {
    if !matches!(value, None | Some("0") | Some("1")) {
        return Err(invalid("valor inválido"));
    }
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(NLA_PATH).map_err(AppError::from_io)?;
    match value {
        Some(v) => key.set_value("Do not use NLA", &v.to_string()).map_err(AppError::from_io),
        None => match key.delete_value("Do not use NLA") {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(AppError::from_io(e)),
            _ => Ok(()),
        },
    }
}

// ---------------- Placa de rede ----------------

pub fn nic_properties() -> AppResult<Vec<NicProperty>> {
    let keywords = NIC_KEYWORDS.iter().map(|(k, _)| ps_quote(k)).collect::<Vec<_>>().join(",");
    let out = powershell(&format!(
        "Get-NetAdapter -Physical | ForEach-Object {{ $n = $_.Name; Get-NetAdapterAdvancedProperty -Name $n -RegistryKeyword {keywords} \
         -ErrorAction SilentlyContinue | Select-Object @{{n='adapter';e={{$n}}}},RegistryKeyword,@{{n='value';e={{$_.RegistryValue -join ''}}}},\
         ValidRegistryValues,ValidDisplayValues }} | ConvertTo-Json -Compress -Depth 3"
    ))?;
    let list = |v: &Value, k: &str| match v.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        Some(Value::String(x)) => vec![x.clone()],
        _ => Vec::new(),
    };
    Ok(json_list(&out)
        .iter()
        .filter_map(|v| {
            let keyword = s(v, "RegistryKeyword");
            let label = NIC_KEYWORDS.iter().find(|(k, _)| *k == keyword)?.1.to_string();
            Some(NicProperty { adapter: s(v, "adapter"), keyword, label, value: s(v, "value"), valid: list(v, "ValidRegistryValues"), display: list(v, "ValidDisplayValues") })
        })
        .collect())
}

/// Só grava um valor que ESTA placa aceita, num adaptador e keyword da listagem atual.
pub fn nic_write(adapter: &str, keyword: &str, value: &str) -> AppResult<()> {
    let props = nic_properties()?;
    let prop = props.iter().find(|p| p.adapter == adapter && p.keyword == keyword).ok_or_else(|| invalid("propriedade não encontrada"))?;
    if !nic_value_ok(prop, value) {
        return Err(invalid("valor que esta placa não aceita"));
    }
    powershell(&format!(
        "Set-NetAdapterAdvancedProperty -Name {} -RegistryKeyword {} -RegistryValue {}",
        ps_quote(&prop.adapter),
        ps_quote(&prop.keyword),
        ps_quote(value)
    ))
    .map(|_| ())
}

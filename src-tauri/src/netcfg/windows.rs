//! Leitura da configuração fixa no registro e aplicação via `netsh`.

use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{is_guid, restore_commands, split_servers, validate, AdapterConfig, StaticIpv4};
use crate::error::{AppError, AppResult};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TCPIP4: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";
const TCPIP6: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces";
/// Classe "Adaptadores de rede": guarda o nome que aparece em "Conexões de rede".
const NET_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Network\{4D36E972-E325-11CE-BFC1-08002BE10318}";

fn hklm() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}

/// Nome atual do adaptador ("Ethernet"), pelo GUID. `None`: o adaptador não existe mais.
pub fn alias_for(guid: &str) -> Option<String> {
    hklm().open_subkey(format!(r"{NET_CLASS}\{guid}\Connection")).ok()?.get_value::<String, _>("Name").ok()
}

fn multi(key: &RegKey, name: &str) -> Vec<String> {
    key.get_value::<Vec<String>, _>(name)
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "0.0.0.0")
        .collect()
}

fn read_adapter(guid: &str) -> Option<AdapterConfig> {
    let name = alias_for(guid)?; // sem nome = entrada órfã de adaptador removido
    let v4 = hklm().open_subkey(format!(r"{TCPIP4}\{guid}")).ok()?;
    let ipv4 = (v4.get_value::<u32, _>("EnableDHCP").unwrap_or(1) == 0)
        .then(|| {
            let ips = multi(&v4, "IPAddress");
            let masks = multi(&v4, "SubnetMask");
            StaticIpv4 { addresses: ips.into_iter().zip(masks).collect(), gateways: multi(&v4, "DefaultGateway") }
        })
        .filter(|ip| !ip.addresses.is_empty());
    let dns4 = split_servers(&v4.get_value::<String, _>("NameServer").unwrap_or_default());
    let dns6 = hklm()
        .open_subkey(format!(r"{TCPIP6}\{guid}"))
        .ok()
        .and_then(|k| k.get_value::<String, _>("NameServer").ok())
        .map(|s| split_servers(&s))
        .unwrap_or_default();
    let a = AdapterConfig { guid: guid.to_string(), name, ipv4, dns4, dns6 };
    // Só entra o que passa na mesma validação da volta: nada de foto que não restaura.
    (a.has_static() && validate(&a).is_ok()).then_some(a)
}

/// Adaptadores com algo configurado à mão (o que o reset apagaria).
pub fn read_static() -> Vec<AdapterConfig> {
    let Ok(root) = hklm().open_subkey(TCPIP4) else { return Vec::new() };
    root.enum_keys().filter_map(Result::ok).filter(|g| is_guid(g)).filter_map(|g| read_adapter(&g)).collect()
}

/// Quando o Windows ligou, em segundos Unix.
pub fn boot_at() -> i64 {
    crate::restore::now() - (unsafe { GetTickCount64() } / 1000) as i64
}

/// Recoloca a configuração (no auxiliar elevado). Valida de novo e usa o nome ATUAL do
/// adaptador lido pelo GUID, nunca o nome que veio no pedido.
pub fn apply(a: &AdapterConfig) -> AppResult<()> {
    validate(a).map_err(AppError::InvalidInput)?;
    let alias = alias_for(&a.guid).ok_or_else(|| AppError::NotFound(format!("o adaptador {} não existe mais", a.name)))?;
    for args in restore_commands(a, &alias) {
        let out = Command::new("netsh").args(&args).creation_flags(CREATE_NO_WINDOW).output()?;
        if !out.status.success() {
            let text = crate::system::decode_console(&out.stdout);
            return Err(AppError::CommandFailed { command: format!("netsh {}", args[..4].join(" ")), detail: text.trim().to_string() });
        }
    }
    Ok(())
}

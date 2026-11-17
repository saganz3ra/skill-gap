//! Foto da configuração de rede fixa (IP e DNS digitados à mão) antes do
//! "Resetar a rede", e o caminho de volta.
//!
//! Por quê: `netsh int ip reset` volta TODOS os adaptadores ao automático (DHCP). Quem
//! tinha DNS fixo (1.1.1.1, o DNS do provedor) ou IP fixo perde essa configuração em
//! silêncio, e às vezes a internet para de resolver nomes (aconteceu no teste: o DNS
//! automático entregue pelo roteador não respondia). Regra do projeto: anota antes de
//! mexer.
//!
//! Onde o Windows guarda isso (lido direto do registro, sem PowerShell):
//! - `Tcpip\Parameters\Interfaces\{GUID}`: `EnableDHCP`, `IPAddress`, `SubnetMask`,
//!   `DefaultGateway` (IPv4) e `NameServer` (DNS fixo IPv4; o automático fica em
//!   `DhcpNameServer`, que não interessa).
//! - `Tcpip6\Parameters\Interfaces\{GUID}`: `NameServer` (DNS fixo IPv6).
//! - O nome do adaptador ("Ethernet") vem de `Control\Network\{classe}\{GUID}\Connection`.
//!
//! Restaurar usa `netsh` com argumentos separados (nada de shell), no auxiliar elevado.
//! O auxiliar valida cada endereço e pega o nome do adaptador pelo GUID no próprio
//! registro: o pedido nunca escolhe em qual adaptador mexer pelo nome.

use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// IPv4 fixo de um adaptador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticIpv4 {
    /// (endereço, máscara), na ordem do Windows.
    pub addresses: Vec<(String, String)>,
    pub gateways: Vec<String>,
}

/// O que um adaptador tinha de configurado à mão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterConfig {
    /// `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`: identifica o adaptador sem depender do nome.
    pub guid: String,
    /// Só pra exibir. Na hora de restaurar, o nome é relido do registro pelo GUID.
    pub name: String,
    pub ipv4: Option<StaticIpv4>,
    pub dns4: Vec<String>,
    pub dns6: Vec<String>,
}

impl AdapterConfig {
    pub fn has_static(&self) -> bool {
        self.ipv4.is_some() || !self.dns4.is_empty() || !self.dns6.is_empty()
    }
}

/// "1.1.1.1,8.8.8.8" ou "1.1.1.1 8.8.8.8" (o Windows usa os dois formatos).
pub fn split_servers(s: &str) -> Vec<String> {
    s.split([',', ' ', ';']).map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect()
}

pub fn is_guid(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else { return false };
    let parts: Vec<&str> = inner.split('-').collect();
    parts.len() == 5
        && parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12])
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn v4(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok()
}

fn mask_ok(s: &str) -> bool {
    // Máscara válida: uns seguidos de zeros (255.255.255.0 sim, 255.0.255.0 não).
    s.parse::<Ipv4Addr>().is_ok_and(|m| {
        let bits = u32::from(m);
        bits.leading_ones() + bits.trailing_zeros() == 32 && bits != 0
    })
}

/// Validação feita DE NOVO no auxiliar elevado, antes de qualquer `netsh`.
pub fn validate(a: &AdapterConfig) -> Result<(), String> {
    if !is_guid(&a.guid) {
        return Err(format!("adaptador inválido: {}", a.guid));
    }
    if let Some(ip) = &a.ipv4 {
        if ip.addresses.is_empty() {
            return Err("IP fixo sem endereço".into());
        }
        for (addr, mask) in &ip.addresses {
            if !v4(addr) || !mask_ok(mask) {
                return Err(format!("endereço/máscara inválidos: {addr}/{mask}"));
            }
        }
        if !ip.gateways.iter().all(|g| v4(g)) {
            return Err("gateway inválido".into());
        }
    }
    if !a.dns4.iter().all(|d| v4(d)) || !a.dns6.iter().all(|d| d.parse::<Ipv6Addr>().is_ok()) {
        return Err("DNS inválido".into());
    }
    if a.dns4.len() > 8 || a.dns6.len() > 8 {
        return Err("DNS demais".into());
    }
    Ok(())
}

/// Comandos `netsh` (argumentos já separados) que recolocam a configuração. `alias` é
/// o nome atual do adaptador, lido do registro pelo GUID. Forma posicional do netsh,
/// pra o nome com espaço ("Ethernet 2") ir como um argumento só.
pub fn restore_commands(a: &AdapterConfig, alias: &str) -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut cmds = Vec::new();
    if let Some(ip) = &a.ipv4 {
        for (i, (addr, mask)) in ip.addresses.iter().enumerate() {
            if i == 0 {
                let mut c = s(&["interface", "ipv4", "set", "address", alias, "static", addr, mask]);
                if let Some(gw) = ip.gateways.first() {
                    c.push(gw.clone());
                }
                cmds.push(c);
            } else {
                cmds.push(s(&["interface", "ipv4", "add", "address", alias, addr, mask]));
            }
        }
    }
    for (family, servers) in [("ipv4", &a.dns4), ("ipv6", &a.dns6)] {
        for (i, d) in servers.iter().enumerate() {
            if i == 0 {
                cmds.push(s(&["interface", family, "set", "dnsservers", alias, "static", d, "primary", "validate=no"]));
            } else {
                let index = format!("index={}", i + 1);
                cmds.push(s(&["interface", family, "add", "dnsservers", alias, d, &index, "validate=no"]));
            }
        }
    }
    cmds
}

// ---------------- Foto pendente (até o usuário restaurar ou descartar) ----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub taken_at: i64,
    /// Quando o Windows ligou (segundos Unix). Igual ao atual = ainda não reiniciou.
    pub boot_at: i64,
    pub adapters: Vec<AdapterConfig>,
}

/// Mesma sessão do Windows? O cálculo "agora − tempo ligado" oscila alguns segundos.
pub fn same_boot(a: i64, b: i64) -> bool {
    (a - b).abs() < 120
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetStatus {
    /// Reset feito e o PC ainda não reiniciou: rodar de novo fica bloqueado.
    pub needs_reboot: bool,
    /// Configuração guardada esperando "Restaurar" ou "Descartar".
    pub pending: Option<Pending>,
    /// O que este PC tem de fixo AGORA (o que o reset apagaria).
    pub current: Vec<AdapterConfig>,
}

pub struct NetState {
    path: PathBuf,
    pending: Mutex<Option<Pending>>,
}

impl NetState {
    pub fn load(path: PathBuf) -> Self {
        let pending = fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        Self { path, pending: Mutex::new(pending) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, value: &Option<Pending>) -> AppResult<()> {
        match value {
            None => match fs::remove_file(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            },
            Some(p) => {
                if let Some(dir) = self.path.parent() {
                    fs::create_dir_all(dir)?;
                }
                let tmp = self.path.with_extension("json.tmp");
                fs::write(&tmp, serde_json::to_vec_pretty(p).expect("serializável"))?;
                fs::rename(&tmp, &self.path)?;
                Ok(())
            }
        }
    }

    pub fn status(&self, boot_at: i64, current: Vec<AdapterConfig>) -> ResetStatus {
        let pending = self.lock().clone();
        ResetStatus { needs_reboot: pending.as_ref().is_some_and(|p| same_boot(p.boot_at, boot_at)), pending, current }
    }

    /// Antes do reset: grava a foto (o diário vem ANTES de mexer). Recusa se já houve
    /// um reset nesta sessão do Windows (o segundo apagaria a foto boa com uma vazia).
    pub fn before_reset(&self, boot_at: i64, now: i64, adapters: Vec<AdapterConfig>) -> AppResult<()> {
        let mut slot = self.lock();
        if let Some(p) = slot.as_ref() {
            if same_boot(p.boot_at, boot_at) {
                return Err(AppError::InvalidInput("a rede já foi resetada; reinicie o PC antes de rodar de novo".into()));
            }
            if p.adapters.iter().any(AdapterConfig::has_static) {
                return Err(AppError::InvalidInput(
                    "há uma configuração de rede guardada do reset anterior: restaure ou descarte antes de resetar de novo".into(),
                ));
            }
        }
        let new = Some(Pending { taken_at: now, boot_at, adapters });
        self.persist(&new)?;
        *slot = new;
        Ok(())
    }

    /// Restaura (pelo `apply`, que fala com o Windows) e só apaga a foto se tudo deu certo.
    pub fn restore(&self, mut apply: impl FnMut(&AdapterConfig) -> AppResult<()>) -> AppResult<()> {
        let mut slot = self.lock();
        let Some(p) = slot.clone() else { return Ok(()) };
        for a in p.adapters.iter().filter(|a| a.has_static()) {
            apply(a)?;
        }
        self.persist(&None)?;
        *slot = None;
        Ok(())
    }

    pub fn discard(&self) -> AppResult<()> {
        let mut slot = self.lock();
        self.persist(&None)?;
        *slot = None;
        Ok(())
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{apply, boot_at, read_static};

#[cfg(not(windows))]
pub fn read_static() -> Vec<AdapterConfig> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn boot_at() -> i64 {
    0
}
#[cfg(not(windows))]
pub fn apply(_a: &AdapterConfig) -> AppResult<()> {
    Err(AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "{4d36e972-e325-11ce-bfc1-08002be10318}";

    fn adapter() -> AdapterConfig {
        AdapterConfig {
            guid: G.into(),
            name: "Ethernet 2".into(),
            ipv4: Some(StaticIpv4 {
                addresses: vec![("192.168.1.50".into(), "255.255.255.0".into()), ("10.0.0.5".into(), "255.0.0.0".into())],
                gateways: vec!["192.168.1.1".into()],
            }),
            dns4: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            dns6: vec!["2606:4700:4700::1111".into()],
        }
    }

    #[test]
    fn le_os_dois_formatos_de_lista() {
        assert_eq!(split_servers("1.1.1.1,8.8.8.8"), ["1.1.1.1", "8.8.8.8"]);
        assert_eq!(split_servers(" 1.1.1.1 8.8.8.8 "), ["1.1.1.1", "8.8.8.8"]);
        assert!(split_servers("").is_empty());
    }

    #[test]
    fn valida_tudo_antes_do_netsh() {
        assert!(validate(&adapter()).is_ok());
        let mut a = adapter();
        a.dns4 = vec!["1.1.1.1 & del C:\\".into()];
        assert!(validate(&a).is_err(), "nada que não seja IP chega no netsh");
        let mut a = adapter();
        a.ipv4.as_mut().unwrap().addresses[0].1 = "255.0.255.0".into();
        assert!(validate(&a).is_err(), "máscara com buraco");
        let mut a = adapter();
        a.guid = "Ethernet".into();
        assert!(validate(&a).is_err());
        let mut a = adapter();
        a.dns6 = vec!["1.1.1.1".into()];
        assert!(validate(&a).is_err(), "IPv4 na lista de IPv6");
    }

    #[test]
    fn comandos_de_volta() {
        let cmds = restore_commands(&adapter(), "Ethernet 2");
        let line = |i: usize| cmds[i].join(" ");
        assert_eq!(line(0), "interface ipv4 set address Ethernet 2 static 192.168.1.50 255.255.255.0 192.168.1.1");
        assert_eq!(line(1), "interface ipv4 add address Ethernet 2 10.0.0.5 255.0.0.0");
        assert_eq!(line(2), "interface ipv4 set dnsservers Ethernet 2 static 1.1.1.1 primary validate=no");
        assert_eq!(line(3), "interface ipv4 add dnsservers Ethernet 2 8.8.8.8 index=2 validate=no");
        assert_eq!(line(4), "interface ipv6 set dnsservers Ethernet 2 static 2606:4700:4700::1111 primary validate=no");
        assert_eq!(cmds[0][4], "Ethernet 2", "o nome com espaço vai como UM argumento");
    }

    #[test]
    fn so_dns_sem_ip_fixo() {
        let a = AdapterConfig { ipv4: None, dns6: vec![], ..adapter() };
        let cmds = restore_commands(&a, "Wi-Fi");
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().all(|c| c[3] == "dnsservers"));
    }

    #[test]
    fn diario_bloqueia_segundo_reset_e_so_apaga_depois_de_restaurar() {
        let dir = std::env::temp_dir().join(format!("sg-net-{}", std::process::id()));
        let path = dir.join("rede.json");
        let _ = fs::remove_dir_all(&dir);
        let s = NetState::load(path.clone());

        s.before_reset(1000, 2000, vec![adapter()]).unwrap();
        assert!(s.status(1030, vec![]).needs_reboot, "mesma sessão do Windows");
        assert!(s.before_reset(1030, 2100, vec![]).is_err(), "segundo reset sem reiniciar apagaria a foto boa");

        // Reiniciou: não precisa mais reiniciar, mas a foto continua lá.
        let s = NetState::load(path.clone());
        let st = s.status(9000, vec![]);
        assert!(!st.needs_reboot && st.pending.is_some());
        assert!(s.before_reset(9000, 9100, vec![]).is_err(), "restaure ou descarte antes");

        // Falha ao aplicar: a foto fica.
        assert!(s.restore(|_| Err(AppError::RequiresAdmin)).is_err());
        assert!(NetState::load(path.clone()).status(9000, vec![]).pending.is_some());

        let mut applied = 0;
        s.restore(|_| {
            applied += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(applied, 1);
        assert!(NetState::load(path.clone()).status(9000, vec![]).pending.is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn sem_nada_fixo_o_segundo_reset_depois_de_reiniciar_e_livre() {
        let dir = std::env::temp_dir().join(format!("sg-net2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let s = NetState::load(dir.join("rede.json"));
        s.before_reset(1000, 2000, vec![]).unwrap();
        assert!(s.before_reset(9000, 9100, vec![]).is_ok());
        let _ = fs::remove_dir_all(dir);
    }
}

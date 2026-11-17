//! Configurações globais de rede lidas/gravadas pelos cmdlets do PowerShell.
//!
//! Por que cmdlets e não `netsh`: a saída do `netsh` vem traduzida ("Estado: desabilitado"),
//! e os valores de enum dos cmdlets (`Disabled`, `Enabled`) são iguais em qualquer idioma.
//! Cada chave tem uma lista FECHADA de valores: o que vai pro script é sempre um item
//! dessa lista, nunca texto livre (inclusive o valor antigo vindo do diário).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetKey {
    /// Túnel IPv6 sobre IPv4 (Teredo).
    Teredo,
    /// Túnel ISATAP (removido nas versões novas: aí o ajuste fica indisponível).
    Isatap,
    /// Túnel 6to4.
    SixToFour,
    /// Endereços IPv6 temporários (privacidade).
    Ipv6Temporary,
    /// Agrupamento de pacotes recebidos (packet coalescing).
    PacketCoalescing,
    /// Ajuste automático da janela de recepção TCP (perfil "Internet").
    TcpAutotuning,
}

impl NetKey {
    pub const ALL: [NetKey; 6] =
        [NetKey::Teredo, NetKey::Isatap, NetKey::SixToFour, NetKey::Ipv6Temporary, NetKey::PacketCoalescing, NetKey::TcpAutotuning];

    /// Todos os valores que o Windows pode devolver (o desfazer precisa poder voltar a qualquer um).
    pub fn allowed(self) -> &'static [&'static str] {
        match self {
            NetKey::Teredo => &["Default", "Disabled", "Client", "Enterpriseclient", "Natawareclient", "Server"],
            NetKey::Isatap | NetKey::SixToFour => &["Default", "Enabled", "Disabled", "Automatic"],
            NetKey::Ipv6Temporary => &["Disabled", "Enabled", "Always", "Counted"],
            NetKey::PacketCoalescing => &["Disabled", "Enabled"],
            NetKey::TcpAutotuning => &["Disabled", "HighlyRestricted", "Restricted", "Normal", "Experimental"],
        }
    }

    /// Normaliza para a grafia da lista (o PowerShell devolve com a capitalização dele).
    pub fn canonical(self, value: &str) -> Option<&'static str> {
        self.allowed().iter().copied().find(|a| a.eq_ignore_ascii_case(value.trim()))
    }

    pub fn get_script(self) -> &'static str {
        match self {
            NetKey::Teredo => "(Get-NetTeredoConfiguration).Type",
            NetKey::Isatap => "(Get-NetIsatapConfiguration).State",
            NetKey::SixToFour => "(Get-Net6to4Configuration).State",
            NetKey::Ipv6Temporary => "(Get-NetIPv6Protocol).UseTemporaryAddresses",
            NetKey::PacketCoalescing => "(Get-NetOffloadGlobalSetting).PacketCoalescingFilter",
            NetKey::TcpAutotuning => "(Get-NetTCPSetting -SettingName Internet).AutoTuningLevelLocal",
        }
    }

    /// Script de escrita. `None` se o valor não está na lista (nunca chega ao PowerShell).
    pub fn set_script(self, value: &str) -> Option<String> {
        let v = self.canonical(value)?;
        Some(match self {
            NetKey::Teredo => format!("Set-NetTeredoConfiguration -Type {v}"),
            NetKey::Isatap => format!("Set-NetIsatapConfiguration -State {v}"),
            NetKey::SixToFour => format!("Set-Net6to4Configuration -State {v}"),
            NetKey::Ipv6Temporary => format!("Set-NetIPv6Protocol -UseTemporaryAddresses {v}"),
            NetKey::PacketCoalescing => format!("Set-NetOffloadGlobalSetting -PacketCoalescingFilter {v}"),
            NetKey::TcpAutotuning => format!("Set-NetTCPSetting -SettingName Internet -AutoTuningLevelLocal {v}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_valores_da_lista_viram_script() {
        assert_eq!(NetKey::Teredo.set_script("disabled").unwrap(), "Set-NetTeredoConfiguration -Type Disabled");
        assert!(NetKey::Teredo.set_script("Disabled; Remove-Item C:\\").is_none());
        assert!(NetKey::PacketCoalescing.set_script("").is_none());
        for k in NetKey::ALL {
            for v in k.allowed() {
                assert!(k.set_script(v).is_some(), "{k:?}: {v}");
            }
        }
    }
}

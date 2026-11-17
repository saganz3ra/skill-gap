//! Correções: reparos conhecidos do Windows, em um clique e com o log de cada passo.
//!
//! Todas usam ferramentas do próprio Windows (ipconfig, netsh, sfc, DISM, w32tm).
//! Nada é baixado ou instalado.

use serde::Serialize;

use crate::tweaks::catalog::Restart;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Cmd { label: &'static str, program: &'static str, args: &'static [&'static str] },
    StopService { label: &'static str, name: &'static str },
    StartService { label: &'static str, name: &'static str },
    /// Apaga o conteúdo de C:\Windows\SoftwareDistribution\Download (com o serviço parado).
    CleanUpdateCache { label: &'static str },
    RestartExplorer { label: &'static str },
}

impl Step {
    pub fn label(&self) -> &'static str {
        match self {
            Step::Cmd { label, .. }
            | Step::StopService { label, .. }
            | Step::StartService { label, .. }
            | Step::CleanUpdateCache { label }
            | Step::RestartExplorer { label } => label,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Fix {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Quando usar, em uma frase.
    pub when: &'static str,
    pub requires_admin: bool,
    pub restart: Restart,
    /// Estimativa pro usuário saber se dá tempo de esperar.
    pub duration: &'static str,
    pub steps: &'static [Step],
}

pub static FIXES: &[Fix] = &[
    Fix {
        id: "flush_dns",
        name: "Limpar cache de DNS",
        description: "Esquece os endereços de sites guardados no PC.",
        when: "Site que mudou de servidor não abre, ou abre a versão antiga.",
        // No Windows 11 atual o `ipconfig /flushdns` responde "requer elevação" sem admin.
        requires_admin: true,
        restart: Restart::None,
        duration: "1 segundo",
        steps: &[Step::Cmd { label: "Limpando cache de DNS", program: "ipconfig", args: &["/flushdns"] }],
    },
    Fix {
        id: "network_reset",
        name: "Resetar a rede",
        description: "Volta Winsock e TCP/IP ao padrão de fábrica do Windows.",
        when: "Internet funciona no celular mas não no PC, mesmo com cabo/Wi-Fi conectado. Apaga IP e DNS fixos: o app guarda os seus antes e oferece restaurar depois de reiniciar. VPN e proxy podem precisar ser refeitos.",
        requires_admin: true,
        restart: Restart::Reboot,
        duration: "alguns segundos",
        steps: &[
            Step::Cmd { label: "Resetando Winsock", program: "netsh", args: &["winsock", "reset"] },
            Step::Cmd { label: "Resetando TCP/IP", program: "netsh", args: &["int", "ip", "reset"] },
            Step::Cmd { label: "Limpando cache de DNS", program: "ipconfig", args: &["/flushdns"] },
        ],
    },
    Fix {
        id: "windows_update_cache",
        name: "Reparar o Windows Update",
        description: "Para o serviço, apaga as atualizações baixadas e liga de novo.",
        when: "Atualização trava em uma porcentagem ou falha sempre com o mesmo erro.",
        requires_admin: true,
        restart: Restart::None,
        duration: "até 1 minuto",
        steps: &[
            Step::StopService { label: "Parando o Windows Update", name: "wuauserv" },
            Step::StopService { label: "Parando a transferência em segundo plano (BITS)", name: "BITS" },
            Step::CleanUpdateCache { label: "Apagando atualizações baixadas" },
            Step::StartService { label: "Ligando o BITS", name: "BITS" },
            Step::StartService { label: "Ligando o Windows Update", name: "wuauserv" },
        ],
    },
    Fix {
        id: "sfc",
        name: "Reparar arquivos do sistema",
        description: "Verifica e restaura arquivos protegidos do Windows (sfc /scannow).",
        when: "Erros estranhos, apps do Windows que não abrem, telas azuis depois de um desligamento brusco.",
        requires_admin: true,
        restart: Restart::Reboot,
        duration: "10 a 20 minutos",
        steps: &[Step::Cmd { label: "Verificando arquivos do sistema", program: "sfc", args: &["/scannow"] }],
    },
    Fix {
        id: "dism",
        name: "Reparar a imagem do Windows",
        description: "Baixa da Microsoft os componentes corrompidos (DISM /RestoreHealth).",
        when: "Quando o “Reparar arquivos do sistema” diz que não conseguiu corrigir tudo. Rode este e depois o sfc de novo. Precisa de internet.",
        requires_admin: true,
        restart: Restart::None,
        duration: "15 a 30 minutos",
        steps: &[Step::Cmd {
            label: "Reparando a imagem do Windows",
            program: "DISM",
            args: &["/Online", "/Cleanup-Image", "/RestoreHealth"],
        }],
    },
    Fix {
        id: "time_sync",
        name: "Sincronizar o relógio",
        description: "Acerta a hora com o servidor de tempo do Windows.",
        when: "Relógio adiantado/atrasado, erro de certificado em sites, launcher de jogo que não conecta.",
        requires_admin: true,
        restart: Restart::None,
        duration: "alguns segundos",
        steps: &[
            Step::StartService { label: "Ligando o serviço de horário", name: "W32Time" },
            Step::Cmd { label: "Sincronizando", program: "w32tm", args: &["/resync", "/force"] },
        ],
    },
    Fix {
        id: "restart_explorer",
        name: "Reiniciar o Explorer",
        description: "Fecha e abre de novo a barra de tarefas e o Explorador de Arquivos.",
        when: "Barra de tarefas ou menu Iniciar travados, ícones sumidos da bandeja.",
        requires_admin: false,
        restart: Restart::None,
        duration: "2 segundos",
        steps: &[Step::RestartExplorer { label: "Reiniciando o Explorer" }],
    },
];

pub fn find(id: &str) -> Option<&'static Fix> {
    FIXES.iter().find(|f| f.id == id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixView {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub when: &'static str,
    pub requires_admin: bool,
    pub restart: Restart,
    pub duration: &'static str,
    pub steps: Vec<&'static str>,
}

impl From<&Fix> for FixView {
    fn from(f: &Fix) -> Self {
        Self {
            id: f.id,
            name: f.name,
            description: f.description,
            when: f.when,
            requires_admin: f.requires_admin,
            restart: f.restart,
            duration: f.duration,
            steps: f.steps.iter().map(Step::label).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub label: &'static str,
    pub ok: bool,
    /// Últimas linhas da saída, já limpas.
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixReport {
    pub id: &'static str,
    pub ok: bool,
    pub steps: Vec<StepResult>,
    pub restart: Restart,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixProgress {
    pub id: &'static str,
    pub step: usize,
    pub total: usize,
    pub label: &'static str,
}

// ---------------- Saída dos comandos ----------------

/// Ver `system::decode_console` (UTF-16 do sfc, UTF-8 ou code page OEM).
pub fn decode_output(bytes: &[u8]) -> String {
    crate::system::decode_console(bytes)
}

/// Barras de progresso (sfc e DISM) reescrevem a mesma linha com `\r`: fica só a
/// última versão de cada linha. Devolve as últimas `max` linhas não vazias.
pub fn clean_output(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .map(|l| l.rsplit('\r').next().unwrap_or(l).trim())
        .filter(|l| !l.is_empty())
        .collect();
    let start = lines.len().saturating_sub(max);
    lines[start..].join("\n")
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::run;

#[cfg(not(windows))]
pub fn run(_fix: &Fix, _on_progress: impl FnMut(FixProgress)) -> FixReport {
    FixReport { id: _fix.id, ok: false, steps: Vec::new(), restart: Restart::None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalogo_valido() {
        let mut ids = HashSet::new();
        for f in FIXES {
            assert!(ids.insert(f.id));
            assert!(!f.steps.is_empty());
        }
    }

    #[test]
    fn utf16_do_sfc() {
        let text = "Verificação 100% concluída.";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_output(&bytes), text);
        assert_eq!(decode_output("ok\r\n".as_bytes()), "ok\r\n");
    }

    #[test]
    fn progresso_com_retorno_de_carro() {
        let raw = "Início\r\n[==   10%  ]\r[=====  50% ]\r[==========100%]\r\nConcluído.\r\n\r\n";
        assert_eq!(clean_output(raw, 10), "Início\n[==========100%]\nConcluído.");
        assert_eq!(clean_output(raw, 1), "Concluído.");
    }
}

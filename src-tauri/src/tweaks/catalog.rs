//! Catálogo de ajustes e perfis prontos. Fonte única dos textos: o front só exibe.
//!
//! Regras do catálogo:
//! - Só entra ajuste com efeito verificável. Placebo conhecido (limpar RAM,
//!   NetworkThrottlingIndex) fica de fora de propósito.
//! - Ajuste de efeito pequeno ou dependente do PC leva `Impact::Small` e uma explicação
//!   honesta em `details`, e nunca entra nos perfis prontos.
//! - Risco médio ou alto exige `breaks`: uma linha dizendo o que pode parar de funcionar.
//! - Risco alto exige confirmação (conferida no backend) e nunca entra em perfil.
//! - `heats` (mais calor/consumo) nunca entra em perfil: só aplicado um a um, com aviso.
//! - Dois ajustes nunca escrevem no mesmo lugar: desfazer um apagaria o outro.

use serde::Serialize;

use super::ops::{Hive::*, NetKey, Op};
use crate::power::PowerPlan;
use crate::system::{DiskKind, SystemProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Geral,
    Aparencia,
    Desempenho,
    Privacidade,
    Gaming,
    Rede,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Impact {
    /// Efeito pequeno ou que depende do hardware: aparece com aviso, fora dos perfis.
    Small,
    Medium,
    High,
}

/// Chance de algo parar de funcionar. Independe do impacto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Low,
    Medium,
    /// Exige confirmação explícita, conferida no backend.
    High,
}

/// Só faz sentido em desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopOnly {
    /// `true`: em notebook aparece com aviso (dá pra aplicar à mão; perfis pulam).
    /// `false`: em notebook fica indisponível.
    pub warn: bool,
}

/// Requisitos declarativos do ajuste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requires {
    pub min_build: Option<u32>,
    pub min_ram_gb: Option<u32>,
    pub ssd: bool,
    pub desktop_only: Option<DesktopOnly>,
}

/// Sem requisitos. Com requisito: `Requires { min_build: Some(22000), ..ANY }`.
pub const ANY: Requires = Requires { min_build: None, min_ram_gb: None, ssd: false, desktop_only: None };

/// Se o ajuste serve para ESTE PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "reason", rename_all = "camelCase")]
pub enum Gate {
    Ok,
    /// Dá pra aplicar à mão, com o aviso; perfis pulam.
    Warn(String),
    /// Some do "aplicar". NUNCA bloqueia o desfazer.
    Unavailable(String),
}

/// Avalia os requisitos. Quando não deu pra ler algo (build 0, disco Unknown), o
/// resultado é aviso, não bloqueio: na dúvida, quem decide é o usuário.
pub fn gate(t: &Tweak, s: &SystemProfile) -> Gate {
    let (mut block, mut warn) = (Vec::new(), Vec::new());
    let r = t.requires;
    if let Some(min) = r.min_build {
        if s.build == 0 {
            warn.push("não deu pra ler a versão do Windows".to_string());
        } else if s.build < min {
            block.push(if min >= 22000 { "exige Windows 11".to_string() } else { format!("exige Windows build {min} ou mais novo") });
        }
    }
    if let Some(min) = r.min_ram_gb {
        if s.ram_gb == 0 {
            warn.push("não deu pra ler a RAM instalada".to_string());
        } else if s.ram_gb < min {
            block.push(format!("exige {min} GB de RAM ou mais (este PC tem {})", s.ram_gb));
        }
    }
    if r.ssd {
        match s.system_disk {
            DiskKind::Ssd => {}
            DiskKind::Hdd => block.push("o Windows está num HD mecânico: aqui atrapalha".to_string()),
            DiskKind::Unknown => warn.push("não deu pra saber se o disco do sistema é SSD".to_string()),
        }
    }
    if let (Some(d), true) = (r.desktop_only, s.is_laptop) {
        let msg = "feito para desktop: em notebook esquenta e gasta bateria".to_string();
        if d.warn {
            warn.push(msg)
        } else {
            block.push(msg)
        }
    }
    if !block.is_empty() {
        Gate::Unavailable(block.join("; "))
    } else if !warn.is_empty() {
        Gate::Warn(warn.join("; "))
    } else {
        Gate::Ok
    }
}

/// Validação de backend antes de aplicar um ajuste avulso. O front também mostra, mas
/// não é ele quem garante.
pub fn check_apply(t: &Tweak, s: &SystemProfile, confirmed: bool) -> crate::error::AppResult<()> {
    if let Gate::Unavailable(reason) = gate(t, s) {
        return Err(crate::error::AppError::Unavailable(reason));
    }
    if t.risk == Risk::High && !confirmed {
        return Err(crate::error::AppError::ConfirmationRequired(t.name.to_string()));
    }
    Ok(())
}

/// Por que um perfil (aplicar em um clique) pula este ajuste, se pula.
pub fn profile_skip_reason(t: &Tweak, s: &SystemProfile) -> Option<String> {
    match gate(t, s) {
        Gate::Unavailable(r) => return Some(r),
        Gate::Warn(r) => return Some(format!("{r} (aplique à mão se quiser)")),
        Gate::Ok => {}
    }
    if t.heats {
        return Some("aumenta calor e consumo: só aplicado um a um".into());
    }
    if t.risk == Risk::High {
        return Some("risco alto: precisa de confirmação, aplique à mão".into());
    }
    None
}


/// O que é preciso pra mudança valer. Ordenado do mais leve pro mais pesado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Restart {
    None,
    Explorer,
    Logoff,
    Reboot,
}

#[derive(Debug, Clone, Copy)]
pub struct Tweak {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Por que existe, quando ajuda e quando não ajuda.
    pub details: &'static str,
    pub category: Category,
    pub impact: Impact,
    pub risk: Risk,
    /// O que pode parar de funcionar (obrigatório com risco médio ou alto).
    pub breaks: Option<&'static str>,
    /// Pode aumentar temperatura e consumo: aviso + fora dos perfis.
    pub heats: bool,
    pub requires: Requires,
    /// Versão do app em que o ajuste entrou (selo "novo"; tem que estar no CHANGELOG.json).
    pub since: &'static str,
    pub restart: Restart,
    pub ops: &'static [Op],
}

impl Tweak {
    /// Exige admin se mexe em HKLM, serviço ou compactação de memória.
    pub fn requires_admin(&self) -> bool {
        self.ops.iter().any(|op| match op {
            Op::Dword { hive, .. } | Op::Str { hive, .. } | Op::KeyWithEmptyDefault { hive, .. } => *hive == Hklm,
            Op::Service { .. }
            | Op::MemoryCompressionOff
            | Op::PageCombiningOff
            | Op::ScheduledTaskOff { .. }
            | Op::Net { .. }
            | Op::PowerSetting { .. } => true,
            Op::PowerPlan(_) | Op::MouseAccelerationOff | Op::MouseSpeed(_) | Op::JsonFileKey { .. } => false,
        })
    }
}

const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const CLASSIC_MENU_ROOT: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";
const CLASSIC_MENU_KEY: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32";

pub static TWEAKS: &[Tweak] = &[
    // ---------------- Gaming ----------------
    Tweak {
        id: "game_mode",
        name: "Modo de Jogo do Windows",
        description: "Prioriza o jogo e segura o Windows Update durante a partida.",
        details: "Recurso nativo do Windows 10/11. Vem ligado por padrão; este ajuste só garante que ninguém desligou.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\GameBar", name: "AutoGameModeEnabled", value: 1 }],
    },
    Tweak {
        id: "game_dvr_off",
        name: "Desativar gravação da Game Bar",
        description: "Para a gravação de clipes da Xbox Game Bar, inclusive em segundo plano.",
        details: "A gravação em segundo plano usa o codificador de vídeo da GPU o tempo todo. Se você grava clipes pela Game Bar, deixe desligado este ajuste.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[
            Op::Dword { hive: Hkcu, path: r"System\GameConfigStore", name: "GameDVR_Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\GameDVR", name: "AppCaptureEnabled", value: 0 },
        ],
    },
    Tweak {
        id: "hags",
        name: "Agendamento de GPU por hardware (HAGS)",
        description: "A própria GPU organiza a fila de quadros.",
        details: "Obrigatório pro DLSS Frame Generation. O ganho de latência varia por jogo e driver; em GPUs antigas pode não aparecer a opção. Precisa reiniciar.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::Reboot,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", name: "HwSchMode", value: 2 }],
    },
    Tweak {
        id: "mouse_accel_off",
        name: "Desativar aceleração do mouse",
        description: "Movimento 1:1: a mesma distância física sempre move o cursor o mesmo tanto.",
        details: "Desliga o “Aumentar precisão do ponteiro”. Essencial pra memória muscular de mira; a maioria dos jogos competitivos já usa raw input, mas menus e jogos sem raw input passam a se comportar igual.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::MouseAccelerationOff],
    },
    Tweak {
        id: "mouse_speed_default",
        name: "Velocidade do ponteiro no padrão (10)",
        description: "Cada contagem do sensor move exatamente 1 pixel.",
        details: "É a 6ª marca (de 11) do controle de velocidade do Windows. Acima disso o Windows pula pixels; abaixo, descarta movimento. Ajuste a sensibilidade pelo DPI do mouse e pelo jogo, não por aqui.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::MouseSpeed(10)],
    },
    Tweak {
        id: "sticky_keys_off",
        name: "Desativar atalho das Teclas de Aderência",
        description: "Apertar Shift 5 vezes não abre mais a janela que tira você do jogo.",
        details: "Só desliga o atalho de teclado; o recurso de acessibilidade continua disponível nas Configurações. Vale após sair e entrar na conta.",
        category: Category::Gaming,
        impact: Impact::Medium,
        restart: Restart::Logoff,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Str { hive: Hkcu, path: r"Control Panel\Accessibility\StickyKeys", name: "Flags", value: "506" }],
    },
    // ---------------- Desempenho ----------------
    Tweak {
        id: "power_high",
        name: "Plano de energia Alto desempenho",
        description: "A CPU não entra nos estados mais profundos de economia.",
        details: "Responde mais rápido a picos de carga, com mais consumo e calor. Por isso não entra nos perfis: aplique à mão se quiser.",
        category: Category::Desempenho,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: true,
        requires: Requires { desktop_only: Some(DesktopOnly { warn: true }), ..ANY },
        since: "0.1.0",
        ops: &[Op::PowerPlan(PowerPlan::HighPerformance)],
    },
    Tweak {
        id: "sysmain_off",
        name: "Desligar SysMain (SuperFetch)",
        description: "Para o pré-carregamento de programas na memória.",
        details: "Efeito pequeno. Em SSD a diferença é imperceptível; em HD mecânico o SysMain AJUDA a abrir programas, então não desligue. Pode reduzir uso de disco em PCs com pouca RAM.",
        category: Category::Desempenho,
        impact: Impact::Small,
        restart: Restart::None,
        risk: Risk::Medium,
        breaks: Some("Em HD mecânico, programas e o próprio Windows passam a abrir mais devagar."),
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Service { name: "SysMain", start: 4 }],
    },
    Tweak {
        id: "memory_compression_off",
        name: "Desativar compactação de memória",
        description: "O Windows para de compactar páginas da RAM.",
        details: "Efeito pequeno. Com 16 GB ou mais quase nada é compactado, então não muda nada; com pouca RAM pode até piorar, porque o Windows passa a usar mais o arquivo de paginação.",
        category: Category::Desempenho,
        impact: Impact::Small,
        restart: Restart::Reboot,
        risk: Risk::Medium,
        breaks: Some("Com pouca RAM o Windows usa mais o arquivo de paginação e pode ficar mais lento."),
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::MemoryCompressionOff],
    },
    Tweak {
        id: "timer_global",
        name: "Timer Resolution global",
        description: "Faz o pedido de relógio mais fino de um programa valer pro sistema todo.",
        details: "Efeito pequeno. Desde o Windows 10 2004 o timer fino vale só pro programa que pede; esta opção (Windows 11) volta ao comportamento antigo. Só faz diferença se algum programa pedir timer fino, e aumenta um pouco o consumo de energia.",
        category: Category::Desempenho,
        impact: Impact::Small,
        restart: Restart::Reboot,
        risk: Risk::Low,
        breaks: None,
        heats: true,
        requires: Requires { desktop_only: Some(DesktopOnly { warn: true }), ..ANY },
        since: "0.1.0",
        ops: &[Op::Dword {
            hive: Hklm,
            path: r"SYSTEM\CurrentControlSet\Control\Session Manager\kernel",
            name: "GlobalTimerResolutionRequests",
            value: 1,
        }],
    },
    // ---------------- Aparência ----------------
    Tweak {
        id: "transparency_off",
        name: "Desativar transparência",
        description: "Barra de tarefas e menus sem o efeito acrílico.",
        details: "Alivia um pouco a GPU integrada e PCs mais fracos. Visualmente, os menus ficam opacos.",
        category: Category::Aparencia,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword {
            hive: Hkcu,
            path: r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            name: "EnableTransparency",
            value: 0,
        }],
    },
    Tweak {
        id: "animations_off",
        name: "Desativar animações de janelas",
        description: "Minimizar, maximizar e a barra de tarefas sem animação.",
        details: "Deixa a interface com resposta instantânea. Vale após sair e entrar na conta.",
        category: Category::Aparencia,
        impact: Impact::Medium,
        restart: Restart::Logoff,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[
            Op::Str { hive: Hkcu, path: r"Control Panel\Desktop\WindowMetrics", name: "MinAnimate", value: "0" },
            Op::Dword { hive: Hkcu, path: ADVANCED, name: "TaskbarAnimations", value: 0 },
        ],
    },
    Tweak {
        id: "taskbar_left",
        name: "Barra de tarefas à esquerda",
        description: "Ícones alinhados à esquerda, como no Windows 10.",
        details: "Só Windows 11.",
        category: Category::Aparencia,
        impact: Impact::Medium,
        restart: Restart::Explorer,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "TaskbarAl", value: 0 }],
    },
    Tweak {
        id: "classic_context_menu",
        name: "Menu de contexto clássico",
        description: "Clique direito abre o menu completo, sem “Mostrar mais opções”.",
        details: "Só Windows 11. Funciona registrando uma entrada vazia que faz o Explorer usar o menu antigo; desfazer apaga a entrada.",
        category: Category::Aparencia,
        impact: Impact::Medium,
        restart: Restart::Explorer,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::KeyWithEmptyDefault { hive: Hkcu, path: CLASSIC_MENU_KEY, root: CLASSIC_MENU_ROOT }],
    },
    // ---------------- Geral ----------------
    Tweak {
        id: "show_extensions",
        name: "Mostrar extensões de arquivo",
        description: "Exibe “.exe”, “.pdf” etc. no nome dos arquivos.",
        details: "Também é segurança: fica fácil perceber um “foto.jpg.exe”.",
        category: Category::Geral,
        impact: Impact::Medium,
        restart: Restart::Explorer,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "HideFileExt", value: 0 }],
    },
    // ---------------- Privacidade ----------------
    Tweak {
        id: "suggestions_off",
        name: "Desativar sugestões e apps automáticos",
        description: "Sem apps instalados sozinhos, sugestões no Iniciar e dicas do Windows.",
        details: "Desliga o “Gerenciador de Entrega de Conteúdo”, responsável por instalar apps patrocinados e mostrar sugestões.",
        category: Category::Privacidade,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[
            Op::Dword { hive: Hkcu, path: CDM, name: "SilentInstalledAppsEnabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SystemPaneSuggestionsEnabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-338388Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-338389Enabled", value: 0 },
        ],
    },
    Tweak {
        id: "ad_id_off",
        name: "Desativar ID de publicidade",
        description: "Apps não recebem mais um identificador pra anúncios personalizados.",
        details: "Mesma opção de Configurações › Privacidade › Geral.",
        category: Category::Privacidade,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", name: "Enabled", value: 0 }],
    },
    Tweak {
        id: "telemetry_min",
        name: "Telemetria no mínimo",
        description: "Reduz os dados de diagnóstico enviados à Microsoft.",
        details: "No Windows Home/Pro o mínimo aceito é “Obrigatório”: o valor 0 só desliga de vez no Enterprise/Education. Ainda assim tira os dados opcionais.",
        category: Category::Privacidade,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Medium,
        breaks: Some("Alguns diagnósticos do Windows e recursos de compatibilidade da Store deixam de funcionar."),
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", name: "AllowTelemetry", value: 0 }],
    },
    Tweak {
        id: "bing_search_off",
        name: "Pesquisa do Iniciar sem Bing",
        description: "A busca do Iniciar procura só no PC, sem resultados da web.",
        details: "Mais rápido e sem enviar o que você digita. Vale após reiniciar o Explorer.",
        category: Category::Privacidade,
        impact: Impact::Medium,
        restart: Restart::Explorer,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Policies\Microsoft\Windows\Explorer", name: "DisableSearchBoxSuggestions", value: 1 }],
    },
    Tweak {
        id: "recall_off",
        name: "Desativar Recall (IA do Windows)",
        description: "Impede que o Windows tire capturas da tela pra busca com IA.",
        details: "O Recall só existe em PCs Copilot+. Em outros PCs este ajuste não muda nada, mas também não atrapalha.",
        category: Category::Privacidade,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Policies\Microsoft\Windows\WindowsAI", name: "DisableAIDataAnalysis", value: 1 }],
    },
    // ---------------- Rede ----------------
    Tweak {
        id: "delivery_opt_off",
        name: "Não compartilhar atualizações pela internet",
        description: "O Windows para de enviar atualizações pra outros PCs usando seu upload.",
        details: "Desliga o modo P2P da Otimização de Entrega. As atualizações continuam vindo direto da Microsoft. Ajuda quando o upload é limitado e você joga ou faz live.",
        category: Category::Rede,
        impact: Impact::Medium,
        restart: Restart::None,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.1.0",
        ops: &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", name: "DODownloadMode", value: 0 }],
    },
    // =====================================================================
    // 0.3.0: catálogo ampliado (Fase 3). Construídos com `tw(...)` + ajustes
    // encadeados; todos com `since: "0.3.0"`.
    // =====================================================================

    // ---------------- Desempenho: CPU e agendamento ----------------
    tw(
        "mmcss_games",
        "Prioridade de jogos no agendador multimídia",
        "Jogos registrados no MMCSS recebem a prioridade de CPU/GPU mais alta da categoria.",
        "Efeito pequeno. O MMCSS (agendador de mídia do Windows) só vale pra jogos que se registram nele; muitos não se registram. Não substitui nada do Modo Jogo.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[
            Op::Dword { hive: Hklm, path: MMCSS_GAMES, name: "GPU Priority", value: 8 },
            Op::Dword { hive: Hklm, path: MMCSS_GAMES, name: "Priority", value: 6 },
            Op::Str { hive: Hklm, path: MMCSS_GAMES, name: "Scheduling Category", value: "High" },
        ],
    ),
    tw(
        "system_responsiveness",
        "Menos CPU reservada para segundo plano (MMCSS)",
        "Reduz de 20% para 10% a fatia de CPU que o agendador multimídia guarda para tarefas de fundo.",
        "Efeito pequeno e só em quem usa apps multimídia registrados no MMCSS. Valores abaixo de 10 o Windows arredonda pra 10.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: MMCSS, name: "SystemResponsiveness", value: 10 }],
    ),
    tw(
        "foreground_priority",
        "Mais CPU para a janela em primeiro plano",
        "Fatias de tempo menores e com reforço para o programa em foco (Win32PrioritySeparation = 0x26).",
        "Efeito pequeno. Favorece o jogo em foco, mas tira um pouco de CPU de quem roda atrás, como o OBS: por isso fica fora do perfil Streamer.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\PriorityControl", name: "Win32PrioritySeparation", value: 0x26 }],
    )
    .risk(Risk::Medium, "Programas em segundo plano (OBS, downloads, compilação) ficam um pouco mais lentos com o jogo em foco."),
    tw(
        "power_throttling_off",
        "Sem freio de energia nos programas",
        "Desliga o Power Throttling, que reduz a CPU de programas em segundo plano para economizar energia.",
        "Útil em desktop quando algo de fundo (stream, gravação) fica lento. Em notebook gasta bateria à toa.",
        Category::Desempenho,
        Impact::Medium,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\Power\PowerThrottling", name: "PowerThrottlingOff", value: 1 }],
    )
    .req(DESKTOP)
    .heats(),
    tw(
        "cpu_epp_performance",
        "Processador prefere desempenho (EPP)",
        "No plano de energia ativo, a preferência de desempenho energético do processador vai para 0 (desempenho máximo).",
        "Em CPUs modernas (Intel Speed Shift, AMD CPPC) o próprio processador decide o clock; isto diz a ele pra sempre preferir desempenho. Só existe em PCs que expõem a opção.",
        Category::Desempenho,
        Impact::Medium,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_PROCESSOR, setting: "36687f9e-e3a5-4dbf-b1dc-15eb381c6863", ac: 0 }],
    )
    .heats(),
    tw(
        "cpu_min_100",
        "Processador nunca abaixo de 100%",
        "Estado mínimo do processador em 100% no plano ativo (na tomada).",
        "Corta a demora de subir o clock em picos curtos, ao custo de calor e consumo o tempo todo. Em CPUs com boost gerenciado pelo próprio chip o ganho é pequeno.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_PROCESSOR, setting: "893dee8e-2bef-41e0-89c6-b55d0929964c", ac: 100 }],
    )
    .req(DESKTOP)
    .heats(),
    tw(
        "cpu_idle_off",
        "Desativar estados de descanso da CPU (idle)",
        "O processador não entra nos estados C de descanso no plano ativo.",
        "Pode reduzir microtravadas em alguns sistemas, mas a CPU fica quente mesmo parada e pode PERDER boost por temperatura. Teste com e sem.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_PROCESSOR, setting: "5d76a2ca-e8c0-402f-a133-2158492d58ad", ac: 1 }],
    )
    .req(DESKTOP_ONLY)
    .heats()
    .risk(Risk::Medium, "A CPU fica bem mais quente em repouso; em coolers fracos o boost pode cair."),
    tw(
        "core_parking_off",
        "Desligar core parking",
        "Todos os núcleos ficam disponíveis no plano ativo (mínimo de núcleos não estacionados = 100%).",
        "O Windows já estaciona pouco em desktops modernos; ajuda mais em CPUs antigas ou planos de economia.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_PROCESSOR, setting: "0cc5b647-c1df-4637-891a-dec35c318583", ac: 100 }],
    )
    .req(DESKTOP)
    .heats(),
    tw(
        "pcie_aspm_off",
        "PCIe sem economia de energia (ASPM)",
        "Desliga o gerenciamento de energia do link PCI Express no plano ativo.",
        "Evita latência ao 'acordar' a placa de vídeo e o SSD NVMe. Gasta alguns watts a mais.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: "501a4d13-42af-4429-9fd1-a8218c268e20", setting: "ee12f906-d277-404b-b6da-e5fa1a576df5", ac: 0 }],
    )
    .heats(),
    tw(
        "usb_suspend_off",
        "USB sem suspensão seletiva",
        "O Windows para de desligar portas USB ociosas no plano ativo.",
        "Resolve mouse/teclado/headset que 'somem' ou travam por um instante depois de parados. Efeito nulo se isso não acontece com você.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: "2a737441-1930-4402-8d77-b2bebba308a3", setting: "48e6b7a6-50f5-4782-a5d4-53bb8f07e226", ac: 0 }],
    ),
    tw(
        "disk_never_off",
        "Disco nunca desliga",
        "No plano ativo, o disco não é desligado por inatividade (na tomada).",
        "Evita a espera de alguns segundos pra um HD voltar a girar. Em SSD a diferença é quase nula.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_DISK, setting: "6738e2c4-e8a5-4a42-b16a-e040e769756e", ac: 0 }],
    )
    .req(DESKTOP),
    tw(
        "sata_lpm_off",
        "SATA sem economia de link (AHCI LPM)",
        "Mantém o link SATA ativo no plano ativo.",
        "Reduz a latência de acesso a SSDs/HDs SATA depois de ociosos. Não afeta NVMe.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::PowerSetting { sub: SUB_DISK, setting: "0b2d69d7-a2a1-449c-9680-f91c70521c60", ac: 0 }],
    )
    .req(DESKTOP_ONLY)
    .heats(),
    tw(
        "no_sleep_ac",
        "PC não dorme na tomada",
        "No plano ativo, suspensão e hibernação automáticas ficam em 'nunca' (na tomada).",
        "Pra quem deixa download, servidor de jogo ou render rodando. Você ainda pode suspender manualmente.",
        Category::Desempenho,
        Impact::Medium,
        Restart::None,
        &[
            Op::PowerSetting { sub: SUB_SLEEP, setting: "29f6c1db-86da-48c5-9fdb-f2b67b1f44da", ac: 0 },
            Op::PowerSetting { sub: SUB_SLEEP, setting: "9d7815a6-7ee4-497e-8888-515a05f02364", ac: 0 },
        ],
    )
    .req(DESKTOP_ONLY),
    tw(
        "modern_standby_off",
        "Desligar o Modern Standby",
        "Volta ao modo de suspensão clássico (S3) em PCs que usam o Modern Standby (S0).",
        "O Modern Standby deixa o PC 'meio acordado' dormindo (rede, atualizações). Só faz diferença em PCs com S0; em muitos desktops nada muda.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: POWER, name: "PlatformAoAcOverride", value: 0 }],
    )
    .req(DESKTOP_ONLY)
    .risk(Risk::Medium, "Em alguns PCs a suspensão para de funcionar, se a placa-mãe não tiver o S3."),
    tw(
        "fast_startup_off",
        "Desligar a Inicialização Rápida",
        "O 'Desligar' passa a desligar de verdade, sem hibernar o núcleo do Windows.",
        "Resolve drivers e atualizações que só aplicam reiniciando, e dual boot. O boot fica alguns segundos mais lento.",
        Category::Desempenho,
        Impact::Medium,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\Session Manager\Power", name: "HiberbootEnabled", value: 0 }],
    ),
    tw(
        "hibernation_off",
        "Desligar a hibernação",
        "Desativa a hibernação e libera o hiberfil.sys (do tamanho de parte da sua RAM) no próximo boot.",
        "Libera vários GB no disco do sistema. Também desliga a Inicialização Rápida, que depende da hibernação.",
        Category::Desempenho,
        Impact::Medium,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: POWER, name: "HibernateEnabled", value: 0 }],
    )
    .req(DESKTOP_ONLY)
    .risk(Risk::Medium, "Sem hibernação, faltar energia com o PC suspenso perde o que estava aberto."),
    tw(
        "page_combining_off",
        "Desligar o page combining",
        "O Windows para de procurar páginas iguais na RAM para juntá-las.",
        "Efeito pequeno: economiza um pouco de CPU em segundo plano. Com 16 GB ou mais a economia de memória que ele dá é irrelevante.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::PageCombiningOff],
    )
    .req(Requires { min_ram_gb: Some(16), ..ANY }),
    tw(
        "prefetcher_off",
        "Desligar o Prefetcher",
        "Para o pré-carregamento de programas que o Windows faz no boot e ao abrir apps.",
        "Efeito pequeno e nem sempre positivo: em SSD quase não muda; em HD piora a abertura de programas (por isso só aparece com SSD).",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters", name: "EnablePrefetcher", value: 0 }],
    )
    .req(Requires { ssd: true, ..ANY })
    .risk(Risk::Medium, "Programas usados com frequência podem abrir um pouco mais devagar."),
    tw(
        "ntfs_last_access_off",
        "NTFS sem registrar último acesso",
        "O Windows para de gravar a data de último acesso de cada arquivo lido.",
        "Menos escritas no disco ao ler muitos arquivos (jogos, compilação). O valor gravado é o mesmo do `fsutil behavior set disablelastaccess 1`.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: FILESYSTEM, name: "NtfsDisableLastAccessUpdate", value: 0x8000_0001 }],
    ),
    tw(
        "ntfs_8dot3_off",
        "NTFS sem nomes curtos 8.3",
        "Arquivos novos deixam de ganhar um segundo nome no formato antigo (ARQUIV~1.TXT).",
        "Acelera pastas com milhares de arquivos. Vale só para arquivos criados depois. Mesmo valor do `fsutil 8dot3name set 1`.",
        Category::Desempenho,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: FILESYSTEM, name: "NtfsDisable8dot3NameCreation", value: 1 }],
    )
    .risk(Risk::Medium, "Instaladores e programas muito antigos que dependem de nomes 8.3 podem falhar."),
    tw(
        "memory_diag_tasks_off",
        "Desligar o diagnóstico automático de memória",
        "Desativa as tarefas agendadas que analisam a RAM em segundo plano.",
        "Efeito pequeno. Você ainda pode rodar o Diagnóstico de Memória do Windows manualmente.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\MemoryDiagnostic\ProcessMemoryDiagnosticEvents" },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\MemoryDiagnostic\RunFullMemoryDiagnostic" },
        ],
    ),
    tw(
        "startup_delay_off",
        "Sem espera para os apps de inicialização",
        "Os programas da inicialização abrem logo após o login, sem o atraso de ~10 s do Windows.",
        "A área de trabalho fica pronta antes. Em HD com muitos programas na inicialização, o login pode ficar mais 'pesado' nos primeiros segundos.",
        Category::Geral,
        Impact::Medium,
        Restart::Logoff,
        &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize", name: "StartupDelayInMSec", value: 0 }],
    ),
    tw(
        "fast_kill_hung",
        "Fechar rápido programa travado",
        "No desligar e em janelas que não respondem, o Windows espera 2 s em vez de 5–20 s.",
        "Desligar fica mais rápido e travamentos se resolvem antes.",
        Category::Geral,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Str { hive: Hkcu, path: DESKTOP_CP, name: "AutoEndTasks", value: "1" },
            Op::Str { hive: Hkcu, path: DESKTOP_CP, name: "HungAppTimeout", value: "2000" },
            Op::Str { hive: Hkcu, path: DESKTOP_CP, name: "WaitToKillAppTimeout", value: "2000" },
        ],
    )
    .risk(Risk::Medium, "Ao desligar, programas com trabalho não salvo são fechados sem perguntar."),
    tw(
        "search_index_off",
        "Desligar a indexação de arquivos",
        "Desativa o serviço Windows Search, que indexa arquivos em segundo plano.",
        "Libera disco e CPU em PCs fracos ou com HD. O Modo Jogo já pausa a indexação só durante a partida, que costuma bastar.",
        Category::Desempenho,
        Impact::Small,
        Restart::None,
        &[Op::Service { name: "WSearch", start: 4 }],
    )
    .risk(Risk::Medium, "A busca do Iniciar e do Explorador fica bem mais lenta e o Outlook pode não achar e-mails."),

    // ---------------- GPU e jogos ----------------
    tw(
        "mpo_off",
        "Desligar o MPO (Multiplane Overlay)",
        "Desativa o recurso de sobreposição de planos do Windows (OverlayTestMode = 5).",
        "Corrige tela piscando, travadas no navegador ao jogar e telas pretas em algumas GPUs NVIDIA/AMD. Se você não tem esses problemas, não precisa.",
        Category::Gaming,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows\Dwm", name: "OverlayTestMode", value: 5 }],
    )
    .risk(Risk::Medium, "Alguns overlays e o HDR podem funcionar pior; o uso de GPU no navegador pode subir."),
    tw(
        "windowed_optimizations",
        "Otimizações para jogos em janela",
        "Jogos DirectX 10/11 em janela ou sem bordas passam a usar o modo de apresentação moderno (flip).",
        "Recurso do Windows 11 (Configurações › Vídeo › Gráficos). Reduz latência em janela sem bordas. Substitui o valor global de preferências da GPU; o desfazer volta o original.",
        Category::Gaming,
        Impact::Medium,
        Restart::None,
        &[Op::Str { hive: Hkcu, path: r"Software\Microsoft\DirectX\UserGpuPreferences", name: "DirectXUserGlobalSettings", value: "SwapEffectUpgradeEnable=1;" }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "fse_global",
        "Tela cheia exclusiva em todos os jogos",
        "Desliga as 'otimizações de tela cheia' globalmente: jogos em tela cheia ficam em modo exclusivo.",
        "Pode reduzir latência em jogos antigos. Em jogos novos e no Windows 11 a tela cheia otimizada costuma ser igual ou melhor.",
        Category::Gaming,
        Impact::Small,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hkcu, path: GAMECONFIG, name: "GameDVR_FSEBehaviorMode", value: 2 },
            Op::Dword { hive: Hkcu, path: GAMECONFIG, name: "GameDVR_HonorUserFSEBehaviorMode", value: 1 },
            Op::Dword { hive: Hkcu, path: GAMECONFIG, name: "GameDVR_DXGIHonorFSEWindowsCompatible", value: 1 },
        ],
    )
    .risk(Risk::Medium, "Alt+Tab fica lento e overlays (Discord, OBS em captura de jogo) podem sumir."),
    tw(
        "wu_no_drivers",
        "Windows Update não troca drivers",
        "As atualizações de qualidade param de instalar drivers (vídeo, áudio, rede) por conta própria.",
        "Evita que o Windows Update troque o driver de vídeo que você instalou do site do fabricante.",
        Category::Gaming,
        Impact::Medium,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: WU_POLICY, name: "ExcludeWUDriversInQualityUpdate", value: 1 }],
    )
    .risk(Risk::Medium, "Drivers passam a depender de você: dispositivo novo pode ficar sem driver até instalar à mão."),
    tw(
        "driver_search_off",
        "Windows não procura driver sozinho",
        "Ao conectar um dispositivo, o Windows não baixa driver automaticamente.",
        "Combina com o ajuste anterior pra manter os drivers que você escolheu.",
        Category::Gaming,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\DriverSearching", name: "SearchOrderConfig", value: 0 }],
    )
    .risk(Risk::Medium, "Dispositivos novos podem não funcionar até você instalar o driver manualmente."),

    // ---------------- Entrada ----------------
    tw(
        "keyboard_fast_repeat",
        "Teclado com repetição rápida",
        "Menor atraso para começar a repetir uma tecla segurada e repetição mais rápida.",
        "Mesmos valores do máximo em Painel de Controle › Teclado. Bom pra quem segura teclas em jogos e no editor.",
        Category::Geral,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Str { hive: Hkcu, path: r"Control Panel\Keyboard", name: "KeyboardDelay", value: "0" },
            Op::Str { hive: Hkcu, path: r"Control Panel\Keyboard", name: "KeyboardSpeed", value: "31" },
        ],
    ),

    // ---------------- Rede (efeito pequeno, com aviso honesto) ----------------
    tw(
        "teredo_off",
        "Desligar o túnel Teredo",
        "Desativa o Teredo, que leva IPv6 por dentro do IPv4.",
        "Efeito pequeno. Com IPv6 nativo do provedor o Teredo nem é usado; desligar só tira um adaptador virtual. Cuidado se você joga pelo Xbox/Game Pass (veja o risco).",
        Category::Rede,
        Impact::Small,
        Restart::None,
        &[Op::Net { key: NetKey::Teredo, value: "Disabled" }],
    )
    .risk(Risk::Medium, "Chat de grupo e partidas do Xbox/Game Pass podem falhar em redes sem IPv6 nativo."),
    tw(
        "ipv6_tunnels_off",
        "Desligar túneis ISATAP e 6to4",
        "Desativa dois túneis IPv6 antigos que quase ninguém usa.",
        "Efeito pequeno: só tira adaptadores virtuais sem uso. Em Windows recentes eles já vêm removidos e o ajuste aparece como indisponível.",
        Category::Rede,
        Impact::Small,
        Restart::None,
        &[Op::Net { key: NetKey::Isatap, value: "Disabled" }, Op::Net { key: NetKey::SixToFour, value: "Disabled" }],
    ),
    tw(
        "ipv6_temp_off",
        "IPv6 sem endereços temporários",
        "O PC usa um endereço IPv6 fixo em vez de trocar de endereço periodicamente.",
        "Efeito pequeno em jogos. Facilita regras de firewall/port forwarding por IPv6.",
        Category::Rede,
        Impact::Small,
        Restart::None,
        &[Op::Net { key: NetKey::Ipv6Temporary, value: "Disabled" }],
    )
    .risk(Risk::Medium, "Privacidade menor: sites conseguem associar seu PC ao mesmo endereço IPv6 por mais tempo."),
    tw(
        "packet_coalescing_off",
        "Desligar o agrupamento de pacotes",
        "A placa de rede entrega cada pacote recebido na hora, sem juntar vários.",
        "Efeito pequeno: troca um pouco de CPU por menos latência no recebimento. Muitas placas nem suportam.",
        Category::Rede,
        Impact::Small,
        Restart::None,
        &[Op::Net { key: NetKey::PacketCoalescing, value: "Disabled" }],
    ),
    tw(
        "tcp_autotuning_restricted",
        "Autotuning TCP restrito (bufferbloat)",
        "Limita o quanto a janela de recepção TCP cresce no perfil Internet.",
        "Pode reduzir o 'bufferbloat' (ping que dispara quando alguém baixa algo) em conexões com fila grande no roteador. Teste com e sem: em links rápidos, downloads grandes podem ficar mais lentos.",
        Category::Rede,
        Impact::Small,
        Restart::None,
        &[Op::Net { key: NetKey::TcpAutotuning, value: "Restricted" }],
    )
    .risk(Risk::Medium, "Downloads grandes podem ficar mais lentos em conexões rápidas."),
    tw(
        "prefer_ipv4",
        "Preferir IPv4 ao IPv6",
        "Quando um site tem os dois, o Windows usa IPv4 primeiro (sem desligar o IPv6).",
        "Útil quando o IPv6 do provedor ou do roteador está instável (como DNS IPv6 que não responde). Com IPv6 saudável não muda nada.",
        Category::Rede,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters", name: "DisabledComponents", value: 0x20 }],
    ),
    tw(
        "icmp_redirect_off",
        "Ignorar redirecionamentos ICMP",
        "O PC ignora mensagens ICMP que tentam mudar a rota dos pacotes.",
        "É um ajuste de SEGURANÇA, não de desempenho: impede que alguém na rede local desvie seu tráfego. Sem efeito em FPS ou ping.",
        Category::Rede,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters", name: "EnableICMPRedirect", value: 0 }],
    ),

    // ---------------- Privacidade ----------------
    tw(
        "copilot_off",
        "Desligar o Copilot do Windows",
        "Política que desliga o Copilot da barra de tarefas e do sistema.",
        "Não afeta o Copilot em sites ou no Office.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[Op::Dword { hive: Hkcu, path: r"Software\Policies\Microsoft\Windows\WindowsCopilot", name: "TurnOffWindowsCopilot", value: 1 }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "widgets_off",
        "Desligar os Widgets",
        "Política que remove o painel de Widgets (notícias e clima) da barra.",
        "Também para o processo do painel em segundo plano.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Dsh", name: "AllowNewsAndInterests", value: 0 }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "search_cloud_off",
        "Busca do Windows só local",
        "Política: sem busca na web, sem Cortana, sem busca na nuvem e sem 'destaques da pesquisa'.",
        "Complementa o 'Pesquisa do Iniciar sem Bing'. A busca de arquivos e apps no PC continua igual.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hklm, path: WIN_SEARCH_POLICY, name: "DisableWebSearch", value: 1 },
            Op::Dword { hive: Hklm, path: WIN_SEARCH_POLICY, name: "AllowCortana", value: 0 },
            Op::Dword { hive: Hklm, path: WIN_SEARCH_POLICY, name: "AllowCloudSearch", value: 0 },
            Op::Dword { hive: Hklm, path: WIN_SEARCH_POLICY, name: "EnableDynamicContentInWSB", value: 0 },
        ],
    ),
    tw(
        "compat_telemetry_off",
        "Telemetria de compatibilidade desligada",
        "Para o inventário de programas e o rastreamento de compatibilidade (AppCompat) e as tarefas que enviam esses dados.",
        "Essas tarefas rodam o CompatTelRunner, conhecido por pesar no disco em PCs fracos.",
        Category::Privacidade,
        Impact::Medium,
        Restart::None,
        &[
            Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\AppCompat", name: "AITEnable", value: 0 },
            Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\AppCompat", name: "DisableInventory", value: 1 },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser" },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\Application Experience\ProgramDataUpdater" },
        ],
    )
    .risk(Risk::Medium, "O assistente de compatibilidade não avisa mais de programas incompatíveis antes de grandes atualizações."),
    tw(
        "diagtrack_off",
        "Desligar o serviço de telemetria (DiagTrack)",
        "Desativa o serviço 'Experiências do Usuário Conectado e Telemetria'.",
        "É o serviço que envia os dados de diagnóstico. Combina com 'Telemetria no mínimo'.",
        Category::Privacidade,
        Impact::Medium,
        Restart::None,
        &[Op::Service { name: "DiagTrack", start: 4 }],
    )
    .risk(Risk::Medium, "Alguns recursos do Windows Insider e o envio de diagnóstico para suporte deixam de funcionar."),
    tw(
        "ceip_off",
        "Desligar o Programa de Aperfeiçoamento (CEIP)",
        "Política e tarefas do Programa de Aperfeiçoamento da Experiência do Usuário.",
        "Efeito pequeno no desempenho; ganho de privacidade.",
        Category::Privacidade,
        Impact::Small,
        Restart::None,
        &[
            Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\SQMClient\Windows", name: "CEIPEnable", value: 0 },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\Customer Experience Improvement Program\Consolidator" },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip" },
        ],
    ),
    tw(
        "consumer_content_off",
        "Sem conteúdo promocional da Microsoft",
        "Política que impede apps sugeridos e conteúdo 'otimizado pela nuvem' no Iniciar.",
        "Complementa 'Desativar sugestões e apps automáticos' com a política do sistema.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hklm, path: CLOUD_CONTENT, name: "DisableWindowsConsumerFeatures", value: 1 },
            Op::Dword { hive: Hklm, path: CLOUD_CONTENT, name: "DisableCloudOptimizedContent", value: 1 },
        ],
    ),
    tw(
        "tips_ads_off",
        "Sem dicas e anúncios no Windows",
        "Desliga dicas, 'boas-vindas', sugestões nas Configurações e textos na tela de bloqueio.",
        "Complementa 'Desativar sugestões' com as chaves que ele ainda não cobria.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-310093Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-338393Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-353694Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SubscribedContent-353696Enabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "SoftLandingEnabled", value: 0 },
            Op::Dword { hive: Hkcu, path: CDM, name: "RotatingLockScreenOverlayEnabled", value: 0 },
        ],
    ),
    tw(
        "activity_history_off",
        "Desligar o histórico de atividades",
        "O Windows para de registrar e enviar quais apps e arquivos você abre (linha do tempo).",
        "Um ajuste só para as três políticas (feed, publicação e envio).",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hklm, path: SYSTEM_POLICY, name: "EnableActivityFeed", value: 0 },
            Op::Dword { hive: Hklm, path: SYSTEM_POLICY, name: "PublishUserActivities", value: 0 },
            Op::Dword { hive: Hklm, path: SYSTEM_POLICY, name: "UploadUserActivities", value: 0 },
        ],
    ),
    tw(
        "error_reporting_off",
        "Desligar o relatório de erros",
        "O Windows para de enviar relatórios de travamento para a Microsoft.",
        "Os registros locais (Visualizador de Eventos) continuam.",
        Category::Privacidade,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows\Windows Error Reporting", name: "Disabled", value: 1 }],
    )
    .risk(Risk::Medium, "Soluções automáticas para travamentos conhecidos deixam de ser oferecidas."),
    tw(
        "feedback_off",
        "Sem pedidos de feedback",
        "O Windows para de perguntar sua opinião em notificações.",
        "Só tira os pop-ups do Hub de Feedback.",
        Category::Privacidade,
        Impact::Medium,
        Restart::None,
        &[
            Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Siuf\Rules", name: "NumberOfSIUFInPeriod", value: 0 },
            Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", name: "DoNotShowFeedbackNotifications", value: 1 },
        ],
    ),
    tw(
        "inking_typing_off",
        "Não coletar escrita, tinta e voz",
        "Para a personalização de escrita e digitação baseada no que você digita e escreve.",
        "O teclado virtual e o reconhecimento de escrita à mão ficam menos personalizados.",
        Category::Privacidade,
        Impact::Small,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hkcu, path: r"Software\Microsoft\InputPersonalization", name: "RestrictImplicitInkCollection", value: 1 },
            Op::Dword { hive: Hkcu, path: r"Software\Microsoft\InputPersonalization", name: "RestrictImplicitTextCollection", value: 1 },
            Op::Dword { hive: Hkcu, path: r"Software\Microsoft\InputPersonalization\TrainedDataStore", name: "HarvestContacts", value: 0 },
        ],
    ),
    tw(
        "experiments_off",
        "Desligar experimentos da Microsoft",
        "Impede que a Microsoft ative recursos experimentais no seu Windows remotamente.",
        "Recursos novos chegam só nas atualizações normais.",
        Category::Privacidade,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\PolicyManager\current\device\System", name: "AllowExperimentation", value: 0 }],
    ),
    tw(
        "remote_assistance_off",
        "Desligar a Assistência Remota",
        "Ninguém consegue pedir para ver/controlar seu PC pela Assistência Remota do Windows.",
        "Ajuste de segurança. Não afeta AnyDesk, TeamViewer nem a Área de Trabalho Remota.",
        Category::Privacidade,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\Remote Assistance", name: "fAllowToGetHelp", value: 0 }],
    ),
    tw(
        "energy_estimation_off",
        "Sem estimativa de energia e sleep study",
        "Para o cálculo de consumo por app e o relatório de suspensão que roda em segundo plano.",
        "Efeito pequeno. A tela de uso de bateria por app deixa de ter dados.",
        Category::Privacidade,
        Impact::Small,
        Restart::Reboot,
        &[
            Op::Dword { hive: Hklm, path: POWER, name: "EnergyEstimationEnabled", value: 0 },
            Op::ScheduledTaskOff { path: r"\Microsoft\Windows\Power Efficiency Diagnostics\AnalyzeSystem" },
        ],
    ),
    tw(
        "location_off",
        "Desligar a localização",
        "Nenhum app pode usar a localização do PC.",
        "Ganho de privacidade.",
        Category::Privacidade,
        Impact::Medium,
        Restart::None,
        &[Op::Str { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location", name: "Value", value: "Deny" }],
    )
    .risk(Risk::Medium, "Mapas, clima local e o ajuste automático de fuso horário param de funcionar."),
    tw(
        "background_apps_off",
        "Apps da Store sem segundo plano",
        "Apps da Microsoft Store não rodam quando estão fechados.",
        "Economiza RAM e CPU em PCs com muitos apps da Store.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications", name: "GlobalUserDisabled", value: 1 }],
    )
    .risk(Risk::Medium, "Apps da Store (WhatsApp, Spotify da Store, e-mail) não notificam nem sincronizam fechados."),
    tw(
        "notifications_off",
        "Desligar notificações",
        "Nenhuma notificação de app aparece na tela.",
        "Útil em PC só de jogo. A Central de Notificações continua acessível.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\PushNotifications", name: "ToastEnabled", value: 0 }],
    )
    .risk(Risk::Medium, "Você para de ver avisos de mensagens, lembretes e alertas de apps."),
    tw(
        "edge_preload_off",
        "Edge sem pré-carregamento",
        "Política: o Edge não abre escondido no login nem fica rodando depois de fechado.",
        "Economiza RAM e alguns segundos de boot pra quem não usa o Edge.",
        Category::Privacidade,
        Impact::Medium,
        Restart::Logoff,
        &[
            Op::Dword { hive: Hklm, path: EDGE_POLICY, name: "StartupBoostEnabled", value: 0 },
            Op::Dword { hive: Hklm, path: EDGE_POLICY, name: "BackgroundModeEnabled", value: 0 },
        ],
    ),
    tw(
        "wu_manual",
        "Windows Update só quando você mandar",
        "O Windows avisa que há atualizações, mas só baixa e instala quando você pedir.",
        "Evita reinícios e downloads no meio da partida. Exige disciplina: atualize pelo menos uma vez por mês.",
        Category::Privacidade,
        Impact::Medium,
        Restart::None,
        &[
            Op::Dword { hive: Hklm, path: WU_AU_POLICY, name: "NoAutoUpdate", value: 0 },
            Op::Dword { hive: Hklm, path: WU_AU_POLICY, name: "AUOptions", value: 2 },
        ],
    )
    .risk(Risk::High, "Atualizações de SEGURANÇA atrasam até você instalar: o PC fica exposto a falhas já corrigidas."),

    // ---------------- Aparência e qualidade de vida ----------------
    tw(
        "start_recommendations_off",
        "Iniciar sem 'Recomendados'",
        "Tira os arquivos e apps recomendados da parte de baixo do menu Iniciar.",
        "Só visual.",
        Category::Aparencia,
        Impact::Medium,
        Restart::Explorer,
        &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "Start_IrisRecommendations", value: 0 }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "taskbar_chat_off",
        "Tirar o Chat/Teams da barra",
        "Remove o ícone do Chat (Teams pessoal) da barra de tarefas.",
        "Só visual.",
        Category::Aparencia,
        Impact::Medium,
        Restart::Explorer,
        &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "TaskbarMn", value: 0 }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "snap_flyout_off",
        "Sem painel de layouts no botão maximizar",
        "Para de abrir o painel de layouts (Snap) ao parar o mouse sobre o botão maximizar.",
        "O Snap continua funcionando pelo Win+Z e arrastando janelas.",
        Category::Aparencia,
        Impact::Medium,
        Restart::Explorer,
        &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "EnableSnapAssistFlyout", value: 0 }],
    )
    .req(Requires { min_build: Some(22000), ..ANY }),
    tw(
        "suggested_actions_off",
        "Desligar ações sugeridas",
        "Para de sugerir ações (ligar, criar evento) ao copiar números e datas.",
        "Só tira o pop-up.",
        Category::Aparencia,
        Impact::Medium,
        Restart::None,
        &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\SmartActionPlatform\SmartClipboard", name: "Disabled", value: 1 }],
    ),
    tw(
        "explorer_this_pc",
        "Explorador abre em 'Este Computador'",
        "O Explorador de Arquivos abre mostrando os discos, em vez do 'Início'.",
        "Só muda a pasta inicial.",
        Category::Aparencia,
        Impact::Medium,
        Restart::None,
        &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "LaunchTo", value: 1 }],
    ),
    tw(
        "show_hidden_files",
        "Mostrar arquivos ocultos",
        "O Explorador passa a mostrar arquivos e pastas ocultos (como AppData).",
        "Os arquivos protegidos do sistema continuam escondidos.",
        Category::Aparencia,
        Impact::Medium,
        Restart::None,
        &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "Hidden", value: 1 }],
    ),
    tw(
        "wallpaper_full_quality",
        "Papel de parede sem compressão",
        "O Windows usa o papel de parede na qualidade original (100%) em vez de recomprimir a 85%.",
        "Só visual. Vale a partir do próximo papel de parede aplicado.",
        Category::Aparencia,
        Impact::Medium,
        Restart::None,
        &[Op::Dword { hive: Hkcu, path: DESKTOP_CP, name: "JPEGImportQuality", value: 100 }],
    ),
    tw(
        "first_logon_animation_off",
        "Sem animação no primeiro login",
        "Tira a tela 'Olá / Estamos preparando tudo' ao criar usuário ou depois de grandes atualizações.",
        "Só pula a animação.",
        Category::Aparencia,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon", name: "EnableFirstLogonAnimation", value: 0 }],
    ),
    tw(
        "bsod_details",
        "Tela azul com detalhes",
        "A tela azul mostra os parâmetros técnicos do erro, além do código.",
        "Ajuda no diagnóstico (e no suporte) quando o PC trava.",
        Category::Geral,
        Impact::Small,
        Restart::Reboot,
        &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\CrashControl", name: "DisplayParameters", value: 1 }],
    ),
    tw(
        "thumbnail_cache_keep",
        "Não apagar o cache de miniaturas sozinho",
        "A limpeza automática do Windows deixa de apagar as miniaturas de imagens e vídeos.",
        "Pastas com muitas fotos abrem rápido sem recriar as miniaturas toda vez.",
        Category::Geral,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\Thumbnail Cache", name: "Autorun", value: 0 }],
    ),
    tw(
        "storage_sense_off",
        "Desligar o Sensor de Armazenamento",
        "O Windows para de apagar arquivos temporários e da Lixeira automaticamente.",
        "A limpeza fica sob seu controle (pela tela Limpeza do app).",
        Category::Geral,
        Impact::Small,
        Restart::None,
        &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy", name: "01", value: 0 }],
    ),

    // ---------------- Apps de terceiros ----------------
    tw(
        "discord_no_autostart",
        "Discord não abre com o Windows",
        "Muda a opção 'Abrir o Discord' nas configurações do próprio Discord.",
        "Feche o Discord antes: ele regrava as configurações ao sair. Sem efeito se o Discord nunca foi aberto neste usuário.",
        Category::Geral,
        Impact::Medium,
        Restart::None,
        &[Op::JsonFileKey { env: "APPDATA", file: DISCORD, key: "OPEN_ON_STARTUP", value: "false" }],
    ),
    tw(
        "discord_start_minimized",
        "Discord inicia minimizado na bandeja",
        "Quando abrir com o Windows, o Discord fica na bandeja em vez de abrir a janela.",
        "Feche o Discord antes de aplicar.",
        Category::Geral,
        Impact::Medium,
        Restart::None,
        &[Op::JsonFileKey { env: "APPDATA", file: DISCORD, key: "START_MINIMIZED", value: "true" }],
    ),
    tw(
        "discord_skip_update",
        "Discord sem checar atualização ao abrir",
        "Pula a tela 'Verificando atualizações' na abertura do Discord.",
        "Abre mais rápido. Dica: a aceleração de hardware do Discord (Configurações › Avançado) pesa na GPU durante o jogo; vale testar desligada.",
        Category::Geral,
        Impact::Small,
        Restart::None,
        &[Op::JsonFileKey { env: "APPDATA", file: DISCORD, key: "SKIP_HOST_UPDATE", value: "true" }],
    )
    .risk(Risk::Medium, "O Discord pode ficar desatualizado até você reinstalar ou desligar este ajuste."),
];


// ---------------- Construtor dos ajustes novos ----------------

/// Ajuste com os padrões mais comuns (risco baixo, sem requisitos, entrou na 0.3.0).
/// Os ajustes antigos continuam escritos por extenso.
#[allow(clippy::too_many_arguments)]
const fn tw(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    details: &'static str,
    category: Category,
    impact: Impact,
    restart: Restart,
    ops: &'static [Op],
) -> Tweak {
    Tweak {
        id,
        name,
        description,
        details,
        category,
        impact,
        risk: Risk::Low,
        breaks: None,
        heats: false,
        requires: ANY,
        since: "0.3.0",
        restart,
        ops,
    }
}

impl Tweak {
    const fn risk(mut self, risk: Risk, breaks: &'static str) -> Self {
        self.risk = risk;
        self.breaks = Some(breaks);
        self
    }
    const fn heats(mut self) -> Self {
        self.heats = true;
        self
    }
    const fn req(mut self, requires: Requires) -> Self {
        self.requires = requires;
        self
    }
}

/// Desktop, com aviso em notebook (dá pra aplicar à mão).
const DESKTOP: Requires = Requires { desktop_only: Some(DesktopOnly { warn: true }), ..ANY };
/// Só desktop: em notebook fica indisponível.
const DESKTOP_ONLY: Requires = Requires { desktop_only: Some(DesktopOnly { warn: false }), ..ANY };

const MMCSS: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
const MMCSS_GAMES: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games";
const POWER: &str = r"SYSTEM\CurrentControlSet\Control\Power";
const FILESYSTEM: &str = r"SYSTEM\CurrentControlSet\Control\FileSystem";
const DESKTOP_CP: &str = r"Control Panel\Desktop";
const GAMECONFIG: &str = r"System\GameConfigStore";
const WU_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
const WU_AU_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU";
const WIN_SEARCH_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\Windows Search";
const CLOUD_CONTENT: &str = r"SOFTWARE\Policies\Microsoft\Windows\CloudContent";
const SYSTEM_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\System";
const EDGE_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Edge";
const DISCORD: &str = r"discord\settings.json";
/// Subgrupos do powercfg (GUIDs fixos do Windows).
const SUB_PROCESSOR: &str = "54533251-82be-4824-96c1-47b60b740d00";
const SUB_DISK: &str = "0012ee47-9041-4b5d-9b77-535fba8b1442";
const SUB_SLEEP: &str = "238c9fa8-0aad-41ed-83f4-97be242c8f20";

pub fn find(id: &str) -> Option<&'static Tweak> {
    TWEAKS.iter().find(|t| t.id == id)
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub tweaks: &'static [&'static str],
}

pub static PROFILES: &[BuiltinProfile] = &[
    BuiltinProfile {
        id: "competitivo",
        name: "Competitivo",
        description: "Menor latência possível no desktop: nada gravando em segundo plano, mouse 1:1, jogo em janela otimizado.",
        tweaks: &[
            "game_mode", "game_dvr_off", "hags", "mouse_accel_off", "sticky_keys_off", "transparency_off",
            "suggestions_off", "delivery_opt_off", "windowed_optimizations", "startup_delay_off", "edge_preload_off",
            "widgets_off",
        ],
    },
    BuiltinProfile {
        id: "equilibrado",
        name: "Equilibrado",
        description: "Só ajustes de risco baixo, sem nada que esquente e sem mudar o visual. Bons pra qualquer PC.",
        tweaks: &[
            "game_mode", "game_dvr_off", "mouse_accel_off", "sticky_keys_off", "suggestions_off", "show_extensions",
            "startup_delay_off", "edge_preload_off", "consumer_content_off", "tips_ads_off",
        ],
    },
    BuiltinProfile {
        id: "notebook",
        name: "Notebook",
        description: "Sem nada que esquente ou gaste bateria.",
        tweaks: &[
            "game_mode", "game_dvr_off", "mouse_accel_off", "sticky_keys_off", "suggestions_off", "delivery_opt_off",
            "startup_delay_off", "edge_preload_off", "widgets_off",
        ],
    },
    BuiltinProfile {
        id: "pc_modesto",
        name: "PC Modesto",
        description: "Pouca RAM, GPU integrada ou HD: tira o peso visual e o que roda escondido.",
        tweaks: &[
            "game_mode", "game_dvr_off", "mouse_accel_off", "transparency_off", "animations_off", "suggestions_off",
            "delivery_opt_off", "startup_delay_off", "edge_preload_off", "widgets_off", "compat_telemetry_off",
            "consumer_content_off",
        ],
    },
    BuiltinProfile {
        id: "streamer",
        name: "Streamer",
        description: "Jogo e transmissão juntos: sem tirar CPU do OBS e sem quebrar o overlay.",
        tweaks: &["game_mode", "hags", "mouse_accel_off", "sticky_keys_off", "suggestions_off", "windowed_optimizations", "startup_delay_off"],
    },
    BuiltinProfile {
        id: "privacidade",
        name: "Privacidade",
        description: "Menos dados enviados, sem anúncios, sem IA nem busca na nuvem, sem histórico de atividades.",
        tweaks: &[
            "suggestions_off", "ad_id_off", "telemetry_min", "bing_search_off", "recall_off", "copilot_off", "widgets_off",
            "search_cloud_off", "compat_telemetry_off", "diagtrack_off", "consumer_content_off", "tips_ads_off",
            "activity_history_off", "feedback_off", "edge_preload_off",
        ],
    },
];

pub fn find_profile(id: &str) -> Option<&'static BuiltinProfile> {
    PROFILES.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_unicos() {
        let mut seen = HashSet::new();
        for t in TWEAKS {
            assert!(seen.insert(t.id), "id repetido: {}", t.id);
            assert!(!t.ops.is_empty(), "{} sem operações", t.id);
        }
    }

    #[test]
    fn perfis_so_usam_ajustes_existentes_e_de_efeito_real() {
        for p in PROFILES {
            for id in p.tweaks {
                let t = find(id).unwrap_or_else(|| panic!("perfil {} usa ajuste inexistente {id}", p.id));
                assert_ne!(t.impact, Impact::Small, "perfil {} inclui ajuste de efeito pequeno {id}", p.id);
                assert!(!t.heats, "perfil {} inclui ajuste que esquenta {id}", p.id);
                assert_ne!(t.risk, Risk::High, "perfil {} inclui ajuste de risco alto {id}", p.id);
            }
        }
    }

    #[test]
    fn perfil_notebook_nao_tem_nada_que_esquente() {
        let p = find_profile("notebook").unwrap();
        assert!(p.tweaks.iter().all(|id| find(id).unwrap().requires.desktop_only.is_none()));
    }

    #[test]
    fn regras_de_cada_perfil() {
        let tweaks = |id: &str| find_profile(id).unwrap().tweaks.iter().map(|t| find(t).unwrap()).collect::<Vec<_>>();
        assert!(tweaks("equilibrado").iter().all(|t| t.risk == Risk::Low), "Equilibrado: só risco baixo");
        assert!(tweaks("equilibrado").iter().all(|t| t.category != Category::Aparencia), "Equilibrado: nada visual");
        let streamer: Vec<_> = tweaks("streamer").iter().map(|t| t.id).collect();
        assert!(!streamer.contains(&"foreground_priority"), "tira CPU do OBS");
        assert!(!streamer.contains(&"fse_global"), "quebra o overlay");
        assert!(tweaks("notebook").iter().all(|t| !t.heats && t.requires.desktop_only.is_none()));
    }

    #[test]
    fn ids_e_nomes_unicos() {
        let mut names = HashSet::new();
        for t in TWEAKS {
            assert!(names.insert(t.name), "nome repetido: {}", t.name);
            assert!(t.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "id fora do padrão: {}", t.id);
        }
    }

    #[test]
    fn guids_e_tarefas_validos() {
        for t in TWEAKS {
            for op in t.ops {
                match *op {
                    Op::PowerSetting { sub, setting, .. } => {
                        assert!(super::super::ops::is_guid_plain(sub) && super::super::ops::is_guid_plain(setting), "{}", t.id)
                    }
                    Op::ScheduledTaskOff { path } => assert!(super::super::ops::is_task_path(path), "{}: {path}", t.id),
                    Op::Net { key, value } => assert!(key.canonical(value).is_some(), "{}", t.id),
                    Op::JsonFileKey { value, .. } => assert!(serde_json::from_str::<serde_json::Value>(value).is_ok(), "{}", t.id),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn risco_medio_ou_alto_diz_o_que_quebra() {
        for t in TWEAKS {
            if t.risk >= Risk::Medium {
                assert!(t.breaks.is_some_and(|b| !b.trim().is_empty()), "{} tem risco {:?} sem `breaks`", t.id, t.risk);
            }
        }
    }

    /// Onde cada operação escreve. Dois ajustes no mesmo destino quebram o desfazer:
    /// reverter um restaura a foto e desliga o outro sem avisar. Por isso a regra é
    /// "nenhum destino repetido", mesmo com o MESMO valor.
    fn target(op: &Op) -> String {
        match *op {
            Op::Dword { hive, path, name, .. } | Op::Str { hive, path, name, .. } => {
                format!("reg:{hive:?}:{}:{}", path.to_ascii_lowercase(), name.to_ascii_lowercase())
            }
            Op::KeyWithEmptyDefault { hive, path, .. } => format!("key:{hive:?}:{}", path.to_ascii_lowercase()),
            Op::Service { name, .. } => format!("service:{}", name.to_ascii_lowercase()),
            Op::PowerPlan(_) => "power:active_plan".into(),
            Op::MouseAccelerationOff => "spi:mouse_accel".into(),
            Op::MouseSpeed(_) => "spi:mouse_speed".into(),
            Op::MemoryCompressionOff => "mmagent:compression".into(),
            Op::PageCombiningOff => "mmagent:pagecombining".into(),
            Op::PowerSetting { sub, setting, .. } => format!("power:{}:{}", sub.to_ascii_lowercase(), setting.to_ascii_lowercase()),
            Op::ScheduledTaskOff { path } => format!("task:{}", path.to_ascii_lowercase()),
            Op::Net { key, .. } => format!("net:{key:?}"),
            Op::JsonFileKey { env, file, key, .. } => format!("json:{env}:{}:{key}", file.to_ascii_lowercase()),
        }
    }

    #[test]
    fn nenhum_destino_escrito_por_dois_ajustes() {
        let mut owner = std::collections::HashMap::new();
        for t in TWEAKS {
            for op in t.ops {
                if let Some(other) = owner.insert(target(op), t.id) {
                    assert_eq!(other, t.id, "{} e {} escrevem em {}", other, t.id, target(op));
                }
            }
        }
    }

    fn pc(build: u32, ram: u32, disk: DiskKind, laptop: bool) -> SystemProfile {
        SystemProfile { windows: String::new(), build, is_laptop: laptop, ram_gb: ram, system_disk: disk }
    }

    const fn sample(risk: Risk, heats: bool, requires: Requires) -> Tweak {
        Tweak {
            id: "x",
            name: "Teste",
            description: "",
            details: "",
            category: Category::Geral,
            impact: Impact::Medium,
            risk,
            breaks: Some("algo"),
            heats,
            requires,
            since: "0.1.0",
            restart: Restart::None,
            ops: &[],
        }
    }

    #[test]
    fn gate_por_requisito() {
        let desktop = pc(26100, 32, DiskKind::Ssd, false);
        let win11 = sample(Risk::Low, false, Requires { min_build: Some(22000), ..ANY });
        assert_eq!(gate(&win11, &desktop), Gate::Ok);
        assert!(matches!(gate(&win11, &pc(19045, 32, DiskKind::Ssd, false)), Gate::Unavailable(r) if r.contains("Windows 11")));
        assert!(matches!(gate(&win11, &pc(0, 32, DiskKind::Ssd, false)), Gate::Warn(_)), "não leu: avisa, não bloqueia");

        let ram16 = sample(Risk::Low, false, Requires { min_ram_gb: Some(16), ..ANY });
        assert!(matches!(gate(&ram16, &pc(26100, 8, DiskKind::Ssd, false)), Gate::Unavailable(r) if r.contains("tem 8")));

        let ssd = sample(Risk::Low, false, Requires { ssd: true, ..ANY });
        assert!(matches!(gate(&ssd, &pc(26100, 32, DiskKind::Hdd, false)), Gate::Unavailable(_)));
        assert!(matches!(gate(&ssd, &pc(26100, 32, DiskKind::Unknown, false)), Gate::Warn(_)));

        let warn = sample(Risk::Low, false, Requires { desktop_only: Some(DesktopOnly { warn: true }), ..ANY });
        let block = sample(Risk::Low, false, Requires { desktop_only: Some(DesktopOnly { warn: false }), ..ANY });
        let laptop = pc(26100, 16, DiskKind::Ssd, true);
        assert!(matches!(gate(&warn, &laptop), Gate::Warn(_)));
        assert!(matches!(gate(&block, &laptop), Gate::Unavailable(_)));
        assert_eq!(gate(&block, &desktop), Gate::Ok);
    }

    #[test]
    fn risco_alto_exige_confirmacao_no_backend() {
        let desktop = pc(26100, 32, DiskKind::Ssd, false);
        let high = sample(Risk::High, false, ANY);
        assert!(matches!(check_apply(&high, &desktop, false), Err(crate::error::AppError::ConfirmationRequired(_))));
        assert!(check_apply(&high, &desktop, true).is_ok());
        let only_desktop = sample(Risk::Low, false, Requires { desktop_only: Some(DesktopOnly { warn: false }), ..ANY });
        let laptop = pc(26100, 16, DiskKind::Ssd, true);
        assert!(
            matches!(check_apply(&only_desktop, &laptop, true), Err(crate::error::AppError::Unavailable(_))),
            "confirmar não fura o requisito"
        );
    }

    #[test]
    fn perfil_pula_com_motivo() {
        let desktop = pc(26100, 32, DiskKind::Ssd, false);
        assert!(profile_skip_reason(&sample(Risk::Low, true, ANY), &desktop).unwrap().contains("calor"));
        assert!(profile_skip_reason(&sample(Risk::High, false, ANY), &desktop).unwrap().contains("risco alto"));
        assert_eq!(profile_skip_reason(&sample(Risk::Medium, false, ANY), &desktop), None);
        let warn = sample(Risk::Low, false, Requires { desktop_only: Some(DesktopOnly { warn: true }), ..ANY });
        assert!(profile_skip_reason(&warn, &pc(26100, 16, DiskKind::Ssd, true)).is_some(), "com aviso: perfil pula");
    }

    #[test]
    fn admin_detectado_pelas_operacoes() {
        assert!(find("hags").unwrap().requires_admin());
        assert!(find("sysmain_off").unwrap().requires_admin());
        assert!(!find("game_mode").unwrap().requires_admin());
        assert!(!find("power_high").unwrap().requires_admin());
    }
}

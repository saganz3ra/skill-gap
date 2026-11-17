//! Catálogo de ajustes e perfis prontos. Fonte única dos textos: o front só exibe.
//!
//! Regras do catálogo:
//! - Só entra ajuste com efeito verificável. Placebo conhecido (limpar RAM,
//!   NetworkThrottlingIndex) fica de fora de propósito.
//! - Ajuste de efeito pequeno ou dependente do PC leva `Impact::Small` e uma explicação
//!   honesta em `details`, e nunca entra nos perfis prontos.

use serde::Serialize;

use super::ops::{Hive::*, Op};
use crate::power::PowerPlan;

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
    Real,
    /// Efeito pequeno ou que depende do hardware: aparece com aviso, fora dos perfis.
    Small,
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
    pub restart: Restart,
    /// Em notebook só esquenta e gasta bateria: perfis pulam em notebook.
    pub laptop_unsafe: bool,
    pub ops: &'static [Op],
}

impl Tweak {
    /// Exige admin se mexe em HKLM, serviço ou compactação de memória.
    pub fn requires_admin(&self) -> bool {
        self.ops.iter().any(|op| match op {
            Op::Dword { hive, .. } | Op::Str { hive, .. } | Op::KeyWithEmptyDefault { hive, .. } => *hive == Hklm,
            Op::Service { .. } | Op::MemoryCompressionOff => true,
            Op::PowerPlan(_) | Op::MouseAccelerationOff | Op::MouseSpeed(_) => false,
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
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\GameBar", name: "AutoGameModeEnabled", value: 1 }],
    },
    Tweak {
        id: "game_dvr_off",
        name: "Desativar gravação da Game Bar",
        description: "Para a gravação de clipes da Xbox Game Bar, inclusive em segundo plano.",
        details: "A gravação em segundo plano usa o codificador de vídeo da GPU o tempo todo. Se você grava clipes pela Game Bar, deixe desligado este ajuste.",
        category: Category::Gaming,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
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
        impact: Impact::Real,
        restart: Restart::Reboot,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hklm, path: r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", name: "HwSchMode", value: 2 }],
    },
    Tweak {
        id: "mouse_accel_off",
        name: "Desativar aceleração do mouse",
        description: "Movimento 1:1: a mesma distância física sempre move o cursor o mesmo tanto.",
        details: "Desliga o “Aumentar precisão do ponteiro”. Essencial pra memória muscular de mira; a maioria dos jogos competitivos já usa raw input, mas menus e jogos sem raw input passam a se comportar igual.",
        category: Category::Gaming,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::MouseAccelerationOff],
    },
    Tweak {
        id: "mouse_speed_default",
        name: "Velocidade do ponteiro no padrão (10)",
        description: "Cada contagem do sensor move exatamente 1 pixel.",
        details: "É a 6ª marca (de 11) do controle de velocidade do Windows. Acima disso o Windows pula pixels; abaixo, descarta movimento. Ajuste a sensibilidade pelo DPI do mouse e pelo jogo, não por aqui.",
        category: Category::Gaming,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::MouseSpeed(10)],
    },
    Tweak {
        id: "sticky_keys_off",
        name: "Desativar atalho das Teclas de Aderência",
        description: "Apertar Shift 5 vezes não abre mais a janela que tira você do jogo.",
        details: "Só desliga o atalho de teclado; o recurso de acessibilidade continua disponível nas Configurações. Vale após sair e entrar na conta.",
        category: Category::Gaming,
        impact: Impact::Real,
        restart: Restart::Logoff,
        laptop_unsafe: false,
        ops: &[Op::Str { hive: Hkcu, path: r"Control Panel\Accessibility\StickyKeys", name: "Flags", value: "506" }],
    },
    // ---------------- Desempenho ----------------
    Tweak {
        id: "power_high",
        name: "Plano de energia Alto desempenho",
        description: "A CPU não entra nos estados mais profundos de economia.",
        details: "Responde mais rápido a picos de carga, com mais consumo e calor. Em notebook os perfis pulam este ajuste.",
        category: Category::Desempenho,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: true,
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
        laptop_unsafe: false,
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
        laptop_unsafe: false,
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
        laptop_unsafe: true,
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
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
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
        impact: Impact::Real,
        restart: Restart::Logoff,
        laptop_unsafe: false,
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
        impact: Impact::Real,
        restart: Restart::Explorer,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "TaskbarAl", value: 0 }],
    },
    Tweak {
        id: "classic_context_menu",
        name: "Menu de contexto clássico",
        description: "Clique direito abre o menu completo, sem “Mostrar mais opções”.",
        details: "Só Windows 11. Funciona registrando uma entrada vazia que faz o Explorer usar o menu antigo; desfazer apaga a entrada.",
        category: Category::Aparencia,
        impact: Impact::Real,
        restart: Restart::Explorer,
        laptop_unsafe: false,
        ops: &[Op::KeyWithEmptyDefault { hive: Hkcu, path: CLASSIC_MENU_KEY, root: CLASSIC_MENU_ROOT }],
    },
    // ---------------- Geral ----------------
    Tweak {
        id: "show_extensions",
        name: "Mostrar extensões de arquivo",
        description: "Exibe “.exe”, “.pdf” etc. no nome dos arquivos.",
        details: "Também é segurança: fica fácil perceber um “foto.jpg.exe”.",
        category: Category::Geral,
        impact: Impact::Real,
        restart: Restart::Explorer,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: ADVANCED, name: "HideFileExt", value: 0 }],
    },
    // ---------------- Privacidade ----------------
    Tweak {
        id: "suggestions_off",
        name: "Desativar sugestões e apps automáticos",
        description: "Sem apps instalados sozinhos, sugestões no Iniciar e dicas do Windows.",
        details: "Desliga o “Gerenciador de Entrega de Conteúdo”, responsável por instalar apps patrocinados e mostrar sugestões.",
        category: Category::Privacidade,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
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
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", name: "Enabled", value: 0 }],
    },
    Tweak {
        id: "telemetry_min",
        name: "Telemetria no mínimo",
        description: "Reduz os dados de diagnóstico enviados à Microsoft.",
        details: "No Windows Home/Pro o mínimo aceito é “Obrigatório”: o valor 0 só desliga de vez no Enterprise/Education. Ainda assim tira os dados opcionais.",
        category: Category::Privacidade,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", name: "AllowTelemetry", value: 0 }],
    },
    Tweak {
        id: "bing_search_off",
        name: "Pesquisa do Iniciar sem Bing",
        description: "A busca do Iniciar procura só no PC, sem resultados da web.",
        details: "Mais rápido e sem enviar o que você digita. Vale após reiniciar o Explorer.",
        category: Category::Privacidade,
        impact: Impact::Real,
        restart: Restart::Explorer,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Policies\Microsoft\Windows\Explorer", name: "DisableSearchBoxSuggestions", value: 1 }],
    },
    Tweak {
        id: "recall_off",
        name: "Desativar Recall (IA do Windows)",
        description: "Impede que o Windows tire capturas da tela pra busca com IA.",
        details: "O Recall só existe em PCs Copilot+. Em outros PCs este ajuste não muda nada, mas também não atrapalha.",
        category: Category::Privacidade,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hkcu, path: r"Software\Policies\Microsoft\Windows\WindowsAI", name: "DisableAIDataAnalysis", value: 1 }],
    },
    // ---------------- Rede ----------------
    Tweak {
        id: "delivery_opt_off",
        name: "Não compartilhar atualizações pela internet",
        description: "O Windows para de enviar atualizações pra outros PCs usando seu upload.",
        details: "Desliga o modo P2P da Otimização de Entrega. As atualizações continuam vindo direto da Microsoft. Ajuda quando o upload é limitado e você joga ou faz live.",
        category: Category::Rede,
        impact: Impact::Real,
        restart: Restart::None,
        laptop_unsafe: false,
        ops: &[Op::Dword { hive: Hklm, path: r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", name: "DODownloadMode", value: 0 }],
    },
];

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
        description: "Menor latência possível no desktop: energia no máximo, nada gravando, mouse 1:1.",
        tweaks: &[
            "game_mode", "game_dvr_off", "hags", "mouse_accel_off", "sticky_keys_off", "power_high",
            "transparency_off", "suggestions_off", "delivery_opt_off",
        ],
    },
    BuiltinProfile {
        id: "equilibrado",
        name: "Equilibrado",
        description: "Ajustes sem efeito colateral, bons pra qualquer PC.",
        tweaks: &["game_mode", "game_dvr_off", "mouse_accel_off", "sticky_keys_off", "suggestions_off", "show_extensions"],
    },
    BuiltinProfile {
        id: "notebook",
        name: "Notebook",
        description: "Sem nada que esquente ou gaste bateria.",
        tweaks: &["game_mode", "game_dvr_off", "mouse_accel_off", "sticky_keys_off", "suggestions_off", "delivery_opt_off"],
    },
    BuiltinProfile {
        id: "pc_modesto",
        name: "PC Modesto",
        description: "Pouca RAM, GPU integrada ou HD: tira o peso visual da interface.",
        tweaks: &[
            "game_mode", "game_dvr_off", "mouse_accel_off", "power_high", "transparency_off", "animations_off",
            "suggestions_off", "delivery_opt_off",
        ],
    },
    BuiltinProfile {
        id: "streamer",
        name: "Streamer",
        description: "Jogo e transmissão juntos: GPU organizando a fila e energia no máximo.",
        tweaks: &["game_mode", "hags", "mouse_accel_off", "sticky_keys_off", "power_high", "suggestions_off"],
    },
    BuiltinProfile {
        id: "privacidade",
        name: "Privacidade",
        description: "Menos dados enviados, sem anúncios, sem Bing no Iniciar e sem Recall.",
        tweaks: &["suggestions_off", "ad_id_off", "telemetry_min", "bing_search_off", "recall_off"],
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
                assert_eq!(t.impact, Impact::Real, "perfil {} inclui ajuste de efeito pequeno {id}", p.id);
            }
        }
    }

    #[test]
    fn perfil_notebook_nao_tem_nada_que_esquente() {
        let p = find_profile("notebook").unwrap();
        assert!(p.tweaks.iter().all(|id| !find(id).unwrap().laptop_unsafe));
    }

    #[test]
    fn admin_detectado_pelas_operacoes() {
        assert!(find("hags").unwrap().requires_admin());
        assert!(find("sysmain_off").unwrap().requires_admin());
        assert!(!find("game_mode").unwrap().requires_admin());
        assert!(!find("power_high").unwrap().requires_admin());
    }
}

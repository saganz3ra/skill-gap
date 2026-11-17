//! Operações primitivas de um ajuste e o "contrato" com o sistema.
//!
//! Cada ajuste é uma lista de `Op` declarativas ("este valor do registro deve ser X").
//! Pra cada Op sabemos: ler se já está aplicada, tirar uma foto do valor atual,
//! aplicar e restaurar a foto. Toda a interação com o Windows passa pelo trait `Sys`,
//! então os testes usam um sistema falso em memória.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::power::PowerPlan;
pub use super::net::NetKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Hive {
    /// HKEY_CURRENT_USER: só o usuário atual, não precisa de admin.
    Hkcu,
    /// HKEY_LOCAL_MACHINE: vale pra todos, exige admin pra escrever.
    Hklm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Dword { hive: Hive, path: &'static str, name: &'static str, value: u32 },
    Str { hive: Hive, path: &'static str, name: &'static str, value: &'static str },
    /// Cria `path` com valor padrão vazio. Ao desfazer, apaga `root` inteira se ela não
    /// existia antes. Usado no menu de contexto clássico do Windows 11.
    KeyWithEmptyDefault { hive: Hive, path: &'static str, root: &'static str },
    /// Tipo de inicialização do serviço: 2 automático, 3 manual, 4 desativado.
    Service { name: &'static str, start: u32 },
    PowerPlan(PowerPlan),
    MouseAccelerationOff,
    /// Velocidade do ponteiro do Windows (1–20).
    MouseSpeed(u32),
    MemoryCompressionOff,
    /// Valor AC de uma configuração DENTRO do plano de energia ativo (sem trocar de plano).
    /// `sub`/`setting` são GUIDs do powercfg.
    PowerSetting { sub: &'static str, setting: &'static str, ac: u32 },
    /// Desativa uma tarefa agendada (caminho completo, ex.: `\Microsoft\Windows\...\Nome`).
    ScheduledTaskOff { path: &'static str },
    /// Desativa o "page combining" do gerenciador de memória (Disable-MMAgent).
    PageCombiningOff,
    /// Configuração global de rede (lista fechada de valores, ver `net.rs`).
    Net { key: NetKey, value: &'static str },
    /// Chave de primeiro nível num JSON de configurações de um app
    /// (ex.: `%APPDATA%\discord\settings.json`). `value` é um literal JSON (`false`).
    JsonFileKey { env: &'static str, file: &'static str, key: &'static str, value: &'static str },
}

/// Valor original de uma Op, salvo em disco antes de mexer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Snap {
    Dword { value: Option<u32> },
    Str { value: Option<String> },
    Key { existed: bool },
    Service {
        start: Option<u32>,
        /// Início atrasado (`DelayedAutostart`). `default`: diários antigos não tinham.
        #[serde(default)]
        delayed: Option<u32>,
    },
    Plan { guid: String },
    Mouse { values: [i32; 3] },
    MouseSpeed { value: u32 },
    MemoryCompression { enabled: bool },
    /// Plano em que o valor foi mudado: o desfazer volta NESTE plano, mesmo que o
    /// ativo tenha mudado depois.
    PowerSetting { plan: String, value: u32 },
    Task { enabled: bool },
    PageCombining { enabled: bool },
    Net { value: String },
    /// Literal JSON anterior; `None` = a chave não existia.
    Json { value: Option<String> },
}

/// Tudo que um ajuste precisa do sistema operacional.
pub trait Sys: Send + Sync {
    fn dword(&self, hive: Hive, path: &str, name: &str) -> Option<u32>;
    /// `None` apaga o valor.
    fn set_dword(&self, hive: Hive, path: &str, name: &str, value: Option<u32>) -> AppResult<()>;
    fn string(&self, hive: Hive, path: &str, name: &str) -> Option<String>;
    fn set_string(&self, hive: Hive, path: &str, name: &str, value: Option<&str>) -> AppResult<()>;
    fn key_exists(&self, hive: Hive, path: &str) -> bool;
    fn delete_key_tree(&self, hive: Hive, path: &str) -> AppResult<()>;
    fn stop_service(&self, name: &str);
    fn start_service(&self, name: &str);
    /// (GUID, plano conhecido) do plano de energia ativo.
    fn active_plan(&self) -> AppResult<(String, Option<PowerPlan>)>;
    fn set_plan(&self, plan: PowerPlan) -> AppResult<()>;
    fn set_plan_guid(&self, guid: &str) -> AppResult<()>;
    fn mouse(&self) -> AppResult<[i32; 3]>;
    fn set_mouse(&self, values: [i32; 3]) -> AppResult<()>;
    fn mouse_speed(&self) -> AppResult<u32>;
    fn set_mouse_speed(&self, value: u32) -> AppResult<()>;
    fn memory_compression(&self) -> AppResult<bool>;
    fn set_memory_compression(&self, enabled: bool) -> AppResult<()>;
    /// (GUID do plano ativo, valor AC). `Ok(None)`: este PC não tem essa configuração.
    fn power_value(&self, sub: &str, setting: &str) -> AppResult<Option<(String, u32)>>;
    fn set_power_value(&self, plan: &str, sub: &str, setting: &str, ac: u32) -> AppResult<()>;
    /// `None`: a tarefa não existe neste PC.
    fn task_enabled(&self, path: &str) -> Option<bool>;
    fn set_task_enabled(&self, path: &str, enabled: bool) -> AppResult<()>;
    fn page_combining(&self) -> AppResult<bool>;
    fn set_page_combining(&self, enabled: bool) -> AppResult<()>;
    /// Valor atual (já na grafia da lista). Erro: o Windows não tem essa configuração.
    fn net_setting(&self, key: NetKey) -> AppResult<String>;
    fn set_net_setting(&self, key: NetKey, value: &str) -> AppResult<()>;
    /// `None`: o arquivo não existe (app não instalado).
    fn json_file_exists(&self, env: &str, file: &str) -> bool;
    /// Literal JSON da chave; `Ok(None)` se a chave não existe.
    fn json_key(&self, env: &str, file: &str, key: &str) -> AppResult<Option<String>>;
    /// `None` remove a chave.
    fn set_json_key(&self, env: &str, file: &str, key: &str, value: Option<&str>) -> AppResult<()>;
}

/// O que o Modo Jogo precisa além do `Sys`: processos e serviços em tempo real.
pub trait GameSys: Sys {
    /// (PID, nome do .exe) de cada processo. Só lista: não abre nenhum processo.
    fn processes(&self) -> Vec<(u32, String)>;
    /// Classe de prioridade (ex.: 0x20 normal). None = não deu pra ler (processo
    /// protegido ou já fechado).
    fn priority(&self, pid: u32) -> Option<u32>;
    fn set_priority(&self, pid: u32, class: u32) -> AppResult<()>;
    fn service_running(&self, name: &str) -> Option<bool>;
    fn pause_service(&self, name: &str) -> AppResult<()>;
    fn resume_service(&self, name: &str) -> AppResult<()>;
    /// Caminho do executável SEM abrir o processo. `None` = não deu pra ler.
    fn image_path(&self, _pid: u32) -> Option<String> {
        None
    }
    /// Segura (true) ou solta (false) a resolução fina do timer neste processo.
    fn set_timer_hold(&self, _on: bool) {}
}

pub const NORMAL_PRIORITY: u32 = 0x20;
pub const BELOW_NORMAL_PRIORITY: u32 = 0x4000;

pub fn service_path(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}

/// Este PC tem o que a Op mexe? (configuração de energia, tarefa, cmdlet, app instalado).
/// `false` deixa o ajuste indisponível, com o motivo.
pub fn supported(sys: &dyn Sys, op: &Op) -> bool {
    match *op {
        Op::PowerSetting { sub, setting, .. } => matches!(sys.power_value(sub, setting), Ok(Some(_))),
        Op::ScheduledTaskOff { path } => sys.task_enabled(path).is_some(),
        Op::Net { key, .. } => sys.net_setting(key).is_ok(),
        Op::JsonFileKey { env, file, .. } => sys.json_file_exists(env, file),
        // Serviço que não existe neste Windows: gravar `Start` criaria uma chave fantasma.
        Op::Service { name, .. } => sys.dword(Hive::Hklm, &service_path(name), "Start").is_some(),
        _ => true,
    }
}

fn json_eq(a: &str, b: &str) -> bool {
    match (serde_json::from_str::<serde_json::Value>(a), serde_json::from_str::<serde_json::Value>(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Some(true) aplicada, Some(false) não aplicada, None = não deu pra ler.
pub fn is_applied(sys: &dyn Sys, op: &Op) -> Option<bool> {
    Some(match *op {
        Op::Dword { hive, path, name, value } => sys.dword(hive, path, name) == Some(value),
        Op::Str { hive, path, name, value } => sys.string(hive, path, name).as_deref() == Some(value),
        Op::KeyWithEmptyDefault { hive, path, .. } => sys.string(hive, path, "").as_deref() == Some(""),
        Op::Service { name, start } => sys.dword(Hive::Hklm, &service_path(name), "Start")? == start,
        Op::PowerPlan(plan) => sys.active_plan().ok()?.1 == Some(plan),
        Op::MouseAccelerationOff => sys.mouse().ok()?[2] == 0,
        Op::MouseSpeed(v) => sys.mouse_speed().ok()? == v,
        Op::MemoryCompressionOff => !sys.memory_compression().ok()?,
        Op::PowerSetting { sub, setting, ac } => sys.power_value(sub, setting).ok()??.1 == ac,
        Op::ScheduledTaskOff { path } => !sys.task_enabled(path)?,
        Op::PageCombiningOff => !sys.page_combining().ok()?,
        Op::Net { key, value } => sys.net_setting(key).ok()?.eq_ignore_ascii_case(value),
        Op::JsonFileKey { env, file, key, value } => {
            if !sys.json_file_exists(env, file) {
                return None;
            }
            sys.json_key(env, file, key).ok()?.is_some_and(|v| json_eq(&v, value))
        }
    })
}

pub fn snapshot(sys: &dyn Sys, op: &Op) -> AppResult<Snap> {
    Ok(match *op {
        Op::Dword { hive, path, name, .. } => Snap::Dword { value: sys.dword(hive, path, name) },
        Op::Str { hive, path, name, .. } => Snap::Str { value: sys.string(hive, path, name) },
        Op::KeyWithEmptyDefault { hive, root, .. } => Snap::Key { existed: sys.key_exists(hive, root) },
        Op::Service { name, .. } => Snap::Service {
            start: sys.dword(Hive::Hklm, &service_path(name), "Start"),
            delayed: sys.dword(Hive::Hklm, &service_path(name), "DelayedAutostart"),
        },
        Op::PowerPlan(_) => Snap::Plan { guid: sys.active_plan()?.0 },
        Op::MouseAccelerationOff => Snap::Mouse { values: sys.mouse()? },
        Op::MouseSpeed(_) => Snap::MouseSpeed { value: sys.mouse_speed()? },
        Op::MemoryCompressionOff => Snap::MemoryCompression { enabled: sys.memory_compression()? },
        Op::PowerSetting { sub, setting, .. } => {
            let (plan, value) = sys.power_value(sub, setting)?.ok_or_else(|| AppError::Unavailable("este PC não tem essa opção de energia".into()))?;
            Snap::PowerSetting { plan, value }
        }
        Op::ScheduledTaskOff { path } => {
            Snap::Task { enabled: sys.task_enabled(path).ok_or_else(|| AppError::Unavailable("a tarefa não existe neste PC".into()))? }
        }
        Op::PageCombiningOff => Snap::PageCombining { enabled: sys.page_combining()? },
        Op::Net { key, .. } => Snap::Net { value: sys.net_setting(key)? },
        Op::JsonFileKey { env, file, key, .. } => Snap::Json { value: sys.json_key(env, file, key)? },
    })
}

pub fn apply(sys: &dyn Sys, op: &Op) -> AppResult<()> {
    match *op {
        Op::Dword { hive, path, name, value } => sys.set_dword(hive, path, name, Some(value)),
        Op::Str { hive, path, name, value } => sys.set_string(hive, path, name, Some(value)),
        Op::KeyWithEmptyDefault { hive, path, .. } => sys.set_string(hive, path, "", Some("")),
        Op::Service { name, start } => {
            sys.set_dword(Hive::Hklm, &service_path(name), "Start", Some(start))?;
            if start == 4 {
                // Desativado: para agora também, sem esperar o próximo boot.
                sys.stop_service(name);
            }
            Ok(())
        }
        Op::PowerPlan(plan) => sys.set_plan(plan),
        Op::MouseAccelerationOff => sys.set_mouse([0, 0, 0]),
        Op::MouseSpeed(v) => sys.set_mouse_speed(v),
        Op::MemoryCompressionOff => sys.set_memory_compression(false),
        Op::PowerSetting { sub, setting, ac } => {
            let (plan, _) = sys.power_value(sub, setting)?.ok_or_else(|| AppError::Unavailable("este PC não tem essa opção de energia".into()))?;
            sys.set_power_value(&plan, sub, setting, ac)
        }
        Op::ScheduledTaskOff { path } => sys.set_task_enabled(path, false),
        Op::PageCombiningOff => sys.set_page_combining(false),
        Op::Net { key, value } => sys.set_net_setting(key, value),
        Op::JsonFileKey { env, file, key, value } => sys.set_json_key(env, file, key, Some(value)),
    }
}

pub fn restore(sys: &dyn Sys, op: &Op, snap: &Snap) -> AppResult<()> {
    match (*op, snap) {
        (Op::Dword { hive, path, name, .. }, Snap::Dword { value }) => sys.set_dword(hive, path, name, *value),
        (Op::Str { hive, path, name, .. }, Snap::Str { value }) => sys.set_string(hive, path, name, value.as_deref()),
        (Op::KeyWithEmptyDefault { hive, path, root }, Snap::Key { existed }) => {
            if *existed {
                // A chave já existia (alguém criou antes de nós): só tiramos o valor padrão.
                sys.set_string(hive, path, "", None)
            } else {
                sys.delete_key_tree(hive, root)
            }
        }
        (Op::Service { name, .. }, Snap::Service { start, delayed }) => {
            sys.set_dword(Hive::Hklm, &service_path(name), "Start", *start)?;
            // Só mexe no início atrasado se ele mudou (diário antigo não tem a foto).
            let path = service_path(name);
            if delayed.is_some() && sys.dword(Hive::Hklm, &path, "DelayedAutostart") != *delayed {
                sys.set_dword(Hive::Hklm, &path, "DelayedAutostart", *delayed)?;
            }
            if *start == Some(2) {
                sys.start_service(name);
            }
            Ok(())
        }
        (Op::PowerPlan(_), Snap::Plan { guid }) => sys.set_plan_guid(guid),
        (Op::MouseAccelerationOff, Snap::Mouse { values }) => sys.set_mouse(*values),
        (Op::MouseSpeed(_), Snap::MouseSpeed { value }) => sys.set_mouse_speed(*value),
        (Op::MemoryCompressionOff, Snap::MemoryCompression { enabled }) => sys.set_memory_compression(*enabled),
        (Op::PowerSetting { sub, setting, .. }, Snap::PowerSetting { plan, value }) => sys.set_power_value(plan, sub, setting, *value),
        (Op::ScheduledTaskOff { path }, Snap::Task { enabled }) => sys.set_task_enabled(path, *enabled),
        (Op::PageCombiningOff, Snap::PageCombining { enabled }) => sys.set_page_combining(*enabled),
        (Op::Net { key, .. }, Snap::Net { value }) => sys.set_net_setting(key, value),
        (Op::JsonFileKey { env, file, key, .. }, Snap::Json { value }) => sys.set_json_key(env, file, key, value.as_deref()),
        _ => Err(AppError::InvalidInput("foto não corresponde à operação".into())),
    }
}

// ---------------- Leitura de saída de ferramentas (pura, testável) ----------------

/// `powercfg /qh <plano> <sub> <config>`: as duas últimas linhas trazem o índice AC e
/// o DC, em hexadecimal. O rótulo vem traduzido; o valor `0x...` não. Pega o
/// penúltimo `0x` da saída (o AC), em qualquer idioma.
pub fn parse_qh_ac(out: &str) -> Option<u32> {
    let hex: Vec<u32> = out
        .split_whitespace()
        .filter_map(|t| t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")))
        .filter_map(|h| u32::from_str_radix(h.trim_end_matches(|c: char| !c.is_ascii_hexdigit()), 16).ok())
        .collect();
    (hex.len() >= 2).then(|| hex[hex.len() - 2])
}

/// `schtasks /Query /XML`: `<Enabled>false</Enabled>` dentro de `<Settings>`.
/// Sem a tag, a tarefa está ligada (padrão do Agendador).
pub fn parse_task_enabled(xml: &str) -> Option<bool> {
    let settings = xml.split("<Settings>").nth(1)?.split("</Settings>").next()?;
    Some(!settings.contains("<Enabled>false</Enabled>"))
}

pub fn is_guid_plain(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 5 && p.iter().map(|x| x.len()).eq([8, 4, 4, 4, 12]) && p.iter().all(|x| x.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Caminho de tarefa agendada: começa com `\`, sem `..`, só caracteres comuns.
pub fn is_task_path(s: &str) -> bool {
    s.starts_with('\\')
        && !s.contains("..")
        && s.len() < 260
        && s.chars().all(|c| c.is_alphanumeric() || " \\-_.{}()".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powercfg_em_qualquer_idioma() {
        let pt = "GUID da Configuração de Energia: 893dee8e-...  (Estado mínimo do processador)\n    \
                  Configuração Mínima Possível: 0x00000000\n  Configuração Máxima Possível: 0x00000064\n  \
                  Incremento das Configurações Possíveis: 0x00000001\n  Unidades das Configurações Possíveis: %\n\
                  Índice da Configuração de Energia de CA Atual: 0x00000064\n\
                  Índice da Configuração de Energia de CC Atual: 0x00000005";
        assert_eq!(parse_qh_ac(pt), Some(100));
        assert_eq!(parse_qh_ac("Current AC Power Setting Index: 0x00000001\nCurrent DC Power Setting Index: 0x00000000"), Some(1));
        assert_eq!(parse_qh_ac("Parâmetros inválidos"), None);
    }

    #[test]
    fn tarefa_ligada_ou_desligada() {
        assert_eq!(parse_task_enabled("<Task><Settings><Enabled>false</Enabled></Settings></Task>"), Some(false));
        assert_eq!(parse_task_enabled("<Task><Settings><Hidden>true</Hidden></Settings></Task>"), Some(true));
        assert_eq!(parse_task_enabled("ERRO: o sistema não pode encontrar o arquivo"), None);
    }

    use crate::tweaks::fake::FakeSys;

    fn round_trip(sys: &FakeSys, op: Op) -> Snap {
        assert_eq!(is_applied(sys, &op), Some(false), "{op:?} começa desligado");
        let snap = snapshot(sys, &op).unwrap();
        apply(sys, &op).unwrap();
        assert_eq!(is_applied(sys, &op), Some(true), "{op:?} aplicado");
        restore(sys, &op, &snap).unwrap();
        assert_eq!(is_applied(sys, &op), Some(false), "{op:?} desfeito");
        snap
    }

    const SUB: &str = "54533251-82be-4824-96c1-47b60b740d00";
    const SET: &str = "893dee8e-2bef-41e0-89c6-b55d0929964c";

    #[test]
    fn energia_no_plano_ativo_e_desfazer_no_mesmo_plano() {
        let sys = FakeSys::default();
        let op = Op::PowerSetting { sub: SUB, setting: SET, ac: 100 };
        assert!(!supported(&sys, &op), "PC sem a configuração");
        sys.add_power_setting(SUB, SET, 5);
        let balanced = sys.active_plan().unwrap().0;
        let snap = snapshot(&sys, &op).unwrap();
        apply(&sys, &op).unwrap();
        // O usuário troca de plano depois: o desfazer volta o valor NO plano em que mudou.
        sys.set_plan(PowerPlan::HighPerformance).unwrap();
        restore(&sys, &op, &snap).unwrap();
        assert_eq!(sys.power_in(&balanced, SUB, SET), Some(5));
    }

    #[test]
    fn tarefa_pagina_rede_e_json() {
        let sys = FakeSys::default();
        let task = Op::ScheduledTaskOff { path: r"\Microsoft\Windows\X\Y" };
        assert!(!supported(&sys, &task));
        sys.add_task(r"\Microsoft\Windows\X\Y", true);
        round_trip(&sys, task);

        round_trip(&sys, Op::PageCombiningOff);

        let net = Op::Net { key: NetKey::Teredo, value: "Disabled" };
        assert!(!supported(&sys, &net), "cmdlet ausente");
        sys.set_net(NetKey::Teredo, "Default");
        assert_eq!(round_trip(&sys, net), Snap::Net { value: "Default".into() });

        let json = Op::JsonFileKey { env: "APPDATA", file: r"discord\settings.json", key: "OPEN_ON_STARTUP", value: "false" };
        assert!(!supported(&sys, &json), "app não instalado");
        sys.add_json("APPDATA", r"discord\settings.json", serde_json::json!({ "a": 1 }));
        assert_eq!(round_trip(&sys, json), Snap::Json { value: None }, "chave não existia: desfazer remove");
        assert_eq!(sys.json("APPDATA", r"discord\settings.json").unwrap(), serde_json::json!({ "a": 1 }));
    }

    #[test]
    fn servico_guarda_inicio_atrasado_e_le_diario_antigo() {
        let sys = FakeSys::default();
        let path = service_path("WSearch");
        sys.set_dword(Hive::Hklm, &path, "Start", Some(2)).unwrap();
        sys.set_dword(Hive::Hklm, &path, "DelayedAutostart", Some(1)).unwrap();
        let op = Op::Service { name: "WSearch", start: 3 };
        let snap = snapshot(&sys, &op).unwrap();
        apply(&sys, &op).unwrap();
        sys.set_dword(Hive::Hklm, &path, "DelayedAutostart", Some(0)).unwrap(); // o Windows mexeu
        restore(&sys, &op, &snap).unwrap();
        assert_eq!(sys.dword(Hive::Hklm, &path, "DelayedAutostart"), Some(1));
        // Diário gravado pela versão antiga, sem `delayed`.
        let old: Snap = serde_json::from_str(r#"{"kind":"service","start":2}"#).unwrap();
        assert_eq!(old, Snap::Service { start: Some(2), delayed: None });
    }

    #[test]
    fn validacoes() {
        assert!(is_guid_plain("893dee8e-2bef-41e0-89c6-b55d0929964c"));
        assert!(!is_guid_plain("SUB_PROCESSOR"));
        assert!(is_task_path(r"\Microsoft\Windows\MemoryDiagnostic\ProcessMemoryDiagnosticEvents"));
        assert!(!is_task_path(r"\Microsoft\..\Evil"));
        assert!(!is_task_path("Microsoft\\x & calc"));
    }
}

//! Operações primitivas de um ajuste e o "contrato" com o sistema.
//!
//! Cada ajuste é uma lista de `Op` declarativas ("este valor do registro deve ser X").
//! Pra cada Op sabemos: ler se já está aplicada, tirar uma foto do valor atual,
//! aplicar e restaurar a foto. Toda a interação com o Windows passa pelo trait `Sys`,
//! então os testes usam um sistema falso em memória.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::power::PowerPlan;

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
}

/// Valor original de uma Op, salvo em disco antes de mexer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Snap {
    Dword { value: Option<u32> },
    Str { value: Option<String> },
    Key { existed: bool },
    Service { start: Option<u32> },
    Plan { guid: String },
    Mouse { values: [i32; 3] },
    MouseSpeed { value: u32 },
    MemoryCompression { enabled: bool },
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
}

pub const NORMAL_PRIORITY: u32 = 0x20;
pub const BELOW_NORMAL_PRIORITY: u32 = 0x4000;

pub fn service_path(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
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
    })
}

pub fn snapshot(sys: &dyn Sys, op: &Op) -> AppResult<Snap> {
    Ok(match *op {
        Op::Dword { hive, path, name, .. } => Snap::Dword { value: sys.dword(hive, path, name) },
        Op::Str { hive, path, name, .. } => Snap::Str { value: sys.string(hive, path, name) },
        Op::KeyWithEmptyDefault { hive, root, .. } => Snap::Key { existed: sys.key_exists(hive, root) },
        Op::Service { name, .. } => Snap::Service { start: sys.dword(Hive::Hklm, &service_path(name), "Start") },
        Op::PowerPlan(_) => Snap::Plan { guid: sys.active_plan()?.0 },
        Op::MouseAccelerationOff => Snap::Mouse { values: sys.mouse()? },
        Op::MouseSpeed(_) => Snap::MouseSpeed { value: sys.mouse_speed()? },
        Op::MemoryCompressionOff => Snap::MemoryCompression { enabled: sys.memory_compression()? },
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
        (Op::Service { name, .. }, Snap::Service { start }) => {
            sys.set_dword(Hive::Hklm, &service_path(name), "Start", *start)?;
            if *start == Some(2) {
                sys.start_service(name);
            }
            Ok(())
        }
        (Op::PowerPlan(_), Snap::Plan { guid }) => sys.set_plan_guid(guid),
        (Op::MouseAccelerationOff, Snap::Mouse { values }) => sys.set_mouse(*values),
        (Op::MouseSpeed(_), Snap::MouseSpeed { value }) => sys.set_mouse_speed(*value),
        (Op::MemoryCompressionOff, Snap::MemoryCompression { enabled }) => sys.set_memory_compression(*enabled),
        _ => Err(crate::error::AppError::InvalidInput("foto não corresponde à operação".into())),
    }
}

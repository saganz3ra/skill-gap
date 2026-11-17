//! Processo auxiliar elevado: o app roda como usuário comum e só pede permissão de
//! administrador (o aviso do UAC) na hora de uma ação que precisa.
//!
//! Como funciona:
//! 1. Uma ação esbarra em "acesso negado" (ou é sabidamente de admin).
//! 2. O app reabre o PRÓPRIO executável com `runas` + `--elevated-helper`: o Windows
//!    mostra o UAC. Recusou? Nada é alterado (`ElevationDenied`).
//! 3. O auxiliar cria um pipe nomeado (nome aleatório) e o app se conecta. Os dois
//!    conferem o PID um do outro.
//! 4. O app manda pedidos em JSON, um por linha; o auxiliar confere cada um contra a
//!    POLÍTICA abaixo, executa e responde.
//! 5. O auxiliar vive até o app fechar: um UAC por sessão, não um por clique.
//!
//! Segurança: o auxiliar NÃO executa comando arbitrário. Cada pedido é uma operação
//! de uma lista fechada, e os parâmetros são conferidos contra o catálogo de ajustes
//! (chave e valor do registro), uma lista de serviços e os ids de correções. Mesmo que
//! outro programa conseguisse falar com o pipe, só conseguiria fazer o que o próprio
//! Skill Gap já faz.

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanupCategory;
use crate::error::{AppError, AppResult};
use crate::startup::{parse_id, StartupSource};
use crate::tweaks::catalog::TWEAKS;
use crate::tweaks::ops::{Hive, Op};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Request {
    /// HKLM
    SetDword { path: String, name: String, value: Option<u32> },
    /// HKLM
    SetString { path: String, name: String, value: Option<String> },
    /// HKLM
    DeleteKeyTree { path: String },
    StopService { name: String },
    StartService { name: String },
    SetStartupApproved { id: String, enabled: bool },
    MemoryCompression { enabled: bool },
    RestorePoint,
    DriversBackup,
    BootReport,
    RunFix { id: String },
    /// Valor de energia dentro de um plano (só pares sub/config do catálogo).
    PowerSetting { plan: String, sub: String, setting: String, value: u32 },
    /// Liga/desliga tarefa agendada (só as do catálogo).
    TaskEnabled { path: String, enabled: bool },
    PageCombining { enabled: bool },
    /// Configuração de rede (só valores da lista fechada).
    NetSetting { key: crate::tweaks::ops::NetKey, value: String },
    /// Recoloca IP/DNS fixos de um adaptador depois do "Resetar a rede".
    RestoreNetwork { adapter: crate::netcfg::AdapterConfig },
    CleanCategory { category: CleanupCategory },
}

/// Linha de resposta do auxiliar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reply {
    Progress(serde_json::Value),
    Ok(serde_json::Value),
    Err { kind: String, message: String },
}

impl Reply {
    pub fn from_result(r: AppResult<serde_json::Value>) -> Self {
        match r {
            Ok(v) => Reply::Ok(v),
            Err(e) => {
                let v = serde_json::to_value(&e).unwrap_or_default();
                Reply::Err {
                    kind: v["kind"].as_str().unwrap_or("helper").to_string(),
                    message: v["message"].as_str().unwrap_or("erro no auxiliar").to_string(),
                }
            }
        }
    }

    /// Reconstrói o erro do lado do app, preservando o "precisa de admin".
    pub fn into_error(kind: &str, message: String) -> AppError {
        match kind {
            "requiresAdmin" => AppError::RequiresAdmin,
            "elevationDenied" => AppError::ElevationDenied,
            _ => AppError::Helper(message),
        }
    }
}

// ---------------- Política: o que o auxiliar aceita ----------------

/// Serviços que o app pode parar/ligar: os do Modo Jogo e os das Correções.
pub const ALLOWED_SERVICES: [&str; 5] = ["SysMain", "WSearch", "wuauserv", "BITS", "W32Time"];

const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";

/// `IFEO\<jogo.exe>\PerfOptions` com um nome de executável válido (sem caminho).
fn is_ifeo_perf_key(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix(&format!("{}\\", IFEO.to_ascii_lowercase())) else { return false };
    let Some(exe) = rest.strip_suffix("\\perfoptions") else { return false };
    exe.ends_with(".exe")
        && !exe.contains(['\\', '/', ':'])
        && exe.len() <= 260
        && !crate::gaming::watcher::is_protected(exe)
}

/// (caminho, nome) de valor HKLM que algum ajuste do catálogo escreve.
fn catalog_allows(path: &str, name: &str, string: bool) -> bool {
    TWEAKS.iter().flat_map(|t| t.ops.iter()).any(|op| match *op {
        Op::Dword { hive: Hive::Hklm, path: p, name: n, .. } if !string => p.eq_ignore_ascii_case(path) && n.eq_ignore_ascii_case(name),
        Op::Str { hive: Hive::Hklm, path: p, name: n, .. } if string => p.eq_ignore_ascii_case(path) && n.eq_ignore_ascii_case(name),
        _ => false,
    })
}

fn all_ops() -> impl Iterator<Item = &'static Op> {
    TWEAKS.iter().flat_map(|t| t.ops.iter())
}

/// O par (sub, config) aparece em algum `Op::PowerSetting` do catálogo.
fn power_setting_allowed<'a>(mut ops: impl Iterator<Item = &'a Op>, sub: &str, setting: &str) -> bool {
    ops.any(|op| matches!(*op, Op::PowerSetting { sub: s, setting: c, .. } if s.eq_ignore_ascii_case(sub) && c.eq_ignore_ascii_case(setting)))
}

/// A tarefa aparece em algum `Op::ScheduledTaskOff` do catálogo.
fn task_allowed<'a>(mut ops: impl Iterator<Item = &'a Op>, path: &str) -> bool {
    ops.any(|op| matches!(*op, Op::ScheduledTaskOff { path: p } if p.eq_ignore_ascii_case(path)))
}

/// Serviços que o app pode mexer: os do Modo Jogo/Correções e os de `Op::Service` do catálogo.
fn service_allowed(name: &str) -> bool {
    ALLOWED_SERVICES.iter().any(|s| s.eq_ignore_ascii_case(name))
        || all_ops().any(|op| matches!(*op, Op::Service { name: n, .. } if n.eq_ignore_ascii_case(name)))
}

/// `Services\<nome>\Start` ou `DelayedAutostart` de um serviço permitido.
fn service_value_allowed(path: &str, name: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let Some(svc) = lower.strip_prefix(r"system\currentcontrolset\services\") else { return false };
    !svc.contains('\\')
        && (name.eq_ignore_ascii_case("Start") || name.eq_ignore_ascii_case("DelayedAutostart"))
        && service_allowed(svc)
}

/// Confere um pedido ANTES de executar. É a única porta de entrada do auxiliar.
pub fn check(req: &Request) -> Result<(), String> {
    let ok = match req {
        Request::SetDword { path, name, .. } => {
            catalog_allows(path, name, false)
                || (is_ifeo_perf_key(path) && name.eq_ignore_ascii_case("CpuPriorityClass"))
                || service_value_allowed(path, name)
        }
        Request::SetString { path, name, .. } => catalog_allows(path, name, true),
        Request::DeleteKeyTree { path } => is_ifeo_perf_key(path),
        Request::StopService { name } | Request::StartService { name } => service_allowed(name),
        Request::SetStartupApproved { id, .. } => parse_id(id).is_some_and(|(s, _)| {
            matches!(s, StartupSource::MachineRun | StartupSource::MachineRun32 | StartupSource::CommonFolder)
        }),
        Request::RunFix { id } => crate::fixes::find(id).is_some(),
        Request::PowerSetting { plan, sub, setting, .. } => {
            crate::tweaks::ops::is_guid_plain(plan) && power_setting_allowed(all_ops(), sub, setting)
        }
        Request::TaskEnabled { path, .. } => task_allowed(all_ops(), path),
        Request::PageCombining { .. } => true,
        Request::NetSetting { key, value } => key.canonical(value).is_some(),
        Request::RestoreNetwork { adapter } => crate::netcfg::validate(adapter).is_ok(),
        Request::CleanCategory { category } => category.requires_admin(),
        Request::MemoryCompression { .. } | Request::RestorePoint | Request::DriversBackup | Request::BootReport => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("pedido recusado pela política do auxiliar: {req:?}"))
    }
}

// ---------------- API usada pelo resto do app ----------------

/// Quando o pedido pode abrir o UAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Ação do usuário: pode mostrar o UAC agora.
    OnDemand,
    /// Tarefa em segundo plano (ex.: Modo Jogo no meio da partida): só usa o auxiliar
    /// se ele já estiver aberto. Um UAC surgindo no meio do jogo seria péssimo.
    IfRunning,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElevationStatus {
    pub is_admin: bool,
    pub helper_running: bool,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{call, helper_main, is_running};

#[cfg(not(windows))]
pub fn call(_req: &Request, _launch: Launch, _on_progress: impl FnMut(serde_json::Value)) -> AppResult<serde_json::Value> {
    Err(AppError::Unsupported)
}
#[cfg(not(windows))]
pub fn is_running() -> bool {
    false
}
#[cfg(not(windows))]
pub fn helper_main(_args: &[String]) -> i32 {
    1
}

pub fn status() -> ElevationStatus {
    ElevationStatus { is_admin: crate::system::is_admin(), helper_running: is_running() }
}

/// Tenta localmente; se esbarrar em "precisa de admin" (e o app não for admin), manda
/// o pedido pro auxiliar. `convert` transforma a resposta JSON no tipo esperado.
pub fn local_or_elevated<T>(
    local: impl FnOnce() -> AppResult<T>,
    req: Request,
    launch: Launch,
    convert: impl FnOnce(serde_json::Value) -> AppResult<T>,
) -> AppResult<T> {
    if crate::system::is_admin() {
        return local();
    }
    match local() {
        Err(AppError::RequiresAdmin) => convert(call(&req, launch, |_| {})?),
        other => other,
    }
}

/// Para ações SABIDAMENTE de admin (que sem admin falham de jeitos variados, não com
/// um "acesso negado" limpo): vai direto pro auxiliar quando o app não é admin.
pub fn admin_or_elevated<T>(
    local: impl FnOnce() -> AppResult<T>,
    req: Request,
    on_progress: impl FnMut(serde_json::Value),
    convert: impl FnOnce(serde_json::Value) -> AppResult<T>,
) -> AppResult<T> {
    if crate::system::is_admin() {
        local()
    } else {
        convert(call(&req, Launch::OnDemand, on_progress)?)
    }
}

pub fn unit(_: serde_json::Value) -> AppResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::ops::service_path;

    fn dword(path: &str, name: &str) -> Request {
        Request::SetDword { path: path.into(), name: name.into(), value: Some(1) }
    }

    #[test]
    fn aceita_so_valores_do_catalogo() {
        assert!(check(&dword(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode")).is_ok());
        assert!(check(&dword(r"system\currentcontrolset\control\graphicsdrivers", "hwschmode")).is_ok(), "registro não diferencia maiúsculas");
        assert!(check(&dword(r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "OutroValor")).is_err());
        // Clássicos de ataque: desligar o UAC, o Defender, criar "debugger" no IFEO.
        assert!(check(&dword(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System", "EnableLUA")).is_err());
        assert!(check(&dword(r"SOFTWARE\Policies\Microsoft\Windows Defender", "DisableAntiSpyware")).is_err());
    }

    #[test]
    fn ifeo_so_prioridade_de_jogo() {
        let ok = format!(r"{IFEO}\cs2.exe\PerfOptions");
        assert!(check(&dword(&ok, "CpuPriorityClass")).is_ok());
        assert!(check(&Request::DeleteKeyTree { path: ok.clone() }).is_ok());
        assert!(check(&dword(&ok, "IoPriority")).is_err());
        // "Debugger" no IFEO sequestra a abertura de um programa: nunca.
        assert!(check(&dword(&format!(r"{IFEO}\cs2.exe"), "Debugger")).is_err());
        assert!(check(&Request::DeleteKeyTree { path: format!(r"{IFEO}\cs2.exe") }).is_err());
        assert!(check(&Request::DeleteKeyTree { path: format!(r"{IFEO}\..\cs2.exe\PerfOptions") }).is_err());
        assert!(check(&dword(&format!(r"{IFEO}\vgc.exe\PerfOptions"), "CpuPriorityClass")).is_err(), "anticheat é protegido");
    }

    #[test]
    fn servicos_e_inicializacao() {
        assert!(check(&Request::StopService { name: "wsearch".into() }).is_ok());
        assert!(check(&Request::StopService { name: "WinDefend".into() }).is_err());
        assert!(check(&dword(&service_path("SysMain"), "Start")).is_ok());
        assert!(check(&dword(&service_path("WinDefend"), "Start")).is_err());
        assert!(check(&dword(&service_path("DiagTrack"), "Start")).is_ok(), "serviço do catálogo");
        assert!(check(&dword(&service_path("DiagTrack"), "DelayedAutostart")).is_ok());
        assert!(check(&dword(&service_path("DiagTrack"), "ImagePath")).is_err());
        assert!(check(&dword(&format!(r"{}\Parameters", service_path("DiagTrack")), "Start")).is_err());
        assert!(check(&Request::StopService { name: "DiagTrack".into() }).is_ok());
        assert!(check(&Request::SetStartupApproved { id: "machineRun::RtkAudUService".into(), enabled: false }).is_ok());
        assert!(check(&Request::SetStartupApproved { id: "userRun::Steam".into(), enabled: false }).is_err(), "HKCU não precisa de admin");
    }

    #[test]
    fn restaurar_rede_so_com_enderecos_validos() {
        let ok = crate::netcfg::AdapterConfig {
            guid: "{4d36e972-e325-11ce-bfc1-08002be10318}".into(),
            name: "Ethernet".into(),
            ipv4: None,
            dns4: vec!["1.1.1.1".into()],
            dns6: vec![],
        };
        assert!(check(&Request::RestoreNetwork { adapter: ok.clone() }).is_ok());
        let bad = crate::netcfg::AdapterConfig { dns4: vec!["1.1.1.1\" & calc".into()], ..ok };
        assert!(check(&Request::RestoreNetwork { adapter: bad }).is_err());
    }

    #[test]
    fn energia_e_tarefas_so_do_catalogo() {
        const SUB: &str = "54533251-82be-4824-96c1-47b60b740d00";
        const SET: &str = "893dee8e-2bef-41e0-89c6-b55d0929964c";
        let ops = [Op::PowerSetting { sub: SUB, setting: SET, ac: 100 }, Op::ScheduledTaskOff { path: r"\Microsoft\Windows\X\Y" }];
        assert!(power_setting_allowed(ops.iter(), SUB, &SET.to_uppercase()));
        assert!(!power_setting_allowed(ops.iter(), SUB, "00000000-0000-0000-0000-000000000000"));
        assert!(task_allowed(ops.iter(), r"\microsoft\windows\x\y"));
        assert!(!task_allowed(ops.iter(), r"\Microsoft\Windows\Defender\Scan"));
        let net = |v: &str| Request::NetSetting { key: crate::tweaks::ops::NetKey::Teredo, value: v.into() };
        assert!(check(&net("Disabled")).is_ok());
        assert!(check(&net("Disabled; calc")).is_err());
        let plan = |p: &str| Request::PowerSetting { plan: p.into(), sub: SUB.into(), setting: SET.into(), value: 1 };
        assert!(check(&plan("not-a-guid")).is_err());
    }

    #[test]
    fn correcoes_e_limpeza() {
        assert!(check(&Request::RunFix { id: "sfc".into() }).is_ok());
        assert!(check(&Request::RunFix { id: "format_c".into() }).is_err());
        assert!(check(&Request::CleanCategory { category: CleanupCategory::WindowsTemp }).is_ok());
        assert!(check(&Request::CleanCategory { category: CleanupCategory::UserTemp }).is_err());
    }

    #[test]
    fn protocolo_ida_e_volta() {
        let req = Request::SetStartupApproved { id: "machineRun::X".into(), enabled: true };
        let line = serde_json::to_string(&req).unwrap();
        assert!(line.contains(r#""op":"setStartupApproved""#));
        assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), req);

        let reply = Reply::from_result(Err(AppError::RequiresAdmin));
        let line = serde_json::to_string(&reply).unwrap();
        match serde_json::from_str::<Reply>(&line).unwrap() {
            Reply::Err { kind, message } => assert!(matches!(Reply::into_error(&kind, message), AppError::RequiresAdmin)),
            other => panic!("{other:?}"),
        }
    }
}

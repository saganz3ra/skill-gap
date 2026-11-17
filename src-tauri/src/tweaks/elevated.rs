//! `Sys` do app: faz tudo localmente e, quando uma escrita esbarra em "precisa de
//! admin", repete a mesma operação no processo auxiliar elevado.
//!
//! Leituras nunca precisam do auxiliar. Escritas em HKLM pedem o UAC na hora (é sempre
//! uma ação do usuário). Serviços do Modo Jogo, que acontecem no meio da partida, só
//! usam o auxiliar se ele já estiver aberto.

use super::ops::{GameSys, Hive, NetKey, Sys};
use super::windows::WinSys;
use crate::elevation::{self, Launch, Request};
use crate::error::{AppError, AppResult};
use crate::power::PowerPlan;

pub struct ElevatedSys;

fn retry<T>(local: AppResult<T>, hive: Hive, req: impl FnOnce() -> Request, launch: Launch) -> AppResult<()>
where
    T: Into<()>,
{
    match local {
        Err(AppError::RequiresAdmin) if hive == Hive::Hklm && !crate::system::is_admin() => {
            elevation::call(&req(), launch, |_| {}).map(|_| ())
        }
        other => other.map(Into::into),
    }
}

impl Sys for ElevatedSys {
    fn dword(&self, hive: Hive, path: &str, name: &str) -> Option<u32> {
        WinSys.dword(hive, path, name)
    }

    fn set_dword(&self, hive: Hive, path: &str, name: &str, value: Option<u32>) -> AppResult<()> {
        retry(
            WinSys.set_dword(hive, path, name, value),
            hive,
            || Request::SetDword { path: path.into(), name: name.into(), value },
            Launch::OnDemand,
        )
    }

    fn string(&self, hive: Hive, path: &str, name: &str) -> Option<String> {
        WinSys.string(hive, path, name)
    }

    fn set_string(&self, hive: Hive, path: &str, name: &str, value: Option<&str>) -> AppResult<()> {
        retry(
            WinSys.set_string(hive, path, name, value),
            hive,
            || Request::SetString { path: path.into(), name: name.into(), value: value.map(str::to_string) },
            Launch::OnDemand,
        )
    }

    fn key_exists(&self, hive: Hive, path: &str) -> bool {
        WinSys.key_exists(hive, path)
    }

    fn delete_key_tree(&self, hive: Hive, path: &str) -> AppResult<()> {
        retry(WinSys.delete_key_tree(hive, path), hive, || Request::DeleteKeyTree { path: path.into() }, Launch::OnDemand)
    }

    fn stop_service(&self, name: &str) {
        if crate::system::is_admin() {
            WinSys.stop_service(name);
        } else {
            let _ = elevation::call(&Request::StopService { name: name.into() }, Launch::IfRunning, |_| {});
        }
    }

    fn start_service(&self, name: &str) {
        if crate::system::is_admin() {
            WinSys.start_service(name);
        } else {
            let _ = elevation::call(&Request::StartService { name: name.into() }, Launch::IfRunning, |_| {});
        }
    }

    fn active_plan(&self) -> AppResult<(String, Option<PowerPlan>)> {
        WinSys.active_plan()
    }

    fn set_plan(&self, plan: PowerPlan) -> AppResult<()> {
        WinSys.set_plan(plan)
    }

    fn set_plan_guid(&self, guid: &str) -> AppResult<()> {
        WinSys.set_plan_guid(guid)
    }

    fn mouse(&self) -> AppResult<[i32; 3]> {
        WinSys.mouse()
    }

    fn set_mouse(&self, values: [i32; 3]) -> AppResult<()> {
        WinSys.set_mouse(values)
    }

    fn mouse_speed(&self) -> AppResult<u32> {
        WinSys.mouse_speed()
    }

    fn set_mouse_speed(&self, value: u32) -> AppResult<()> {
        WinSys.set_mouse_speed(value)
    }

    fn memory_compression(&self) -> AppResult<bool> {
        WinSys.memory_compression()
    }

    fn set_memory_compression(&self, enabled: bool) -> AppResult<()> {
        // Sem admin o PowerShell falha com mensagens variadas: vai direto pro auxiliar.
        elevation::admin_or_elevated(
            || WinSys.set_memory_compression(enabled),
            Request::MemoryCompression { enabled },
            |_| {},
            elevation::unit,
        )
    }

    fn power_value(&self, sub: &str, setting: &str) -> AppResult<Option<(String, u32)>> {
        WinSys.power_value(sub, setting)
    }

    fn set_power_value(&self, plan: &str, sub: &str, setting: &str, ac: u32) -> AppResult<()> {
        elevation::local_or_elevated(
            || WinSys.set_power_value(plan, sub, setting, ac),
            Request::PowerSetting { plan: plan.into(), sub: sub.into(), setting: setting.into(), value: ac },
            Launch::OnDemand,
            elevation::unit,
        )
    }

    fn task_enabled(&self, path: &str) -> Option<bool> {
        WinSys.task_enabled(path)
    }

    fn set_task_enabled(&self, path: &str, enabled: bool) -> AppResult<()> {
        elevation::local_or_elevated(
            || WinSys.set_task_enabled(path, enabled),
            Request::TaskEnabled { path: path.into(), enabled },
            Launch::OnDemand,
            elevation::unit,
        )
    }

    fn page_combining(&self) -> AppResult<bool> {
        WinSys.page_combining()
    }

    fn set_page_combining(&self, enabled: bool) -> AppResult<()> {
        elevation::admin_or_elevated(|| WinSys.set_page_combining(enabled), Request::PageCombining { enabled }, |_| {}, elevation::unit)
    }

    fn net_setting(&self, key: NetKey) -> AppResult<String> {
        WinSys.net_setting(key)
    }

    fn set_net_setting(&self, key: NetKey, value: &str) -> AppResult<()> {
        elevation::admin_or_elevated(
            || WinSys.set_net_setting(key, value),
            Request::NetSetting { key, value: value.into() },
            |_| {},
            elevation::unit,
        )
    }

    // Arquivos do próprio usuário: nunca precisam de admin.
    fn json_file_exists(&self, env: &str, file: &str) -> bool {
        WinSys.json_file_exists(env, file)
    }

    fn json_key(&self, env: &str, file: &str, key: &str) -> AppResult<Option<String>> {
        WinSys.json_key(env, file, key)
    }

    fn set_json_key(&self, env: &str, file: &str, key: &str, value: Option<&str>) -> AppResult<()> {
        WinSys.set_json_key(env, file, key, value)
    }
}

impl GameSys for ElevatedSys {
    fn processes(&self) -> Vec<(u32, String)> {
        WinSys.processes()
    }

    fn image_path(&self, pid: u32) -> Option<String> {
        WinSys.image_path(pid)
    }

    fn set_timer_hold(&self, on: bool) {
        WinSys.set_timer_hold(on)
    }

    fn priority(&self, pid: u32) -> Option<u32> {
        WinSys.priority(pid)
    }

    fn set_priority(&self, pid: u32, class: u32) -> AppResult<()> {
        WinSys.set_priority(pid, class)
    }

    fn service_running(&self, name: &str) -> Option<bool> {
        WinSys.service_running(name)
    }

    fn pause_service(&self, name: &str) -> AppResult<()> {
        retry(WinSys.pause_service(name), Hive::Hklm, || Request::StopService { name: name.into() }, Launch::IfRunning)
    }

    fn resume_service(&self, name: &str) -> AppResult<()> {
        retry(WinSys.resume_service(name), Hive::Hklm, || Request::StartService { name: name.into() }, Launch::IfRunning)
    }
}

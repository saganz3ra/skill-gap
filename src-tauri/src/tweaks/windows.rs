//! Implementação real do `Sys` no Windows.

use std::io::ErrorKind;
use std::os::windows::process::CommandExt;
use std::process::Command;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE};
use winreg::RegKey;

use std::mem::zeroed;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus, StartServiceW, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS,
    SERVICE_STOP,
};
use windows_sys::Win32::System::Threading::{
    GetPriorityClass, OpenProcess, SetPriorityClass, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
};

use super::ops::{is_guid_plain, is_task_path, parse_qh_ac, parse_task_enabled, GameSys, Hive, NetKey, Sys};
use crate::error::{AppError, AppResult};
use crate::power::{self, PowerPlan};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct WinSys;

fn root(hive: Hive) -> RegKey {
    RegKey::predef(match hive {
        Hive::Hkcu => HKEY_CURRENT_USER,
        Hive::Hklm => HKEY_LOCAL_MACHINE,
    })
}

fn sc(args: &[&str]) {
    // Melhor esforço: parar/iniciar serviço é complemento; o que vale é o tipo de
    // inicialização gravado no registro.
    let _ = Command::new("sc.exe").args(args).creation_flags(CREATE_NO_WINDOW).status();
}

/// Abre a chave pra escrita. `create`: cria se não existir (pra gravar);
/// sem `create`, chave inexistente = nada a apagar.
fn writable(hive: Hive, path: &str, create: bool) -> AppResult<Option<RegKey>> {
    if create {
        let (key, _) = root(hive).create_subkey_with_flags(path, KEY_SET_VALUE).map_err(AppError::from_io)?;
        return Ok(Some(key));
    }
    match root(hive).open_subkey_with_flags(path, KEY_SET_VALUE) {
        Ok(k) => Ok(Some(k)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::from_io(e)),
    }
}

fn delete_value(key: Option<RegKey>, name: &str) -> AppResult<()> {
    let Some(key) = key else { return Ok(()) };
    match key.delete_value(name) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        other => other.map_err(AppError::from_io),
    }
}

impl Sys for WinSys {
    fn dword(&self, hive: Hive, path: &str, name: &str) -> Option<u32> {
        root(hive).open_subkey(path).ok()?.get_value::<u32, _>(name).ok()
    }

    fn set_dword(&self, hive: Hive, path: &str, name: &str, value: Option<u32>) -> AppResult<()> {
        match value {
            Some(v) => writable(hive, path, true)?.expect("criada").set_value(name, &v).map_err(AppError::from_io),
            None => delete_value(writable(hive, path, false)?, name),
        }
    }

    fn string(&self, hive: Hive, path: &str, name: &str) -> Option<String> {
        root(hive).open_subkey(path).ok()?.get_value::<String, _>(name).ok()
    }

    fn set_string(&self, hive: Hive, path: &str, name: &str, value: Option<&str>) -> AppResult<()> {
        match value {
            Some(v) => writable(hive, path, true)?
                .expect("criada")
                .set_value(name, &v.to_string())
                .map_err(AppError::from_io),
            None => delete_value(writable(hive, path, false)?, name),
        }
    }

    fn key_exists(&self, hive: Hive, path: &str) -> bool {
        root(hive).open_subkey(path).is_ok()
    }

    fn delete_key_tree(&self, hive: Hive, path: &str) -> AppResult<()> {
        match root(hive).delete_subkey_all(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other.map_err(AppError::from_io),
        }
    }

    fn stop_service(&self, name: &str) {
        sc(&["stop", name]);
    }

    fn start_service(&self, name: &str) {
        sc(&["start", name]);
    }

    fn active_plan(&self) -> AppResult<(String, Option<PowerPlan>)> {
        let p = power::active()?;
        Ok((p.guid, p.plan))
    }

    fn set_plan(&self, plan: PowerPlan) -> AppResult<()> {
        power::set_active(plan)
    }

    fn set_plan_guid(&self, guid: &str) -> AppResult<()> {
        power::set_active_guid(guid)
    }

    fn mouse(&self) -> AppResult<[i32; 3]> {
        crate::gaming::mouse_get()
    }

    fn set_mouse(&self, values: [i32; 3]) -> AppResult<()> {
        crate::gaming::mouse_set(values)
    }

    fn mouse_speed(&self) -> AppResult<u32> {
        crate::gaming::mouse_speed_get()
    }

    fn set_mouse_speed(&self, value: u32) -> AppResult<()> {
        crate::gaming::mouse_speed_set(value)
    }

    fn memory_compression(&self) -> AppResult<bool> {
        mm_agent().map(|(compression, _)| compression)
    }

    fn set_memory_compression(&self, enabled: bool) -> AppResult<()> {
        let cmd = if enabled { "Enable-MMAgent -MemoryCompression" } else { "Disable-MMAgent -MemoryCompression" };
        crate::system::powershell(cmd).map(|_| ())
    }

    fn power_value(&self, sub: &str, setting: &str) -> AppResult<Option<(String, u32)>> {
        if !is_guid_plain(sub) || !is_guid_plain(setting) {
            return Err(AppError::InvalidInput("GUID de energia inválido".into()));
        }
        let plan = super::readcache::get("power-active", || power::active().map(|p| p.guid).map_err(|e| e.to_string()))
            .map_err(AppError::Unavailable)?;
        let out = tool_cached("powercfg", &["/qh", &plan, sub, setting])?;
        // Configuração inexistente: o powercfg falha (ou não imprime os índices).
        Ok(out.ok().and_then(|text| parse_qh_ac(&text)).map(|v| (plan, v)))
    }

    fn set_power_value(&self, plan: &str, sub: &str, setting: &str, ac: u32) -> AppResult<()> {
        if !is_guid_plain(plan) || !is_guid_plain(sub) || !is_guid_plain(setting) {
            return Err(AppError::InvalidInput("GUID de energia inválido".into()));
        }
        let value = ac.to_string();
        must(tool("powercfg", &["/setacvalueindex", plan, sub, setting, &value])?, "powercfg")?;
        // Só reativa se for o plano ativo: valor no plano ativo só vale depois do /setactive.
        if power::active()?.guid.eq_ignore_ascii_case(plan) {
            must(tool("powercfg", &["/setactive", plan])?, "powercfg")?;
        }
        Ok(())
    }

    fn task_enabled(&self, path: &str) -> Option<bool> {
        if !is_task_path(path) {
            return None;
        }
        tool_cached("schtasks", &["/Query", "/TN", path, "/XML"]).ok()?.ok().and_then(|xml| parse_task_enabled(&xml))
    }

    fn set_task_enabled(&self, path: &str, enabled: bool) -> AppResult<()> {
        if !is_task_path(path) {
            return Err(AppError::InvalidInput("caminho de tarefa inválido".into()));
        }
        must(tool("schtasks", &["/Change", "/TN", path, if enabled { "/ENABLE" } else { "/DISABLE" }])?, "schtasks")
    }

    fn page_combining(&self) -> AppResult<bool> {
        mm_agent().map(|(_, combining)| combining)
    }

    fn set_page_combining(&self, enabled: bool) -> AppResult<()> {
        let cmd = if enabled { "Enable-MMAgent -PageCombining" } else { "Disable-MMAgent -PageCombining" };
        crate::system::powershell(cmd).map(|_| ())
    }

    fn net_setting(&self, key: NetKey) -> AppResult<String> {
        let script = key.get_script();
        let out = super::readcache::get(&format!("ps:{script}"), || crate::system::powershell(script).map_err(|e| e.to_string()))
            .map_err(|e| AppError::CommandFailed { command: "PowerShell".into(), detail: e })?;
        key.canonical(&out)
            .map(str::to_string)
            .ok_or_else(|| AppError::Unavailable(format!("este Windows não informa {key:?}")))
    }

    fn set_net_setting(&self, key: NetKey, value: &str) -> AppResult<()> {
        let script = key.set_script(value).ok_or_else(|| AppError::InvalidInput(format!("valor fora da lista: {value}")))?;
        crate::system::powershell(&script).map(|_| ())
    }

    fn json_file_exists(&self, env: &str, file: &str) -> bool {
        super::jsonfile::resolve(env, file).is_some_and(|p| p.is_file())
    }

    fn json_key(&self, env: &str, file: &str, key: &str) -> AppResult<Option<String>> {
        let path = super::jsonfile::resolve(env, file).ok_or_else(|| AppError::InvalidInput(file.into()))?;
        super::jsonfile::get(&path, key)
    }

    fn set_json_key(&self, env: &str, file: &str, key: &str, value: Option<&str>) -> AppResult<()> {
        let path = super::jsonfile::resolve(env, file).ok_or_else(|| AppError::InvalidInput(file.into()))?;
        super::jsonfile::set(&path, key, value)
    }
}

/// As duas opções do Get-MMAgent numa leitura só (cada PowerShell custa ~0,5 s).
fn mm_agent() -> AppResult<(bool, bool)> {
    super::readcache::get("mmagent", || {
        crate::system::powershell("$m = Get-MMAgent; \"$($m.MemoryCompression) $($m.PageCombining)\"").map_err(|e| e.to_string())
    })
    .map_err(|e| AppError::CommandFailed { command: "PowerShell".into(), detail: e })
    .and_then(|out| {
        let mut it = out.split_whitespace().map(|w| w.eq_ignore_ascii_case("true"));
        match (it.next(), it.next()) {
            (Some(a), Some(b)) => Ok((a, b)),
            _ => Err(AppError::Unavailable("Get-MMAgent sem resposta".into())),
        }
    })
}

/// `tool` para leituras, aproveitando o resultado dentro de uma rodada de leitura.
fn tool_cached(program: &str, args: &[&str]) -> AppResult<Result<String, String>> {
    let key = format!("{program} {}", args.join(" "));
    super::readcache::get(&key, || tool(program, args).map_err(|e| e.to_string())).map_err(AppError::Unavailable)
}

/// Roda uma ferramenta do Windows. `Ok(Err(texto))` = rodou e falhou (o texto ajuda a
/// decidir); `Err` = nem conseguiu rodar.
fn tool(program: &str, args: &[&str]) -> AppResult<Result<String, String>> {
    let out = Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output()?;
    let text = crate::system::decode_console(&out.stdout);
    if out.status.success() {
        Ok(Ok(text))
    } else {
        let err = crate::system::decode_console(&out.stderr);
        Ok(Err(if err.trim().is_empty() { text } else { err }))
    }
}

/// Falha de ferramenta vira erro tipado; "acesso negado" vira RequiresAdmin (o
/// `ElevatedSys` repete no auxiliar elevado).
fn must(r: Result<String, String>, program: &str) -> AppResult<()> {
    match r {
        Ok(_) => Ok(()),
        Err(text) => {
            let lower = text.to_lowercase();
            if lower.contains("acesso negado") || lower.contains("access is denied") || lower.contains("0x80070005") {
                Err(AppError::RequiresAdmin)
            } else {
                Err(AppError::CommandFailed { command: program.into(), detail: text.trim().to_string() })
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Handle de serviço que fecha sozinho (RAII): impossível esquecer o CloseServiceHandle.
struct ScHandle(SC_HANDLE);

impl Drop for ScHandle {
    fn drop(&mut self) {
        // SAFETY: handle válido, fechado uma única vez.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn open_service(name: &str, access: u32) -> AppResult<(ScHandle, ScHandle)> {
    // SAFETY: nomes UTF-16 terminados em zero; handles nulos são tratados como erro.
    unsafe {
        let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return Err(AppError::from_io(std::io::Error::last_os_error()));
        }
        let scm = ScHandle(scm);
        let svc = OpenServiceW(scm.0, wide(name).as_ptr(), access);
        if svc.is_null() {
            return Err(AppError::from_io(std::io::Error::last_os_error()));
        }
        Ok((scm, ScHandle(svc)))
    }
}

/// Handle de processo que fecha sozinho.
struct ProcHandle(windows_sys::Win32::Foundation::HANDLE);

impl Drop for ProcHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open_process(pid: u32, access: u32) -> Option<ProcHandle> {
    // SAFETY: OpenProcess devolve nulo em caso de erro; nunca usamos handle nulo.
    let h = unsafe { OpenProcess(access, 0, pid) };
    (!h.is_null()).then_some(ProcHandle(h))
}

impl GameSys for WinSys {
    fn processes(&self) -> Vec<(u32, String)> {
        crate::gaming::process_list()
    }

    fn image_path(&self, pid: u32) -> Option<String> {
        crate::gaming::image_path(pid)
    }

    fn set_timer_hold(&self, on: bool) {
        crate::timer::hold(on)
    }

    fn priority(&self, pid: u32) -> Option<u32> {
        // Só leitura limitada: o mesmo acesso que o Gerenciador de Tarefas usa.
        let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let class = unsafe { GetPriorityClass(h.0) };
        (class != 0).then_some(class)
    }

    fn set_priority(&self, pid: u32, class: u32) -> AppResult<()> {
        let h = open_process(pid, PROCESS_SET_INFORMATION)
            .ok_or_else(|| AppError::from_io(std::io::Error::last_os_error()))?;
        if unsafe { SetPriorityClass(h.0, class) } == 0 {
            return Err(AppError::from_io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn service_running(&self, name: &str) -> Option<bool> {
        let (_scm, svc) = open_service(name, SERVICE_QUERY_STATUS).ok()?;
        let mut status: SERVICE_STATUS = unsafe { zeroed() };
        if unsafe { QueryServiceStatus(svc.0, &mut status) } == 0 {
            return None;
        }
        Some(status.dwCurrentState == SERVICE_RUNNING)
    }

    fn pause_service(&self, name: &str) -> AppResult<()> {
        let (_scm, svc) = open_service(name, SERVICE_STOP)?;
        let mut status: SERVICE_STATUS = unsafe { zeroed() };
        if unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status) } == 0 {
            return Err(AppError::from_io(std::io::Error::last_os_error()));
        }
        Ok(())
    }

    fn resume_service(&self, name: &str) -> AppResult<()> {
        let (_scm, svc) = open_service(name, SERVICE_START)?;
        if unsafe { StartServiceW(svc.0, 0, std::ptr::null()) } == 0 {
            let err = std::io::Error::last_os_error();
            // 1056 = ERROR_SERVICE_ALREADY_RUNNING: o objetivo já foi atingido.
            if err.raw_os_error() == Some(1056) {
                return Ok(());
            }
            return Err(AppError::from_io(err));
        }
        Ok(())
    }
}

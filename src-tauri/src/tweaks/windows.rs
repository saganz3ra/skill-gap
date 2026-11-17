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

use super::ops::{GameSys, Hive, Sys};
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
        let out = crate::system::powershell("(Get-MMAgent).MemoryCompression")?;
        Ok(out.trim().eq_ignore_ascii_case("true"))
    }

    fn set_memory_compression(&self, enabled: bool) -> AppResult<()> {
        let cmd = if enabled { "Enable-MMAgent -MemoryCompression" } else { "Disable-MMAgent -MemoryCompression" };
        crate::system::powershell(cmd).map(|_| ())
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

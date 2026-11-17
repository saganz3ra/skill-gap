//! Lado Windows do auxiliar: abrir com UAC, pipe nomeado protegido e execução.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, WaitNamedPipeW,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessId, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW};

use super::{check, Launch, Reply, Request};
use crate::error::{AppError, AppResult};

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const ERROR_CANCELLED: u32 = 1223;
const ERROR_PIPE_CONNECTED: u32 = 535;
const PIPE_PREFIX: &str = r"\\.\pipe\skillgap-elev-";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> AppError {
    AppError::from_io(std::io::Error::last_os_error())
}

// ======================= Lado do app (cliente) =======================

struct Conn {
    reader: BufReader<File>,
    writer: File,
}

static CONN: Mutex<Option<Conn>> = Mutex::new(None);

pub fn is_running() -> bool {
    CONN.try_lock().map(|c| c.is_some()).unwrap_or(true)
}

/// 128 bits aleatórios do gerador do sistema, em hexadecimal.
fn random_hex() -> AppResult<String> {
    let mut buf = [0u8; 16];
    let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status != 0 {
        return Err(AppError::Helper(format!("gerador aleatório falhou: 0x{status:08X}")));
    }
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// SID do usuário que roda o app (ex.: S-1-5-21-...). O auxiliar libera o pipe só pra ele.
fn current_user_sid() -> AppResult<String> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_error());
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
        CloseHandle(token);
        if ok == 0 {
            return Err(last_error());
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut wstr: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut wstr) == 0 {
            return Err(last_error());
        }
        let mut n = 0;
        while *wstr.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(wstr, n));
        LocalFree(wstr.cast());
        Ok(sid)
    }
}

fn launch() -> AppResult<Conn> {
    let pipe = format!("{PIPE_PREFIX}{}", random_hex()?);
    let exe = std::env::current_exe()?;
    let params = format!("--elevated-helper {pipe} {} {}", unsafe { GetCurrentProcessId() }, current_user_sid()?);

    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let args = wide(&params);
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = args.as_ptr();
    info.nShow = 0; // SW_HIDE: o auxiliar não tem janela.

    // Aqui o Windows mostra o UAC e espera a resposta do usuário.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(if unsafe { GetLastError() } == ERROR_CANCELLED { AppError::ElevationDenied } else { last_error() });
    }
    let helper_pid = unsafe { GetProcessId(info.hProcess) };
    unsafe { CloseHandle(info.hProcess) };

    // Espera o auxiliar criar o pipe (normalmente < 1 s).
    let name = wide(&pipe);
    let deadline = Instant::now() + Duration::from_secs(15);
    let handle = loop {
        if unsafe { WaitNamedPipeW(name.as_ptr(), 500) } != 0 {
            let h = unsafe {
                CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut())
            };
            if h != INVALID_HANDLE_VALUE {
                break h;
            }
        }
        if Instant::now() > deadline {
            return Err(AppError::Helper("o auxiliar de administrador não respondeu".into()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    // Confere que do outro lado está o processo que NÓS abrimos, não um impostor.
    let mut server_pid = 0u32;
    if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) } == 0 || server_pid != helper_pid {
        unsafe { CloseHandle(handle) };
        return Err(AppError::Helper("o pipe do auxiliar não pertence ao processo esperado".into()));
    }

    let writer = unsafe { File::from_raw_handle(handle as _) };
    let reader = BufReader::new(writer.try_clone()?);
    Ok(Conn { reader, writer })
}

fn exchange(conn: &mut Conn, req: &Request, mut on_progress: impl FnMut(serde_json::Value)) -> std::io::Result<AppResult<serde_json::Value>> {
    let mut line = serde_json::to_string(req).expect("pedido serializável");
    line.push('\n');
    conn.writer.write_all(line.as_bytes())?;
    conn.writer.flush()?;
    loop {
        let mut buf = String::new();
        if conn.reader.read_line(&mut buf)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        match serde_json::from_str::<Reply>(&buf) {
            Ok(Reply::Progress(p)) => on_progress(p),
            Ok(Reply::Ok(v)) => return Ok(Ok(v)),
            Ok(Reply::Err { kind, message }) => return Ok(Err(Reply::into_error(&kind, message))),
            Err(e) => return Ok(Err(AppError::Helper(format!("resposta inválida do auxiliar: {e}")))),
        }
    }
}

/// Manda um pedido pro auxiliar, abrindo-o (com UAC) se preciso e permitido.
pub fn call(req: &Request, launch_mode: Launch, on_progress: impl FnMut(serde_json::Value)) -> AppResult<serde_json::Value> {
    // Confere aqui também: pedido fora da política nem chega a abrir o UAC.
    check(req).map_err(AppError::Helper)?;

    let mut guard = match launch_mode {
        Launch::OnDemand => CONN.lock().unwrap_or_else(|e| e.into_inner()),
        // Em segundo plano, não espera: se o auxiliar está ocupado (ex.: sfc de 20 min), desiste.
        Launch::IfRunning => CONN.try_lock().map_err(|_| AppError::RequiresAdmin)?,
    };
    if guard.is_none() {
        if launch_mode == Launch::IfRunning {
            return Err(AppError::RequiresAdmin);
        }
        *guard = Some(launch()?);
    }
    match exchange(guard.as_mut().expect("conectado"), req, on_progress) {
        Ok(result) => result,
        Err(_) => {
            // Auxiliar fechou (ou travou): esquece a conexão; a próxima ação abre outro.
            *guard = None;
            Err(AppError::Helper("o auxiliar de administrador foi fechado; tente de novo".into()))
        }
    }
}

// ======================= Lado do auxiliar (servidor) =======================

fn valid_args(pipe: &str, sid: &str) -> bool {
    pipe.strip_prefix(PIPE_PREFIX).is_some_and(|h| h.len() == 32 && h.chars().all(|c| c.is_ascii_hexdigit()))
        && sid.starts_with("S-1-")
        && sid.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 'S')
}

fn execute(req: Request, mut progress: impl FnMut(serde_json::Value)) -> AppResult<serde_json::Value> {
    use crate::tweaks::ops::{GameSys, Hive, Sys};
    use crate::tweaks::windows::WinSys;
    let unit = |r: AppResult<()>| r.map(|_| serde_json::Value::Null);
    match req {
        Request::SetDword { path, name, value } => unit(WinSys.set_dword(Hive::Hklm, &path, &name, value)),
        Request::SetString { path, name, value } => unit(WinSys.set_string(Hive::Hklm, &path, &name, value.as_deref())),
        Request::DeleteKeyTree { path } => unit(WinSys.delete_key_tree(Hive::Hklm, &path)),
        Request::StopService { name } => match WinSys.pause_service(&name) {
            Err(AppError::Io(e)) if e.raw_os_error() == Some(1062) => Ok(serde_json::Value::Null), // já parado
            r => unit(r),
        },
        Request::StartService { name } => unit(WinSys.resume_service(&name)),
        Request::SetStartupApproved { id, enabled } => unit(crate::startup::set_enabled(&id, enabled)),
        Request::MemoryCompression { enabled } => unit(WinSys.set_memory_compression(enabled)),
        Request::RestorePoint => Ok(serde_json::to_value(crate::restore::create()?).unwrap_or_default()),
        Request::DriversBackup => Ok(serde_json::to_value(crate::drivers::backup()?).unwrap_or_default()),
        Request::BootReport => Ok(serde_json::to_value(crate::bootperf::report()?).unwrap_or_default()),
        Request::RunFix { id } => {
            let fix = crate::fixes::find(&id).ok_or_else(|| AppError::NotFound(id.clone()))?;
            let report = crate::fixes::run(fix, |p| progress(serde_json::to_value(p).unwrap_or_default()));
            Ok(serde_json::to_value(report).unwrap_or_default())
        }
        Request::RestoreNetwork { adapter } => unit(crate::netcfg::apply(&adapter)),
        Request::PowerSetting { plan, sub, setting, value } => unit(WinSys.set_power_value(&plan, &sub, &setting, value)),
        Request::TaskEnabled { path, enabled } => unit(WinSys.set_task_enabled(&path, enabled)),
        Request::PageCombining { enabled } => unit(WinSys.set_page_combining(enabled)),
        Request::AppxManifest { name } => crate::apps::provisioned_manifest(&name).map(serde_json::Value::String),
        Request::MsiWrite { instance_id, value } => unit(crate::hardware::msi_write(&instance_id, value)),
        Request::QosSet { exe, enabled } => unit(crate::hardware::qos_set(&exe, enabled)),
        Request::QosNla { value } => unit(crate::hardware::nla_write(value.as_deref())),
        Request::NicWrite { adapter, keyword, value } => unit(crate::hardware::nic_write(&adapter, &keyword, &value)),
        Request::NetSetting { key, value } => unit(WinSys.set_net_setting(key, &value)),
        Request::CleanCategory { category } => Ok(serde_json::to_value(crate::cleanup::clean_one(category)).unwrap_or_default()),
    }
}

/// Ponto de entrada do executável quando aberto com `--elevated-helper <pipe> <pid> <sid>`.
pub fn helper_main(args: &[String]) -> i32 {
    let (Some(pipe), Some(pid), Some(sid)) = (args.first(), args.get(1).and_then(|p| p.parse::<u32>().ok()), args.get(2)) else {
        return 2;
    };
    if !valid_args(pipe, sid) || !crate::system::is_admin() {
        return 2;
    }

    // DACL: só SYSTEM, Administradores e o usuário do app. Rótulo de integridade "médio"
    // pra que o app (que roda sem elevação) consiga escrever no pipe de um processo elevado.
    let sddl = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{sid})S:(ML;;NW;;;ME)"));
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) } == 0 {
        return 3;
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };

    // FIRST_PIPE_INSTANCE: se alguém já criou um pipe com esse nome, falha em vez de
    // compartilhar. Uma instância só, e nada de clientes pela rede.
    let name = wide(pipe);
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            &sa,
        )
    };
    unsafe { LocalFree(sd) };
    if handle == INVALID_HANDLE_VALUE {
        return 4;
    }

    // Se o app não conectar em 30 s (fechou, travou), o auxiliar se encerra sozinho.
    let connected = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&connected);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        if !flag.load(Ordering::SeqCst) {
            std::process::exit(5);
        }
    });

    let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) } != 0 || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
    connected.store(true, Ordering::SeqCst);
    let mut client_pid = 0u32;
    if !ok || unsafe { GetNamedPipeClientProcessId(handle, &mut client_pid) } == 0 || client_pid != pid {
        // Só atende o processo que pediu a elevação.
        unsafe { CloseHandle(handle) };
        return 6;
    }

    let file = unsafe { File::from_raw_handle(handle as _) };
    let Ok(read_half) = file.try_clone() else { return 7 };
    let mut writer = file;
    let reader = BufReader::new(read_half);

    let mut send = |reply: &Reply| -> bool {
        let mut line = serde_json::to_string(reply).expect("resposta serializável");
        line.push('\n');
        writer.write_all(line.as_bytes()).and_then(|_| writer.flush()).is_ok()
    };

    // Uma linha por pedido. Fim do arquivo = o app fechou: o auxiliar fecha junto.
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let reply = match serde_json::from_str::<Request>(&line) {
            Err(e) => Reply::Err { kind: "invalidInput".into(), message: format!("pedido inválido: {e}") },
            Ok(req) => match check(&req) {
                Err(msg) => Reply::Err { kind: "invalidInput".into(), message: msg },
                Ok(()) => {
                    // Progresso sai em tempo real (ex.: "passo 1 de 3" de uma correção).
                    let result = execute(req, |p| {
                        send(&Reply::Progress(p));
                    });
                    Reply::from_result(result)
                }
            },
        };
        if !send(&reply) {
            break;
        }
    }
    0
}

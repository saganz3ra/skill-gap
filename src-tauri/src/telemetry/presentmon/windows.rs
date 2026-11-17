//! Download verificado, grupo de log de desempenho e captura.

use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use super::{frametime_column, parse_frametime, Frames, PERF_LOG_USERS, SHA256, URL};
use crate::error::{AppError, AppResult};
use crate::system::powershell;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static CHILD: Mutex<Option<(String, Child)>> = Mutex::new(None);

fn tools_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(r"app.skillgap.desktop\tools")
}

fn exe_path() -> PathBuf {
    tools_dir().join("PresentMon.exe")
}

fn q(p: &std::path::Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "''"))
}

pub fn installed() -> bool {
    exe_path().is_file()
}

/// "HASH|Status|Subject" do arquivo.
fn verify(path: &std::path::Path) -> AppResult<()> {
    let out = powershell(&format!(
        "$h = (Get-FileHash -LiteralPath {p} -Algorithm SHA256).Hash; $s = Get-AuthenticodeSignature -LiteralPath {p}; \"$h|$($s.Status)|$($s.SignerCertificate.Subject)\"",
        p = q(path)
    ))?;
    let parts: Vec<&str> = out.trim().splitn(3, '|').collect();
    let ok = parts.len() == 3 && parts[0].eq_ignore_ascii_case(SHA256) && parts[1] == "Valid" && parts[2].contains("O=Intel Corporation");
    if ok {
        Ok(())
    } else {
        Err(AppError::CommandFailed {
            command: "Verificação do PresentMon".into(),
            detail: "o arquivo baixado não bate com o hash publicado ou não tem a assinatura da Intel; nada foi executado".into(),
        })
    }
}

/// Baixa a versão fixa, confere hash e assinatura e só então coloca no lugar.
pub fn install() -> AppResult<()> {
    let dir = tools_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join("PresentMon.download");
    let _ = std::fs::remove_file(&tmp);
    powershell(&format!("$ProgressPreference = 'SilentlyContinue'; Invoke-WebRequest -Uri '{URL}' -OutFile {} -UseBasicParsing", q(&tmp)))?;
    match verify(&tmp) {
        Ok(()) => {
            std::fs::rename(&tmp, exe_path())?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Sessão atual do Windows já tem o grupo "Usuários de log de desempenho".
pub fn in_perf_group() -> bool {
    Command::new("whoami")
        .args(["/groups", "/fo", "csv"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(PERF_LOG_USERS))
}

fn sid_ok(sid: &str) -> bool {
    sid.starts_with("S-1-5-21-") && sid.chars().all(|c| c.is_ascii_digit() || c == '-' || c == 'S') && sid.len() < 100
}

/// No auxiliar elevado: põe/tira o usuário (pelo SID) do grupo.
pub fn set_perf_group(sid: &str, add: bool) -> AppResult<()> {
    if !sid_ok(sid) {
        return Err(AppError::InvalidInput("SID inválido".into()));
    }
    let verb = if add { "Add-LocalGroupMember" } else { "Remove-LocalGroupMember" };
    powershell(&format!(
        "{verb} -SID '{PERF_LOG_USERS}' -Member ([System.Security.Principal.SecurityIdentifier]'{sid}') -ErrorAction SilentlyContinue"
    ))
    .map(|_| ())
}

/// Começa a medir um jogo (nome do exe simples). Confere o arquivo de novo antes.
pub fn start(exe: &str, frames: Arc<Frames>) -> AppResult<()> {
    if crate::hardware::qos_name(exe).is_none() {
        return Err(AppError::InvalidInput("nome de executável inválido".into()));
    }
    let mut slot = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((name, child)) = slot.as_mut() {
        if name.eq_ignore_ascii_case(exe) && child.try_wait().ok().flatten().is_none() {
            return Ok(());
        }
        let _ = child.kill();
        *slot = None;
    }
    let path = exe_path();
    verify(&path)?;
    frames.clear();
    let mut child = Command::new(&path)
        .args(["--process_name", exe, "--output_stdout", "--stop_existing_session", "--session_name", "SkillGap", "--terminate_on_proc_exit"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| AppError::Helper("PresentMon sem saída".into()))?;
    std::thread::spawn(move || {
        let mut col = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            match col {
                None => col = frametime_column(&line),
                Some(c) => {
                    if let Some(ms) = parse_frametime(&line, c) {
                        frames.push(ms);
                    }
                }
            }
        }
    });
    *slot = Some((exe.to_string(), child));
    Ok(())
}

pub fn stop() {
    if let Some((_, mut child)) = CHILD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = child.kill();
    }
}

pub fn capturing() -> Option<String> {
    let mut slot = CHILD.lock().unwrap_or_else(|e| e.into_inner());
    let alive = slot.as_mut().is_some_and(|(_, c)| c.try_wait().ok().flatten().is_none());
    if !alive {
        *slot = None;
    }
    slot.as_ref().map(|(n, _)| n.clone())
}

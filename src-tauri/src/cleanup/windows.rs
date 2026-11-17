use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};

use super::CleanupCategory;
use crate::error::{AppError, AppResult};

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn system_root() -> PathBuf {
    env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
}

/// Pastas de cache de cada perfil dos navegadores (Chromium: um por perfil).
fn browser_caches() -> Vec<PathBuf> {
    let Some(local) = env_dir("LOCALAPPDATA") else { return Vec::new() };
    let mut out = Vec::new();
    for base in [r"Google\Chrome\User Data", r"Microsoft\Edge\User Data", r"BraveSoftware\Brave-Browser\User Data"] {
        let Ok(profiles) = std::fs::read_dir(local.join(base)) else { continue };
        for p in profiles.flatten().map(|e| e.path()) {
            out.push(p.join(r"Cache\Cache_Data"));
            out.push(p.join("Code Cache"));
        }
    }
    if let Ok(profiles) = std::fs::read_dir(local.join(r"Mozilla\Firefox\Profiles")) {
        out.extend(profiles.flatten().map(|e| e.path().join("cache2")));
    }
    out
}

/// Onde fica cada categoria neste PC (variáveis de ambiente, não caminhos fixos:
/// o Windows pode estar em outra unidade ou com o perfil redirecionado).
pub fn folders(category: CleanupCategory) -> Vec<PathBuf> {
    let local = env_dir("LOCALAPPDATA");
    match category {
        CleanupCategory::UserTemp => vec![std::env::temp_dir()],
        CleanupCategory::WindowsTemp => vec![system_root().join("Temp")],
        CleanupCategory::WindowsUpdateCache => vec![system_root().join(r"SoftwareDistribution\Download")],
        CleanupCategory::ShaderCache => local.map(|d| d.join("D3DSCache")).into_iter().collect(),
        CleanupCategory::RecycleBin => Vec::new(),
        CleanupCategory::BrowserCache => browser_caches(),
        CleanupCategory::CrashDumps => local
            .map(|d| vec![d.join("CrashDumps"), d.join(r"Microsoft\Windows\WER\ReportArchive"), d.join(r"Microsoft\Windows\WER\ReportQueue")])
            .unwrap_or_default(),
        CleanupCategory::SystemDumps => vec![system_root().join("Minidump")],
        CleanupCategory::ThumbnailCache => local.map(|d| d.join(r"Microsoft\Windows\Explorer")).into_iter().collect(),
    }
}

/// Programa que precisa estar fechado pra limpar a categoria.
pub fn blocked_by(category: CleanupCategory) -> Option<&'static str> {
    if category != CleanupCategory::BrowserCache {
        return None;
    }
    let running = crate::gaming::process_list();
    [("chrome.exe", "Chrome"), ("msedge.exe", "Edge"), ("brave.exe", "Brave"), ("firefox.exe", "Firefox")]
        .into_iter()
        .find(|(exe, _)| running.iter().any(|(_, n)| n.eq_ignore_ascii_case(exe)))
        .map(|(_, name)| name)
}

/// String Rust → UTF-16 terminada em zero, que é o que a API Win32 espera.
fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(once(0)).collect()
}

fn hresult(hr: i32, what: &str) -> AppResult<()> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(AppError::CommandFailed { command: what.into(), detail: format!("HRESULT 0x{:08X}", hr as u32) })
    }
}

/// (bytes, itens) somando a Lixeira de todas as unidades.
pub fn recycle_bin_query() -> AppResult<(u64, u64)> {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
    // SAFETY: `info` é uma struct válida com cbSize preenchido; caminho nulo = todas as unidades.
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    hresult(hr, "SHQueryRecycleBinW")?;
    Ok((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

pub fn recycle_bin_empty() -> AppResult<()> {
    // SAFETY: sem janela-pai, caminho nulo = todas as unidades; flags evitam diálogo e som.
    let hr = unsafe {
        SHEmptyRecycleBinW(
            std::ptr::null_mut(),
            std::ptr::null(),
            SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
        )
    };
    hresult(hr, "SHEmptyRecycleBinW")
}

/// Espaço livre na unidade do Windows (normalmente C:).
pub fn system_drive_free() -> Option<u64> {
    let drive = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
    let mut root = drive;
    root.push("\\");
    let path = wide(&root);
    let mut free_to_caller = 0u64;
    // SAFETY: `path` é UTF-16 terminado em zero; ponteiros de saída nulos são permitidos.
    let ok = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free_to_caller, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free_to_caller)
}

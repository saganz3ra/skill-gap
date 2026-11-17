use std::ffi::c_void;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_TEST, CDS_UPDATEREGISTRY, DEVMODEW,
    DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, DISP_CHANGE_RESTART,
    DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS,
};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETMOUSE, SPI_GETMOUSESPEED, SPI_SETMOUSE,
    SPI_SETMOUSESPEED,
};

use super::{best_refresh, Display, Mode};
use crate::error::{AppError, AppResult};

/// Array UTF-16 de tamanho fixo (terminado em zero) → String.
fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn new_devmode() -> DEVMODEW {
    // SAFETY: DEVMODEW é uma struct C "plain old data"; zerada é um valor válido.
    let mut dm: DEVMODEW = unsafe { zeroed() };
    dm.dmSize = size_of::<DEVMODEW>() as u16;
    dm
}

fn mode_of(dm: &DEVMODEW) -> Mode {
    Mode { width: dm.dmPelsWidth, height: dm.dmPelsHeight, hz: dm.dmDisplayFrequency }
}

/// Monitores ligados à área de trabalho, com o principal primeiro.
pub fn displays() -> Vec<Display> {
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        // SAFETY: estrutura zerada com `cb` preenchido, como a API exige.
        let mut adapter: DISPLAY_DEVICEW = unsafe { zeroed() };
        adapter.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut adapter, 0) } == 0 {
            break;
        }
        index += 1;
        if adapter.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let dev = adapter.DeviceName.as_ptr();

        let mut dm = new_devmode();
        // SAFETY: `dev` aponta pro nome terminado em zero dentro de `adapter`, vivo neste escopo.
        if unsafe { EnumDisplaySettingsW(dev, ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
            continue;
        }
        let current = mode_of(&dm);
        let bits = dm.dmBitsPerPel;

        // Todos os modos que o driver anuncia; só os com a mesma profundidade de cor contam.
        let mut modes = Vec::new();
        let mut i = 0;
        loop {
            let mut m = new_devmode();
            if unsafe { EnumDisplaySettingsW(dev, i, &mut m) } == 0 {
                break;
            }
            if m.dmBitsPerPel == bits {
                modes.push(mode_of(&m));
            }
            i += 1;
        }

        // Segundo nível do EnumDisplayDevices: o monitor ligado a esta saída.
        let mut monitor: DISPLAY_DEVICEW = unsafe { zeroed() };
        monitor.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        let name = if unsafe { EnumDisplayDevicesW(dev, 0, &mut monitor, 0) } != 0 {
            from_wide(&monitor.DeviceString)
        } else {
            from_wide(&adapter.DeviceString)
        };

        let display = Display { device: from_wide(&adapter.DeviceName), name, current, max_hz: best_refresh(&modes, current) };
        if adapter.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
            out.insert(0, display);
        } else {
            out.push(display);
        }
    }
    out
}

pub fn set_refresh(device: &str, hz: u32) -> AppResult<()> {
    let name = to_wide(device);
    let mut dm = new_devmode();
    // Parte do modo atual e troca só a frequência: resolução e cor ficam iguais.
    if unsafe { EnumDisplaySettingsW(name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
        return Err(AppError::NotFound(device.to_string()));
    }
    dm.dmDisplayFrequency = hz;
    dm.dmFields = DM_DISPLAYFREQUENCY;

    let apply = |flags: u32| unsafe { ChangeDisplaySettingsExW(name.as_ptr(), &dm, std::ptr::null_mut(), flags, std::ptr::null::<c_void>()) };

    // Testa antes de aplicar: se o driver recusar, a tela nem pisca.
    let test = apply(CDS_TEST);
    if test != DISP_CHANGE_SUCCESSFUL {
        return Err(AppError::CommandFailed { command: "ChangeDisplaySettingsEx (teste)".into(), detail: format!("código {test}") });
    }
    match apply(CDS_UPDATEREGISTRY) {
        DISP_CHANGE_SUCCESSFUL | DISP_CHANGE_RESTART => Ok(()),
        code => Err(AppError::CommandFailed { command: "ChangeDisplaySettingsEx".into(), detail: format!("código {code}") }),
    }
}

pub fn mouse_get() -> AppResult<[i32; 3]> {
    let mut v = [0i32; 3];
    // SAFETY: SPI_GETMOUSE escreve exatamente 3 inteiros no buffer.
    let ok = unsafe { SystemParametersInfoW(SPI_GETMOUSE, 0, v.as_mut_ptr().cast(), 0) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(v)
}

pub fn mouse_set(mut v: [i32; 3]) -> AppResult<()> {
    // UPDATEINIFILE grava no perfil (sobrevive ao reboot); SENDCHANGE avisa os apps abertos.
    let ok = unsafe { SystemParametersInfoW(SPI_SETMOUSE, 0, v.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Velocidade do ponteiro (1–20; 10 = padrão, sensibilidade 1:1).
pub fn mouse_speed_get() -> AppResult<u32> {
    let mut v: u32 = 0;
    let ok = unsafe { SystemParametersInfoW(SPI_GETMOUSESPEED, 0, (&mut v as *mut u32).cast(), 0) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(v)
}

pub fn mouse_speed_set(v: u32) -> AppResult<()> {
    // Detalhe da API: aqui o valor vai NO PRÓPRIO ponteiro (não é endereço de memória).
    let ok = unsafe {
        SystemParametersInfoW(SPI_SETMOUSESPEED, 0, v as usize as *mut core::ffi::c_void, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE)
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// (PID, nome do executável) de cada processo, via snapshot do Toolhelp.
///
/// Por que não abrir cada processo (OpenProcess) pra pegar o caminho completo? Porque
/// abrir handle no processo do jogo é exatamente o que um cheat faz, e o anticheat
/// vigia isso. O snapshot só lê a tabela de processos do sistema: nenhum handle é
/// aberto em processo nenhum.
pub fn process_list() -> Vec<(u32, String)> {
    // SAFETY: chamadas Toolhelp com struct de tamanho correto; o handle é fechado no fim.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut names = Vec::new();
        let mut ok = Process32FirstW(snap, &mut entry);
        while ok != 0 {
            names.push((entry.th32ProcessID, from_wide(&entry.szExeFile)));
            ok = Process32NextW(snap, &mut entry);
        }
        CloseHandle(snap);
        names
    }
}

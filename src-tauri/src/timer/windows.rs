//! Medição com QueryPerformanceCounter e resolução via ntdll (sem PowerShell).

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation, Sleep, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{mode_reaches_game, stats, TimerReport, FINE_100NS};

#[link(name = "ntdll")]
extern "system" {
    /// Unidades de 100 ns. Atenção aos nomes: "Maximum" é o intervalo MAIS LONGO (o mais grosso).
    fn NtQueryTimerResolution(maximum: *mut u32, minimum: *mut u32, current: *mut u32) -> i32;
    fn NtSetTimerResolution(desired: u32, set: u8, current: *mut u32) -> i32;
}

static HELD: AtomicBool = AtomicBool::new(false);

fn ms(units_100ns: u32) -> f64 {
    units_100ns as f64 / 10_000.0
}

fn global_requests() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\kernel")
        .and_then(|k| k.get_value::<u32, _>("GlobalTimerResolutionRequests"))
        .map(|v| v == 1)
        .unwrap_or(false)
}

/// Mede sem alterar nada (~120 × o tique atual: de 0,1 s a 2 s).
pub fn measure(build: u32) -> TimerReport {
    let (mut coarse, mut fine, mut current) = (0u32, 0u32, 0u32);
    let resolution_ok = unsafe { NtQueryTimerResolution(&mut coarse, &mut fine, &mut current) } >= 0;

    let mut freq = 0i64;
    let mut samples = Vec::with_capacity(120);
    if unsafe { QueryPerformanceFrequency(&mut freq) } != 0 && freq > 0 {
        for _ in 0..120 {
            let (mut a, mut b) = (0i64, 0i64);
            unsafe {
                QueryPerformanceCounter(&mut a);
                Sleep(1);
                QueryPerformanceCounter(&mut b);
            }
            samples.push((b - a) as f64 * 1000.0 / freq as f64);
        }
    }
    let global = global_requests();
    TimerReport {
        sleep: stats(&samples),
        current_ms: resolution_ok.then(|| ms(current)),
        finest_ms: resolution_ok.then(|| ms(fine)),
        coarsest_ms: resolution_ok.then(|| ms(coarse)),
        global_requests: global,
        mode_reaches_game: mode_reaches_game(build, global),
        held_by_app: HELD.load(Ordering::SeqCst),
    }
}

/// Segura (ou solta) 0,5 ms. Também pede ao Windows 11 pra NÃO ignorar o pedido deste
/// processo quando ele está minimizado/em segundo plano.
pub fn hold(on: bool) {
    if HELD.swap(on, Ordering::SeqCst) == on {
        return;
    }
    unsafe {
        if on {
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
                StateMask: 0, // 0 = sempre respeitar o pedido de resolução
            };
            let process: HANDLE = GetCurrentProcess();
            SetProcessInformation(
                process,
                ProcessPowerThrottling,
                (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            );
        }
        let mut current = 0u32;
        NtSetTimerResolution(FINE_100NS, on as u8, &mut current);
    }
}

pub fn is_held() -> bool {
    HELD.load(Ordering::SeqCst)
}

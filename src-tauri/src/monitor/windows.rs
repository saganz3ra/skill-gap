use std::mem::{size_of, zeroed};

use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_PRIMARY_DEVICE};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use super::{active_from_idle, gpu_total, HardwareInfo, Sample};
use crate::error::{AppError, AppResult};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Consulta PDH com os contadores do monitor. Os handles PDH são inteiros (isize),
/// então a struct pode ir pro estado compartilhado do Tauri sem `unsafe impl Send`.
pub struct Pdh {
    query: isize,
    cpu: isize,
    disk_idle: isize,
    disk_read: isize,
    disk_write: isize,
    gpu: Option<isize>,
    vram: Option<isize>,
    net_down: isize,
    net_up: isize,
    thermal: Option<isize>,
}

impl Pdh {
    pub fn open() -> AppResult<Self> {
        let mut query = 0isize;
        // SAFETY: fonte nula = dados ao vivo; o handle é fechado no Drop.
        let status = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) };
        if status != 0 {
            return Err(AppError::CommandFailed { command: "PdhOpenQuery".into(), detail: format!("0x{status:08X}") });
        }
        // "English" = nomes dos contadores em inglês: os nomes normais são traduzidos
        // ("\Processador(...)" no Windows em português) e quebrariam aqui.
        let add = |path: &str| -> Option<isize> {
            let mut counter = 0isize;
            let ok = unsafe { PdhAddEnglishCounterW(query, wide(path).as_ptr(), 0, &mut counter) } == 0;
            ok.then_some(counter)
        };
        let required = |path: &str| {
            add(path).ok_or_else(|| AppError::CommandFailed { command: "PdhAddEnglishCounter".into(), detail: path.into() })
        };
        let pdh = Self {
            query,
            cpu: required(r"\Processor Information(_Total)\% Processor Utility")?,
            disk_idle: required(r"\PhysicalDisk(_Total)\% Idle Time")?,
            disk_read: required(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec")?,
            disk_write: required(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec")?,
            // GPU: só existe com driver WDDM 2.x; sem ele, o monitor segue sem GPU.
            gpu: add(r"\GPU Engine(*engtype_3D)\Utilization Percentage"),
            vram: add(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            net_down: required(r"\Network Interface(*)\Bytes Received/sec")?,
            net_up: required(r"\Network Interface(*)\Bytes Sent/sec")?,
            // Kelvin. Nem toda placa-mãe expõe zona térmica.
            thermal: add(r"\Thermal Zone Information(*)\Temperature"),
        };
        // Primeira leitura: base pros contadores de taxa.
        unsafe { PdhCollectQueryData(pdh.query) };
        Ok(pdh)
    }

    fn value(&self, counter: isize) -> Option<f64> {
        // SAFETY: struct POD zerada, preenchida pela API.
        let mut v: PDH_FMT_COUNTERVALUE = unsafe { zeroed() };
        let status = unsafe { PdhGetFormattedCounterValue(counter, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut v) };
        (status == 0 && v.CStatus == PDH_CSTATUS_VALID_DATA).then(|| unsafe { v.Anonymous.doubleValue })
    }

    /// Valores de todas as instâncias de um contador com curinga (*).
    fn values(&self, counter: isize) -> Vec<f64> {
        let mut size = 0u32;
        let mut count = 0u32;
        // 1ª chamada: descobre o tamanho do buffer.
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut())
        };
        if status != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        // O buffer guarda os itens E os nomes das instâncias logo depois; aloca em
        // unidades de item pra manter o alinhamento correto.
        let item = size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(size as usize / item + 1);
        let status = unsafe {
            PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, buf.as_mut_ptr())
        };
        if status != 0 {
            return Vec::new();
        }
        // SAFETY: a API escreveu `count` itens válidos no início do buffer.
        unsafe { buf.set_len(count as usize) };
        buf.iter()
            .filter(|i| i.FmtValue.CStatus == PDH_CSTATUS_VALID_DATA)
            .map(|i| unsafe { i.FmtValue.Anonymous.doubleValue })
            .collect()
    }

    pub fn sample(&mut self) -> Sample {
        unsafe { PdhCollectQueryData(self.query) };
        let (ram_used, ram_total) = memory();
        Sample {
            cpu: self.value(self.cpu).unwrap_or(0.0).clamp(0.0, 100.0),
            ram_used,
            ram_total,
            gpu: self.gpu.map(|c| gpu_total(&self.values(c))),
            vram_used: self.vram.map(|c| self.values(c).iter().sum::<f64>() as u64),
            disk_active: self.value(self.disk_idle).map(active_from_idle).unwrap_or(0.0),
            disk_read: self.value(self.disk_read).unwrap_or(0.0),
            disk_write: self.value(self.disk_write).unwrap_or(0.0),
            net_down: self.values(self.net_down).iter().sum(),
            net_up: self.values(self.net_up).iter().sum(),
            cpu_temp: self
                .thermal
                .map(|c| self.values(c))
                .and_then(|v| v.into_iter().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))))
                .map(|k| k - 273.15)
                .filter(|c| (1.0..130.0).contains(c)),
        }
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query) };
    }
}

fn memory() -> (u64, u64) {
    let mut mem: MEMORYSTATUSEX = unsafe { zeroed() };
    mem.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut mem) } == 0 {
        return (0, 0);
    }
    (mem.ullTotalPhys - mem.ullAvailPhys, mem.ullTotalPhys)
}

fn primary_gpu() -> String {
    let mut i = 0;
    let mut first = String::new();
    loop {
        let mut dd: DISPLAY_DEVICEW = unsafe { zeroed() };
        dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        i += 1;
        let name = from_wide(&dd.DeviceString);
        if dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
            return name;
        }
        if first.is_empty() {
            first = name;
        }
    }
    first
}

pub fn hardware_info() -> HardwareInfo {
    let cpu = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
    let (cpu_name, cpu_mhz) = match cpu {
        Ok(k) => (
            k.get_value::<String, _>("ProcessorNameString").unwrap_or_default().trim().to_string(),
            k.get_value::<u32, _>("~MHz").unwrap_or(0),
        ),
        Err(_) => (String::new(), 0),
    };
    HardwareInfo {
        cpu_name,
        cpu_threads: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(0),
        cpu_mhz,
        gpu_name: primary_gpu(),
        ram_total: memory().1,
    }
}

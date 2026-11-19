//! Eco ICMP (IP Helper, sem processo externo) e temperatura da GPU NVIDIA.

use std::os::windows::process::CommandExt;
use std::process::Command;

use windows_sys::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Um eco com timeout de 800 ms. Some(RTT em ms) ou None (sem resposta).
pub fn echo(target: [u8; 4]) -> Option<f64> {
    unsafe {
        let h = IcmpCreateFile();
        if h.is_null() || h as isize == -1 {
            return None;
        }
        let data = *b"skillgap";
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
        // IPAddr em ordem de rede: os bytes do endereço na ordem em que se escreve.
        let addr = u32::from_ne_bytes(target);
        let n = IcmpSendEcho(
            h,
            addr,
            data.as_ptr().cast(),
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            800,
        );
        IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime as f64)
    }
}

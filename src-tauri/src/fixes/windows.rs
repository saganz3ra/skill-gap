use std::os::windows::process::CommandExt;
use std::process::Command;

use super::{clean_output, decode_output, Fix, FixProgress, FixReport, Step, StepResult};
use crate::tweaks::ops::GameSys;
use crate::tweaks::windows::WinSys;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LINES: usize = 25;

fn run_step(step: &Step) -> (bool, String) {
    match *step {
        Step::Cmd { program, args, .. } => match Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output() {
            Ok(out) => {
                let mut text = decode_output(&out.stdout);
                let err = decode_output(&out.stderr);
                if !err.trim().is_empty() {
                    text.push('\n');
                    text.push_str(&err);
                }
                (out.status.success(), clean_output(&text, MAX_LINES))
            }
            Err(e) => (false, e.to_string()),
        },
        Step::OptimizeSystemDrive { .. } => {
            // Tipo do disco lido do Windows na hora: TRIM em HD não faz nada e desfragmentar
            // SSD só gasta escrita.
            let mode = match crate::system::read_profile().system_disk {
                crate::system::DiskKind::Hdd => "-Defrag",
                _ => "-ReTrim",
            };
            let script = format!("Optimize-Volume -DriveLetter ($env:SystemDrive.Substring(0,1)) {mode} -Verbose 4>&1 | Out-String");
            match crate::system::powershell(&script) {
                Ok(out) => (true, clean_output(&out, MAX_LINES)),
                Err(e) => (false, e.to_string()),
            }
        }
        Step::StopService { name, .. } => match WinSys.pause_service(name) {
            Ok(()) => (true, format!("{name} parado.")),
            // 1062 = ERROR_SERVICE_NOT_ACTIVE: já estava parado, que é o objetivo.
            Err(crate::error::AppError::Io(e)) if e.raw_os_error() == Some(1062) => (true, format!("{name} já estava parado.")),
            Err(e) => (false, e.to_string()),
        },
        Step::StartService { name, .. } => match WinSys.resume_service(name) {
            Ok(()) => (true, format!("{name} ligado.")),
            Err(e) => (false, e.to_string()),
        },
        Step::CleanUpdateCache { .. } => {
            let dir = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| r"C:\Windows".into())
                .join(r"SoftwareDistribution\Download");
            let stats = crate::cleanup::clean_dir(&dir, None);
            (
                true,
                format!(
                    "{} arquivos apagados ({} MB), {} em uso mantidos.",
                    stats.deleted_files,
                    stats.freed_bytes / (1024 * 1024),
                    stats.skipped_files
                ),
            )
        }
        Step::RestartExplorer { .. } => match crate::system::restart_explorer() {
            Ok(()) => (true, "Explorer reiniciado.".into()),
            Err(e) => (false, e.to_string()),
        },
    }
}

/// Roda os passos em ordem. Se um passo falhar, os seguintes são pulados, MENOS os de
/// "ligar serviço": um reparo que parou o Windows Update tem que religá-lo mesmo assim.
pub fn run(fix: &Fix, mut on_progress: impl FnMut(FixProgress)) -> FixReport {
    let total = fix.steps.len();
    let mut ok = true;
    let mut steps = Vec::with_capacity(total);
    for (i, step) in fix.steps.iter().enumerate() {
        let must_run = matches!(step, Step::StartService { .. });
        if !ok && !must_run {
            continue;
        }
        on_progress(FixProgress { id: fix.id, step: i + 1, total, label: step.label() });
        let (step_ok, output) = run_step(step);
        ok &= step_ok;
        steps.push(StepResult { label: step.label(), ok: step_ok, output });
    }
    FixReport { id: fix.id, ok, steps, restart: if ok { fix.restart } else { crate::tweaks::catalog::Restart::None } }
}

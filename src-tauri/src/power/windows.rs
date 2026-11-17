use std::os::windows::process::CommandExt;
use std::process::Command;

use super::plans::{parse_list, CustomPlan, EnergyState, EnergyStatus, PlanView};
use super::{is_guid, parse_active_scheme, ActivePlan, PowerPlan};
use crate::error::{AppError, AppResult};

/// Sem essa flag, cada chamada ao powercfg pisca uma janela de console na tela.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn powercfg(args: &[&str]) -> AppResult<String> {
    let out = Command::new("powercfg")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    // O powercfg escreve na code page OEM do console (CP850 no Windows em português).
    // O GUID é ASCII, então a conversão "lossy" não atrapalha a leitura dele.
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = if stderr.trim().is_empty() { stdout } else { stderr.into_owned() };
        Err(AppError::CommandFailed {
            command: format!("powercfg {}", args.join(" ")),
            detail: detail.trim().to_string(),
        })
    }
}

pub fn active() -> AppResult<ActivePlan> {
    let out = powercfg(&["/getactivescheme"])?;
    parse_active_scheme(&out).ok_or_else(|| AppError::CommandFailed {
        command: "powercfg /getactivescheme".into(),
        detail: format!("saída inesperada: {}", out.trim()),
    })
}

pub fn set_active(plan: PowerPlan) -> AppResult<()> {
    // Em notebooks com Modern Standby o Windows esconde o "Alto desempenho" e o
    // setactive falha. Próximo passo: recriá-lo com `powercfg -duplicatescheme <GUID>`.
    powercfg(&["/setactive", plan.alias()]).map(|_| ())
}

/// Restaura um plano pelo GUID (inclusive planos personalizados, que não têm alias).
pub fn set_active_guid(guid: &str) -> AppResult<()> {
    if !is_guid(guid) {
        return Err(AppError::InvalidInput(format!("GUID inválido: {guid}")));
    }
    powercfg(&["/setactive", guid]).map(|_| ())
}

fn installed_guids() -> Vec<String> {
    powercfg(&["/list"]).map(|o| parse_list(&o)).unwrap_or_default()
}

/// Cria (se ainda não existir) e configura um plano do Skill Gap. Devolve os ajustes
/// que este hardware não tem e foram pulados.
fn install(plan: CustomPlan) -> AppResult<Vec<&'static str>> {
    let guid = plan.guid();
    if !installed_guids().iter().any(|g| g == guid) {
        // Notebooks com Modern Standby não têm o "Alto desempenho": cai pro Balanceado.
        let copied = powercfg(&["-duplicatescheme", plan.base().guid(), guid])
            .or_else(|_| powercfg(&["-duplicatescheme", PowerPlan::Balanced.guid(), guid]));
        copied?;
    }
    powercfg(&["-changename", guid, plan.name(), plan.description()])?;

    let mut skipped = Vec::new();
    for s in plan.settings() {
        let ac = s.ac.to_string();
        let dc = s.dc.to_string();
        let ok_ac = powercfg(&["-setacvalueindex", guid, s.sub, s.setting, &ac]).is_ok();
        let ok_dc = powercfg(&["-setdcvalueindex", guid, s.sub, s.setting, &dc]).is_ok();
        if !(ok_ac && ok_dc) {
            skipped.push(s.what);
        }
    }
    Ok(skipped)
}

pub fn energy_status(state: &EnergyState) -> AppResult<EnergyStatus> {
    let active = active()?;
    let installed = installed_guids();
    let plans = CustomPlan::ALL
        .into_iter()
        .map(|p| PlanView {
            plan: p,
            name: p.name(),
            description: p.description(),
            changes: p.settings().iter().map(|s| s.what).collect(),
            installed: installed.iter().any(|g| g == p.guid()),
            active: active.guid.eq_ignore_ascii_case(p.guid()),
        })
        .collect();
    Ok(EnergyStatus {
        active_name: active.name,
        active_guid: active.guid,
        plans,
        can_revert: state.previous().is_some(),
        skipped: state.skipped(),
    })
}

pub fn apply_custom(state: &EnergyState, plan: CustomPlan) -> AppResult<EnergyStatus> {
    // Anota o plano atual ANTES de mexer.
    state.remember(&active()?.guid)?;
    let skipped = install(plan)?;
    powercfg(&["-setactive", plan.guid()])?;
    state.set_skipped(skipped);
    energy_status(state)
}

pub fn revert_custom(state: &EnergyState) -> AppResult<EnergyStatus> {
    let target = state.previous().unwrap_or_else(|| PowerPlan::Balanced.guid().to_string());
    set_active_guid(&target)?;
    // Plano ativo não pode ser apagado; por isso o setactive vem antes.
    for p in CustomPlan::ALL {
        let _ = powercfg(&["-delete", p.guid()]);
    }
    state.forget()?;
    state.set_skipped(Vec::new());
    energy_status(state)
}

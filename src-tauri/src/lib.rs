mod apps;
mod bootperf;
mod cleanup;
mod diagnosis;
mod drivers;
mod elevation;
mod error;
mod fixes;
mod gaming;
mod monitor;
mod netcfg;
mod news;
mod power;
mod restore;
mod startup;
mod system;
mod timer;
mod tweaks;

use error::{AppError, AppResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gaming::watcher::{WatchConfig, WatchStatus, Watcher};
use tauri::{Emitter, Manager};

/// Registro e powercfg são chamadas bloqueantes. Rodamos fora do runtime async
/// pra não segurar uma thread do Tokio (e, por tabela, a resposta da interface).
async fn blocking<T, F>(f: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::CommandFailed { command: "tarefa interna".into(), detail: e.to_string() })?
}

#[tauri::command]
async fn list_startup_items() -> AppResult<Vec<startup::StartupItem>> {
    blocking(startup::list).await
}

#[tauri::command]
async fn set_startup_item_enabled(id: String, enabled: bool) -> AppResult<()> {
    blocking(move || {
        elevation::local_or_elevated(
            || startup::set_enabled(&id, enabled),
            elevation::Request::SetStartupApproved { id: id.clone(), enabled },
            elevation::Launch::OnDemand,
            elevation::unit,
        )
    })
    .await
}

#[tauri::command]
async fn get_power_plan() -> AppResult<power::ActivePlan> {
    blocking(power::active).await
}

#[tauri::command]
async fn set_power_plan(plan: power::PowerPlan) -> AppResult<()> {
    blocking(move || power::set_active(plan)).await
}

#[tauri::command]
async fn scan_cleanup() -> AppResult<Vec<cleanup::CategoryScan>> {
    blocking(|| Ok(cleanup::scan())).await
}

/// Emite o evento "cleanup-progress" ao terminar cada categoria, pra interface
/// mostrar o andamento sem esperar a limpeza inteira acabar.
#[tauri::command]
async fn run_cleanup(
    app: tauri::AppHandle,
    categories: Vec<cleanup::CleanupCategory>,
) -> AppResult<cleanup::CleanupReport> {
    blocking(move || {
        let admin = system::is_admin();
        Ok(cleanup::clean_with(
            &categories,
            |c| {
                if c.requires_admin() && !admin {
                    // Pastas do sistema: limpa no auxiliar elevado (UAC na hora).
                    match elevation::call(&elevation::Request::CleanCategory { category: c }, elevation::Launch::OnDemand, |_| {})
                        .and_then(|v| serde_json::from_value(v).map_err(|e| AppError::Helper(e.to_string())))
                    {
                        Ok(r) => r,
                        Err(e) => cleanup::CategoryResult { category: c, freed_bytes: 0, deleted_files: 0, skipped_files: 0, error: Some(e.to_string()) },
                    }
                } else {
                    cleanup::clean_one(c)
                }
            },
            |result| {
                let _ = app.emit("cleanup-progress", result);
            },
        ))
        .inspect(|report: &cleanup::CleanupReport| app.state::<diagnosis::BaselineState>().add_cleaned(report.freed_bytes))
    })
    .await
}

#[tauri::command]
async fn gaming_diagnose(app: tauri::AppHandle) -> AppResult<gaming::GamingReport> {
    // tauri::State não é 'static, então pegamos o estado de dentro da thread via AppHandle.
    blocking(move || Ok(gaming::diagnose(&app.state::<gaming::GamingState>(), &engine(&app)))).await
}

#[tauri::command]
async fn gaming_fix(app: tauri::AppHandle, id: gaming::CheckId) -> AppResult<()> {
    blocking(move || gaming::fix(&app.state::<gaming::GamingState>(), &engine(&app), &sys_profile(&app, false), id)).await
}

#[tauri::command]
async fn gaming_revert(app: tauri::AppHandle, id: gaming::CheckId) -> AppResult<()> {
    blocking(move || gaming::revert(&app.state::<gaming::GamingState>(), &engine(&app), id)).await
}

#[tauri::command]
async fn game_watch_status(app: tauri::AppHandle) -> AppResult<WatchStatus> {
    Ok(app.state::<Arc<Watcher>>().status())
}

#[tauri::command]
async fn game_watch_save(app: tauri::AppHandle, config: WatchConfig) -> AppResult<WatchStatus> {
    blocking(move || {
        let w = app.state::<Arc<Watcher>>();
        w.save(config)?;
        Ok(w.status())
    })
    .await
}

/// Executáveis rodando agora (sem repetição), pra sugerir na hora de adicionar um jogo.
#[tauri::command]
async fn list_running_processes() -> AppResult<Vec<String>> {
    blocking(|| {
        let mut names: Vec<String> = tweaks::platform_game_sys().processes().into_iter().map(|(_, n)| n).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        Ok(names)
    })
    .await
}

// ---------------- Ajustes e perfis ----------------

fn engine(app: &tauri::AppHandle) -> tauri::State<'_, tweaks::TweakEngine> {
    app.state::<tweaks::TweakEngine>()
}

/// Fatos do PC, em cache (o tipo de disco custa um PowerShell).
fn sys_profile(app: &tauri::AppHandle, refresh: bool) -> system::SystemProfile {
    app.state::<system::SystemCache>().get(refresh)
}

#[tauri::command]
async fn tweaks_list(app: tauri::AppHandle) -> AppResult<Vec<tweaks::TweakView>> {
    blocking(move || Ok(engine(&app).list(&sys_profile(&app, false)))).await
}

/// `confirmed`: o usuário confirmou um ajuste de risco alto (conferido no backend).
#[tauri::command]
async fn tweak_apply(app: tauri::AppHandle, id: String, confirmed: Option<bool>) -> AppResult<()> {
    blocking(move || engine(&app).apply_checked(&id, &sys_profile(&app, false), confirmed.unwrap_or(false))).await
}

#[tauri::command]
async fn tweak_revert(app: tauri::AppHandle, id: String) -> AppResult<()> {
    blocking(move || engine(&app).revert(&id)).await
}

#[tauri::command]
async fn tweaks_revert_all(app: tauri::AppHandle) -> AppResult<tweaks::BatchReport> {
    blocking(move || Ok(engine(&app).revert_all())).await
}

#[tauri::command]
async fn tweak_set_favorite(app: tauri::AppHandle, id: String, favorite: bool) -> AppResult<()> {
    blocking(move || engine(&app).set_favorite(&id, favorite)).await
}

#[tauri::command]
async fn profiles_list(app: tauri::AppHandle) -> AppResult<Vec<tweaks::ProfileView>> {
    Ok(engine(&app).profiles())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileApplyResult {
    /// O que aconteceu com o ponto pedido (`None` = não pedido).
    restore_point: Option<restore::Outcome>,
    /// `None` = nada foi aplicado (proteção desligada com o ponto pedido).
    report: Option<tweaks::BatchReport>,
}

/// Aplica um perfil. Com `restore_point`, garante o ponto ANTES e aborta se não houver
/// rede de segurança: melhor não mexer do que mexer sem a proteção que o usuário pediu.
#[tauri::command]
async fn profile_apply(app: tauri::AppHandle, id: String, restore_point: bool) -> AppResult<ProfileApplyResult> {
    blocking(move || {
        let outcome = if restore_point { Some(restore_point_now(&app, false)?) } else { None };
        if outcome == Some(restore::Outcome::ProtectionOff) {
            return Ok(ProfileApplyResult { restore_point: outcome, report: None });
        }
        let report = engine(&app).apply_profile(&id, &sys_profile(&app, false))?;
        Ok(ProfileApplyResult { restore_point: outcome, report: Some(report) })
    })
    .await
}

#[tauri::command]
async fn profile_save(app: tauri::AppHandle, name: String) -> AppResult<tweaks::ProfileView> {
    blocking(move || engine(&app).save_profile(&name)).await
}

#[tauri::command]
async fn profile_delete(app: tauri::AppHandle, id: String) -> AppResult<()> {
    blocking(move || engine(&app).delete_profile(&id)).await
}

#[tauri::command]
/// `refresh`: botão "Reanalisar" (relê o hardware; o monitor é sempre lido na hora).
async fn system_info(app: tauri::AppHandle, refresh: Option<bool>) -> AppResult<system::SystemInfo> {
    blocking(move || {
        let profile = sys_profile(&app, refresh.unwrap_or(false));
        // Monitor principal: o primeiro da lista do Windows.
        let display = gaming::displays().into_iter().next().map(|d| (d.current.hz, d.max_hz));
        Ok(system::build_info(&profile, system::is_admin(), display))
    })
    .await
}

/// `manual`: clique em "Criar ponto" (sempre pergunta ao Windows). Automático (antes
/// de um perfil) reaproveita o ponto que o app criou há menos de 24 h.
fn restore_point_now(app: &tauri::AppHandle, manual: bool) -> AppResult<restore::Outcome> {
    let state = app.state::<restore::RestoreState>();
    state.ensure(manual, restore::protection_is_off(), || {
        elevation::admin_or_elevated(restore::create, elevation::Request::RestorePoint, |_| {}, |v| {
            serde_json::from_value(v).map_err(|e| AppError::Helper(e.to_string()))
        })
    })
}

#[tauri::command]
async fn create_restore_point(app: tauri::AppHandle) -> AppResult<restore::Outcome> {
    blocking(move || restore_point_now(&app, true)).await
}

#[tauri::command]
async fn open_system_protection() -> AppResult<()> {
    blocking(restore::open_protection_settings).await
}

#[tauri::command]
fn whats_new(app: tauri::AppHandle) -> news::WhatsNew {
    app.state::<news::NewsState>().whats_new()
}

#[tauri::command]
fn whats_new_seen(app: tauri::AppHandle) -> AppResult<()> {
    app.state::<news::NewsState>().mark_seen()
}

#[tauri::command]
fn elevation_status() -> elevation::ElevationStatus {
    elevation::status()
}

#[tauri::command]
async fn restart_explorer() -> AppResult<()> {
    blocking(system::restart_explorer).await
}

// ---------------- Monitor, drivers e correções ----------------

#[tauri::command]
async fn monitor_sample(app: tauri::AppHandle) -> AppResult<monitor::Sample> {
    blocking(move || monitor::sample(&app.state::<monitor::MonitorState>())).await
}

#[tauri::command]
async fn hardware_info() -> AppResult<monitor::HardwareInfo> {
    blocking(|| Ok(monitor::hardware_info())).await
}

#[tauri::command]
async fn drivers_inventory() -> AppResult<drivers::Inventory> {
    blocking(drivers::inventory).await
}

#[tauri::command]
async fn drivers_backup() -> AppResult<drivers::BackupResult> {
    blocking(|| {
        elevation::admin_or_elevated(drivers::backup, elevation::Request::DriversBackup, |_| {}, |v| {
            serde_json::from_value(v).map_err(|e| AppError::Helper(e.to_string()))
        })
    })
    .await
}

/// Só abre URLs de uma lista fixa (sites oficiais e Windows Update).
#[tauri::command]
async fn open_official_url(url: String) -> AppResult<()> {
    blocking(move || drivers::open_url(&url)).await
}

#[tauri::command]
async fn open_backup_folder() -> AppResult<()> {
    blocking(drivers::open_backup_folder).await
}

#[tauri::command]
fn fixes_list() -> Vec<fixes::FixView> {
    fixes::FIXES.iter().map(fixes::FixView::from).collect()
}

/// Uma correção por vez: sfc e DISM ao mesmo tempo disputam os mesmos arquivos.
struct FixLock(AtomicBool);

#[tauri::command]
async fn fix_run(app: tauri::AppHandle, id: String) -> AppResult<serde_json::Value> {
    let fix = fixes::find(&id).ok_or_else(|| AppError::NotFound(format!("correção {id}")))?;
    if app.state::<FixLock>().0.swap(true, Ordering::SeqCst) {
        return Err(AppError::InvalidInput("já existe uma correção rodando; espere ela terminar".into()));
    }
    let handle = app.clone();
    let result = blocking(move || {
        if fix.id == "network_reset" {
            // Anota antes de mexer: IP/DNS fixos de cada adaptador vão pro diário. Recusa
            // um segundo reset antes de reiniciar (apagaria a foto boa).
            let net = handle.state::<netcfg::NetState>();
            net.before_reset(netcfg::boot_at(), restore::now(), netcfg::read_static())?;
        }
        if fix.requires_admin && !system::is_admin() {
            // Roda no auxiliar elevado; o progresso volta pelo pipe e é repassado à tela.
            elevation::call(&elevation::Request::RunFix { id: fix.id.to_string() }, elevation::Launch::OnDemand, |p| {
                let _ = handle.emit("fix-progress", p);
            })
        } else {
            Ok(serde_json::to_value(fixes::run(fix, |p| {
                let _ = handle.emit("fix-progress", p);
            }))
            .unwrap_or_default())
        }
    })
    .await;
    app.state::<FixLock>().0.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
async fn net_reset_status(app: tauri::AppHandle) -> AppResult<netcfg::ResetStatus> {
    blocking(move || Ok(app.state::<netcfg::NetState>().status(netcfg::boot_at(), netcfg::read_static()))).await
}

/// Recoloca o IP/DNS fixo que o reset apagou (auxiliar elevado quando preciso).
#[tauri::command]
async fn net_restore(app: tauri::AppHandle) -> AppResult<()> {
    blocking(move || {
        let net = app.state::<netcfg::NetState>();
        if net.status(netcfg::boot_at(), vec![]).needs_reboot {
            return Err(AppError::InvalidInput("reinicie o PC primeiro: o reset de rede só termina depois do reinício".into()));
        }
        net.restore(|a| {
            elevation::admin_or_elevated(
                || netcfg::apply(a),
                elevation::Request::RestoreNetwork { adapter: a.clone() },
                |_| {},
                elevation::unit,
            )
        })
    })
    .await
}

#[tauri::command]
async fn net_discard(app: tauri::AppHandle) -> AppResult<()> {
    blocking(move || app.state::<netcfg::NetState>().discard()).await
}

// ---------------- Timer ----------------

/// Mede o timer agora (sem alterar nada). Leva até ~2 s.
#[tauri::command]
async fn timer_measure(app: tauri::AppHandle) -> AppResult<timer::TimerReport> {
    blocking(move || Ok(timer::measure(sys_profile(&app, false).build))).await
}

/// Jogos rodando agora que dá pra adicionar ao Modo Jogo (detectados pela pasta).
#[tauri::command]
async fn game_suggestions(app: tauri::AppHandle) -> AppResult<Vec<gaming::watcher::GameEntry>> {
    blocking(move || Ok(app.state::<std::sync::Arc<gaming::watcher::Watcher>>().suggestions())).await
}

// ---------------- Diagnóstico e resultado ----------------

const SKILLGAP_PLANS: [&str; 2] = ["9f3c1e2a-7b4d-4c8e-a1f0-5d2b8c6e4a01", "9f3c1e2a-7b4d-4c8e-a1f0-5d2b8c6e4a02"];

/// Lê o Windows de verdade e monta diagnóstico + leitura (sem nada do histórico do app).
fn read_now(app: &tauri::AppHandle) -> (diagnosis::Diagnosis, diagnosis::Reading) {
    let profile = sys_profile(app, false);
    let views = engine(app).list(&profile);
    let states = views.iter().map(|v| (v.id, v.state)).collect();
    let game = tweaks::platform_game_sys();
    let plan = power::active().ok();
    let startup_enabled = startup::list().ok().map(|items| items.iter().filter(|i| i.enabled).count());
    let extras = diagnosis::Extras {
        diagtrack_running: game.service_running("DiagTrack"),
        sysmain_running: game.service_running("SysMain"),
        wsearch_running: game.service_running("WSearch"),
        hiberfil: system::hiberfil_present(),
        performance_plan: plan.as_ref().map(|p| {
            p.plan == Some(power::PowerPlan::HighPerformance) || SKILLGAP_PLANS.iter().any(|g| g.eq_ignore_ascii_case(&p.guid))
        }),
        startup_enabled,
        is_laptop: profile.is_laptop,
        hdd: profile.system_disk == system::DiskKind::Hdd,
    };
    let d = diagnosis::build(&states, &extras);
    let reading = diagnosis::Reading {
        at: restore::now(),
        potential: d.potential,
        startup_enabled,
        services_running: system::running_services(),
        power_plan: plan.map(|p| p.name),
        tweaks_applied: views.iter().filter(|v| v.state == tweaks::TweakState::Active).count(),
        ram_used: system::ram_used(),
        processes: game.processes().len(),
    };
    (d, reading)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosisReport {
    diagnosis: diagnosis::Diagnosis,
    comparison: diagnosis::Comparison,
}

#[tauri::command]
async fn diagnosis_run(app: tauri::AppHandle) -> AppResult<DiagnosisReport> {
    blocking(move || {
        let (d, reading) = read_now(&app);
        let base = app.state::<diagnosis::BaselineState>();
        let has_changes = engine(&app).list(&sys_profile(&app, false)).iter().any(|v| v.can_revert);
        let _ = base.ensure(reading.clone(), has_changes);
        Ok(DiagnosisReport { diagnosis: d, comparison: base.comparison(&reading) })
    })
    .await
}

// ---------------- Energia, boot e apps de fábrica ----------------

fn energy(app: &tauri::AppHandle) -> tauri::State<'_, power::plans::EnergyState> {
    app.state::<power::plans::EnergyState>()
}

#[tauri::command]
async fn energy_status(app: tauri::AppHandle) -> AppResult<power::plans::EnergyStatus> {
    blocking(move || power::energy_status(&energy(&app))).await
}

#[tauri::command]
async fn energy_apply(app: tauri::AppHandle, plan: power::plans::CustomPlan) -> AppResult<power::plans::EnergyStatus> {
    blocking(move || power::apply_custom(&energy(&app), plan)).await
}

#[tauri::command]
async fn energy_revert(app: tauri::AppHandle) -> AppResult<power::plans::EnergyStatus> {
    blocking(move || power::revert_custom(&energy(&app))).await
}

/// `ask`: o usuário clicou pra ver (pode abrir o UAC). Sem `ask` (abrir a tela), só
/// usa o auxiliar se ele já estiver aberto.
#[tauri::command]
async fn boot_report(ask: bool) -> AppResult<serde_json::Value> {
    blocking(move || {
        let launch = if ask { elevation::Launch::OnDemand } else { elevation::Launch::IfRunning };
        elevation::local_or_elevated(
            || bootperf::report().map(|r| serde_json::to_value(r).unwrap_or_default()),
            elevation::Request::BootReport,
            launch,
            Ok,
        )
    })
    .await
}

#[tauri::command]
async fn bloat_list() -> AppResult<Vec<apps::BloatApp>> {
    blocking(apps::list).await
}

#[tauri::command]
async fn bloat_remove(name: String) -> AppResult<()> {
    blocking(move || apps::remove(&name)).await
}

/// Entrada do processo auxiliar elevado (ver `elevation`).
pub fn run_elevated_helper(args: &[String]) -> i32 {
    elevation::helper_main(args)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(gaming::GamingState::default())
        .manage(monitor::MonitorState::default())
        .manage(system::SystemCache::default())
        .manage(FixLock(AtomicBool::new(false)))
        .setup(|app| {
            // Configs e diários em %APPDATA%\app.skillgap.desktop\
            let dir = app.path().app_config_dir()?;
            let watcher = Arc::new(Watcher::load(tweaks::platform_game_sys(), &dir));
            // O app fechou no meio de uma partida da última vez? Desfaz o que ficou pendente.
            // (Sem sync da prioridade aqui: abrir o app nunca deve disparar o UAC.)
            watcher.recover();
            let handle = app.handle().clone();
            watcher.start(move |status| {
                let _ = handle.emit("game-session", status);
            });
            app.manage(watcher);

            // Diário dos ajustes em %APPDATA%\app.skillgap.desktop\tweaks.json
            let journal = dir.join("tweaks.json");
            app.manage(tweaks::TweakEngine::load(tweaks::platform_sys(), journal));
            app.manage(power::plans::EnergyState::load(dir.join("energia.json")));
            app.manage(restore::RestoreState::load(dir.join("ponto-restauracao.json")));
            app.manage(news::NewsState::load(dir.join("novidades.json")));
            app.manage(netcfg::NetState::load(dir.join("rede-antes-do-reset.json")));
            app.manage(diagnosis::BaselineState::load(dir.join("linha-de-base.json")));
            // Primeira leitura logo ao abrir (antes de o usuário mexer em algo), em segundo plano.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let (_, reading) = read_now(&handle);
                let has_changes = handle.state::<tweaks::TweakEngine>().list(&sys_profile(&handle, false)).iter().any(|v| v.can_revert);
                let _ = handle.state::<diagnosis::BaselineState>().ensure(reading, has_changes);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_startup_items,
            set_startup_item_enabled,
            get_power_plan,
            set_power_plan,
            scan_cleanup,
            run_cleanup,
            gaming_diagnose,
            gaming_fix,
            gaming_revert,
            game_watch_status,
            game_watch_save,
            list_running_processes,
            tweaks_list,
            tweak_apply,
            tweak_revert,
            tweaks_revert_all,
            tweak_set_favorite,
            profiles_list,
            profile_apply,
            profile_save,
            profile_delete,
            system_info,
            create_restore_point,
            open_system_protection,
            restart_explorer,
            monitor_sample,
            hardware_info,
            drivers_inventory,
            drivers_backup,
            open_official_url,
            open_backup_folder,
            fixes_list,
            fix_run,
            energy_status,
            energy_apply,
            energy_revert,
            boot_report,
            bloat_list,
            bloat_remove,
            elevation_status,
            whats_new,
            whats_new_seen,
            net_reset_status,
            net_restore,
            net_discard,
            diagnosis_run,
            timer_measure,
            game_suggestions
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o Skill Gap");

    app.run(|app, event| {
        // Fechou o app no meio do jogo: devolve o plano de energia antes de sair.
        if let tauri::RunEvent::Exit = event {
            app.state::<Arc<Watcher>>().shutdown();
        }
    });
}

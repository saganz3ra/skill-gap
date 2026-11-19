mod apps;
mod bootperf;
mod cleanup;
mod diagnosis;
mod drivers;
mod elevation;
mod error;
mod fixes;
mod gaming;
mod hardware;
mod monitor;
mod netcfg;
mod news;
mod power;
mod remote;
mod restore;
mod sensors;
mod startup;
mod telemetry;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TweakStatus {
    state: tweaks::TweakState,
    can_revert: bool,
}

/// Estado de um ajuste só: depois de aplicar/desfazer, a tela atualiza o cartão
/// sem reler os ~95 ajustes.
#[tauri::command]
async fn tweak_status(app: tauri::AppHandle, id: String) -> AppResult<TweakStatus> {
    blocking(move || {
        let (state, can_revert) = engine(&app).status_of(&id)?;
        Ok(TweakStatus { state, can_revert })
    })
    .await
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

// ---------------- Hardware e por jogo ----------------

fn hw(app: &tauri::AppHandle) -> tauri::State<'_, hardware::HardwareState> {
    app.state::<hardware::HardwareState>()
}

/// Executa local se admin, senão no auxiliar (UAC na hora).
fn elevated(local: impl FnOnce() -> AppResult<()>, req: elevation::Request) -> AppResult<()> {
    elevation::admin_or_elevated(local, req, |_| {}, elevation::unit)
}

#[tauri::command]
async fn hw_msi_list(app: tauri::AppHandle) -> AppResult<(Vec<hardware::MsiDevice>, Vec<String>)> {
    blocking(move || Ok((hardware::msi_devices()?, hw(&app).snapshot().msi.into_keys().collect()))).await
}

/// `enabled`: liga o MSI (anota o valor anterior); `false`: volta ao valor anotado.
#[tauri::command]
async fn hw_msi_set(app: tauri::AppHandle, instance_id: String, enabled: bool) -> AppResult<()> {
    blocking(move || {
        let device = hardware::msi_devices()?
            .into_iter()
            .find(|d| d.instance_id.eq_ignore_ascii_case(&instance_id))
            .ok_or_else(|| hardware::invalid("dispositivo fora da lista atual do Windows"))?;
        let id = device.instance_id.clone();
        let value = if enabled {
            hw(&app).record(|j| {
                j.msi.entry(id.clone()).or_insert(device.msi);
            })?;
            Some(1)
        } else {
            hw(&app).snapshot().msi.get(&id).copied().ok_or_else(|| hardware::invalid("nada a desfazer neste dispositivo"))?
        };
        let req = elevation::Request::MsiWrite { instance_id: id.clone(), value };
        elevated(|| hardware::msi_write(&id, value), req)?;
        if !enabled {
            hw(&app).record(|j| {
                j.msi.remove(&id);
            })?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn hw_steam_games() -> AppResult<Vec<hardware::SteamGame>> {
    blocking(|| Ok(hardware::steam_games())).await
}

fn steam_exe(path: &str) -> AppResult<String> {
    hardware::steam_games()
        .into_iter()
        .filter_map(|g| g.exe)
        .find(|e| e.eq_ignore_ascii_case(path))
        .ok_or_else(|| hardware::invalid("jogo fora da lista da Steam"))
}

#[tauri::command]
async fn hw_fse_set(app: tauri::AppHandle, exe_path: String, enabled: bool) -> AppResult<()> {
    blocking(move || {
        let exe = steam_exe(&exe_path)?;
        if enabled {
            let prev = hardware::fse_read(&exe);
            hw(&app).record(|j| {
                j.fse.entry(exe.clone()).or_insert(prev.clone());
            })?;
            hardware::fse_write(&exe, Some(&hardware::add_flag(prev.as_deref())))
        } else {
            let prev = hw(&app).snapshot().fse.get(&exe).cloned().ok_or_else(|| hardware::invalid("nada a desfazer neste jogo"))?;
            hardware::fse_write(&exe, prev.as_deref())?;
            hw(&app).record(|j| {
                j.fse.remove(&exe);
            })
        }
    })
    .await
}

#[tauri::command]
async fn hw_qos_set(app: tauri::AppHandle, exe_path: String, enabled: bool) -> AppResult<()> {
    blocking(move || {
        let full = steam_exe(&exe_path)?;
        let exe = std::path::Path::new(&full).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        hardware::qos_name(&exe).ok_or_else(|| hardware::invalid("nome de executável não suportado"))?;
        if enabled {
            if hw(&app).snapshot().nla.is_none() {
                let prev = hardware::nla_read();
                hw(&app).record(|j| j.nla = Some(prev))?;
                elevated(|| hardware::nla_write(Some("1")), elevation::Request::QosNla { value: Some("1".into()) })?;
            }
            hw(&app).record(|j| {
                j.qos.insert(exe.clone(), true);
            })?;
            elevated(|| hardware::qos_set(&exe, true), elevation::Request::QosSet { exe: exe.clone(), enabled: true })
        } else {
            elevated(|| hardware::qos_set(&exe, false), elevation::Request::QosSet { exe: exe.clone(), enabled: false })?;
            hw(&app).record(|j| {
                j.qos.remove(&exe);
            })?;
            // Última política removida: devolve o "Do not use NLA" como era.
            let j = hw(&app).snapshot();
            if let (true, Some(prev)) = (j.qos.is_empty(), j.nla.clone()) {
                elevated(|| hardware::nla_write(prev.as_deref()), elevation::Request::QosNla { value: prev.clone() })?;
                hw(&app).record(|j| j.nla = None)?;
            }
            Ok(())
        }
    })
    .await
}

#[tauri::command]
async fn hw_nic_list(app: tauri::AppHandle) -> AppResult<(Vec<hardware::NicProperty>, Vec<String>)> {
    blocking(move || Ok((hardware::nic_properties()?, hw(&app).snapshot().nic.into_keys().collect()))).await
}

/// `value: None` volta ao valor anotado antes da primeira mudança.
#[tauri::command]
async fn hw_nic_set(app: tauri::AppHandle, adapter: String, keyword: String, value: Option<String>) -> AppResult<()> {
    blocking(move || {
        let key = format!("{adapter}|{keyword}");
        let current = hardware::nic_properties()?
            .into_iter()
            .find(|p| p.adapter == adapter && p.keyword == keyword)
            .ok_or_else(|| hardware::invalid("propriedade não encontrada"))?;
        let target = match &value {
            Some(v) => {
                hw(&app).record(|j| {
                    j.nic.entry(key.clone()).or_insert(current.value.clone());
                })?;
                v.clone()
            }
            None => hw(&app).snapshot().nic.get(&key).cloned().ok_or_else(|| hardware::invalid("nada a desfazer"))?,
        };
        let req = elevation::Request::NicWrite { adapter: adapter.clone(), keyword: keyword.clone(), value: target.clone() };
        elevated(|| hardware::nic_write(&adapter, &keyword, &target), req)?;
        if value.is_none() {
            hw(&app).record(|j| {
                j.nic.remove(&key);
            })?;
        }
        Ok(())
    })
    .await
}

// ---------------- Overlay, FPS e segunda tela ----------------

type Frames = Arc<telemetry::presentmon::Frames>;
struct RemoteSlot(std::sync::Mutex<Option<remote::RemoteServer>>);
type Sensors = Arc<sensors::SensorsState>;

/// Uma leitura completa dos sensores (roda numa thread, a cada `sensors::REFRESH`).
/// SSD/HD: direto se o app for admin; pelo auxiliar só se ele JÁ estiver aberto
/// (o celular pedindo dados nunca pode fazer surgir um UAC na tela do PC).
fn read_sensors(acpi_cpu: Option<f64>) -> sensors::Sensors {
    let disks = if system::is_admin() {
        sensors::disks().ok()
    } else if elevation::is_running() {
        elevation::call(&elevation::Request::DiskTemps, elevation::Launch::IfRunning, |_| {})
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
    } else {
        None
    };
    sensors::combine(sensors::gpu(), sensors::lhm(), acpi_cpu, disks)
}

fn sensors_now(app: &tauri::AppHandle, acpi_cpu: Option<f64>) -> sensors::Sensors {
    app.state::<Sensors>().get(move || read_sensors(acpi_cpu))
}

#[tauri::command]
async fn sensors_read(app: tauri::AppHandle) -> AppResult<sensors::Sensors> {
    blocking(move || {
        let acpi = monitor::sample(&app.state::<monitor::MonitorState>()).ok().and_then(|s| s.cpu_temp);
        Ok(sensors_now(&app, acpi))
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Telemetry {
    sample: Option<monitor::Sample>,
    sensors: sensors::Sensors,
    ping: telemetry::PingStats,
    frames: Option<telemetry::presentmon::FrameStats>,
    game: Option<String>,
}

/// Uma leitura (puxada pela tela). Mede FPS só do jogo da partida atual do Modo Jogo,
/// e só se o PresentMon estiver instalado e com permissão.
fn telemetry_snapshot(app: &tauri::AppHandle) -> Telemetry {
    let sample = monitor::sample(&app.state::<monitor::MonitorState>()).ok();
    let ping = app.state::<telemetry::PingState>().tick(telemetry::echo);
    let sensors = sensors_now(app, sample.as_ref().and_then(|s| s.cpu_temp));
    let game = app.state::<Arc<Watcher>>().status().session.map(|s| s.game.exe);
    let frames = app.state::<Frames>().inner().clone();
    let can = system::is_admin() || telemetry::presentmon::in_perf_group();
    match &game {
        Some(exe) if can && telemetry::presentmon::installed() => {
            let _ = telemetry::presentmon::start(exe, frames.clone());
        }
        _ => telemetry::presentmon::stop(),
    }
    let frames = telemetry::presentmon::capturing().and_then(|_| frames.stats());
    Telemetry { sample, sensors, ping, frames, game }
}

#[tauri::command]
async fn telemetry_sample(app: tauri::AppHandle) -> AppResult<Telemetry> {
    blocking(move || Ok(telemetry_snapshot(&app))).await
}

/// Mostra/esconde o overlay: janela transparente, sempre no topo, que deixa o clique
/// passar. Não injeta nada no jogo (por isso não aparece em tela cheia EXCLUSIVA).
#[tauri::command]
async fn overlay_show(app: tauri::AppHandle, show: bool) -> AppResult<()> {
    let err = |e: tauri::Error| AppError::Helper(e.to_string());
    match (app.get_webview_window("overlay"), show) {
        (Some(w), false) => {
            w.close().map_err(err)?;
            if app.state::<RemoteSlot>().0.lock().map(|s| s.is_none()).unwrap_or(true) {
                telemetry::presentmon::stop();
            }
        }
        (None, true) => {
            let w = tauri::WebviewWindowBuilder::new(&app, "overlay", tauri::WebviewUrl::App("index.html".into()))
                .title("Skill Gap · overlay")
                .inner_size(230.0, 136.0)
                .position(16.0, 16.0)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .focused(false)
                .shadow(false)
                .build()
                .map_err(err)?;
            w.set_ignore_cursor_events(true).map_err(err)?;
        }
        _ => {}
    }
    Ok(())
}

#[tauri::command]
async fn pm_status(app: tauri::AppHandle) -> AppResult<telemetry::presentmon::PresentMonStatus> {
    blocking(move || {
        let in_group = telemetry::presentmon::in_perf_group();
        Ok(telemetry::presentmon::PresentMonStatus {
            version: telemetry::presentmon::VERSION,
            installed: telemetry::presentmon::installed(),
            can_capture: system::is_admin() || in_group,
            needs_logoff: hw(&app).snapshot().perf_log_users && !in_group,
            capturing: telemetry::presentmon::capturing(),
        })
    })
    .await
}

#[tauri::command]
async fn pm_install() -> AppResult<()> {
    blocking(telemetry::presentmon::install).await
}

/// Põe (ou tira) o seu usuário no grupo que permite medir FPS sem administrador.
#[tauri::command]
async fn pm_permission(app: tauri::AppHandle, add: bool) -> AppResult<()> {
    blocking(move || {
        let sid = elevation::current_user_sid()?;
        if add {
            hw(&app).record(|j| j.perf_log_users = true)?;
        } else if !hw(&app).snapshot().perf_log_users {
            return Err(hardware::invalid("o app não colocou você nesse grupo; nada a desfazer"));
        }
        elevated(
            || telemetry::presentmon::set_perf_group(&sid, add),
            elevation::Request::PerfLogGroup { sid: sid.clone(), add },
        )?;
        if !add {
            hw(&app).record(|j| j.perf_log_users = false)?;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn remote_start(app: tauri::AppHandle) -> AppResult<remote::RemoteInfo> {
    blocking(move || {
        let slot = app.state::<RemoteSlot>();
        let mut s = slot.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(srv) = s.as_ref() {
            return Ok(srv.info.clone());
        }
        let handle = app.clone();
        let snapshot: remote::Snapshot = Arc::new(move || serde_json::to_value(telemetry_snapshot(&handle)).unwrap_or_default());
        let srv = remote::start(elevation::random_hex()?, snapshot)?;
        let info = srv.info.clone();
        *s = Some(srv);
        Ok(info)
    })
    .await
}

#[tauri::command]
async fn remote_stop(app: tauri::AppHandle) -> AppResult<()> {
    blocking(move || {
        app.state::<RemoteSlot>().0.lock().unwrap_or_else(|e| e.into_inner()).take();
        if app.get_webview_window("overlay").is_none() {
            telemetry::presentmon::stop();
        }
        Ok(())
    })
    .await
}

#[tauri::command]
fn remote_status(app: tauri::AppHandle) -> Option<remote::RemoteInfo> {
    app.state::<RemoteSlot>().0.lock().ok().and_then(|s| s.as_ref().map(|r| r.info.clone()))
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

/// Reinstala um app removido a partir da cópia provisionada do próprio PC. O caminho
/// é lido do Windows na hora (no auxiliar elevado), nunca de um diário.
#[tauri::command]
async fn bloat_restore(name: String) -> AppResult<()> {
    blocking(move || {
        let manifest = elevation::admin_or_elevated(
            || apps::provisioned_manifest(&name),
            elevation::Request::AppxManifest { name: name.clone() },
            |_| {},
            |v| v.as_str().map(String::from).ok_or_else(|| AppError::Helper("resposta inválida".into())),
        )?;
        // Registra no usuário ATUAL (sem elevação): é pra ele que o app volta.
        apps::register_manifest(&manifest)
    })
    .await
}

// ---------------- Guardião e relatório ----------------

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Undone {
    id: &'static str,
    name: &'static str,
}

#[tauri::command]
async fn guardian_status(app: tauri::AppHandle) -> AppResult<Vec<Undone>> {
    blocking(move || {
        Ok(engine(&app)
            .undone_by_windows()
            .into_iter()
            .filter_map(|id| tweaks::catalog::find(id).map(|t| Undone { id, name: t.name }))
            .collect())
    })
    .await
}

/// Reaplica o que o Windows desfez (escolha já feita pelo usuário antes).
#[tauri::command]
async fn guardian_reapply(app: tauri::AppHandle) -> AppResult<tweaks::BatchReport> {
    blocking(move || {
        let sys = sys_profile(&app, false);
        let e = engine(&app);
        let mut report = tweaks::BatchReport { applied: vec![], skipped: vec![], failed: vec![], restart: tweaks::catalog::Restart::None };
        for id in e.undone_by_windows() {
            match e.apply_checked(id, &sys, true) {
                Ok(()) => {
                    if let Some(t) = tweaks::catalog::find(id) {
                        report.restart = report.restart.max(t.restart);
                    }
                    report.applied.push(id.to_string());
                }
                Err(err) => report.failed.push(tweaks::Failure { id: id.to_string(), error: err.to_string() }),
            }
        }
        Ok(report)
    })
    .await
}

/// Texto para colar num chamado: máquina e o que o app mudou. Sem nome de usuário,
/// e-mail nem caminhos de pasta pessoal.
#[tauri::command]
async fn support_report(app: tauri::AppHandle) -> AppResult<String> {
    blocking(move || {
        let p = sys_profile(&app, false);
        let views = engine(&app).list(&p);
        let active: Vec<&str> = views.iter().filter(|v| v.can_revert).map(|v| v.id).collect();
        let undone = engine(&app).undone_by_windows();
        let hw = hw(&app).snapshot();
        let plan = power::active().map(|a| a.name).unwrap_or_else(|_| "?".into());
        let (diag, _) = read_now(&app);
        let watch = app.state::<Arc<Watcher>>().status().config;
        let mut t = String::new();
        t += &format!("Skill Gap {}\n", news::CURRENT);
        t += &format!("Windows: {} (build {})\n", p.windows, p.build);
        t += &format!("Tipo: {} · RAM {} GB · disco do sistema {:?}\n", if p.is_laptop { "notebook" } else { "desktop" }, p.ram_gb, p.system_disk);
        t += &format!("Plano de energia: {plan}\n");
        t += &format!("Potencial de otimização: {}/100\n", diag.potential);
        t += &format!("Ajustes aplicados pelo app ({}): {}\n", active.len(), active.join(", "));
        if !undone.is_empty() {
            t += &format!("Desfeitos pelo Windows: {}\n", undone.join(", "));
        }
        t += &format!(
            "Hardware: MSI em {} dispositivo(s), tela cheia clássica em {} jogo(s), QoS em {} jogo(s), GPU de alto desempenho em {} jogo(s), {} propriedade(s) de rede\n",
            hw.msi.len(),
            hw.fse.len(),
            hw.qos.len(),
            hw.gpu.len(),
            hw.nic.len()
        );
        t += &format!(
            "Modo Jogo: {} · {} jogo(s) · timer {} · detecção por pasta {}\n",
            if watch.enabled { "ligado" } else { "desligado" },
            watch.games.len(),
            if watch.timer_mode { "sim" } else { "não" },
            if watch.auto_detect { "sim" } else { "não" }
        );
        Ok(t)
    })
    .await
}

#[tauri::command]
async fn hw_gpu_set(app: tauri::AppHandle, exe_path: String, enabled: bool) -> AppResult<()> {
    blocking(move || {
        let exe = steam_exe(&exe_path)?;
        if enabled {
            let prev = hardware::gpu_read(&exe);
            hw(&app).record(|j| {
                j.gpu.entry(exe.clone()).or_insert(prev.clone());
            })?;
            hardware::gpu_write(&exe, Some(hardware::GPU_HIGH))
        } else {
            let prev = hw(&app).snapshot().gpu.get(&exe).cloned().ok_or_else(|| hardware::invalid("nada a desfazer neste jogo"))?;
            hardware::gpu_write(&exe, prev.as_deref())?;
            hw(&app).record(|j| {
                j.gpu.remove(&exe);
            })
        }
    })
    .await
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
        .manage(telemetry::PingState::default())
        .manage(Frames::default())
        .manage(RemoteSlot(std::sync::Mutex::new(None)))
        .manage(Sensors::default())
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
            app.manage(hardware::HardwareState::load(dir.join("hardware.json")));
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
            tweak_status,
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
            game_suggestions,
            hw_msi_list,
            hw_msi_set,
            hw_steam_games,
            hw_fse_set,
            hw_qos_set,
            hw_nic_list,
            hw_nic_set,
            hw_gpu_set,
            bloat_restore,
            guardian_status,
            guardian_reapply,
            support_report,
            telemetry_sample,
            overlay_show,
            pm_status,
            pm_install,
            pm_permission,
            remote_start,
            remote_stop,
            remote_status,
            sensors_read
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

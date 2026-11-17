//! Modo Jogo automático: uma thread em segundo plano percebe quando um jogo (ou
//! programa) da lista abre e aplica o perfil de jogo; quando ele fecha, tudo volta.
//!
//! O que o perfil faz, e como:
//! - **Prioridade alta do jogo** via registro (IFEO `PerfOptions\CpuPriorityClass`):
//!   o próprio Windows aplica quando o jogo inicia. O Skill Gap NUNCA abre o processo
//!   do jogo — é isso que anticheats vigiam.
//! - **Apps em segundo plano** (navegador, Discord, launchers) vão para "abaixo do
//!   normal". Esses processos são abertos, mas nenhum é o jogo, antivírus, anticheat
//!   ou do Windows (lista `PROTECTED`).
//! - **Serviços** WSearch (indexação) e SysMain (pré-carregamento) param durante a
//!   partida e voltam depois — só se estavam rodando.
//! - **Plano de energia** Alto desempenho durante a partida.
//!
//! "Anota antes de mexer": a sessão é gravada em disco antes de cada mudança. Se o PC
//! desligar no meio, ao abrir o app de novo `recover()` desfaz o que ficou pendente.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::power::PowerPlan;
use crate::tweaks::ops::{GameSys, Hive, BELOW_NORMAL_PRIORITY, NORMAL_PRIORITY};

const POLL: Duration = Duration::from_secs(3);
const MAX_ENTRIES: usize = 50;
/// Classe "Alta" no IFEO (1 ociosa, 2 normal, 3 alta, 5 abaixo do normal, 6 acima do normal).
const IFEO_HIGH: u32 = 3;
const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options";
pub const PAUSABLE_SERVICES: [&str; 2] = ["WSearch", "SysMain"];

/// Nunca entram em nenhuma lista: sistema, áudio, antivírus, anticheats e o próprio app.
/// Mexer na prioridade deles pode travar o PC, cortar o áudio ou banir a conta.
pub const PROTECTED: &[&str] = &[
    "system", "registry", "idle", "smss.exe", "csrss.exe", "wininit.exe", "winlogon.exe", "services.exe",
    "lsass.exe", "svchost.exe", "dwm.exe", "explorer.exe", "audiodg.exe", "fontdrvhost.exe", "msmpeng.exe",
    "nissrv.exe", "securityhealthservice.exe", "mpdefendercoreservice.exe", "vgc.exe", "vgtray.exe",
    "vgk.sys", "easyanticheat.exe", "easyanticheat_eos.exe", "beservice.exe", "faceit.exe",
    "faceitservice.exe", "eaanticheat.gameservice.exe", "eaanticheat.gameservicelauncher.exe",
    "skill-gap.exe",
];

pub const DEFAULT_BACKGROUND: &[&str] = &[
    "chrome.exe", "msedge.exe", "firefox.exe", "opera.exe", "brave.exe", "Discord.exe", "steamwebhelper.exe",
    "EpicGamesLauncher.exe", "Spotify.exe", "OneDrive.exe", "ms-teams.exe",
];

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub name: String,
    pub exe: String,
    /// Prioridade alta via IFEO. Padrão ligado (configs antigas não têm o campo).
    #[serde(default = "yes")]
    pub high_priority: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchConfig {
    pub enabled: bool,
    pub games: Vec<GameEntry>,
    pub background: Vec<String>,
    pub lower_background: bool,
    pub pause_services: bool,
    pub power_boost: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            games: Vec::new(),
            background: DEFAULT_BACKGROUND.iter().map(|s| s.to_string()).collect(),
            lower_background: true,
            pause_services: true,
            power_boost: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lowered {
    pub pid: u32,
    pub exe: String,
    pub previous: u32,
}

/// Uma partida em andamento e tudo que precisa voltar quando ela acabar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub game: GameEntry,
    pub started_at: u64,
    /// GUID do plano a restaurar. None = não mexemos no plano.
    pub restore_guid: Option<String>,
    pub paused_services: Vec<String>,
    pub lowered: Vec<Lowered>,
}

/// Valor original do IFEO de um executável.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IfeoSnap {
    previous: Option<u32>,
    perf_key_existed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchStatus {
    pub config: WatchConfig,
    pub session: Option<Session>,
    /// Executáveis com prioridade alta aplicada no registro agora.
    pub high_priority_applied: Vec<String>,
    pub last_error: Option<String>,
}

// ---------------- Lógica pura ----------------

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

pub fn is_protected(exe: &str) -> bool {
    PROTECTED.contains(&lower(exe).as_str())
}

/// Primeiro jogo da lista que está rodando (sem diferenciar maiúsculas, como o Windows).
pub fn find_running<'a, I, S>(games: &'a [GameEntry], running: I) -> Option<&'a GameEntry>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let running: Vec<String> = running.into_iter().map(|s| lower(s.as_ref())).collect();
    games.iter().find(|g| running.contains(&lower(&g.exe)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Nothing,
    Start(GameEntry),
    Stop,
    Switch(GameEntry),
}

/// Máquina de estados: dado o jogo ativo e o que foi visto agora, decide o que fazer.
pub fn decide(active: Option<&GameEntry>, found: Option<&GameEntry>, enabled: bool) -> Action {
    let found = if enabled { found } else { None };
    match (active, found) {
        (None, None) => Action::Nothing,
        (None, Some(g)) => Action::Start(g.clone()),
        (Some(_), None) => Action::Stop,
        (Some(a), Some(g)) if a.exe.eq_ignore_ascii_case(&g.exe) => Action::Nothing,
        (Some(_), Some(g)) => Action::Switch(g.clone()),
    }
}

fn valid_exe(exe: &str) -> AppResult<()> {
    let l = lower(exe);
    if !l.ends_with(".exe") || exe.len() > 260 || exe.contains(['\\', '/', ':']) {
        return Err(AppError::InvalidInput(format!("“{exe}” não é um nome de executável válido")));
    }
    if is_protected(exe) {
        return Err(AppError::InvalidInput(format!(
            "“{exe}” é protegido (sistema, áudio, antivírus ou anticheat) e não pode entrar na lista"
        )));
    }
    Ok(())
}

pub fn validate(mut cfg: WatchConfig) -> AppResult<WatchConfig> {
    let mut seen = std::collections::HashSet::new();
    let mut games = Vec::new();
    for mut g in cfg.games {
        g.exe = g.exe.trim().to_string();
        g.name = g.name.trim().to_string();
        valid_exe(&g.exe)?;
        if g.name.is_empty() {
            g.name = g.exe[..g.exe.len() - 4].to_string();
        }
        if seen.insert(lower(&g.exe)) {
            games.push(g);
        }
    }
    let mut background = Vec::new();
    let mut seen_bg = std::collections::HashSet::new();
    for b in cfg.background {
        let b = b.trim().to_string();
        valid_exe(&b)?;
        // Um jogo da lista nunca é tratado como "segundo plano".
        if !seen.contains(&lower(&b)) && seen_bg.insert(lower(&b)) {
            background.push(b);
        }
    }
    if games.len() > MAX_ENTRIES || background.len() > MAX_ENTRIES {
        return Err(AppError::InvalidInput(format!("no máximo {MAX_ENTRIES} itens por lista")));
    }
    cfg.games = games;
    cfg.background = background;
    Ok(cfg)
}

// ---------------- Arquivos ----------------

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Grava num .tmp e renomeia: se o PC desligar no meio, o arquivo antigo fica inteiro.
fn write_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).expect("serializável"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------- Watcher ----------------

enum Msg {
    Wake,
    Stop,
}

pub struct Watcher {
    sys: Box<dyn GameSys>,
    config_path: PathBuf,
    ifeo_path: PathBuf,
    session_path: PathBuf,
    config: Mutex<WatchConfig>,
    ifeo: Mutex<BTreeMap<String, IfeoSnap>>,
    session: Mutex<Option<Session>>,
    last_error: Mutex<Option<String>>,
    tx: Mutex<Option<Sender<Msg>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Watcher {
    /// Arquivos ausentes ou corrompidos = começa limpo; o app sempre abre.
    pub fn load(sys: Box<dyn GameSys>, dir: &Path) -> Self {
        let config_path = dir.join("modo-jogo.json");
        let config = read_json::<WatchConfig>(&config_path).and_then(|c| validate(c).ok()).unwrap_or_default();
        let ifeo_path = dir.join("modo-jogo-prioridade.json");
        let ifeo = read_json(&ifeo_path).unwrap_or_default();
        Self {
            sys,
            config_path,
            ifeo_path,
            session_path: dir.join("modo-jogo-sessao.json"),
            config: Mutex::new(config),
            ifeo: Mutex::new(ifeo),
            session: Mutex::new(None),
            last_error: Mutex::new(None),
            tx: Mutex::new(None),
        }
    }

    /// Chamado ao abrir o app: se existe uma sessão gravada, o app fechou (ou o PC
    /// desligou) no meio de uma partida. Desfaz o que ficou pendente.
    pub fn recover(&self) -> bool {
        let Some(pending) = read_json::<Session>(&self.session_path) else { return false };
        self.end_session(pending);
        true
    }

    pub fn status(&self) -> WatchStatus {
        WatchStatus {
            config: lock(&self.config).clone(),
            session: lock(&self.session).clone(),
            high_priority_applied: lock(&self.ifeo).keys().cloned().collect(),
            last_error: lock(&self.last_error).clone(),
        }
    }

    fn set_error(&self, e: Option<String>) {
        *lock(&self.last_error) = e;
    }

    pub fn save(&self, cfg: WatchConfig) -> AppResult<()> {
        let cfg = validate(cfg)?;
        write_json(&self.config_path, &cfg)?;
        *lock(&self.config) = cfg;
        self.sync_priority();
        if let Some(tx) = lock(&self.tx).as_ref() {
            let _ = tx.send(Msg::Wake);
        }
        Ok(())
    }

    // ---------- Prioridade alta via IFEO ----------

    fn perf_key(exe: &str) -> String {
        format!(r"{IFEO}\{exe}\PerfOptions")
    }

    /// Deixa o registro igual à configuração: jogos com prioridade alta (e Modo Jogo
    /// ligado) têm a entrada; o resto volta ao valor original.
    pub fn sync_priority(&self) {
        let cfg = lock(&self.config).clone();
        let desired: Vec<String> = if cfg.enabled {
            cfg.games.iter().filter(|g| g.high_priority).map(|g| lower(&g.exe)).collect()
        } else {
            Vec::new()
        };

        let mut journal = lock(&self.ifeo);
        let mut error = None;

        // Remove o que não deveria mais estar lá.
        let stale: Vec<String> = journal.keys().filter(|k| !desired.contains(k)).cloned().collect();
        for exe in stale {
            let snap = journal[&exe].clone();
            let key = Self::perf_key(&exe);
            let result = if snap.perf_key_existed {
                self.sys.set_dword(Hive::Hklm, &key, "CpuPriorityClass", snap.previous)
            } else {
                self.sys.delete_key_tree(Hive::Hklm, &key)
            };
            match result {
                Ok(()) => {
                    journal.remove(&exe);
                }
                Err(e) => error = Some(e),
            }
        }

        // Aplica o que falta, anotando o valor original ANTES.
        let missing: Vec<String> = desired.iter().filter(|e| !journal.contains_key(*e)).cloned().collect();
        for exe in &missing {
            let key = Self::perf_key(exe);
            let snap = IfeoSnap {
                previous: self.sys.dword(Hive::Hklm, &key, "CpuPriorityClass"),
                perf_key_existed: self.sys.key_exists(Hive::Hklm, &key),
            };
            journal.insert(exe.clone(), snap);
            if let Err(e) = write_json(&self.ifeo_path, &*journal) {
                journal.remove(exe);
                error = Some(e);
                continue;
            }
            if let Err(e) = self.sys.set_dword(Hive::Hklm, &key, "CpuPriorityClass", Some(IFEO_HIGH)) {
                journal.remove(exe);
                error = Some(e);
            }
        }

        let _ = write_json(&self.ifeo_path, &*journal);
        drop(journal);
        self.set_error(error.map(|e| match e {
            AppError::RequiresAdmin => {
                "A prioridade alta dos jogos fica no registro do sistema e precisa do Skill Gap aberto como administrador. O resto do Modo Jogo funciona normalmente.".to_string()
            }
            other => format!("Prioridade alta: {other}"),
        }));
    }

    // ---------- Sessão ----------

    fn persist_session(&self, session: &Session) {
        let _ = write_json(&self.session_path, session);
    }

    fn start_session(&self, game: GameEntry, cfg: &WatchConfig) -> Session {
        // 1) decide e anota tudo antes de mexer
        let mut session = Session { game, started_at: now(), restore_guid: None, paused_services: Vec::new(), lowered: Vec::new() };

        let plan = if cfg.power_boost { self.sys.active_plan().ok() } else { None };
        if let Some((guid, current)) = &plan {
            if *current != Some(PowerPlan::HighPerformance) {
                session.restore_guid = Some(guid.clone());
            }
        }
        if cfg.pause_services {
            session.paused_services = PAUSABLE_SERVICES
                .iter()
                .filter(|s| self.sys.service_running(s) == Some(true))
                .map(|s| s.to_string())
                .collect();
        }
        self.persist_session(&session);

        // 2) aplica; o que falhar sai da sessão pra não ser "desfeito" à toa
        if session.restore_guid.is_some() {
            if let Err(e) = self.sys.set_plan(PowerPlan::HighPerformance) {
                session.restore_guid = None;
                self.set_error(Some(format!("Não foi possível ativar o Alto desempenho: {e}")));
            }
        }
        session.paused_services.retain(|s| self.sys.pause_service(s).is_ok());
        self.persist_session(&session);
        session
    }

    /// Baixa a prioridade dos apps de segundo plano que ainda não foram baixados.
    /// Só mexe em quem está em "normal": se o usuário escolheu outra prioridade, respeita.
    fn lower_background(&self, session: &mut Session, cfg: &WatchConfig, procs: &[(u32, String)]) -> bool {
        if !cfg.lower_background {
            return false;
        }
        let targets: Vec<String> = cfg.background.iter().map(|b| lower(b)).collect();
        let game = lower(&session.game.exe);
        let mut changed = false;
        for (pid, exe) in procs {
            let l = lower(exe);
            if !targets.contains(&l) || l == game || is_protected(exe) || session.lowered.iter().any(|x| x.pid == *pid) {
                continue;
            }
            if self.sys.priority(*pid) != Some(NORMAL_PRIORITY) {
                continue;
            }
            session.lowered.push(Lowered { pid: *pid, exe: exe.clone(), previous: NORMAL_PRIORITY });
            self.persist_session(session);
            if self.sys.set_priority(*pid, BELOW_NORMAL_PRIORITY).is_ok() {
                changed = true;
            } else {
                session.lowered.pop();
                self.persist_session(session);
            }
        }
        changed
    }

    fn end_session(&self, session: Session) {
        let procs = self.sys.processes();
        for l in &session.lowered {
            // Confere PID + nome (o Windows reaproveita PIDs) e se a prioridade ainda é a
            // que nós pusemos: se o usuário mudou depois, não desfazemos a escolha dele.
            let same = procs.iter().any(|(pid, exe)| *pid == l.pid && exe.eq_ignore_ascii_case(&l.exe));
            if same && self.sys.priority(l.pid) == Some(BELOW_NORMAL_PRIORITY) {
                let _ = self.sys.set_priority(l.pid, l.previous);
            }
        }
        for s in &session.paused_services {
            let _ = self.sys.resume_service(s);
        }
        if let Some(guid) = &session.restore_guid {
            if let Err(e) = self.sys.set_plan_guid(guid) {
                self.set_error(Some(format!("Não foi possível voltar ao plano anterior: {e}")));
            }
        }
        let _ = fs::remove_file(&self.session_path);
    }

    /// Uma verificação. Devolve true se algo mudou (pra avisar a interface).
    pub fn tick(&self) -> bool {
        let cfg = lock(&self.config).clone();
        let procs = if cfg.enabled && !cfg.games.is_empty() { self.sys.processes() } else { Vec::new() };
        let found = find_running(&cfg.games, procs.iter().map(|(_, n)| n)).cloned();

        let mut slot = lock(&self.session);
        match decide(slot.as_ref().map(|s| &s.game), found.as_ref(), cfg.enabled) {
            Action::Start(game) => {
                let mut session = self.start_session(game, &cfg);
                self.lower_background(&mut session, &cfg, &procs);
                *slot = Some(session);
                true
            }
            Action::Switch(game) => {
                let s = slot.as_mut().expect("sessão ativa");
                s.game = game;
                self.persist_session(s);
                true
            }
            Action::Stop => {
                if let Some(s) = slot.take() {
                    self.end_session(s);
                }
                true
            }
            Action::Nothing => match slot.as_mut() {
                // Durante a partida, apps abertos depois também entram na fila.
                Some(s) => self.lower_background(s, &cfg, &procs),
                None => false,
            },
        }
    }

    pub fn start<F>(self: &Arc<Self>, on_change: F)
    where
        F: Fn(WatchStatus) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        *lock(&self.tx) = Some(tx);
        let this = Arc::clone(self);
        std::thread::Builder::new()
            .name("skillgap-game-watcher".into())
            .spawn(move || loop {
                match rx.recv_timeout(POLL) {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(Msg::Wake) | Err(RecvTimeoutError::Timeout) => {}
                }
                if this.tick() {
                    on_change(this.status());
                }
            })
            .expect("não foi possível criar a thread do Modo Jogo");
    }

    /// Ao fechar o app: para a thread e desfaz a sessão em andamento. A prioridade via
    /// registro continua (ela é configuração, não estado de sessão).
    pub fn shutdown(&self) {
        if let Some(tx) = lock(&self.tx).take() {
            let _ = tx.send(Msg::Stop);
        }
        if let Some(s) = lock(&self.session).take() {
            self.end_session(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tweaks::fake::FakeSys;
    use crate::tweaks::ops::Sys;

    fn game(exe: &str) -> GameEntry {
        GameEntry { name: exe.trim_end_matches(".exe").into(), exe: exe.into(), high_priority: true }
    }

    fn setup(name: &str) -> (Watcher, FakeSys, PathBuf) {
        let dir = std::env::temp_dir().join(format!("skillgap-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let fake = FakeSys::default();
        (Watcher::load(Box::new(fake.clone()), &dir), fake, dir)
    }

    fn enable(w: &Watcher, games: &[&str]) {
        let cfg = WatchConfig { enabled: true, games: games.iter().map(|g| game(g)).collect(), ..Default::default() };
        w.save(cfg).unwrap();
    }

    #[test]
    fn encontra_jogo_sem_diferenciar_maiusculas() {
        let games = [game("cs2.exe"), game("VALORANT-Win64-Shipping.exe")];
        let found = find_running(&games, ["explorer.exe", "valorant-win64-shipping.exe"]);
        assert_eq!(found.map(|g| g.exe.as_str()), Some("VALORANT-Win64-Shipping.exe"));
        assert_eq!(find_running(&games, ["chrome.exe"]), None);
    }

    #[test]
    fn maquina_de_estados() {
        let cs = game("cs2.exe");
        let val = game("valorant.exe");
        assert_eq!(decide(None, None, true), Action::Nothing);
        assert_eq!(decide(None, Some(&cs), true), Action::Start(cs.clone()));
        assert_eq!(decide(Some(&cs), Some(&cs), true), Action::Nothing);
        assert_eq!(decide(Some(&cs), None, true), Action::Stop);
        assert_eq!(decide(Some(&cs), Some(&val), true), Action::Switch(val.clone()));
        assert_eq!(decide(Some(&cs), Some(&cs), false), Action::Stop);
    }

    #[test]
    fn validacao_recusa_protegidos_e_caminhos() {
        for bad in ["jogo.bat", r"C:\jogos\cs2.exe", "../cs2.exe", "cs2", "svchost.exe", "vgc.exe", "MsMpEng.exe"] {
            let cfg = WatchConfig { games: vec![game(bad)], ..Default::default() };
            assert!(validate(cfg).is_err(), "{bad} deveria ser recusado");
        }
        let cfg = WatchConfig { background: vec!["audiodg.exe".into()], ..Default::default() };
        assert!(validate(cfg).is_err());
        // Jogo que também está em "segundo plano" sai do segundo plano.
        let cfg = WatchConfig { games: vec![game("chrome.exe")], ..Default::default() };
        assert!(!validate(cfg).unwrap().background.iter().any(|b| b == "chrome.exe"));
    }

    #[test]
    fn config_antiga_carrega_com_padroes() {
        let (_, _, dir) = setup("legacy");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("modo-jogo.json"), r#"{"enabled":true,"games":[{"name":"CS2","exe":"cs2.exe"}]}"#).unwrap();
        let w = Watcher::load(Box::new(FakeSys::default()), &dir);
        let c = w.status().config;
        assert!(c.games[0].high_priority);
        assert!(c.pause_services && c.lower_background && c.power_boost);
        assert!(!c.background.is_empty());
    }

    #[test]
    fn sessao_completa_aplica_e_desfaz_tudo() {
        let (w, fake, _) = setup("session");
        fake.set_service("WSearch", true);
        fake.set_service("SysMain", false); // já parado: não é "pausado" por nós
        fake.spawn(10, "chrome.exe");
        fake.spawn(11, "Discord.exe");
        fake.spawn(12, "svchost.exe");
        fake.set_priority(11, 0x8000).unwrap(); // usuário pôs Discord acima do normal
        enable(&w, &["cs2.exe"]);

        assert!(!w.tick(), "sem jogo, nada muda");
        fake.spawn(20, "cs2.exe");
        assert!(w.tick());
        let s = w.status().session.unwrap();
        assert_eq!(s.paused_services, vec!["WSearch".to_string()]);
        assert_eq!(fake.service("WSearch"), Some(false));
        assert_eq!(fake.prio(10), Some(BELOW_NORMAL_PRIORITY));
        assert_eq!(fake.prio(11), Some(0x8000), "escolha do usuário respeitada");
        assert_eq!(fake.prio(12), Some(NORMAL_PRIORITY), "protegido nunca é tocado");
        assert_eq!(fake.prio(20), Some(NORMAL_PRIORITY), "o processo do jogo nunca é aberto");
        assert_eq!(fake.active_plan().unwrap().1, Some(PowerPlan::HighPerformance));

        // App aberto no meio da partida também entra.
        fake.spawn(13, "Spotify.exe");
        assert!(w.tick());
        assert_eq!(fake.prio(13), Some(BELOW_NORMAL_PRIORITY));

        fake.kill(20);
        assert!(w.tick());
        assert!(w.status().session.is_none());
        assert_eq!(fake.prio(10), Some(NORMAL_PRIORITY));
        assert_eq!(fake.prio(13), Some(NORMAL_PRIORITY));
        assert_eq!(fake.service("WSearch"), Some(true));
        assert_eq!(fake.service("SysMain"), Some(false), "não liga o que não estava ligado");
        assert_eq!(fake.active_plan().unwrap().1, Some(PowerPlan::Balanced));
    }

    #[test]
    fn queda_de_energia_e_recuperada_ao_abrir() {
        let (w, fake, dir) = setup("crash");
        fake.set_service("WSearch", true);
        enable(&w, &["cs2.exe"]);
        fake.spawn(20, "cs2.exe");
        w.tick();
        assert_eq!(fake.service("WSearch"), Some(false));
        // "PC desligou": o Watcher some sem shutdown(). Abre de novo:
        drop(w);
        let reopened = Watcher::load(Box::new(fake.clone()), &dir);
        assert!(reopened.recover());
        assert_eq!(fake.service("WSearch"), Some(true));
        assert_eq!(fake.active_plan().unwrap().1, Some(PowerPlan::Balanced));
        assert!(!reopened.recover(), "nada pendente na segunda vez");
    }

    #[test]
    fn prioridade_alta_no_registro_e_reversivel() {
        let (w, fake, _) = setup("ifeo");
        let key = Watcher::perf_key("cs2.exe");
        enable(&w, &["cs2.exe"]);
        assert_eq!(fake.dword(Hive::Hklm, &key, "CpuPriorityClass"), Some(IFEO_HIGH));
        assert_eq!(w.status().high_priority_applied, vec!["cs2.exe".to_string()]);

        // Desligar o Modo Jogo remove a entrada que criamos.
        w.save(WatchConfig { enabled: false, games: vec![game("cs2.exe")], ..Default::default() }).unwrap();
        assert_eq!(fake.dword(Hive::Hklm, &key, "CpuPriorityClass"), None);
        assert!(!fake.key_exists(Hive::Hklm, &key));
    }

    #[test]
    fn prioridade_preserva_valor_que_ja_existia() {
        let (w, fake, _) = setup("ifeo-prev");
        let key = Watcher::perf_key("cs2.exe");
        fake.set_dword(Hive::Hklm, &key, "CpuPriorityClass", Some(6)).unwrap();
        enable(&w, &["cs2.exe"]);
        w.save(WatchConfig { enabled: true, games: vec![], ..Default::default() }).unwrap();
        assert_eq!(fake.dword(Hive::Hklm, &key, "CpuPriorityClass"), Some(6));
    }

    #[test]
    fn sem_admin_o_resto_funciona() {
        let (w, fake, _) = setup("noadmin");
        fake.deny_hklm();
        enable(&w, &["cs2.exe"]);
        let st = w.status();
        assert!(st.high_priority_applied.is_empty());
        assert!(st.last_error.unwrap().contains("administrador"));
        fake.spawn(20, "cs2.exe");
        assert!(w.tick());
        assert!(w.status().session.is_some());
    }
}

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Tipos espelham as structs do Rust (serde rename_all = "camelCase").
// Se mudar um lado, mude o outro: é o "contrato" entre front e backend.

export type StartupSource = "userRun" | "machineRun" | "machineRun32" | "userFolder" | "commonFolder";

export interface StartupItem {
  id: string;
  name: string;
  command: string;
  source: StartupSource;
  sourceLabel: string;
  enabled: boolean;
  requiresAdmin: boolean;
}

export type PowerPlan = "powerSaver" | "balanced" | "highPerformance";

export interface ActivePlan {
  /** null = plano personalizado */
  plan: PowerPlan | null;
  guid: string;
  name: string;
}

export type CleanupCategory = "userTemp" | "windowsTemp" | "windowsUpdateCache" | "shaderCache" | "recycleBin";

export interface CategoryScan {
  category: CleanupCategory;
  path: string;
  bytes: number;
  files: number;
  available: boolean;
  partial: boolean;
  requiresAdmin: boolean;
}

export interface CategoryResult {
  category: CleanupCategory;
  freedBytes: number;
  deletedFiles: number;
  skippedFiles: number;
  error: string | null;
}

export interface CleanupReport {
  results: CategoryResult[];
  freedBytes: number;
  /** Espaço livre medido no disco antes x depois; null se não deu pra medir. */
  diskFreedBytes: number | null;
}

export type CheckId =
  | "refreshRate"
  | "mouseAcceleration"
  | "backgroundRecording"
  | "hags"
  | "gameMode"
  | "driverVsync";

export interface GamingCheck {
  id: CheckId;
  status: "ok" | "warn" | "info";
  current: string;
  fixable: boolean;
  requiresAdmin: boolean;
  requiresRestart: boolean;
  canRevert: boolean;
}

export interface DisplayInfo {
  device: string;
  name: string;
  current: { width: number; height: number; hz: number };
  maxHz: number;
}

export interface GamingReport {
  checks: GamingCheck[];
  displays: DisplayInfo[];
  fpsCap: number | null;
}

export interface GameEntry {
  name: string;
  exe: string;
  highPriority: boolean;
}

export interface WatchConfig {
  enabled: boolean;
  games: GameEntry[];
  background: string[];
  lowerBackground: boolean;
  pauseServices: boolean;
  powerBoost: boolean;
  /** Segura o timer em 0,5 ms durante a partida. */
  timerMode: boolean;
  /** Reconhece jogos pela pasta (Steam, Epic, Riot…). */
  autoDetect: boolean;
}

export interface TimerStats {
  meanMs: number;
  minMs: number;
  maxMs: number;
  stddevMs: number;
}

export interface TimerReport {
  sleep: TimerStats | null;
  currentMs: number | null;
  finestMs: number | null;
  coarsestMs: number | null;
  globalRequests: boolean;
  modeReachesGame: boolean;
  heldByApp: boolean;
}

export interface GameSession {
  game: GameEntry;
  startedAt: number;
  restoreGuid: string | null;
  pausedServices: string[];
  lowered: { pid: number; exe: string; previous: number }[];
}

export interface WatchStatus {
  config: WatchConfig;
  session: GameSession | null;
  highPriorityApplied: string[];
  lastError: string | null;
}

export type TweakCategory = "geral" | "aparencia" | "desempenho" | "privacidade" | "gaming" | "rede";
export type Restart = "none" | "explorer" | "logoff" | "reboot";

export interface TweakView {
  id: string;
  name: string;
  description: string;
  details: string;
  category: TweakCategory;
  impact: "small" | "medium" | "high";
  restart: Restart;
  requiresAdmin: boolean;
  risk: "low" | "medium" | "high";
  /** O que pode parar de funcionar (sempre presente em risco médio/alto). */
  breaks: string | null;
  /** Aumenta calor e consumo: fica fora dos perfis. */
  heats: boolean;
  /** Se serve para este PC (calculado no backend). Indisponível nunca bloqueia o desfazer. */
  gate: Gate;
  state: "active" | "inactive" | "partial" | "unknown";
  canRevert: boolean;
  favorite: boolean;
}

export type NewsKind = "novo" | "melhoria" | "correcao" | "mudanca";

export interface Release {
  version: string;
  /** `null`: versão em desenvolvimento. */
  date: string | null;
  items: { kind: NewsKind; text: string }[];
}

export interface WhatsNew {
  current: string;
  lastSeen: string;
  /** Versões depois da última que você abriu. */
  unseen: Release[];
  all: Release[];
  /** Ajustes que entraram depois da última versão que você abriu. */
  newTweaks: string[];
}

export const NEWS_KIND: Record<NewsKind, [string, string]> = {
  novo: ["Novo", "badge-ok"],
  melhoria: ["Melhoria", "badge-info"],
  correcao: ["Correção", "badge-warn"],
  mudanca: ["Mudança", "badge-idle"],
};

/** Disparado quando as novidades são marcadas como vistas (a barra lateral escuta). */
export const NEWS_SEEN_EVENT = "skillgap:news-seen";

/** IP/DNS digitados à mão num adaptador (o que o "Resetar a rede" apaga). */
export interface AdapterConfig {
  guid: string;
  name: string;
  ipv4: { addresses: [string, string][]; gateways: string[] } | null;
  dns4: string[];
  dns6: string[];
}

export interface NetResetStatus {
  /** Reset feito e o PC ainda não reiniciou. */
  needsReboot: boolean;
  pending: { takenAt: number; bootAt: number; adapters: AdapterConfig[] } | null;
  /** O que o PC tem de fixo agora. */
  current: AdapterConfig[];
}

/** "Ethernet: IP 192.168.1.50 · DNS 1.1.1.1, 8.8.8.8" */
export function describeAdapter(a: AdapterConfig): string {
  const parts: string[] = [];
  if (a.ipv4) parts.push(`IP fixo ${a.ipv4.addresses.map(([ip]) => ip).join(", ")}`);
  const dns = [...a.dns4, ...a.dns6];
  if (dns.length) parts.push(`DNS ${dns.join(", ")}`);
  return `${a.name}: ${parts.join(" · ")}`;
}

export interface Finding {
  id: string;
  label: string;
  weight: number;
  optimized: boolean | null;
  hint: string;
}

export interface ComparisonRow {
  label: string;
  kind: "count" | "bytes" | "score" | "text";
  before: number | string | null;
  now: number | string | null;
  firm: boolean;
  lowerIsBetter: boolean | null;
}

export interface DiagnosisReport {
  diagnosis: { potential: number; findings: Finding[] };
  comparison: { baselineAt: number | null; afterChanges: boolean; rows: ComparisonRow[] };
}

export interface MsiDevice {
  instanceId: string;
  name: string;
  class: string;
  msi: number | null;
}

export interface SteamGame {
  appId: string;
  name: string;
  installDir: string;
  exe: string | null;
  classicFullscreen: boolean;
  qos: boolean;
}

export interface NicProperty {
  adapter: string;
  keyword: string;
  label: string;
  value: string;
  valid: string[];
  display: string[];
}

export type Gate = { status: "ok" } | { status: "warn"; reason: string } | { status: "unavailable"; reason: string };

/** Mesma regra do backend (`profile_skip_reason`): o que um perfil pula. */
export const skippedByProfiles = (t: TweakView) => t.gate.status !== "ok" || t.heats || t.risk === "high";

export interface ProfileView {
  id: string;
  name: string;
  description: string;
  tweaks: string[];
  builtin: boolean;
}

/** O que o Windows fez com o pedido de ponto de restauração. */
export type RestoreOutcome =
  | { status: "created"; at: number }
  | { status: "recentValid"; at: number }
  | { status: "protectionOff" };

export interface ProfileApplyResult {
  restorePoint: RestoreOutcome | null;
  /** `null`: nada foi aplicado (a Proteção do Sistema está desligada). */
  report: BatchReport | null;
}

export interface BatchReport {
  applied: string[];
  skipped: { id: string; reason: string }[];
  failed: { id: string; error: string }[];
  restart: Restart;
}

export interface SystemInfo {
  windows: string;
  build: number;
  isLaptop: boolean;
  ramGb: number;
  systemDisk: "ssd" | "hdd" | "unknown";
  isAdmin: boolean;
  recommendedProfile: string;
  reason: string;
  displayHz: number | null;
  maxDisplayHz: number | null;
}

export const CATEGORY_LABEL: Record<TweakCategory, string> = {
  geral: "Geral",
  aparencia: "Aparência",
  desempenho: "Desempenho",
  privacidade: "Privacidade",
  gaming: "Gaming",
  rede: "Rede",
};

export const RESTART_LABEL: Record<Restart, string | null> = {
  none: null,
  explorer: "Reinicia o Explorer",
  logoff: "Sair e entrar na conta",
  reboot: "Reiniciar o PC",
};

export interface MonitorSample {
  cpu: number;
  ramUsed: number;
  ramTotal: number;
  gpu: number | null;
  vramUsed: number | null;
  diskActive: number;
  diskRead: number;
  diskWrite: number;
  netDown: number;
  netUp: number;
}

export interface HardwareInfo {
  cpuName: string;
  cpuThreads: number;
  cpuMhz: number;
  gpuName: string;
  ramTotal: number;
}

export interface DriverInfo {
  name: string;
  version: string;
  date: string;
  provider: string;
  class: string;
  downloadUrl: string | null;
}

export interface ProblemDevice {
  name: string;
  code: number;
  class: string;
  description: string;
}

export interface DriverInventory {
  drivers: DriverInfo[];
  problems: ProblemDevice[];
}

export interface FixView {
  id: string;
  name: string;
  description: string;
  when: string;
  requiresAdmin: boolean;
  restart: Restart;
  duration: string;
  steps: string[];
}

export interface FixReport {
  id: string;
  ok: boolean;
  steps: { label: string; ok: boolean; output: string }[];
  restart: Restart;
}

export interface FixProgress {
  id: string;
  step: number;
  total: number;
  label: string;
}

export const WINDOWS_UPDATE_URL = "ms-settings:windowsupdate";

export type CustomPlan = "desktop" | "notebook";

export interface EnergyStatus {
  activeName: string;
  activeGuid: string;
  plans: { plan: CustomPlan; name: string; description: string; changes: string[]; installed: boolean; active: boolean }[];
  canRevert: boolean;
  skipped: string[];
}

export interface BootTime {
  at: string;
  totalMs: number;
  mainMs: number;
  postMs: number;
}

export interface SlowItem {
  name: string;
  friendly: string;
  kind: "app" | "service";
  totalMs: number;
  degradationMs: number;
  at: string;
}

export interface BootReport {
  boots: BootTime[];
  slow: SlowItem[];
}

export interface BloatApp {
  name: string;
  label: string;
  description: string;
  installed: boolean;
  storeUrl: string;
}

export type AppErrorKind =
  | "requiresAdmin"
  | "elevationDenied"
  | "helper"
  | "notFound"
  | "invalidInput"
  | "commandFailed"
  | "unavailable"
  | "confirmationRequired"
  | "unsupported"
  | "io";

/** Situação do auxiliar de administrador (UAC sob demanda). */
export interface ElevationStatus {
  isAdmin: boolean;
  helperRunning: boolean;
}

export interface AppError {
  kind: AppErrorKind;
  message: string;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && "kind" in e && "message" in e;
}

export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** true quando roda dentro do Tauri (false no navegador comum, ex.: `npm run dev` sozinho). */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const api = {
  listStartupItems: () => invoke<StartupItem[]>("list_startup_items"),
  setStartupItemEnabled: (id: string, enabled: boolean) =>
    invoke<void>("set_startup_item_enabled", { id, enabled }),
  getPowerPlan: () => invoke<ActivePlan>("get_power_plan"),
  setPowerPlan: (plan: PowerPlan) => invoke<void>("set_power_plan", { plan }),
  scanCleanup: () => invoke<CategoryScan[]>("scan_cleanup"),
  runCleanup: (categories: CleanupCategory[]) => invoke<CleanupReport>("run_cleanup", { categories }),
  gamingDiagnose: () => invoke<GamingReport>("gaming_diagnose"),
  gamingFix: (id: CheckId) => invoke<void>("gaming_fix", { id }),
  gamingRevert: (id: CheckId) => invoke<void>("gaming_revert", { id }),
  gameWatchStatus: () => invoke<WatchStatus>("game_watch_status"),
  gameWatchSave: (config: WatchConfig) => invoke<WatchStatus>("game_watch_save", { config }),
  listRunningProcesses: () => invoke<string[]>("list_running_processes"),
  timerMeasure: () => invoke<TimerReport>("timer_measure"),
  /** [dispositivos, InstanceIds com valor anotado pelo app] */
  hwMsiList: () => invoke<[MsiDevice[], string[]]>("hw_msi_list"),
  hwMsiSet: (instanceId: string, enabled: boolean) => invoke<void>("hw_msi_set", { instanceId, enabled }),
  hwSteamGames: () => invoke<SteamGame[]>("hw_steam_games"),
  hwFseSet: (exePath: string, enabled: boolean) => invoke<void>("hw_fse_set", { exePath, enabled }),
  hwQosSet: (exePath: string, enabled: boolean) => invoke<void>("hw_qos_set", { exePath, enabled }),
  hwNicList: () => invoke<[NicProperty[], string[]]>("hw_nic_list"),
  /** `value: null` volta ao valor anotado. */
  hwNicSet: (adapter: string, keyword: string, value: string | null) => invoke<void>("hw_nic_set", { adapter, keyword, value }),
  gameSuggestions: () => invoke<GameEntry[]>("game_suggestions"),
  onGameSession: (cb: (s: WatchStatus) => void): Promise<UnlistenFn> =>
    listen<WatchStatus>("game-session", (e) => cb(e.payload)),
  tweaksList: () => invoke<TweakView[]>("tweaks_list"),
  tweakApply: (id: string, confirmed = false) => invoke<void>("tweak_apply", { id, confirmed }),
  tweakRevert: (id: string) => invoke<void>("tweak_revert", { id }),
  tweaksRevertAll: () => invoke<BatchReport>("tweaks_revert_all"),
  tweakSetFavorite: (id: string, favorite: boolean) => invoke<void>("tweak_set_favorite", { id, favorite }),
  profilesList: () => invoke<ProfileView[]>("profiles_list"),
  profileApply: (id: string, restorePoint: boolean) => invoke<ProfileApplyResult>("profile_apply", { id, restorePoint }),
  profileSave: (name: string) => invoke<ProfileView>("profile_save", { name }),
  profileDelete: (id: string) => invoke<void>("profile_delete", { id }),
  systemInfo: () => invoke<SystemInfo>("system_info", { refresh: false }),
  systemInfoRefresh: () => invoke<SystemInfo>("system_info", { refresh: true }),
  createRestorePoint: () => invoke<RestoreOutcome>("create_restore_point"),
  openSystemProtection: () => invoke<void>("open_system_protection"),
  restartExplorer: () => invoke<void>("restart_explorer"),
  monitorSample: () => invoke<MonitorSample>("monitor_sample"),
  hardwareInfo: () => invoke<HardwareInfo>("hardware_info"),
  driversInventory: () => invoke<DriverInventory>("drivers_inventory"),
  driversBackup: () => invoke<{ path: string; count: number }>("drivers_backup"),
  openOfficialUrl: (url: string) => invoke<void>("open_official_url", { url }),
  openBackupFolder: () => invoke<void>("open_backup_folder"),
  fixesList: () => invoke<FixView[]>("fixes_list"),
  fixRun: (id: string) => invoke<FixReport>("fix_run", { id }),
  netResetStatus: () => invoke<NetResetStatus>("net_reset_status"),
  netRestore: () => invoke<void>("net_restore"),
  netDiscard: () => invoke<void>("net_discard"),
  onFixProgress: (cb: (p: FixProgress) => void): Promise<UnlistenFn> =>
    listen<FixProgress>("fix-progress", (e) => cb(e.payload)),
  energyStatus: () => invoke<EnergyStatus>("energy_status"),
  energyApply: (plan: CustomPlan) => invoke<EnergyStatus>("energy_apply", { plan }),
  energyRevert: () => invoke<EnergyStatus>("energy_revert"),
  /** `ask`: pode abrir o UAC. Sem ele, só lê se já houver permissão nesta sessão. */
  bootReport: (ask = false) => invoke<BootReport>("boot_report", { ask }),
  elevationStatus: () => invoke<ElevationStatus>("elevation_status"),
  whatsNew: () => invoke<WhatsNew>("whats_new"),
  diagnosisRun: () => invoke<DiagnosisReport>("diagnosis_run"),
  whatsNewSeen: () => invoke<void>("whats_new_seen"),
  bloatList: () => invoke<BloatApp[]>("bloat_list"),
  bloatRemove: (name: string) => invoke<void>("bloat_remove", { name }),
  onCleanupProgress: (cb: (r: CategoryResult) => void): Promise<UnlistenFn> =>
    listen<CategoryResult>("cleanup-progress", (e) => cb(e.payload)),
};

export const CLEANUP_INFO: Record<CleanupCategory, { name: string; desc: string; defaultOn: boolean }> = {
  userTemp: {
    name: "Arquivos temporários do usuário",
    desc: "Só arquivos com mais de 24 h; os mais novos podem estar em uso por um instalador.",
    defaultOn: true,
  },
  windowsTemp: {
    name: "Temporários do Windows",
    desc: "Mesma regra de 24 h. Precisa de administrador.",
    defaultOn: true,
  },
  windowsUpdateCache: {
    name: "Cache do Windows Update",
    desc: "Atualizações já baixadas. Se alguma ainda for necessária, o Windows baixa de novo.",
    defaultOn: true,
  },
  shaderCache: {
    name: "Cache de shaders (DirectX)",
    desc: "Apagar faz o próximo jogo engasgar enquanto recompila os shaders. Só limpe se um jogo estiver com bug gráfico.",
    defaultOn: false,
  },
  recycleBin: {
    name: "Lixeira",
    desc: "Esvaziar é permanente.",
    defaultOn: true,
  },
};

export const PLAN_INFO: Record<PowerPlan, { name: string; desc: string; alias: string }> = {
  powerSaver: {
    name: "Economia de energia",
    desc: "Reduz clock e brilho. Bom para notebook na bateria.",
    alias: "SCHEME_MAX",
  },
  balanced: {
    name: "Balanceado",
    desc: "Padrão do Windows. Sobe o clock sob demanda.",
    alias: "SCHEME_BALANCED",
  },
  highPerformance: {
    name: "Alto desempenho",
    desc: "CPU não entra em estados profundos de economia. Mais consumo e calor.",
    alias: "SCHEME_MIN",
  },
};

export const CHECK_INFO: Record<CheckId, { name: string; desc: string; fixLabel: string }> = {
  refreshRate: {
    name: "Taxa de atualização do monitor",
    desc: "Monitor rodando abaixo do máximo é a causa mais comum de jogo “pesado”. Se o Windows não lista a taxa que o monitor promete, o limite é o cabo ou a porta (ex.: HDMI 1.4, saída da placa-mãe).",
    fixLabel: "Usar o máximo",
  },
  driverVsync: {
    name: "VSync forçado no driver",
    desc: "Adiciona até um quadro inteiro de atraso. No Painel de Controle NVIDIA / AMD Software, deixe “Sincronização vertical” como “Usar configuração do aplicativo” ou desligado.",
    fixLabel: "",
  },
  mouseAcceleration: {
    name: "Aceleração do mouse",
    desc: "“Aumentar precisão do ponteiro” muda a distância do cursor conforme a velocidade do movimento, o que deixa a memória muscular inconsistente.",
    fixLabel: "Desligar",
  },
  backgroundRecording: {
    name: "Gravação em segundo plano (Game Bar)",
    desc: "Grava os últimos minutos do jogo o tempo todo, usando o codificador de vídeo da GPU.",
    fixLabel: "Desligar",
  },
  hags: {
    name: "Agendamento de GPU acelerado por hardware",
    desc: "A própria GPU organiza a fila de quadros. Necessário pro DLSS Frame Generation; o ganho de latência varia por jogo. Exige reiniciar.",
    fixLabel: "Ativar",
  },
  gameMode: {
    name: "Modo de Jogo do Windows",
    desc: "Prioriza o jogo e segura atualizações do Windows Update durante a partida.",
    fixLabel: "Ligar",
  },
};

/** Executáveis conhecidos, pra adicionar com um clique. */
export const GAME_PRESETS: Omit<GameEntry, "highPriority">[] = [
  { name: "Counter-Strike 2", exe: "cs2.exe" },
  { name: "Valorant", exe: "VALORANT-Win64-Shipping.exe" },
  { name: "Fortnite", exe: "FortniteClient-Win64-Shipping.exe" },
  { name: "League of Legends", exe: "League of Legends.exe" },
  { name: "Apex Legends", exe: "r5apex.exe" },
  { name: "Rainbow Six Siege", exe: "RainbowSix.exe" },
  { name: "Rocket League", exe: "RocketLeague.exe" },
  { name: "OBS Studio", exe: "obs64.exe" },
  { name: "Premiere Pro", exe: "Adobe Premiere Pro.exe" },
  { name: "DaVinci Resolve", exe: "Resolve.exe" },
  { name: "Blender", exe: "blender.exe" },
];

/** Serviços que o Modo Jogo pausa, com o nome que o usuário entende. */
export const SERVICE_LABEL: Record<string, string> = {
  WSearch: "Indexação da pesquisa",
  SysMain: "Pré-carregamento (SysMain)",
};

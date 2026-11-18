import { useEffect, useState, type FormEvent } from "react";
import { X } from "lucide-react";

import { api, errorMessage, GAME_PRESETS, SERVICE_LABEL, type GameEntry, type TimerReport, type WatchConfig, type WatchStatus } from "../lib/api";
import { Switch } from "./ui";

/** "há 12 min" atualizando sozinho. */
function useElapsed(since: number | undefined) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!since) return;
    const t = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(t);
  }, [since]);
  if (!since) return "";
  const min = Math.max(0, Math.floor((now / 1000 - since) / 60));
  if (min < 1) return "agora há pouco";
  if (min < 60) return `há ${min} min`;
  return `há ${Math.floor(min / 60)} h ${min % 60} min`;
}

export function SessionCard({ status }: { status: WatchStatus }) {
  const { config, session } = status;
  const elapsed = useElapsed(session?.startedAt);
  const highApplied = new Set(status.highPriorityApplied.map((e) => e.toLowerCase()));

  let headline = "Modo Jogo desligado";
  if (config.enabled) headline = session ? `${session.game.name} aberto ${elapsed}` : "Aguardando um jogo ou programa da lista";

  return (
    <section className="card" aria-labelledby="sessao-titulo" aria-live="polite">
      <div className="card-head">
        <h2 id="sessao-titulo" className="card-title">
          {headline}
        </h2>
        {session && <span className="badge badge-ok">Em jogo</span>}
      </div>

      <div className="session-stats">
        <div>
          <span className="stat-label">Na lista</span>
          <strong>{config.games.length}</strong>
        </div>
        <div>
          <span className="stat-label">Apps em espera</span>
          <strong>{session?.lowered.length ?? 0}</strong>
        </div>
        <div>
          <span className="stat-label">Serviços pausados</span>
          <strong>{session?.pausedServices.length ?? 0}</strong>
        </div>
      </div>

      {session && (
        <div className="table-wrap" style={{ border: 0 }}>
          <table className="table" style={{ minWidth: 0 }}>
            <thead>
              <tr>
                <th scope="col">Processo</th>
                <th scope="col">Situação</th>
                <th scope="col" className="align-right">
                  Prioridade
                </th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td className="mono">{session.game.exe}</td>
                <td>jogo da lista</td>
                <td className="align-right">
                  {highApplied.has(session.game.exe.toLowerCase()) ? (
                    <span className="badge badge-ok">Alta</span>
                  ) : (
                    <span className="badge badge-idle">Normal</span>
                  )}
                </td>
              </tr>
              {session.lowered.map((l) => (
                <tr key={l.pid}>
                  <td className="mono">{l.exe}</td>
                  <td>segundo plano</td>
                  <td className="align-right">
                    <span className="badge badge-idle">Abaixo do normal</span>
                  </td>
                </tr>
              ))}
              {session.pausedServices.map((s) => (
                <tr key={s}>
                  <td className="mono">{s}</td>
                  <td>{SERVICE_LABEL[s] ?? "serviço"}</td>
                  <td className="align-right">
                    <span className="badge badge-warn">Pausado</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="muted small" style={{ margin: 0 }}>
        Fechou o jogo, tudo volta sozinho para o que estava antes. Sistema, antivírus e anticheat nunca entram na fila.
      </p>
    </section>
  );
}

interface EditorProps {
  status: WatchStatus;
  saving: boolean;
  save: (cfg: WatchConfig) => void;
}

export function GameListCard({ status, saving, save }: EditorProps) {
  const { config } = status;
  const [exe, setExe] = useState("");
  const [processes, setProcesses] = useState<string[]>([]);
  const added = new Set(config.games.map((g) => g.exe.toLowerCase()));
  const presets = GAME_PRESETS.filter((p) => !added.has(p.exe.toLowerCase()));
  const highApplied = new Set(status.highPriorityApplied.map((e) => e.toLowerCase()));

  const add = (g: Omit<GameEntry, "highPriority">) => save({ ...config, games: [...config.games, { ...g, highPriority: true }] });
  const update = (exe: string, patch: Partial<GameEntry>) =>
    save({ ...config, games: config.games.map((g) => (g.exe === exe ? { ...g, ...patch } : g)) });

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    const v = exe.trim();
    if (!v) return;
    add({ name: "", exe: v.toLowerCase().endsWith(".exe") ? v : `${v}.exe` });
    setExe("");
  }

  return (
    <section className="card" aria-labelledby="lista-titulo">
      <h2 id="lista-titulo" className="card-title">
        Jogos e programas
      </h2>
      <p className="plan-desc" style={{ margin: 0 }}>
        Vale pra jogo e pra programa pesado (OBS, Premiere, Blender). A prioridade alta fica gravada no Windows e vale toda
        vez que ele abre; o Skill Gap não abre o processo dele.
      </p>

      {config.games.length > 0 && (
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {config.games.map((g) => (
            <li key={g.exe} className="list-row" style={{ minHeight: 56, gap: 10 }}>
              <span className="list-row-text">
                <span style={{ fontWeight: 500 }}>{g.name}</span>
                <span className="mono small muted">{g.exe}</span>
              </span>
              <span className="small muted" style={{ textAlign: "right" }}>
                Prioridade alta
                {g.highPriority && config.enabled && !highApplied.has(g.exe.toLowerCase()) && (
                  <span className="badge badge-warn" style={{ marginLeft: 6 }}>
                    Pendente
                  </span>
                )}
              </span>
              <Switch
                checked={g.highPriority}
                disabled={saving}
                onChange={(v) => update(g.exe, { highPriority: v })}
                label={`Prioridade alta para ${g.name}`}
              />
              <button
                type="button"
                className="icon-btn"
                style={{ width: 36, height: 36 }}
                aria-label={`Remover ${g.name}`}
                disabled={saving}
                onClick={() => save({ ...config, games: config.games.filter((x) => x.exe !== g.exe) })}
              >
                <X size={16} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      )}

      <form onSubmit={onSubmit} style={{ display: "flex", gap: 8 }}>
        <label htmlFor="novo-jogo" className="sr-only">
          Executável do jogo ou programa
        </label>
        <input
          id="novo-jogo"
          className="input"
          list="processos"
          placeholder="Executável, ex.: cs2.exe"
          value={exe}
          onFocus={async () => {
            if (processes.length === 0) setProcesses(await api.listRunningProcesses().catch(() => []));
          }}
          onChange={(e) => setExe(e.target.value)}
          disabled={saving}
        />
        <datalist id="processos">
          {processes.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
        <button type="submit" className="btn btn-ghost" disabled={!exe.trim() || saving}>
          Adicionar
        </button>
      </form>

      {presets.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {presets.map((p) => (
            <button key={p.exe} type="button" className="btn btn-ghost btn-sm" disabled={saving} onClick={() => add(p)}>
              + {p.name}
            </button>
          ))}
        </div>
      )}
    </section>
  );
}

export function OptionsCard({ status, saving, save }: EditorProps) {
  const { config } = status;
  const [app, setApp] = useState("");

  function addApp(e: FormEvent) {
    e.preventDefault();
    const v = app.trim();
    if (!v) return;
    save({ ...config, background: [...config.background, v.toLowerCase().endsWith(".exe") ? v : `${v}.exe`] });
    setApp("");
  }

  const options: { key: "powerBoost" | "pauseServices" | "lowerBackground" | "timerMode" | "autoDetect"; title: string; desc: string }[] = [
    {
      key: "powerBoost",
      title: "Plano Alto desempenho",
      desc: "Durante o jogo, volta ao plano anterior ao fechar. Em notebook, esquenta e gasta bateria.",
    },
    {
      key: "pauseServices",
      title: "Pausar indexação e SysMain",
      desc: "Para a indexação da pesquisa e o pré-carregamento, que leem o disco em segundo plano. Precisa de admin.",
    },
    {
      key: "autoDetect",
      title: "Reconhecer jogos pela pasta",
      desc: "Também conta como jogo o que roda de dentro das pastas da Steam, Epic, Riot, Xbox, EA, Ubisoft e GOG. Lido pelo caminho do arquivo, sem abrir o processo.",
    },
    {
      key: "timerMode",
      title: "Modo Timer (0,5 ms)",
      desc: "Segura o timer fino do Windows enquanto o jogo roda. Só chega no jogo no Windows 11 com o “Timer Resolution global” ativo (veja a medição abaixo).",
    },
    {
      key: "lowerBackground",
      title: "Apps em segundo plano esperam",
      desc: "Os apps abaixo vão para prioridade “abaixo do normal”: só perdem a vez quando o jogo precisa da CPU.",
    },
  ];

  return (
    <section className="card" aria-labelledby="opcoes-titulo" style={{ gap: 0 }}>
      <h2 id="opcoes-titulo" className="card-title" style={{ paddingBottom: 8 }}>
        Durante o jogo
      </h2>
      {options.map((o) => (
        <div key={o.key} className="list-row">
          <span className="list-row-text">
            <span style={{ fontWeight: 500 }}>{o.title}</span>
            <span className="list-row-sub">{o.desc}</span>
          </span>
          <Switch checked={config[o.key]} disabled={saving} onChange={(v) => save({ ...config, [o.key]: v })} label={o.title} />
        </div>
      ))}

      {config.lowerBackground && (
        <div style={{ display: "flex", flexDirection: "column", gap: 10, paddingTop: 14 }}>
          <span className="stat-label">Apps que esperam</span>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            {config.background.map((b) => (
              <span key={b} className="tag">
                <span className="mono">{b}</span>
                <button
                  type="button"
                  aria-label={`Tirar ${b} da lista`}
                  disabled={saving}
                  onClick={() => save({ ...config, background: config.background.filter((x) => x !== b) })}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              </span>
            ))}
          </div>
          <form onSubmit={addApp} style={{ display: "flex", gap: 8 }}>
            <label htmlFor="novo-app" className="sr-only">
              Executável de um app de segundo plano
            </label>
            <input
              id="novo-app"
              className="input"
              placeholder="ex.: whatsapp.exe"
              value={app}
              onChange={(e) => setApp(e.target.value)}
              disabled={saving}
            />
            <button type="submit" className="btn btn-ghost btn-sm" disabled={!app.trim() || saving}>
              Adicionar
            </button>
          </form>
        </div>
      )}
    </section>
  );
}

/** Medição real do timer: o "antes" deste PC, sem alterar nada. */
export function TimerCard() {
  const [report, setReport] = useState<TimerReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const f = (n: number | null | undefined, d = 2) => (n === null || n === undefined ? "n/d" : `${n.toFixed(d)} ms`);

  async function measure() {
    setBusy(true);
    setError(null);
    try {
      setReport(await api.timerMeasure());
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card" aria-labelledby="timer-titulo">
      <div className="card-head">
        <h2 id="timer-titulo" className="card-title">
          Timer do Windows
        </h2>
        <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void measure()}>
          {busy ? "Medindo…" : "Medir agora"}
        </button>
      </div>
      <p className="muted small" style={{ margin: 0 }}>
        Mede 120 pausas de 1 ms de verdade. No padrão do Windows cada uma dura ~15,6 ms; com timer fino, ~1 ms. Não altera nada.
      </p>
      {error && <div className="error-box">{error}</div>}
      {report && (
        <>
          <dl className="spec-grid">
            <div>
              <dt>Sleep(1) médio</dt>
              <dd>{f(report.sleep?.meanMs)}</dd>
            </div>
            <div>
              <dt>Mín · Máx</dt>
              <dd>
                {f(report.sleep?.minMs)} · {f(report.sleep?.maxMs)}
              </dd>
            </div>
            <div>
              <dt>Desvio</dt>
              <dd>{f(report.sleep?.stddevMs)}</dd>
            </div>
            <div>
              <dt>Resolução atual</dt>
              <dd>
                {f(report.currentMs, 3)} <span className="muted small">(padrão {f(report.coarsestMs, 3)}, mais fina {f(report.finestMs, 3)})</span>
              </dd>
            </div>
          </dl>
          <p className={`small ${report.modeReachesGame ? "" : "warn-text"}`} style={{ margin: 0 }}>
            {report.modeReachesGame
              ? "Neste Windows, o Modo Timer do app chega até o jogo."
              : report.globalRequests
                ? "Neste Windows o pedido de timer vale só pra quem pede: o Modo Timer do app não chega no jogo."
                : "O Modo Timer só chega no jogo com o ajuste “Timer Resolution global” (Windows 11) ativo em Todos os ajustes."}
            {report.heldByApp && " O app está segurando o timer agora (partida em andamento)."}
          </p>
        </>
      )}
    </section>
  );
}

/** Jogos rodando agora que ainda não estão na lista. */
export function SuggestionsCard({ status, saving, save }: EditorProps) {
  const [list, setList] = useState<GameEntry[] | null>(null);
  async function look() {
    setList(await api.gameSuggestions().catch(() => []));
  }
  return (
    <section className="card" aria-labelledby="sug-titulo">
      <div className="card-head">
        <h2 id="sug-titulo" className="card-title">
          Jogos abertos agora
        </h2>
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => void look()}>
          Procurar
        </button>
      </div>
      <p className="muted small" style={{ margin: 0 }}>
        Abra o jogo e clique em Procurar: o app reconhece pela pasta de instalação, sem abrir o processo.
      </p>
      {list && list.length === 0 && <p className="small" style={{ margin: 0 }}>Nenhum jogo novo rodando agora.</p>}
      {list?.map((g) => (
        <div key={g.exe} className="list-row">
          <span className="list-row-text">
            <span style={{ fontWeight: 500 }}>{g.name}</span>
            <span className="list-row-sub mono">{g.exe}</span>
          </span>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            disabled={saving}
            onClick={() => {
              save({ ...status.config, games: [...status.config.games, g] });
              setList((l) => l?.filter((x) => x.exe !== g.exe) ?? null);
            }}
          >
            Adicionar
          </button>
        </div>
      ))}
    </section>
  );
}

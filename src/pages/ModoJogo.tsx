import { useCallback, useEffect, useState } from "react";
import { RotateCw, ShieldCheck } from "lucide-react";

import { api, CHECK_INFO, errorMessage, type CheckId, type GamingCheck, type WatchConfig } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { Notice, PageHeader, Switch } from "../components/ui";
import { GameListCard, OptionsCard, SessionCard, SuggestionsCard, TimerCard } from "../components/GameMode";
import { friendlyError } from "../components/feedback";

const BADGE: Record<GamingCheck["status"], string> = { ok: "badge-ok", warn: "badge-warn", info: "badge-idle" };

export function ModoJogo() {
  const { data: report, error, loading, reload } = useResource(api.gamingDiagnose);
  const [busy, setBusy] = useState<CheckId | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const watch = useResource(api.gameWatchStatus);
  const setWatch = watch.setData;
  const [saving, setSaving] = useState(false);

  // O backend avisa quando um jogo abre ou fecha: a tela atualiza sem ficar perguntando.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void api.onGameSession((s) => setWatch(s)).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setWatch]);

  const save = useCallback(
    async (cfg: WatchConfig) => {
      setSaving(true);
      setMessage(null);
      try {
        setWatch(await api.gameWatchSave(cfg));
      } catch (e) {
        setMessage(friendlyError(e));
      } finally {
        setSaving(false);
      }
    },
    [setWatch],
  );

  async function act(check: GamingCheck, action: "fix" | "revert") {
    setBusy(check.id);
    setMessage(null);
    try {
      await (action === "fix" ? api.gamingFix(check.id) : api.gamingRevert(check.id));
      if (check.requiresRestart) setMessage("Pronto. Essa mudança só vale depois de reiniciar o PC.");
      await reload();
    } catch (e) {
      setMessage(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  const checks = report?.checks ?? [];
  const okCount = checks.filter((c) => c.status === "ok").length;
  const primary = report?.displays[0];

  return (
    <>
      <PageHeader
        eyebrow="Jogos"
        title="Modo Jogo"
        subtitle="Menos atraso entre o clique e a tela. Nada aqui toca no processo do jogo."
        actions={
          <>
            <span className="check-line" style={{ cursor: "default" }}>
              <span style={{ fontWeight: 600 }}>Automático</span>
              <Switch
                checked={watch.data?.config.enabled ?? false}
                disabled={!watch.data || saving}
                onChange={(enabled) => watch.data && void save({ ...watch.data.config, enabled })}
                label="Ligar o Modo Jogo automático"
              />
            </span>
            <button type="button" className="btn btn-ghost" onClick={() => void reload()} disabled={loading || busy !== null}>
              <RotateCw size={16} aria-hidden="true" />
              Verificar de novo
            </button>
          </>
        }
      />

      {error ? <div className="error-box">Não foi possível verificar: {errorMessage(error)}</div> : null}
      {watch.data?.lastError && <div className="error-box">{watch.data.lastError}</div>}
      {watch.data && <SessionCard status={watch.data} />}
      {message && (
        <div className="error-box" role="status">
          {message}
        </div>
      )}

      <div className="row-wrap">
        <section
          className="card"
          aria-labelledby="diag-titulo"
          style={{ flex: "3 1 480px", minWidth: 0, gap: 0, paddingTop: 16, paddingBottom: 8 }}
        >
          <div className="card-head" style={{ paddingBottom: 8 }}>
            <h2 id="diag-titulo" className="card-title">
              Diagnóstico de latência
            </h2>
            <span className="mono small muted">{loading ? "verificando…" : `${okCount} de ${checks.length} ok`}</span>
          </div>

          {checks.map((check) => {
            const info = CHECK_INFO[check.id];
            const isBusy = busy === check.id;
            return (
              <div key={check.id} className="list-row" style={{ alignItems: "flex-start", padding: "14px 0" }}>
                <span className="list-row-text">
                  <span style={{ fontWeight: 600 }}>
                    {info.name}
                    {check.requiresAdmin && (
                      <span className="badge badge-idle" style={{ marginLeft: 8 }} title="O Windows pede permissão de administrador na hora">
                        Pede permissão
                      </span>
                    )}
                  </span>
                  <span className="list-row-sub" style={{ lineHeight: 1.5 }}>
                    {info.desc}
                  </span>
                </span>
                <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 8, flexShrink: 0 }}>
                  <span className={`badge ${BADGE[check.status]}`}>{check.current}</span>
                  <div style={{ display: "flex", gap: 6 }}>
                    {check.canRevert && (
                      <button
                        type="button"
                        className="btn btn-ghost btn-sm"
                        disabled={busy !== null}
                        onClick={() => void act(check, "revert")}
                      >
                        Desfazer
                      </button>
                    )}
                    {check.fixable && (
                      <button
                        type="button"
                        className="btn btn-primary btn-sm"
                        disabled={busy !== null}
                        onClick={() => void act(check, "fix")}
                      >
                        {isBusy ? "Aplicando…" : info.fixLabel}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </section>

        <div style={{ flex: "2 1 300px", minWidth: 0, display: "flex", flexDirection: "column", gap: 16 }}>
          <section className="card" aria-labelledby="fps-titulo">
            <span id="fps-titulo" className="stat-label">
              Limite de FPS sugerido
            </span>
            {report?.fpsCap && primary ? (
              <>
                <span className="stat-value" style={{ color: "var(--accent)", fontSize: 44 }}>
                  {report.fpsCap}
                  <span className="muted" style={{ fontSize: 15, fontWeight: 500, marginLeft: 8, fontFamily: "var(--font-body)" }}>
                    FPS · monitor de {primary.maxHz} Hz
                  </span>
                </span>
                <p className="plan-desc" style={{ margin: 0 }}>
                  Com a GPU sempre em 100%, os quadros fazem fila e chegam atrasados. Um limite um pouco abaixo da taxa
                  do monitor corta essa fila e mantém o G-Sync/FreeSync ativo. É o mesmo cálculo do limitador do NVIDIA
                  Reflex. Configure no jogo ou no painel da GPU.
                </p>
              </>
            ) : (
              <p className="plan-desc" style={{ margin: 0 }}>
                {loading ? "Detectando monitor…" : "Monitor não detectado."}
              </p>
            )}
          </section>
          {watch.data && <TimerCard />}
        </div>
      </div>

      {watch.data && (
        <div className="row-wrap">
          <div style={{ flex: "1 1 380px", minWidth: 0, display: "flex", flexDirection: "column", gap: 16 }}>
            <GameListCard status={watch.data} saving={saving} save={(c) => void save(c)} />
            <SuggestionsCard status={watch.data} saving={saving} save={(c) => void save(c)} />
          </div>
          <div style={{ flex: "1 1 380px", minWidth: 0, display: "flex", flexDirection: "column", gap: 16 }}>
            <OptionsCard status={watch.data} saving={saving} save={(c) => void save(c)} />
          </div>
        </div>
      )}

      <Notice title="Seguro com anticheat" icon={<ShieldCheck size={18} aria-hidden="true" />}>
        O Skill Gap só ajusta o Windows: não injeta nada, não abre nem lê o processo do jogo e não instala driver. A
        prioridade do jogo é definida pelo próprio Windows quando ele inicia (registro IFEO).
      </Notice>
    </>
  );
}

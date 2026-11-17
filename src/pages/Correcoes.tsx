import { useEffect, useState } from "react";
import { CircleCheck, CircleX } from "lucide-react";

import { api, type FixProgress, type FixReport, type FixView } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { friendlyError, RestartNotice } from "../components/feedback";

export function Correcoes() {
  const { data: fixes } = useResource(api.fixesList);
  const [running, setRunning] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [progress, setProgress] = useState<FixProgress | null>(null);
  const [reports, setReports] = useState<Record<string, FixReport>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void api.onFixProgress(setProgress).then((fn) => (cancelled ? fn() : (unlisten = fn)));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  async function run(fix: FixView) {
    // Correções demoradas ou que pedem reinício confirmam antes (dois cliques).
    const needsConfirm = fix.restart !== "none" || fix.duration.includes("minuto");
    if (needsConfirm && confirming !== fix.id) {
      setConfirming(fix.id);
      return;
    }
    setConfirming(null);
    setRunning(fix.id);
    setProgress(null);
    setErrors((e) => ({ ...e, [fix.id]: "" }));
    try {
      const report = await api.fixRun(fix.id);
      setReports((r) => ({ ...r, [fix.id]: report }));
    } catch (e) {
      setErrors((x) => ({ ...x, [fix.id]: friendlyError(e) }));
    } finally {
      setRunning(null);
      setProgress(null);
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="Manutenção"
        title="Correções"
        subtitle="Reparos conhecidos do Windows, com as ferramentas do próprio sistema. Uma por vez, com o log de cada passo."
      />

      <div className="fix-list">
        {(fixes ?? []).map((fix) => {
          const report = reports[fix.id];
          const isRunning = running === fix.id;
          return (
            <article key={fix.id} className="card" aria-labelledby={`fix-${fix.id}`}>
              <div className="card-head" style={{ alignItems: "flex-start" }}>
                <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                  <h2 id={`fix-${fix.id}`} className="card-title">
                    {fix.name}
                  </h2>
                  <p className="tweak-desc">{fix.description}</p>
                  <p className="small" style={{ margin: 0 }}>
                    <span className="muted">Quando usar:</span> {fix.when}
                  </p>
                  <div className="tweak-badges">
                    <span className="badge badge-idle">{fix.duration}</span>
                    {fix.requiresAdmin && <span className="badge badge-idle" title="O Windows pede permissão de administrador na hora de aplicar">Pede permissão</span>}
                    {fix.restart === "reboot" && <span className="badge badge-info">Reiniciar o PC</span>}
                  </div>
                </div>
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  style={{ flexShrink: 0 }}
                  disabled={running !== null}
                  onClick={() => void run(fix)}
                >
                  {isRunning ? "Rodando…" : confirming === fix.id ? "Confirmar" : "Executar"}
                </button>
              </div>

              {confirming === fix.id && !isRunning && (
                <p className="small" role="status" style={{ margin: 0 }}>
                  Clique em <strong>Confirmar</strong> pra rodar.{" "}
                  {fix.restart === "reboot" ? "Vai ser preciso reiniciar o PC depois. " : ""}
                  Duração: {fix.duration}.
                </p>
              )}
              {isRunning && progress?.id === fix.id && (
                <p className="small" role="status" style={{ margin: 0 }}>
                  Passo {progress.step} de {progress.total}: {progress.label}… <span className="muted">({fix.duration})</span>
                </p>
              )}
              {errors[fix.id] && <div className="error-box">{errors[fix.id]}</div>}

              {report && !isRunning && (
                <div className="fix-report" aria-live="polite">
                  {report.steps.map((s, i) => (
                    <details key={i} open={!s.ok}>
                      <summary>
                        {s.ok ? (
                          <CircleCheck size={16} aria-hidden="true" style={{ color: "var(--ok)" }} />
                        ) : (
                          <CircleX size={16} aria-hidden="true" style={{ color: "var(--accent)" }} />
                        )}
                        <span>{s.label}</span>
                        <span className="sr-only">{s.ok ? "concluído" : "falhou"}</span>
                      </summary>
                      {s.output && <pre>{s.output}</pre>}
                    </details>
                  ))}
                  {report.ok ? <RestartNotice restart={report.restart} /> : null}
                </div>
              )}
            </article>
          );
        })}
      </div>
    </>
  );
}

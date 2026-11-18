import { useEffect, useState } from "react";
import { CircleCheck, CircleX } from "lucide-react";

import { api, describeAdapter, type FixProgress, type FixReport, type FixView } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { friendlyError, RestartNotice } from "../components/feedback";

export function Correcoes() {
  const { data: fixes } = useResource(api.fixesList);
  const net = useResource(api.netResetStatus);
  const [netBusy, setNetBusy] = useState(false);
  const [netMsg, setNetMsg] = useState<string | null>(null);
  const saved = net.data?.pending?.adapters.filter((a) => a.ipv4 || a.dns4.length || a.dns6.length) ?? [];

  async function netAction(kind: "restore" | "discard") {
    setNetBusy(true);
    setNetMsg(null);
    try {
      await (kind === "restore" ? api.netRestore() : api.netDiscard());
      setNetMsg(kind === "restore" ? "Configuração de rede restaurada." : "Configuração guardada descartada.");
      await net.reload();
    } catch (e) {
      setNetMsg(friendlyError(e));
    } finally {
      setNetBusy(false);
    }
  }
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
      if (fix.id === "network_reset") await net.reload();
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

      {net.data?.needsReboot && (
        <div className="card" role="status" style={{ borderColor: "var(--soft-border)" }}>
          <strong>Reinicie o PC pra concluir o reset de rede.</strong>
          <p className="muted small" style={{ margin: 0 }}>
            Até reiniciar, a rede fica num estado intermediário e o reset não pode ser rodado de novo.
            {saved.length > 0 && " Depois de reiniciar, a configuração que você tinha aparece aqui para restaurar."}
          </p>
        </div>
      )}
      {!net.data?.needsReboot && saved.length > 0 && (
        <div className="card" role="status" style={{ borderColor: "var(--soft-border)" }}>
          <strong>Restaurar a configuração de rede que você tinha</strong>
          <p className="muted small" style={{ margin: 0 }}>O reset apagou isto, que estava configurado à mão:</p>
          <ul className="small" style={{ margin: 0, paddingLeft: 18 }}>
            {saved.map((a) => (
              <li key={a.guid}>{describeAdapter(a)}</li>
            ))}
          </ul>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button type="button" className="btn btn-primary btn-sm" disabled={netBusy} onClick={() => void netAction("restore")}>
              {netBusy ? "Restaurando…" : "Restaurar"}
            </button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={netBusy} onClick={() => void netAction("discard")}>
              Manter no automático
            </button>
          </div>
        </div>
      )}
      {netMsg && <p className="small" role="status">{netMsg}</p>}

      <div className="fix-list">
        {(fixes ?? []).map((fix) => {
          const report = reports[fix.id];
          const isRunning = running === fix.id;
          return (
            <article key={fix.id} className="tweak-card" aria-labelledby={`fix-${fix.id}`}>
              <div className="tweak-head">
                <h2 id={`fix-${fix.id}`} className="tweak-name">
                  {fix.name}
                </h2>
              </div>
              <p className="tweak-desc">{fix.description}</p>
              <p className="small" style={{ margin: 0 }}>
                <span className="muted">Quando usar:</span> {fix.when}
              </p>
              <div className="tweak-badges">
                <span className="badge badge-idle">{fix.duration}</span>
                {fix.requiresAdmin && <span className="badge badge-idle" title="O Windows pede permissão de administrador na hora de aplicar">Pede permissão</span>}
                {fix.restart === "reboot" && <span className="badge badge-info">Reiniciar o PC</span>}
              </div>

              {fix.id === "network_reset" && (net.data?.current.length ?? 0) > 0 && !net.data?.needsReboot && (
                <div className="small" style={{ margin: 0 }}>
                  <strong className="warn-text">Este PC tem configuração de rede fixa, que o reset apaga:</strong>
                  <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
                    {net.data!.current.map((a) => (
                      <li key={a.guid}>{describeAdapter(a)}</li>
                    ))}
                  </ul>
                  <span className="muted">O app guarda isso antes e oferece restaurar depois de reiniciar.</span>
                </div>
              )}
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

              <div className="tweak-foot" style={{ justifyContent: "flex-end" }}>
            <button
              type="button"
              className="btn btn-primary btn-sm"
              style={{ flexShrink: 0 }}
              disabled={running !== null || (fix.id === "network_reset" && (net.data?.needsReboot || saved.length > 0))}
              onClick={() => void run(fix)}
            >
              {isRunning ? "Rodando…" : confirming === fix.id ? "Confirmar" : "Executar"}
            </button>
              </div>
            </article>
          );
        })}
      </div>
    </>
  );
}

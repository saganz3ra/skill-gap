import { useState } from "react";

import { api, type CustomPlan, type EnergyStatus } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { friendlyError } from "./feedback";

/** Os dois planos próprios do Skill Gap (cópias de planos do Windows com ajustes). */
export function EnergyPlans({ onChange }: { onChange: () => void }) {
  const status = useResource(api.energyStatus);
  const sys = useResource(api.systemInfo);
  const [busy, setBusy] = useState<CustomPlan | "revert" | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function run(action: () => Promise<EnergyStatus>, key: CustomPlan | "revert") {
    setBusy(key);
    setMessage(null);
    try {
      status.setData(await action());
      onChange();
    } catch (e) {
      setMessage(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  const recommended: CustomPlan | null = sys.data ? (sys.data.isLaptop ? "notebook" : "desktop") : null;

  return (
    <section aria-labelledby="planos-sg" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <div className="card-head" style={{ alignItems: "center" }}>
        <h2 id="planos-sg" className="card-title">
          Planos do Skill Gap
        </h2>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          {status.data && (
            <span className="muted small">
              Ativo: <strong style={{ color: "var(--text)" }}>{status.data.activeName || "—"}</strong>
            </span>
          )}
          <button
            type="button"
            className="btn btn-ghost btn-sm"
            disabled={!status.data?.canRevert || busy !== null}
            onClick={() => void run(api.energyRevert, "revert")}
          >
            {busy === "revert" ? "Voltando…" : "Desfazer"}
          </button>
        </div>
      </div>

      {status.error ? <div className="error-box">{friendlyError(status.error)}</div> : null}
      {message && <div className="error-box">{message}</div>}
      {status.data && status.data.skipped.length > 0 && (
        <p className="muted small" style={{ margin: 0 }}>
          Este PC não tem algumas opções, que ficaram de fora: {status.data.skipped.join("; ")}.
        </p>
      )}

      <div className="plan-grid">
        {(status.data?.plans ?? []).map((p) => (
          <article key={p.plan} className={`tweak-card${p.active ? " tweak-on" : ""}`}>
            <div className="tweak-head">
              <h3 className="tweak-name">{p.plan === "desktop" ? "Plano PC de mesa" : "Plano Notebook"}</h3>
              {p.active ? (
                <span className="badge badge-ok">Ativo</span>
              ) : (
                recommended === p.plan && <span className="badge badge-info">Indicado pro seu PC</span>
              )}
            </div>
            <p className="tweak-desc">{p.description}</p>
            <ul className="change-list">
              {p.changes.map((c) => (
                <li key={c}>{c}</li>
              ))}
            </ul>
            <div className="tweak-foot">
              <span className="muted small">{p.installed ? "Criado no Windows" : "Ainda não criado"}</span>
              <button
                type="button"
                className="btn btn-primary btn-sm"
                disabled={busy !== null || p.active}
                onClick={() => void run(() => api.energyApply(p.plan), p.plan)}
              >
                {busy === p.plan ? "Aplicando…" : p.active ? "Em uso" : "Aplicar"}
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

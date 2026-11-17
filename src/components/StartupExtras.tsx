import { useState } from "react";
import { ExternalLink } from "lucide-react";

import { api, isAppError, type BloatApp, type BootReport } from "../lib/api";
import { formatMs } from "../lib/format";
import { useResource } from "../hooks/useResource";
import { friendlyError } from "./feedback";

/** "2026-11-17T08:00:02" → "17/11 às 08:00" */
function when(iso: string) {
  const [date, time] = iso.split("T");
  const [, m, d] = (date ?? "").split("-");
  return d && m && time ? `${d}/${m} às ${time.slice(0, 5)}` : iso;
}

export function BootCard({
  report,
  error,
  loading,
  onAsk,
}: {
  report?: BootReport;
  error: unknown;
  loading: boolean;
  onAsk: () => Promise<void>;
}) {
  const last = report?.boots[0];
  const needsAdmin = !report && isAppError(error) && error.kind === "requiresAdmin";
  const [asking, setAsking] = useState(false);

  return (
    <section className="card" aria-labelledby="boot-titulo">
      <div className="card-head">
        <h2 id="boot-titulo" className="card-title">
          Tempo de boot e login
        </h2>
        {last && <span className="muted small">Último boot: {when(last.at)}</span>}
      </div>

      {loading && <p className="muted small" style={{ margin: 0 }}>Lendo o registro de desempenho do Windows…</p>}
      {needsAdmin && (
        <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
          <p className="muted small" style={{ margin: 0, flex: "1 1 260px" }}>
            O Windows só libera o registro de desempenho do boot pra administrador. Não pedimos isso sozinhos ao abrir a tela.
          </p>
          <button
            type="button"
            className="btn btn-primary btn-sm"
            disabled={asking}
            onClick={async () => {
              setAsking(true);
              await onAsk();
              setAsking(false);
            }}
          >
            {asking ? "Aguardando permissão…" : "Ver tempo de boot"}
          </button>
        </div>
      )}
      {error && !needsAdmin && !report ? <div className="error-box">{friendlyError(error)}</div> : null}
      {report && !last && <p className="muted small" style={{ margin: 0 }}>O Windows ainda não registrou nenhum boot.</p>}

      {last && (
        <>
          <div className="session-stats">
            <div>
              <span className="stat-label">Boot completo</span>
              <strong>{formatMs(last.totalMs)}</strong>
            </div>
            <div>
              <span className="stat-label">Até a tela de login</span>
              <strong>{formatMs(last.mainMs)}</strong>
            </div>
            <div>
              <span className="stat-label">Depois do login</span>
              <strong>{formatMs(last.postMs)}</strong>
            </div>
          </div>
          <p className="muted small" style={{ margin: 0 }}>
            “Depois do login” é o tempo até a área de trabalho ficar utilizável: é aí que os programas de inicialização
            pesam. A coluna <strong>Atraso no boot</strong> mostra quanto o Windows registrou que cada um atrasou.
          </p>
          {report && report.slow.filter((s) => s.kind === "service").length > 0 && (
            <p className="muted small" style={{ margin: 0 }}>
              Serviços que atrasaram:{" "}
              {report.slow
                .filter((s) => s.kind === "service")
                .slice(0, 5)
                .map((s) => `${s.friendly || s.name} (+${formatMs(s.degradationMs)})`)
                .join(", ")}
              .
            </p>
          )}
        </>
      )}
    </section>
  );
}

export function BloatApps() {
  const apps = useResource(api.bloatList);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const installed = (apps.data ?? []).filter((a) => a.installed);
  const removed = (apps.data ?? []).filter((a) => !a.installed);

  async function remove(app: BloatApp) {
    if (confirm !== app.name) {
      setConfirm(app.name);
      return;
    }
    setConfirm(null);
    setBusy(app.name);
    setMessage(null);
    try {
      await api.bloatRemove(app.name);
      await apps.reload();
    } catch (e) {
      setMessage(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="card" aria-labelledby="bloat-titulo" style={{ gap: 0 }}>
      <div className="card-head" style={{ paddingBottom: 6 }}>
        <h2 id="bloat-titulo" className="card-title">
          Apps de fábrica
        </h2>
        <span className="badge badge-warn">Reinstala pela Store</span>
      </div>
      <p className="plan-desc" style={{ margin: "0 0 8px" }}>
        Apps que o Windows instala sozinho. Remover vale só pra sua conta e libera um pouco de espaço e de itens em segundo
        plano. Xbox, Game Bar, Store, Fotos e Calculadora ficam de fora de propósito.
      </p>
      {apps.error ? <div className="error-box">{friendlyError(apps.error)}</div> : null}
      {message && <div className="error-box">{message}</div>}
      {apps.loading && <p className="muted small">Lendo apps instalados…</p>}

      {installed.map((a) => (
        <div key={a.name} className="list-row" style={{ minHeight: 56 }}>
          <span className="list-row-text">
            <span style={{ fontWeight: 500 }}>{a.label}</span>
            <span className="list-row-sub">{a.description}</span>
          </span>
          <button type="button" className="btn btn-ghost btn-sm" disabled={busy !== null} onClick={() => void remove(a)}>
            {busy === a.name ? "Removendo…" : confirm === a.name ? "Confirmar remoção" : "Remover"}
          </button>
        </div>
      ))}
      {apps.data && installed.length === 0 && (
        <p className="muted small" style={{ margin: "8px 0" }}>
          Nenhum app da lista instalado.
        </p>
      )}

      {removed.length > 0 && (
        <details style={{ paddingTop: 10 }}>
          <summary className="small muted" style={{ cursor: "pointer", minHeight: 32 }}>
            Não instalados ({removed.length}) · reinstalar pela Store
          </summary>
          {removed.map((a) => (
            <div key={a.name} className="list-row" style={{ minHeight: 48 }}>
              <span className="list-row-text">{a.label}</span>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void api.openOfficialUrl(a.storeUrl).catch((e) => setMessage(friendlyError(e)))}
              >
                <ExternalLink size={14} aria-hidden="true" />
                Abrir na Store
              </button>
            </div>
          ))}
        </details>
      )}
    </section>
  );
}

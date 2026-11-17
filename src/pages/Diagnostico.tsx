import { useState } from "react";
import { CircleCheck, CircleDashed, CircleX } from "lucide-react";

import { api, type ComparisonRow } from "../lib/api";
import { formatBytes } from "../lib/format";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { friendlyError } from "../components/feedback";

function show(row: ComparisonRow, value: unknown): string {
  if (value === null || value === undefined) return "—";
  if (row.kind === "bytes") return formatBytes(Number(value));
  if (row.kind === "score") return `${value}/100`;
  return String(value);
}

/** Melhorou, piorou ou igual (só quando há direção e os dois números). */
function trend(row: ComparisonRow): "better" | "worse" | null {
  if (row.lowerIsBetter === null || typeof row.before !== "number" || typeof row.now !== "number" || row.before === row.now) return null;
  return row.now < row.before === row.lowerIsBetter ? "better" : "worse";
}

export function Diagnostico() {
  const res = useResource(api.diagnosisRun);
  const [copied, setCopied] = useState<string | null>(null);

  async function copyReport() {
    try {
      await navigator.clipboard.writeText(await api.supportReport());
      setCopied("Relatório copiado. Cole no chamado ou na conversa com o suporte.");
    } catch (e) {
      setCopied(friendlyError(e));
    }
  }
  const d = res.data?.diagnosis;
  const c = res.data?.comparison;
  const pending = d?.findings.filter((f) => f.optimized === false) ?? [];

  return (
    <>
      <PageHeader
        eyebrow="Geral"
        title="Diagnóstico"
        subtitle="Lido do Windows agora: quanto do seu PC ainda está no padrão. Não é promessa de FPS."
      />
      {res.error ? <div className="error-box">{friendlyError(res.error)}</div> : null}
      <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => void copyReport()}>
          Copiar relatório para suporte
        </button>
        <span className="muted small">{copied ?? "Máquina e o que o app mudou, sem nome de usuário nem e-mail."}</span>
      </div>

      <section className="card" aria-labelledby="pot">
        <div className="card-head">
          <h2 id="pot" className="card-title">
            Potencial de otimização
          </h2>
          <button type="button" className="btn btn-ghost btn-sm" disabled={res.loading} onClick={() => void res.reload()}>
            {res.loading ? "Lendo…" : "Ler de novo"}
          </button>
        </div>
        <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
          <span className="stat-value">{d ? d.potential : "—"}</span>
          <span className="muted small">de 100 · quanto menor, mais otimizado</span>
        </div>
        <div className="progress" aria-hidden="true">
          <span style={{ width: `${d?.potential ?? 0}%` }} />
        </div>
        <p className="muted small" style={{ margin: 0 }}>
          A nota pesa {d?.findings.length ?? "—"} itens conhecidos e vem só do estado atual do Windows: se alguém desfizer algo
          por fora do app, ela sobe de novo.
        </p>
        <ul className="diag-list">
          {d?.findings.map((f) => (
            <li key={f.id}>
              {f.optimized === true ? (
                <CircleCheck size={16} aria-hidden="true" style={{ color: "var(--ok)" }} />
              ) : f.optimized === false ? (
                <CircleX size={16} aria-hidden="true" style={{ color: "var(--warn)" }} />
              ) : (
                <CircleDashed size={16} aria-hidden="true" className="muted" />
              )}
              <span>
                {f.label}
                {f.optimized === false && <span className="muted small"> — {f.hint}</span>}
                {f.optimized === null && <span className="muted small"> — não deu pra ler</span>}
              </span>
            </li>
          ))}
        </ul>
        {d && pending.length === 0 && <p className="small" style={{ margin: 0 }}>Nada pendente nos itens que a nota mede.</p>}
      </section>

      {c && (
        <section className="card" aria-labelledby="cmp">
          <div className="card-head">
            <h2 id="cmp" className="card-title">
              Antes e agora
            </h2>
            <span className="muted small">
              {c.baselineAt ? `Primeira leitura: ${new Date(c.baselineAt * 1000).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" })}` : ""}
            </span>
          </div>
          {c.afterChanges && (
            <p className="small warn-text" style={{ margin: 0 }}>
              A primeira leitura foi feita com ajustes já aplicados por uma versão anterior do app: o "antes" não é o Windows de fábrica.
            </p>
          )}
          {(["firm", "moment"] as const).map((group) => (
            <div key={group}>
              <h3 className="small" style={{ margin: "8px 0 4px" }}>
                {group === "firm" ? "Números firmes (configuração)" : "Leitura do momento (oscila a cada segundo)"}
              </h3>
              <table className="cmp-table">
                <thead>
                  <tr>
                    <th scope="col">Item</th>
                    <th scope="col">Antes</th>
                    <th scope="col">Agora</th>
                  </tr>
                </thead>
                <tbody>
                  {c.rows
                    .filter((r) => (group === "firm") === r.firm)
                    .map((r) => {
                      const t = trend(r);
                      return (
                        <tr key={r.label}>
                          <td>{r.label}</td>
                          <td>{show(r, r.before)}</td>
                          <td className={t === "better" ? "ok-text" : t === "worse" ? "warn-text" : undefined}>{show(r, r.now)}</td>
                        </tr>
                      );
                    })}
                </tbody>
              </table>
            </div>
          ))}
        </section>
      )}
    </>
  );
}

import { useMemo, useState } from "react";
import { Link } from "react-router-dom";

import { api, NEWS_SEEN_EVENT, skippedByProfiles, CLEANUP_INFO, PLAN_INFO, type BatchReport, type RestoreOutcome } from "../lib/api";
import { formatBytes } from "../lib/format";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { BatchSummary, friendlyError, RestoreNotice } from "../components/feedback";
import { ReleaseCard } from "./Novidades";

export function Inicio() {
  const startup = useResource(api.listStartupItems);
  const power = useResource(api.getPowerPlan);
  const cleanup = useResource(api.scanCleanup);
  const tweaks = useResource(api.tweaksList);
  const profiles = useResource(api.profilesList);
  const sys = useResource(api.systemInfo);
  const news = useResource(api.whatsNew);
  const guardian = useResource(api.guardianStatus);
  const latestNews = news.data?.unseen[0];

  const [restorePoint, setRestorePoint] = useState<boolean | null>(null);
  const [busy, setBusy] = useState<"optimize" | "restore" | "undo" | null>(null);
  const [report, setReport] = useState<{ verb: string; report: BatchReport } | null>(null);
  const [restore, setRestore] = useState<RestoreOutcome | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const names = useMemo(() => new Map((tweaks.data ?? []).map((t) => [t.id, t.name])), [tweaks.data]);
  const recommended = profiles.data?.find((p) => p.id === sys.data?.recommendedProfile);

  // Progresso = ajustes do perfil recomendado que já estão ativos (sem contar os que o
  // perfil pula em notebook).
  const target = useMemo(() => {
    if (!recommended || !tweaks.data) return [];
    const byId = new Map(tweaks.data.map((t) => [t.id, t]));
    return recommended.tweaks.map((id) => byId.get(id)).filter((t) => t && !skippedByProfiles(t));
  }, [recommended, tweaks.data, sys.data]);
  const done = target.filter((t) => t?.state === "active").length;
  const pct = target.length ? Math.round((done / target.length) * 100) : 0;
  const revertable = tweaks.data?.filter((t) => t.canRevert).length ?? 0;
  const wantsRestore = restorePoint ?? true;

  const activeStartup = startup.data?.filter((i) => i.enabled).length;
  const planName = power.data ? (power.data.plan ? PLAN_INFO[power.data.plan].name : power.data.name) : undefined;
  const reclaimable = cleanup.data
    ?.filter((s) => s.available && CLEANUP_INFO[s.category].defaultOn)
    .reduce((sum, s) => sum + s.bytes, 0);

  async function run(kind: "optimize" | "restore" | "undo") {
    setBusy(kind);
    setMessage(null);
    setReport(null);
    setRestore(null);
    try {
      if (kind === "optimize" && recommended) {
        const r = await api.profileApply(recommended.id, wantsRestore);
        setRestore(r.restorePoint);
        if (r.report) setReport({ verb: `aplicados pelo perfil ${recommended.name}`, report: r.report });
      } else if (kind === "undo") {
        setReport({ verb: "desfeitos", report: await api.tweaksRevertAll() });
      } else if (kind === "restore") {
        setRestore(await api.createRestorePoint());
      }
      await Promise.all([tweaks.reload(), power.reload()]);
    } catch (e) {
      setMessage(
        kind === "optimize" && wantsRestore
          ? `Nada foi alterado, porque o ponto de restauração falhou: ${friendlyError(e)}`
          : friendlyError(e),
      );
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <PageHeader eyebrow="Painel" title="Visão geral" subtitle="Analise o sistema e revise cada ajuste antes de aplicar." />

      {guardian.data && guardian.data.length > 0 && (
        <div className="card" role="status" style={{ borderColor: "var(--soft-border)" }}>
          <strong>
            O Windows desfez {guardian.data.length} {guardian.data.length === 1 ? "ajuste" : "ajustes"} que você tinha aplicado
          </strong>
          <p className="muted small" style={{ margin: 0 }}>
            Costuma acontecer depois de uma grande atualização: {guardian.data.map((g) => g.name).join(", ")}.
          </p>
          <div>
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={async () => {
                try {
                  setReport({ verb: "reaplicados", report: await api.guardianReapply() });
                } catch (e) {
                  setMessage(friendlyError(e));
                }
                await Promise.all([guardian.reload(), tweaks.reload()]);
              }}
            >
              Reaplicar
            </button>
          </div>
        </div>
      )}
      {news.data && latestNews && (
        <div className="news-banner">
          <ReleaseCard release={latestNews} fresh />
          <div style={{ display: "flex", gap: 8 }}>
            <button
              type="button"
              className="btn btn-primary btn-sm"
              onClick={async () => {
                await api.whatsNewSeen().catch(() => undefined);
                window.dispatchEvent(new Event(NEWS_SEEN_EVENT));
                await news.reload();
              }}
            >
              Entendi
            </button>
            {news.data.unseen.length > 1 && (
              <Link to="/novidades" className="btn btn-ghost btn-sm" style={{ textDecoration: "none" }}>
                Ver as {news.data.unseen.length} versões novas
              </Link>
            )}
          </div>
        </div>
      )}

      <section className="stat-grid" aria-label="Resumo">
        <Link to="/inicializacao" className="stat-card">
          <span className="stat-label">Inicialização</span>
          <span className="stat-value">{activeStartup === undefined ? "…" : `${activeStartup} ativos`}</span>
          <span className="stat-link">Revisar programas →</span>
        </Link>
        <Link to="/desempenho" className="stat-card">
          <span className="stat-label">Plano de energia</span>
          <span className="stat-value">{planName ?? "…"}</span>
          <span className="stat-link">Alterar plano →</span>
        </Link>
        <Link to="/limpeza" className="stat-card">
          <span className="stat-label">Espaço recuperável</span>
          <span className="stat-value">{reclaimable !== undefined ? formatBytes(reclaimable) : "…"}</span>
          <span className="stat-link">Revisar limpeza →</span>
        </Link>
      </section>

      <div className="row-wrap">
        <section className="card" aria-labelledby="otim-titulo" style={{ flex: "3 1 440px", minWidth: 0 }}>
          <div className="card-head">
            <h2 id="otim-titulo" className="card-title">
              Otimização
            </h2>
            {recommended && (
              <Link to="/perfis" className="muted small">
                Perfil {recommended.name} →
              </Link>
            )}
          </div>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
            <span className="stat-value">{tweaks.data ? `${pct}%` : "…"}</span>
            <span className="muted small">
              {done} de {target.length} ajustes recomendados ativos
            </span>
          </div>
          <div className="progress" aria-hidden="true">
            <span style={{ width: `${pct}%` }} />
          </div>
          <p className="muted small" style={{ margin: 0 }}>
            {sys.data?.reason ?? "Analisando o PC…"} O botão aplica o perfil recomendado; cada ajuste guarda o valor
            original e pode ser desfeito.
          </p>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 14 }}>
            <button
              type="button"
              className="btn btn-primary"
              disabled={busy !== null || !recommended || target.length === done}
              onClick={() => void run("optimize")}
            >
              {busy === "optimize" ? "Otimizando…" : target.length > 0 && target.length === done ? "Tudo otimizado" : "Otimizar agora"}
            </button>
            <label className="check-line small">
              <input type="checkbox" checked={wantsRestore} onChange={(e) => setRestorePoint(e.target.checked)} />
              Criar ponto de restauração antes
            </label>
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 8, borderTop: "1px solid var(--border)", paddingTop: 12 }}>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy !== null} onClick={() => void run("restore")}>
              {busy === "restore" ? "Criando…" : "Criar ponto agora"}
            </button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy !== null || revertable === 0} onClick={() => void run("undo")}>
              Desfazer tudo
            </button>
            <Link to="/ajustes" className="btn btn-ghost btn-sm" style={{ textDecoration: "none" }}>
              Ver todos os ajustes
            </Link>
          </div>
        </section>

        <section className="card" aria-labelledby="sis-titulo" style={{ flex: "2 1 280px", minWidth: 0 }}>
          <h2 id="sis-titulo" className="card-title">
            Sistema
          </h2>
          {sys.data ? (
            <dl className="spec-list">
              <div>
                <dt>Windows</dt>
                <dd>
                  {sys.data.windows} · {sys.data.build}
                </dd>
              </div>
              <div>
                <dt>Tipo</dt>
                <dd>{sys.data.isLaptop ? "Notebook" : "Desktop"}</dd>
              </div>
              <div>
                <dt>Memória</dt>
                <dd>{sys.data.ramGb} GB</dd>
              </div>
              <div>
                <dt>Disco do sistema</dt>
                <dd>{sys.data.systemDisk === "ssd" ? "SSD" : sys.data.systemDisk === "hdd" ? "HD mecânico" : "desconhecido"}</dd>
              </div>
              <div>
                <dt>Executando como</dt>
                <dd>{sys.data.isAdmin ? "Administrador" : "Usuário comum (pede permissão quando precisa)"}</dd>
              </div>
              <div>
                <dt>Ajustes ativos</dt>
                <dd>
                  {tweaks.data?.filter((t) => t.state === "active").length ?? "…"} de {tweaks.data?.length ?? "…"}
                </dd>
              </div>
            </dl>
          ) : (
            <p className="muted small">Analisando…</p>
          )}
        </section>
      </div>

      {message && (
        <div className="error-box" role="status">
          {message}
        </div>
      )}
      {restore && <RestoreNotice outcome={restore} applied={report !== null} />}
      {report && <BatchSummary report={report.report} names={names} verb={report.verb} />}
    </>
  );
}

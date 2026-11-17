import { useMemo, useState } from "react";
import { Trash2 } from "lucide-react";

import { api, type BatchReport, type RestoreOutcome, type ProfileView, type SystemInfo } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { BatchSummary, friendlyError, RestoreNotice } from "../components/feedback";

const DISK_LABEL: Record<SystemInfo["systemDisk"], string> = { ssd: "SSD", hdd: "HD mecânico", unknown: "Não identificado" };

export function Perfis() {
  const profiles = useResource(api.profilesList);
  const tweaks = useResource(api.tweaksList);
  const sys = useResource(api.systemInfo);
  const [restorePoint, setRestorePoint] = useState<boolean | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [report, setReport] = useState<{ profile: string; report: BatchReport } | null>(null);
  const [restore, setRestore] = useState<RestoreOutcome | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const names = useMemo(() => new Map((tweaks.data ?? []).map((t) => [t.id, t.name])), [tweaks.data]);
  const active = useMemo(
    () => new Set((tweaks.data ?? []).filter((t) => t.state === "active").map((t) => t.id)),
    [tweaks.data],
  );
  const laptopSkip = useMemo(
    () => new Set(sys.data?.isLaptop ? (tweaks.data ?? []).filter((t) => t.laptopUnsafe).map((t) => t.id) : []),
    [tweaks.data, sys.data],
  );
  // Ponto de restauração exige admin: marcado por padrão só quando dá pra criar.
  const wantsRestore = restorePoint ?? true;
  const recommended = sys.data?.recommendedProfile;

  async function apply(p: ProfileView) {
    setBusy(p.id);
    setMessage(null);
    setReport(null);
    setRestore(null);
    try {
      const r = await api.profileApply(p.id, wantsRestore);
      setRestore(r.restorePoint);
      if (r.report) setReport({ profile: p.name, report: r.report });
      await tweaks.reload();
    } catch (e) {
      setMessage(
        wantsRestore
          ? `Nada foi alterado, porque o ponto de restauração falhou: ${friendlyError(e)} Se quiser seguir sem ele, desmarque a opção.`
          : friendlyError(e),
      );
    } finally {
      setBusy(null);
    }
  }

  async function remove(p: ProfileView) {
    try {
      await api.profileDelete(p.id);
      await profiles.reload();
    } catch (e) {
      setMessage(friendlyError(e));
    }
  }

  const sorted = [...(profiles.data ?? [])].sort((a, b) => Number(b.id === recommended) - Number(a.id === recommended));

  return (
    <>
      <PageHeader
        eyebrow="Geral"
        title="Perfis"
        subtitle="Conjuntos prontos de ajustes. Aplicar só liga o que falta; cada ajuste continua com seu desfazer."
      />

      <section className="card" aria-labelledby="seu-pc">
        <div className="card-head">
          <h2 id="seu-pc" className="card-title">
            Perfil para o seu PC
          </h2>
          {sys.loading && <span className="muted small">Analisando…</span>}
        </div>
        {sys.data && (
          <>
            <dl className="spec-grid">
              <div>
                <dt>Sistema</dt>
                <dd>
                  {sys.data.windows} · build {sys.data.build}
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
                <dd>{DISK_LABEL[sys.data.systemDisk]}</dd>
              </div>
              <div>
                <dt>Executando como</dt>
                <dd>{sys.data.isAdmin ? "Administrador" : "Usuário comum (pede permissão quando precisa)"}</dd>
              </div>
            </dl>
            <p style={{ margin: 0 }}>
              Recomendado: <strong>{profiles.data?.find((p) => p.id === recommended)?.name ?? recommended}</strong>.{" "}
              <span className="muted">{sys.data.reason}</span>
            </p>
          </>
        )}
      </section>

      <label className="check-line">
        <input type="checkbox" checked={wantsRestore} onChange={(e) => setRestorePoint(e.target.checked)} />
        Criar ponto de restauração antes de aplicar
        {sys.data && !sys.data.isAdmin && <span className="muted small"> (o Windows vai pedir permissão)</span>}
      </label>

      {message && (
        <div className="error-box" role="status">
          {message}
        </div>
      )}
      {restore && <RestoreNotice outcome={restore} applied={report !== null} />}
      {report && <BatchSummary report={report.report} names={names} verb={`aplicados pelo perfil ${report.profile}`} />}

      <div className="tweak-grid">
        {sorted.map((p) => {
          const missing = p.tweaks.filter((id) => !active.has(id) && !laptopSkip.has(id)).length;
          return (
            <article key={p.id} className={`tweak-card${p.id === recommended ? " tweak-on" : ""}`}>
              <div className="tweak-head">
                <h2 className="tweak-name">{p.name}</h2>
                {p.id === recommended && <span className="badge badge-ok">Recomendado</span>}
                {!p.builtin && (
                  <button type="button" className="icon-btn star" aria-label={`Apagar perfil ${p.name}`} onClick={() => void remove(p)}>
                    <Trash2 size={16} aria-hidden="true" />
                  </button>
                )}
              </div>
              <p className="tweak-desc">{p.description}</p>
              <div className="tweak-badges">
                {p.tweaks.map((id) => (
                  <span key={id} className={`badge ${active.has(id) ? "badge-ok" : "badge-idle"}`}>
                    {names.get(id) ?? id}
                  </span>
                ))}
              </div>
              <div className="tweak-foot">
                <span className="muted small">{missing === 0 ? "Tudo aplicado" : `${missing} a aplicar`}</span>
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  disabled={busy !== null || missing === 0}
                  onClick={() => void apply(p)}
                >
                  {busy === p.id ? "Aplicando…" : "Aplicar"}
                </button>
              </div>
            </article>
          );
        })}
      </div>
    </>
  );
}

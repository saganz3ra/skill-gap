import { useEffect, useRef, useState } from "react";
import { RotateCw } from "lucide-react";

import {
  api,
  CLEANUP_INFO,
  errorMessage,
  type CategoryResult,
  type CategoryScan,
  type CleanupCategory,
  type CleanupReport,
} from "../lib/api";
import { formatBytes } from "../lib/format";
import { useResource } from "../hooks/useResource";
import { Notice, PageHeader } from "../components/ui";

type Phase = "idle" | "confirm" | "running" | "done";

export function Limpeza() {
  // Ao abrir, aceita o escaneamento guardado (vale 1 min, dividido com o Início);
  // "Escanear de novo" pede um novo de verdade.
  const fresh = useRef(false);
  const loadScans = useRef(() => api.scanCleanup(fresh.current)).current;
  const { data: scans, error, loading, reload } = useResource(loadScans);
  const [selected, setSelected] = useState<Set<CleanupCategory>>(new Set());
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<Partial<Record<CleanupCategory, CategoryResult>>>({});
  const [report, setReport] = useState<CleanupReport | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const confirmTimer = useRef<number | undefined>(undefined);

  // Pré-seleciona as categorias recomendadas que existem e têm algo pra limpar.
  useEffect(() => {
    if (!scans) return;
    setSelected(new Set(scans.filter((s) => s.available && s.bytes > 0 && CLEANUP_INFO[s.category].defaultOn).map((s) => s.category)));
  }, [scans]);

  useEffect(() => () => window.clearTimeout(confirmTimer.current), []);

  const selectedBytes = (scans ?? []).filter((s) => selected.has(s.category)).reduce((sum, s) => sum + s.bytes, 0);
  const busy = loading || phase === "running";

  function toggle(category: CleanupCategory) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(category)) next.delete(category);
      else next.add(category);
      return next;
    });
  }

  // Confirmação em dois cliques: apagar arquivos (e a Lixeira) não tem desfazer.
  function onCleanClick() {
    if (phase !== "confirm") {
      setPhase("confirm");
      confirmTimer.current = window.setTimeout(() => setPhase((p) => (p === "confirm" ? "idle" : p)), 4000);
      return;
    }
    window.clearTimeout(confirmTimer.current);
    void run();
  }

  async function run() {
    setPhase("running");
    setProgress({});
    setReport(null);
    setRunError(null);
    // Escuta o progresso ANTES de disparar, senão o primeiro evento pode se perder.
    const unlisten = await api.onCleanupProgress((r) => setProgress((p) => ({ ...p, [r.category]: r })));
    try {
      const order = (scans ?? []).map((s) => s.category).filter((c) => selected.has(c));
      setReport(await api.runCleanup(order));
      setPhase("done");
      await reload();
    } catch (e) {
      setRunError(errorMessage(e));
      setPhase("idle");
    } finally {
      unlisten();
    }
  }

  const buttonLabel =
    phase === "running"
      ? "Limpando…"
      : phase === "confirm"
        ? `Confirmar: apagar ${formatBytes(selectedBytes)}`
        : "Limpar agora";

  return (
    <>
      <PageHeader
        eyebrow="Manutenção"
        title="Limpeza"
        subtitle="Somente categorias seguras de apagar. Nada é removido sem a sua confirmação."
        actions={
          <>
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => {
                setProgress({});
                setReport(null);
                setPhase("idle");
                fresh.current = true;
                void reload();
              }}
              disabled={busy}
            >
              <RotateCw size={16} aria-hidden="true" />
              Analisar
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={onCleanClick}
              disabled={busy || selected.size === 0 || selectedBytes === 0}
            >
              {buttonLabel}
            </button>
          </>
        }
      />

      {error ? <div className="error-box">Não foi possível analisar: {errorMessage(error)}</div> : null}
      {runError && <div className="error-box">A limpeza falhou: {runError}</div>}

      {report && phase === "done" && <ReportCard report={report} />}

      <section className="card" aria-label="Categorias de limpeza" style={{ gap: 0, paddingTop: 8, paddingBottom: 8 }}>
        <div className="card-head" style={{ padding: "8px 0" }}>
          <span className="stat-label">{loading ? "Analisando…" : "Selecionado"}</span>
          <span className="mono" style={{ fontWeight: 500 }}>
            {loading ? "…" : formatBytes(selectedBytes)}
          </span>
        </div>
        {(scans ?? []).map((scan) => (
          <CategoryRow
            key={scan.category}
            scan={scan}
            checked={selected.has(scan.category)}
            disabled={busy || !scan.available}
            onToggle={() => toggle(scan.category)}
            result={progress[scan.category]}
            running={phase === "running" && selected.has(scan.category) && !progress[scan.category]}
          />
        ))}
      </section>

      <Notice title="Por que não limpamos “RAM” nem a pasta Prefetch">
        O Windows usa a memória livre como cache e a Prefetch para abrir programas mais rápido. Apagar isso deixa o
        PC mais lento, não mais rápido.
      </Notice>
    </>
  );
}

interface RowProps {
  scan: CategoryScan;
  checked: boolean;
  disabled: boolean;
  onToggle: () => void;
  result?: CategoryResult;
  running: boolean;
}

function CategoryRow({ scan, checked, disabled, onToggle, result, running }: RowProps) {
  const info = CLEANUP_INFO[scan.category];
  const id = `cat-${scan.category}`;

  let status: React.ReactNode;
  if (running) {
    status = <span className="badge badge-info">Limpando…</span>;
  } else if (result) {
    status = <ResultBadge scan={scan} result={result} />;
  } else if (!scan.available) {
    status = <span className="badge badge-idle">Não encontrado</span>;
  } else {
    status = (
      <span className="mono" style={{ fontWeight: 500, minWidth: 80, textAlign: "right" }}>
        {formatBytes(scan.bytes)}
      </span>
    );
  }

  return (
    <div className="list-row" style={{ minHeight: 76 }}>
      <input
        id={id}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={onToggle}
        style={{ width: 18, height: 18, margin: 0, accentColor: "var(--brand)", flexShrink: 0 }}
      />
      <label htmlFor={id} className="list-row-text" style={{ cursor: disabled ? "default" : "pointer" }}>
        <span style={{ fontWeight: 600 }}>
          {info.name}
          {scan.requiresAdmin && (
            <span className="badge badge-idle" style={{ marginLeft: 8 }} title="O Windows pede permissão de administrador na hora de limpar">
              Pede permissão
            </span>
          )}
        </span>
        <span className="list-row-sub">{info.desc}</span>
        {scan.path && (
          <span className="mono small muted" style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {scan.path}
            {scan.partial && " · parte da pasta não pôde ser lida"}
          </span>
        )}
      </label>
      {status}
    </div>
  );
}

function ResultBadge({ scan, result }: { scan: CategoryScan; result: CategoryResult }) {
  if (result.error) return <span className="badge badge-warn" title={result.error}>Erro</span>;
  // Pasta do sistema, nada apagado e tudo pulado: quase sempre é falta de admin.
  if (scan.requiresAdmin && result.deletedFiles === 0 && result.skippedFiles > 0) {
    return <span className="badge badge-warn">Precisa de admin</span>;
  }
  const skipped = result.skippedFiles > 0 ? ` · ${result.skippedFiles} em uso` : "";
  return <span className="badge badge-ok">{`${formatBytes(result.freedBytes)} liberados${skipped}`}</span>;
}

function ReportCard({ report }: { report: CleanupReport }) {
  const skipped = report.results.reduce((s, r) => s + r.skippedFiles, 0);
  return (
    <section className="card" aria-live="polite" style={{ borderColor: "var(--soft-border)" }}>
      <span className="stat-label">Limpeza concluída</span>
      <span className="stat-value">{formatBytes(report.freedBytes)} liberados</span>
      <p className="muted small" style={{ margin: 0 }}>
        {report.diskFreedBytes !== null &&
          `Espaço livre no disco aumentou ${formatBytes(report.diskFreedBytes)} (pode diferir um pouco: tamanho em disco ≠ tamanho do arquivo, e outros programas escrevem no disco ao mesmo tempo). `}
        {skipped > 0 && `${skipped} arquivos estavam em uso ou eram recentes e foram mantidos.`}
      </p>
    </section>
  );
}

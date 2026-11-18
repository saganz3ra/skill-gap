import { useMemo, useState, type FormEvent } from "react";
import { ChevronDown, Search, Star } from "lucide-react";

import { api, CATEGORY_LABEL, RESTART_LABEL, type Restart, type TweakCategory, type TweakView } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader, Switch } from "../components/ui";
import { BatchSummary, friendlyError, RestartNotice } from "../components/feedback";
import type { BatchReport } from "../lib/api";

type Filter = "todos" | "favoritos" | TweakCategory;
const FILTERS: Filter[] = ["todos", "favoritos", "geral", "aparencia", "desempenho", "privacidade", "gaming", "rede"];

export function Ajustes() {
  const { data: tweaks, error, loading, reload, setData } = useResource(api.tweaksList);
  const news = useResource(api.whatsNew);
  const fresh = useMemo(() => new Set(news.data?.newTweaks ?? []), [news.data]);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("todos");
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [restart, setRestart] = useState<Restart>("none");
  const [profileName, setProfileName] = useState("");
  const [revertReport, setRevertReport] = useState<BatchReport | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (tweaks ?? []).filter((t) => {
      if (filter === "favoritos" && !t.favorite) return false;
      if (filter !== "todos" && filter !== "favoritos" && t.category !== filter) return false;
      return !q || `${t.name} ${t.description}`.toLowerCase().includes(q);
    });
  }, [tweaks, query, filter]);

  const activeCount = tweaks?.filter((t) => t.state === "active").length ?? 0;
  const revertable = tweaks?.filter((t) => t.canRevert).length ?? 0;
  const names = useMemo(() => new Map((tweaks ?? []).map((t) => [t.id, t.name])), [tweaks]);

  async function toggle(t: TweakView, on: boolean, confirmed = false) {
    // Risco alto: pede confirmação no card antes (o backend também recusa sem ela).
    if (on && t.risk === "high" && !confirmed) {
      setConfirming(t.id);
      return;
    }
    setConfirming(null);
    setBusy(t.id);
    setMessage(null);
    setRevertReport(null);
    try {
      await (on ? api.tweakApply(t.id, confirmed) : api.tweakRevert(t.id));
      // Guarda a reinicialização mais pesada pendente.
      setRestart((r) => (order(t.restart) > order(r) ? t.restart : r));
      // Só o cartão mexido: reler a lista inteira abre dezenas de processos no Windows.
      const st = await api.tweakStatus(t.id);
      setData((list) => list?.map((x) => (x.id === t.id ? { ...x, ...st } : x)));
    } catch (e) {
      setMessage(`${t.name}: ${friendlyError(e)}`);
    } finally {
      setBusy(null);
    }
  }

  async function favorite(t: TweakView) {
    const next = !t.favorite;
    setData((list) => list?.map((x) => (x.id === t.id ? { ...x, favorite: next } : x)));
    try {
      await api.tweakSetFavorite(t.id, next);
    } catch (e) {
      setData((list) => list?.map((x) => (x.id === t.id ? { ...x, favorite: !next } : x)));
      setMessage(friendlyError(e));
    }
  }

  async function saveProfile(e: FormEvent) {
    e.preventDefault();
    setMessage(null);
    try {
      const p = await api.profileSave(profileName);
      setProfileName("");
      setMessage(`Perfil “${p.name}” salvo com ${p.tweaks.length} ajustes. Ele aparece na tela Perfis.`);
    } catch (err) {
      setMessage(friendlyError(err));
    }
  }

  async function revertAll() {
    setBusy("*");
    setMessage(null);
    try {
      const report = await api.tweaksRevertAll();
      setRevertReport(report);
      setRestart("none");
      await reload();
    } catch (e) {
      setMessage(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="Otimização"
        title="Todos os ajustes"
        subtitle={tweaks ? `${tweaks.length} ajustes reversíveis. Cada um guarda o valor original antes de mudar.` : "Lendo ajustes…"}
        actions={
          <>
            <span className="badge badge-ok">
              {activeCount} de {tweaks?.length ?? 0} ativos
            </span>
            <button type="button" className="btn btn-ghost" disabled={revertable === 0 || busy !== null} onClick={() => void revertAll()}>
              Desfazer tudo
            </button>
          </>
        }
      />

      {error ? <div className="error-box">Não foi possível ler os ajustes: {friendlyError(error)}</div> : null}
      {message && (
        <div className="error-box" role="status">
          {message}
        </div>
      )}
      {revertReport && <BatchSummary report={revertReport} names={names} verb="desfeitos" />}
      {!revertReport && <RestartNotice restart={restart} />}

      <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
        <label className="search">
          <Search size={16} aria-hidden="true" />
          <span className="sr-only">Buscar ajustes</span>
          <input
            type="search"
            placeholder="Buscar ajustes por nome ou descrição…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <form onSubmit={saveProfile} style={{ display: "flex", gap: 8, flex: "1 1 320px" }}>
          <label htmlFor="nome-perfil" className="sr-only">
            Nome do perfil
          </label>
          <input
            id="nome-perfil"
            className="input"
            style={{ fontFamily: "var(--font-body)" }}
            placeholder="Salvar os ajustes ativos como um perfil…"
            value={profileName}
            maxLength={40}
            onChange={(e) => setProfileName(e.target.value)}
          />
          <button type="submit" className="btn btn-ghost" disabled={!profileName.trim() || activeCount === 0}>
            Salvar perfil
          </button>
        </form>
      </div>

      <div role="group" aria-label="Filtrar por categoria" style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
        {FILTERS.map((f) => (
          <button
            key={f}
            type="button"
            className={`chip${filter === f ? " chip-on" : ""}`}
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
          >
            {f === "todos" ? "Todos" : f === "favoritos" ? "Favoritos" : CATEGORY_LABEL[f]}
          </button>
        ))}
      </div>

      <div className="tweak-grid">
        {visible.map((t) => (
          <TweakCard
            key={t.id}
            tweak={t}
            fresh={fresh.has(t.id)}
            busy={busy === t.id || busy === "*"}
            onToggle={(on) => void toggle(t, on)}
            confirming={confirming === t.id}
            onConfirm={() => void toggle(t, true, true)}
            onCancel={() => setConfirming(null)}
            onFavorite={() => void favorite(t)}
          />
        ))}
        {!loading && visible.length === 0 && <p className="muted">Nenhum ajuste encontrado.</p>}
      </div>
    </>
  );
}

const RESTART_ORDER: Restart[] = ["none", "explorer", "logoff", "reboot"];
const order = (r: Restart) => RESTART_ORDER.indexOf(r);

const STATE_BADGE: Record<TweakView["state"], [string, string] | null> = {
  active: null,
  inactive: null,
  partial: ["badge-warn", "Parcial"],
  unknown: ["badge-idle", "Não deu pra ler"],
};

function TweakCard({
  tweak: t,
  busy,
  fresh,
  onToggle,
  onFavorite,
  confirming,
  onConfirm,
  onCancel,
}: {
  tweak: TweakView;
  busy: boolean;
  /** Entrou depois da última versão que o usuário abriu. */
  fresh: boolean;
  onToggle: (on: boolean) => void;
  onFavorite: () => void;
  confirming: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const [open, setOpen] = useState(false);
  const on = t.state === "active";
  // Ativo sem foto = já estava assim no Windows; não temos um "antes" pra voltar.
  const lockedOn = on && !t.canRevert;
  // Indisponível bloqueia só o APLICAR: se já está aplicado, o desfazer continua livre.
  const blocked = !on && t.gate.status === "unavailable";
  const restartLabel = RESTART_LABEL[t.restart];
  const stateBadge = STATE_BADGE[t.state];
  const detailsId = `det-${t.id}`;

  return (
    <article className="tweak-card">
      <div className="tweak-head">
        <h2 className="tweak-name">{t.name}</h2>
        <button
          type="button"
          className="icon-btn star"
          aria-pressed={t.favorite}
          aria-label={t.favorite ? `Tirar ${t.name} dos favoritos` : `Favoritar ${t.name}`}
          onClick={onFavorite}
        >
          <Star size={16} aria-hidden="true" fill={t.favorite ? "currentColor" : "none"} />
        </button>
      </div>
      <p className="tweak-desc">{t.description}</p>

      <div className="tweak-badges">
        {fresh && <span className="badge badge-info">Novo</span>}
        <span className="badge badge-idle">{CATEGORY_LABEL[t.category]}</span>
        {t.impact === "small" && <span className="badge badge-warn">Efeito pequeno</span>}
        {t.impact === "high" && <span className="badge badge-ok">Efeito alto</span>}
        {t.risk === "medium" && <span className="badge badge-warn">Risco médio</span>}
        {t.risk === "high" && <span className="badge badge-danger">Risco alto</span>}
        {t.heats && <span className="badge badge-warn" title="Aumenta temperatura e consumo; fica fora dos perfis">Esquenta</span>}
        {t.gate.status === "unavailable" && <span className="badge badge-idle">Indisponível neste PC</span>}
        {t.requiresAdmin && <span className="badge badge-idle" title="O Windows pede permissão de administrador na hora de aplicar">Pede permissão</span>}
        {restartLabel && <span className="badge badge-info">{restartLabel}</span>}
        {stateBadge && <span className={`badge ${stateBadge[0]}`}>{stateBadge[1]}</span>}
      </div>

      {t.gate.status !== "ok" && (
        <p className={`small ${t.gate.status === "warn" ? "warn-text" : "muted"}`} style={{ margin: 0 }}>
          {t.gate.status === "warn" ? "Atenção: " : "Indisponível: "}
          {t.gate.reason}.
        </p>
      )}
      {t.breaks && (
        <p className="small muted" style={{ margin: 0 }}>
          <strong>Pode afetar:</strong> {t.breaks}
        </p>
      )}

      {open && (
        <p id={detailsId} className="tweak-details">
          {t.details}
        </p>
      )}

      {confirming && (
        <div className="error-box" role="alertdialog" aria-label={`Confirmar ${t.name}`}>
          <strong>Risco alto.</strong> {t.breaks} Dá pra desfazer depois, mas confirme que quer aplicar.
          <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 8 }}>
            <button type="button" className="btn btn-primary btn-sm" style={{ whiteSpace: "nowrap" }} onClick={onConfirm}>
              Aplicar mesmo assim
            </button>
            <button type="button" className="btn btn-ghost btn-sm" onClick={onCancel}>
              Cancelar
            </button>
          </div>
        </div>
      )}

      <div className="tweak-foot">
        <button type="button" className="link-btn" aria-expanded={open} aria-controls={detailsId} onClick={() => setOpen((o) => !o)}>
          Detalhes <ChevronDown size={14} aria-hidden="true" style={{ transform: open ? "rotate(180deg)" : undefined }} />
        </button>
        <span title={lockedOn ? "Já estava assim no seu Windows antes do Skill Gap" : blocked ? "Não serve para este PC" : undefined}>
          <Switch
            checked={on}
            disabled={busy || lockedOn || blocked || t.state === "unknown"}
            onChange={onToggle}
            label={`${on ? "Desfazer" : "Aplicar"} ${t.name}`}
          />
        </span>
      </div>
    </article>
  );
}

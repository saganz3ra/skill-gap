import { useMemo, useState } from "react";
import { RotateCw } from "lucide-react";

import { api, errorMessage, type StartupItem } from "../lib/api";
import { exeOf, formatMs } from "../lib/format";
import { BootCard, BloatApps } from "../components/StartupExtras";
import { useResource } from "../hooks/useResource";
import { PageHeader, Switch } from "../components/ui";
import { friendlyError } from "../components/feedback";

export function Inicializacao() {
  const { data: items, error, loading, reload, setData } = useResource(api.listStartupItems);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [toast, setToast] = useState<string | null>(null);

  async function toggle(item: StartupItem, enabled: boolean) {
    // Atualização otimista: a chave muda na hora; se o backend falhar, volta.
    setData((list) => list?.map((i) => (i.id === item.id ? { ...i, enabled } : i)));
    setPending((p) => new Set(p).add(item.id));
    try {
      await api.setStartupItemEnabled(item.id, enabled);
    } catch (e) {
      setData((list) => list?.map((i) => (i.id === item.id ? { ...i, enabled: item.enabled } : i)));
      setToast(friendlyError(e));
      window.setTimeout(() => setToast(null), 5000);
    } finally {
      setPending((p) => {
        const next = new Set(p);
        next.delete(item.id);
        return next;
      });
    }
  }

  const active = items?.filter((i) => i.enabled).length ?? 0;
  const boot = useResource(api.bootReport);
  // Atraso que o Windows registrou pra cada executável (log de desempenho de boot).
  const delayByExe = useMemo(
    () => new Map((boot.data?.slow ?? []).filter((s) => s.kind === "app").map((s) => [s.name.toLowerCase(), s.degradationMs])),
    [boot.data],
  );
  const delayOf = (item: StartupItem) => {
    const exe = exeOf(item.command)?.toLowerCase();
    return exe ? delayByExe.get(exe) : undefined;
  };
  const total = items?.length ?? 0;

  return (
    <>
      <PageHeader
        eyebrow="Otimização"
        title="Inicialização"
        subtitle={items ? `${active} de ${total} programas iniciam com o Windows` : "Lendo programas de inicialização…"}
        actions={
          <button type="button" className="btn btn-ghost" onClick={() => void reload()} disabled={loading}>
            <RotateCw size={16} aria-hidden="true" />
            Atualizar
          </button>
        }
      />

      <BootCard
        report={boot.data}
        error={boot.error}
        loading={boot.loading}
        onAsk={async () => {
          try {
            boot.setData(await api.bootReport(true));
          } catch (e) {
            setToast(friendlyError(e));
            window.setTimeout(() => setToast(null), 5000);
          }
        }}
      />

      {error ? <div className="error-box">Não foi possível ler a inicialização: {errorMessage(error)}</div> : null}

      {items && (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th scope="col">Programa</th>
                <th scope="col">Origem</th>
                <th scope="col">Atraso no boot</th>
                <th scope="col">Status</th>
                <th scope="col" className="align-right">
                  Ativo
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => (
                <tr key={item.id}>
                  <td>
                    <div className="cell-main">
                      <span className="cell-name">{item.name}</span>
                      <span className="cell-sub" title={item.command}>
                        {item.command}
                      </span>
                    </div>
                  </td>
                  <td>
                    <span className="mono small muted">{item.sourceLabel}</span>
                    {item.requiresAdmin && (
                      <span className="badge badge-idle" style={{ marginLeft: 8 }} title="O Windows pede permissão de administrador na hora">
                        Pede permissão
                      </span>
                    )}
                  </td>
                  <td>
                    {delayOf(item) !== undefined ? (
                      <span className="badge badge-warn">+{formatMs(delayOf(item)!)}</span>
                    ) : (
                      <span className="muted small">—</span>
                    )}
                  </td>
                  <td>
                    <span className={`badge ${item.enabled ? "badge-ok" : "badge-idle"}`}>
                      {item.enabled ? "Ativo" : "Desativado"}
                    </span>
                  </td>
                  <td className="align-right">
                    <Switch
                      checked={item.enabled}
                      disabled={pending.has(item.id)}
                      onChange={(v) => void toggle(item, v)}
                      label={`${item.enabled ? "Desativar" : "Ativar"} ${item.name} na inicialização`}
                    />
                  </td>
                </tr>
              ))}
              {items.length === 0 && (
                <tr>
                  <td colSpan={5} className="muted">
                    Nenhum programa configurado para iniciar com o Windows.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}

      <p className="muted small">
        Desativar não desinstala nem apaga nada: o Skill Gap usa o mesmo marcador do Gerenciador de Tarefas,
        então dá pra reativar aqui ou lá.
      </p>

      <BloatApps />

      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
    </>
  );
}

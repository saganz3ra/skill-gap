import { useState } from "react";
import { api, errorMessage, isAppError, type BatchReport, type Restart } from "../lib/api";

/** Mensagem de erro amigável, com o caso de admin explicado. */
export function friendlyError(e: unknown): string {
  if (isAppError(e) && e.kind === "requiresAdmin") {
    // Só chega aqui em tarefa de segundo plano (que nunca abre o UAC) ou se o Windows
    // negou mesmo com o auxiliar elevado.
    return "Isso exige permissão de administrador e o Windows não liberou agora. Tente de novo pelo botão: o Windows vai mostrar o pedido de permissão.";
  }
  if (isAppError(e) && e.kind === "elevationDenied") {
    return "Você recusou o pedido de permissão do Windows. Nada foi alterado.";
  }
  return errorMessage(e);
}

/** Diz o que falta pra mudança valer e, quando dá, resolve com um botão. */
export function RestartNotice({ restart }: { restart: Restart }) {
  const [state, setState] = useState<"idle" | "busy" | "done" | string>("idle");
  if (restart === "none") return null;

  if (restart === "explorer") {
    return (
      <div className="notice" role="status" style={{ alignItems: "center" }}>
        <div style={{ flexGrow: 1 }}>
          <strong>Falta reiniciar o Explorer</strong>
          <p>A barra de tarefas some por um segundo e volta com o ajuste aplicado. Janelas abertas não fecham.</p>
          {state !== "idle" && state !== "busy" && state !== "done" && <p>{state}</p>}
        </div>
        <button
          type="button"
          className="btn btn-primary btn-sm"
          disabled={state === "busy" || state === "done"}
          onClick={async () => {
            setState("busy");
            try {
              await api.restartExplorer();
              setState("done");
            } catch (e) {
              setState(errorMessage(e));
            }
          }}
        >
          {state === "done" ? "Reiniciado" : "Reiniciar Explorer"}
        </button>
      </div>
    );
  }

  return (
    <div className="notice" role="status">
      <div>
        <strong>{restart === "reboot" ? "Reinicie o PC pra concluir" : "Saia e entre na conta pra concluir"}</strong>
        <p>Alguns ajustes só são lidos pelo Windows na inicialização. Até lá, eles aparecem como aplicados mas ainda não fazem efeito.</p>
      </div>
    </div>
  );
}

/** Resumo de uma aplicação em lote (perfil ou desfazer tudo). */
export function BatchSummary({ report, names, verb }: { report: BatchReport; names: Map<string, string>; verb: string }) {
  const label = (id: string) => names.get(id) ?? id;
  return (
    <section className="card" aria-live="polite" style={{ borderColor: "var(--soft-border)" }}>
      <span className="stat-label">Concluído</span>
      <p style={{ margin: 0 }}>
        {report.applied.length === 0 && report.failed.length === 0
          ? "Nada pra mudar: tudo já estava como deveria."
          : `${report.applied.length} ${report.applied.length === 1 ? "ajuste" : "ajustes"} ${verb}.`}
      </p>
      {report.skipped.length > 0 && (
        <p className="muted small" style={{ margin: 0 }}>
          Pulados por ser notebook (esquentam ou gastam bateria): {report.skipped.map(label).join(", ")}.
        </p>
      )}
      {report.failed.length > 0 && (
        <div className="error-box">
          <strong>Não deu pra aplicar:</strong>
          <ul style={{ margin: "6px 0 0", paddingLeft: 18 }}>
            {report.failed.map((f) => (
              <li key={f.id}>
                {label(f.id)}: {f.error}
              </li>
            ))}
          </ul>
        </div>
      )}
      <RestartNotice restart={report.restart} />
    </section>
  );
}

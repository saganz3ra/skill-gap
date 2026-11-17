import { useEffect, useRef, useState } from "react";

import { api, type TweakView } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { Notice, PageHeader, Switch } from "../components/ui";
import { friendlyError } from "../components/feedback";

const MOUSE_TWEAKS = ["mouse_accel_off", "mouse_speed_default"];
const STANDARD_RATES = [125, 250, 500, 1000, 2000, 4000, 8000];

/** Taxa padrão mais próxima (em escala logarítmica, já que elas dobram). */
export function nearestRate(hz: number): number {
  return STANDARD_RATES.reduce((best, r) => (Math.abs(Math.log(r / hz)) < Math.abs(Math.log(best / hz)) ? r : best));
}

export function Mouse() {
  const tweaks = useResource(api.tweaksList);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const mouseTweaks = (tweaks.data ?? []).filter((t) => MOUSE_TWEAKS.includes(t.id));

  async function toggle(t: TweakView, on: boolean) {
    setBusy(t.id);
    setMessage(null);
    try {
      await (on ? api.tweakApply(t.id) : api.tweakRevert(t.id));
      await tweaks.reload();
    } catch (e) {
      setMessage(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="Jogos"
        title="Mouse"
        subtitle="Ajustes do Windows que mexem no mouse e o teste de taxa de resposta. Os ajustes são os mesmos de “Todos os ajustes”."
      />
      {message && <div className="error-box">{message}</div>}

      <section className="card" aria-labelledby="mouse-ajustes" style={{ gap: 0 }}>
        <h2 id="mouse-ajustes" className="card-title" style={{ paddingBottom: 8 }}>
          Ajustes do Windows
        </h2>
        {mouseTweaks.map((t) => (
          <div key={t.id} className="list-row">
            <span className="list-row-text">
              <span style={{ fontWeight: 500 }}>{t.name}</span>
              <span className="list-row-sub">{t.details}</span>
            </span>
            <Switch
              checked={t.state === "active"}
              disabled={busy !== null || (t.state === "active" && !t.canRevert) || t.state === "unknown"}
              onChange={(v) => void toggle(t, v)}
              label={t.name}
            />
          </div>
        ))}
      </section>

      <PollingTest />

      <Notice title="Por que não “turbinamos” a taxa do mouse">
        Subir a taxa acima do que o fabricante oferece exige instalar um driver de terceiros e desligar a verificação de
        assinatura do Windows, e Vanguard e FACEIT não rodam assim. Se o seu mouse suporta 1000 Hz ou mais, escolha no
        software do próprio fabricante.
      </Notice>
    </>
  );
}

function PollingTest() {
  const area = useRef<HTMLDivElement>(null);
  const stamps = useRef<number[]>([]);
  const [live, setLive] = useState(0);
  const [peaks, setPeaks] = useState<number[]>([]);
  const [api] = useState<"raw" | "coalesced">(() => ("onpointerrawupdate" in window ? "raw" : "coalesced"));

  useEffect(() => {
    const el = area.current;
    if (!el) return;

    const push = (t: number) => {
      const s = stamps.current;
      s.push(t);
      // Mantém só o último segundo.
      while (s.length && s[0]! < t - 1000) s.shift();
    };

    // pointerrawupdate entrega cada relatório do mouse; sem ele, os eventos "coalescidos"
    // do pointermove trazem os relatórios que o navegador juntou entre dois quadros.
    const onRaw = (e: Event) => push((e as PointerEvent).timeStamp);
    const onMove = (e: PointerEvent) => {
      const list = e.getCoalescedEvents?.() ?? [e];
      for (const c of list.length ? list : [e]) push(c.timeStamp);
    };

    if (api === "raw") el.addEventListener("pointerrawupdate", onRaw);
    else el.addEventListener("pointermove", onMove);

    // A cada 250 ms, a contagem do último segundo vira a taxa em Hz.
    const timer = window.setInterval(() => {
      const s = stamps.current;
      const now = performance.now();
      while (s.length && s[0]! < now - 1000) s.shift();
      const hz = s.length;
      setLive(hz);
      if (hz > 50) setPeaks((p) => [...p.slice(-39), hz]);
    }, 250);

    return () => {
      el.removeEventListener("pointerrawupdate", onRaw);
      el.removeEventListener("pointermove", onMove);
      window.clearInterval(timer);
    };
  }, [api]);

  // Mediana dos melhores momentos: um pico isolado não define a taxa.
  const top = [...peaks].sort((a, b) => b - a).slice(0, 8);
  const median = top.length ? top[Math.floor(top.length / 2)]! : 0;
  const estimate = median > 0 ? nearestRate(median) : null;

  return (
    <section className="card" aria-labelledby="hz-titulo">
      <div className="card-head">
        <h2 id="hz-titulo" className="card-title">
          Teste de taxa de resposta
        </h2>
        <button type="button" className="btn btn-ghost btn-sm" onClick={() => setPeaks([])}>
          Recomeçar
        </button>
      </div>
      <p className="plan-desc" style={{ margin: 0 }}>
        Quantas vezes por segundo o mouse informa a posição ao PC. Mova o mouse em círculos rápidos dentro da área por uns 3
        segundos.
      </p>
      <div ref={area} className="poll-area" role="img" aria-label={`Área de teste. Taxa atual ${live} Hz`}>
        <span className="poll-live">{live} Hz</span>
        <span className="muted small">agora</span>
      </div>
      <div className="session-stats" aria-live="polite">
        <div>
          <span className="stat-label">Medido</span>
          <strong>{median ? `${median} Hz` : "—"}</strong>
        </div>
        <div>
          <span className="stat-label">Taxa provável</span>
          <strong>{estimate ? `${estimate} Hz` : "—"}</strong>
        </div>
        <div>
          <span className="stat-label">Método</span>
          <strong style={{ fontSize: 14 }}>{api === "raw" ? "Eventos brutos" : "Eventos agrupados"}</strong>
        </div>
      </div>
      <p className="muted small" style={{ margin: 0 }}>
        A medição oscila um pouco: o mouse só envia relatório quando está se mexendo, e acima de 2000 Hz o próprio PC pode
        não acompanhar.
      </p>
    </section>
  );
}

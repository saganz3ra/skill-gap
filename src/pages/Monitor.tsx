import { useEffect, useRef, useState } from "react";

import { api, errorMessage, type MonitorSample } from "../lib/api";
import { formatBytes, formatRate } from "../lib/format";
import { OverlayCard } from "../components/Overlay";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { LiveChart } from "../components/LiveChart";

const CAPACITY = 60;
const pct = (v: number) => `${Math.round(v)}%`;

export function Monitor() {
  const hw = useResource(api.hardwareInfo);
  const [history, setHistory] = useState<MonitorSample[]>([]);
  const [error, setError] = useState<unknown>(null);
  const first = useRef(true);

  // Uma amostra por segundo. Pausa quando a janela está minimizada/oculta: monitor
  // que consome CPU enquanto ninguém olha seria irônico num otimizador.
  useEffect(() => {
    let busy = false;
    const tick = async () => {
      if (busy || document.visibilityState !== "visible") return;
      busy = true;
      try {
        const s = await api.monitorSample();
        // A 1ª leitura dos contadores de taxa é sempre zero (precisa de duas): descarta.
        if (first.current) first.current = false;
        else setHistory((h) => [...h.slice(-(CAPACITY - 1)), s]);
        setError(null);
      } catch (e) {
        setError(e);
      } finally {
        busy = false;
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), 1000);
    return () => window.clearInterval(id);
  }, []);

  const now = history[history.length - 1];
  const series = <K extends keyof MonitorSample>(k: K) => history.map((h) => Number(h[k] ?? 0));
  const ramPct = history.map((h) => (h.ramTotal ? (h.ramUsed / h.ramTotal) * 100 : 0));

  return (
    <>
      <PageHeader eyebrow="Geral" title="Monitor" subtitle="CPU, memória, GPU, disco e rede ao vivo, nos últimos 60 segundos." />
      {error ? <div className="error-box">Não foi possível ler os contadores: {errorMessage(error)}</div> : null}
      <OverlayCard />

      <section className="card monitor-card" aria-labelledby="m-cpu">
        <div className="monitor-head">
          <h2 id="m-cpu" className="card-title">
            CPU
          </h2>
          <span className="monitor-value">{now ? pct(now.cpu) : "…"}</span>
        </div>
        <LiveChart values={series("cpu")} max={100} format={pct} label="Uso de CPU" />
        <p className="monitor-meta">
          {hw.data ? (
            <>
              <strong>{hw.data.cpuName || "Processador"}</strong> · {hw.data.cpuThreads} threads
              {hw.data.cpuMhz ? ` · ${(hw.data.cpuMhz / 1000).toFixed(2).replace(".", ",")} GHz base` : ""}
            </>
          ) : (
            "…"
          )}
        </p>
      </section>

      <div className="monitor-grid">
        <section className="card monitor-card" aria-labelledby="m-ram">
          <div className="monitor-head">
            <h2 id="m-ram" className="card-title">
              Memória
            </h2>
            <span className="monitor-value">
              {now ? `${formatBytes(now.ramUsed)} / ${formatBytes(now.ramTotal)}` : "…"}
            </span>
          </div>
          <LiveChart values={ramPct} max={100} format={pct} label="Memória em uso" />
          <p className="monitor-meta">
            {now ? (
              <>
                {pct((now.ramUsed / now.ramTotal) * 100)} em uso · <strong>{formatBytes(now.ramTotal - now.ramUsed)}</strong> disponível
              </>
            ) : (
              "…"
            )}
          </p>
        </section>

        <section className="card monitor-card" aria-labelledby="m-gpu">
          <div className="monitor-head">
            <h2 id="m-gpu" className="card-title">
              GPU
            </h2>
            <span className="monitor-value">{now?.gpu != null ? pct(now.gpu) : now ? "n/d" : "…"}</span>
          </div>
          <LiveChart values={series("gpu")} max={100} format={pct} label="Uso da GPU (3D)" />
          <p className="monitor-meta">
            <strong>{hw.data?.gpuName || "GPU"}</strong>
            {now?.vramUsed != null && ` · ${formatBytes(now.vramUsed)} de VRAM em uso`}
            {now && now.gpu == null && " · o driver não expõe o contador de uso"}
          </p>
        </section>

        <section className="card monitor-card" aria-labelledby="m-disk">
          <div className="monitor-head">
            <h2 id="m-disk" className="card-title">
              Disco
            </h2>
            <span className="monitor-value">{now ? pct(now.diskActive) : "…"}</span>
          </div>
          <LiveChart values={series("diskActive")} max={100} format={pct} label="Tempo ativo do disco" />
          <p className="monitor-meta">
            {now ? (
              <>
                Leitura <strong>{formatRate(now.diskRead)}</strong> · Escrita <strong>{formatRate(now.diskWrite)}</strong>
              </>
            ) : (
              "…"
            )}
          </p>
        </section>

        <section className="card monitor-card" aria-labelledby="m-net">
          <div className="monitor-head">
            <h2 id="m-net" className="card-title">
              Rede
            </h2>
            <span className="monitor-value">{now ? `↓ ${formatRate(now.netDown)}` : "…"}</span>
          </div>
          <LiveChart values={series("netDown")} format={formatRate} label="Recebendo" />
          <p className="monitor-meta">
            {now ? (
              <>
                Recebendo <strong>{formatRate(now.netDown)}</strong> · Enviando <strong>{formatRate(now.netUp)}</strong>
              </>
            ) : (
              "…"
            )}
          </p>
        </section>
      </div>
    </>
  );
}

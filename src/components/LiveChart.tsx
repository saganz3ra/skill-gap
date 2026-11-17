import { useRef, useState } from "react";

interface LiveChartProps {
  /** Mais antigo primeiro; um ponto por segundo. */
  values: number[];
  /** Topo do eixo. Sem valor fixo, usa o maior valor visto (com folga). */
  max?: number;
  capacity?: number;
  format: (v: number) => string;
  label: string;
}

const W = 600;
const H = 120;

/**
 * Gráfico de área de uma série só (sem legenda: o título do cartão já nomeia a série),
 * linha fina de 2px, grade recessiva e camada de hover com cruz + valor.
 */
export function LiveChart({ values, max, capacity = 60, format, label }: LiveChartProps) {
  const [hover, setHover] = useState<number | null>(null);
  const ref = useRef<SVGSVGElement>(null);

  const top = max ?? Math.max(1, ...values) * 1.15;
  const step = W / (capacity - 1);
  // Alinha à direita: o ponto mais novo sempre na borda direita, como no Gerenciador de Tarefas.
  const offset = capacity - values.length;
  const pts = values.map((v, i) => [(offset + i) * step, H - (Math.min(v, top) / top) * H] as const);
  const line = pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = pts.length ? `${line} L${pts[pts.length - 1]![0].toFixed(1)},${H} L${pts[0]![0].toFixed(1)},${H} Z` : "";

  const last = values[values.length - 1];
  const peak = values.length ? Math.max(...values) : undefined;
  const summary = last === undefined ? `${label}: aguardando dados` : `${label} nos últimos ${values.length} s: agora ${format(last)}, pico ${format(peak!)}`;

  function onMove(e: React.PointerEvent<SVGSVGElement>) {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect || !values.length) return;
    const x = ((e.clientX - rect.left) / rect.width) * W;
    const i = Math.round(x / step) - offset;
    setHover(i >= 0 && i < values.length ? i : null);
  }

  const hp = hover !== null ? pts[hover] : undefined;
  const secondsAgo = hover !== null ? values.length - 1 - hover : 0;

  return (
    <div className="live-chart">
      <svg
        ref={ref}
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={summary}
        onPointerMove={onMove}
        onPointerLeave={() => setHover(null)}
      >
        {[0.25, 0.5, 0.75].map((f) => (
          <line key={f} x1="0" x2={W} y1={H * f} y2={H * f} className="chart-grid" vectorEffect="non-scaling-stroke" />
        ))}
        {area && <path d={area} className="chart-area" />}
        {line && <path d={line} className="chart-line" vectorEffect="non-scaling-stroke" />}
        {hp && <line x1={hp[0]} x2={hp[0]} y1="0" y2={H} className="chart-cross" vectorEffect="non-scaling-stroke" />}
      </svg>
      {hp && hover !== null && (
        <div className="chart-tip" style={{ left: `${Math.min(92, Math.max(8, (hp[0] / W) * 100))}%` }} aria-hidden="true">
          <strong>{format(values[hover]!)}</strong>
          <span>{secondsAgo === 0 ? "agora" : `há ${secondsAgo} s`}</span>
        </div>
      )}
    </div>
  );
}

import { useEffect, useState } from "react";

import { api, heatLevel, type HeatKind, type Sensors } from "../lib/api";

const BADGE = { ok: "badge-ok", warn: "badge-warn", crit: "badge-danger" } as const;
const LABEL = { ok: "normal", warn: "atenção", crit: "quente" } as const;

function Heat({ kind, c, approx }: { kind: HeatKind; c: number | null | undefined; approx?: boolean }) {
  const lvl = heatLevel(kind, c);
  if (c === null || c === undefined || !lvl) return <span className="muted">n/d</span>;
  return (
    <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
      <span className="mono" style={{ fontWeight: 600 }}>
        {approx ? "~" : ""}
        {c.toFixed(0)} °C
      </span>
      <span className={`badge ${BADGE[lvl]}`}>{LABEL[lvl]}</span>
    </span>
  );
}

function Row({ name, sub, children }: { name: string; sub?: string; children: React.ReactNode }) {
  return (
    <div className="list-row">
      <span className="list-row-text">
        <span style={{ fontWeight: 500 }}>{name}</span>
        {sub && <span className="list-row-sub">{sub}</span>}
      </span>
      {children}
    </div>
  );
}

const fmt = (v: number | null | undefined, unit: string, d = 0) => (v === null || v === undefined ? null : `${v.toFixed(d)} ${unit}`);

/** Temperaturas e ventoinhas (só leitura). Relê a cada 2 s; o backend já devolve do cache. */
export function SensorsCard() {
  const [s, setS] = useState<Sensors | null>(null);
  useEffect(() => {
    let alive = true;
    const tick = () =>
      api
        .sensorsRead()
        .then((x) => alive && setS(x))
        .catch(() => undefined);
    tick();
    const id = window.setInterval(tick, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  const g = s?.gpu;
  const gpuSub = g
    ? [fmt(g.powerW, "W"), g.powerLimitW ? `de ${g.powerLimitW.toFixed(0)} W` : null, fmt(g.clockMhz, "MHz"), g.memUsedMb !== null && g.memTotalMb ? `VRAM ${(g.memUsedMb / 1024).toFixed(1)}/${(g.memTotalMb / 1024).toFixed(0)} GB` : null]
        .filter(Boolean)
        .join(" · ")
    : undefined;

  return (
    <section className="card" aria-labelledby="sens-t">
      <h2 id="sens-t" className="card-title">
        Temperaturas e ventoinhas
      </h2>
      {!s ? (
        <p className="muted small">Lendo os sensores…</p>
      ) : (
        <>
          <Row name={g ? g.name.replace(/^NVIDIA /, "") : "GPU"} sub={g ? gpuSub : "Temperatura da GPU só em placas NVIDIA (nvidia-smi)."}>
            <Heat kind="gpu" c={g?.temp} />
          </Row>
          <Row
            name="Processador"
            sub={
              s.cpuSource === "lhm"
                ? "Sensor do processador, via LibreHardwareMonitor."
                : s.cpuSource === "acpi"
                  ? "Zona térmica do Windows: aproximada. Abra o LibreHardwareMonitor para o valor real."
                  : "O Windows não informa a temperatura deste processador sem um driver. Abra o LibreHardwareMonitor para ver."
            }
          >
            <Heat kind="cpu" c={s.cpuTemp} approx={s.cpuSource === "acpi"} />
          </Row>
          {s.disks.map((d) => (
            <Row key={`d-${d.name}`} name={d.name} sub="SSD/HD">
              <Heat kind="disk" c={d.celsius} />
            </Row>
          ))}
          {s.disks.length === 0 && s.disksNeedAdmin && (
            <Row name="SSD/HD" sub="O Windows só informa com permissão de administrador: aparece quando o auxiliar estiver aberto.">
              <span className="muted">n/d</span>
            </Row>
          )}
          {s.board.map((b) => (
            <Row key={`b-${b.name}`} name={b.name} sub="Placa-mãe">
              <Heat kind="board" c={b.celsius} />
            </Row>
          ))}

          <h3 className="small" style={{ margin: "14px 0 4px", color: "var(--text-muted)", letterSpacing: ".06em", textTransform: "uppercase" }}>
            Ventoinhas
          </h3>
          {g && g.fanPct !== null && (
            <Row name="Ventoinha da GPU" sub={g.fanPct === 0 ? "Parada: muitas placas desligam a ventoinha em uso leve (modo silencioso)." : undefined}>
              <span className="mono">{g.fanPct.toFixed(0)}%</span>
            </Row>
          )}
          {s.fans.map((f) => (
            <Row key={`f-${f.name}`} name={f.name}>
              <span className="mono">{[fmt(f.rpm, "RPM"), (f.pct === null ? null : `${f.pct.toFixed(0)}%`)].filter(Boolean).join(" · ")}</span>
            </Row>
          ))}
          {!s.lhm && (
            <p className="small muted" style={{ margin: "6px 0 0" }}>
              Ventoinhas da placa-mãe e temperatura real do Ryzen aparecem aqui com o <strong>LibreHardwareMonitor</strong> aberto (código
              aberto). O Skill Gap só lê o que ele publica; não instala driver.
            </p>
          )}

          <details style={{ marginTop: 10 }}>
            <summary className="small" style={{ cursor: "pointer" }}>
              Como controlar a velocidade das ventoinhas
            </summary>
            <div className="small" style={{ display: "grid", gap: 6, marginTop: 8 }}>
              <p style={{ margin: 0 }}>
                O Windows não tem um jeito oficial de mudar a velocidade das ventoinhas. Todo programa que faz isso instala um driver de kernel,
                que antivírus e alguns anticheats tratam como risco. Por isso o Skill Gap só mostra os valores.
              </p>
              <p style={{ margin: 0 }}>
                <strong>Melhor caminho: a BIOS.</strong> Na tela de ventoinhas (Q-Fan, Smart Fan ou Fan Control, conforme a marca), troque o modo
                para PWM nas ventoinhas de 4 pinos e monte uma curva: devagar até uns 50 °C e acelerando até 100% perto de 80 °C. Vale para
                qualquer sistema e não depende de programa aberto.
              </p>
              <p style={{ margin: 0 }}>
                <strong>Pelo Windows:</strong> o FanControl (código aberto) faz curvas por temperatura da CPU ou da GPU. Ele instala o driver
                PawnIO; confira antes se o anticheat dos seus jogos aceita.
              </p>
              <p style={{ margin: 0 }}>
                <strong>Só a GPU:</strong> o MSI Afterburner ou o painel da fabricante da placa controlam a curva da ventoinha da placa de vídeo.
              </p>
            </div>
          </details>
        </>
      )}
    </section>
  );
}

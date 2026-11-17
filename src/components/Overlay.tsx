import { useEffect, useState } from "react";
import QRCode from "qrcode";

import { api, errorMessage, type PresentMonStatus, type RemoteInfo, type Telemetry } from "../lib/api";
import { Switch } from "./ui";

const n = (v: number | null | undefined, d = 0) => (v === null || v === undefined ? "—" : v.toFixed(d));

/** Janela do overlay: transparente, clique passa direto. Lê 1×/s enquanto aberta. */
export function OverlayWindow() {
  const [t, setT] = useState<Telemetry | null>(null);
  useEffect(() => {
    document.documentElement.classList.add("overlay-root");
    let alive = true;
    const tick = () =>
      api
        .telemetrySample()
        .then((x) => alive && setT(x))
        .catch(() => undefined);
    tick();
    const id = window.setInterval(tick, 1000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  const s = t?.sample;
  return (
    <div className="overlay-panel" aria-hidden="true">
      <div className="overlay-fps">
        {n(t?.frames?.fps)} <span>FPS</span>
      </div>
      <div className="overlay-sub">
        {t?.frames ? `${n(t.frames.frametimeMs, 1)} ms · var ${n(t.frames.varMs, 1)}` : t?.game ? "medindo…" : "sem jogo do Modo Jogo"}
      </div>
      <div className="overlay-row">
        CPU {n(s?.cpu)}%{s?.cpuTemp ? ` · ~${n(s.cpuTemp)}°` : ""} · GPU {s?.gpu === null || s?.gpu === undefined ? "—" : `${n(s.gpu)}%`}
        {t?.gpuTemp ? ` · ${n(t.gpuTemp)}°` : ""}
      </div>
      <div className="overlay-row">
        Ping {n(t?.ping.lastMs)} ms · jit {n(t?.ping.jitterMs, 1)} · perda {n(t?.ping.lossPct)}%
      </div>
    </div>
  );
}

/** Cartão no Monitor: overlay, FPS (PresentMon) e segunda tela. */
export function OverlayCard() {
  const [overlay, setOverlay] = useState(false);
  const [pm, setPm] = useState<PresentMonStatus | null>(null);
  const [remote, setRemote] = useState<RemoteInfo | null>(null);
  const [qr, setQr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = () => {
    void api.pmStatus().then(setPm).catch(() => undefined);
    void api.remoteStatus().then(setRemote).catch(() => undefined);
  };
  useEffect(refresh, []);
  useEffect(() => {
    if (remote) void QRCode.toDataURL(remote.url, { margin: 1, width: 180 }).then(setQr);
    else setQr(null);
  }, [remote]);

  async function act(f: () => Promise<unknown>) {
    setBusy(true);
    setMsg(null);
    try {
      await f();
    } catch (e) {
      setMsg(errorMessage(e));
    } finally {
      setBusy(false);
      refresh();
    }
  }

  return (
    <section className="card" aria-labelledby="ov-t">
      <h2 id="ov-t" className="card-title">
        Overlay e segunda tela
      </h2>
      {msg && <div className="error-box">{msg}</div>}

      <div className="list-row">
        <span className="list-row-text">
          <span style={{ fontWeight: 500 }}>Overlay no canto da tela</span>
          <span className="list-row-sub">
            Janela transparente por cima de tudo; o clique passa direto. Não injeta nada no jogo, por isso não aparece em tela
            cheia EXCLUSIVA (use janela sem bordas).
          </span>
        </span>
        <Switch
          checked={overlay}
          disabled={busy}
          label="Mostrar overlay"
          onChange={(on) =>
            void act(async () => {
              await api.overlayShow(on);
              setOverlay(on);
            })
          }
        />
      </div>

      <div className="list-row">
        <span className="list-row-text">
          <span style={{ fontWeight: 500 }}>FPS e frametime (PresentMon {pm?.version ?? ""})</span>
          <span className="list-row-sub">
            {!pm
              ? "…"
              : !pm.installed
                ? "Ferramenta da Intel (código aberto). Baixada uma vez do GitHub oficial e conferida (hash + assinatura da Intel) antes de rodar."
                : pm.needsLogoff
                  ? "Permissão dada: saia e entre no Windows para valer."
                  : !pm.canCapture
                    ? "Para medir sem abrir o app como administrador, o Windows exige o grupo “Usuários de log de desempenho”."
                    : pm.capturing
                      ? `Medindo ${pm.capturing}.`
                      : "Pronto: mede o jogo da partida atual do Modo Jogo."}
          </span>
        </span>
        {pm && !pm.installed && (
          <button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={() => void act(api.pmInstall)}>
            {busy ? "Baixando…" : "Baixar e verificar"}
          </button>
        )}
        {pm && pm.installed && !pm.canCapture && !pm.needsLogoff && (
          <button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={() => void act(() => api.pmPermission(true))}>
            Permitir medir FPS
          </button>
        )}
        {pm?.needsLogoff && (
          <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void act(() => api.pmPermission(false))}>
            Desfazer permissão
          </button>
        )}
      </div>

      <div className="list-row">
        <span className="list-row-text">
          <span style={{ fontWeight: 500 }}>Segunda tela no celular</span>
          <span className="list-row-sub">
            Mesma rede Wi-Fi. Endereço com chave nova a cada vez; só a rede local acessa; desliga junto com o interruptor. O
            Windows pode pedir pra liberar o app no firewall.
          </span>
        </span>
        <Switch
          checked={remote !== null}
          disabled={busy}
          label="Segunda tela"
          onChange={(on) => void act(async () => setRemote(on ? await api.remoteStart() : (await api.remoteStop(), null)))}
        />
      </div>
      {remote && (
        <div style={{ display: "flex", gap: 14, alignItems: "center", flexWrap: "wrap" }}>
          {qr && <img src={qr} alt="QR code da segunda tela" width={150} height={150} style={{ borderRadius: 8, background: "#fff" }} />}
          <span className="small mono" style={{ wordBreak: "break-all" }}>
            {remote.url}
          </span>
        </div>
      )}
    </section>
  );
}

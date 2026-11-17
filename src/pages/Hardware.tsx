import { useState } from "react";

import { api, type MsiDevice, type NicProperty, type SteamGame } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader, Switch } from "../components/ui";
import { friendlyError, RestartNotice } from "../components/feedback";

export function Hardware() {
  const msi = useResource(api.hwMsiList);
  const games = useResource(api.hwSteamGames);
  const nic = useResource(api.hwNicList);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [msiChanged, setMsiChanged] = useState(false);

  async function run(key: string, action: () => Promise<void>, reload: () => Promise<void>) {
    setBusy(key);
    setMsg(null);
    try {
      await action();
      await reload();
    } catch (e) {
      setMsg(friendlyError(e));
    } finally {
      setBusy(null);
    }
  }

  const [devices, msiSaved] = msi.data ?? [[], []];
  const [props, nicSaved] = nic.data ?? [[], []];

  return (
    <>
      <PageHeader
        eyebrow="Jogos"
        title="Hardware e jogos"
        subtitle="Ajustes por dispositivo e por jogo. Cada um anota o valor anterior antes de mudar e volta exatamente a ele."
      />
      {msg && <div className="error-box">{msg}</div>}

      <section className="card" aria-labelledby="msi-t">
        <h2 id="msi-t" className="card-title">
          Modo MSI por dispositivo
        </h2>
        <p className="muted small" style={{ margin: 0 }}>
          Interrupções por mensagem (MSI) em vez de linha compartilhada: pode reduzir latência de placa de vídeo, rede e armazenamento.
          Muitos drivers já usam MSI. Exige reiniciar; se um dispositivo parar de funcionar, desligue aqui ou pelo Modo de Segurança.
        </p>
        {msi.error ? <div className="error-box">{friendlyError(msi.error)}</div> : null}
        {devices.map((d: MsiDevice) => {
          const saved = msiSaved.includes(d.instanceId);
          return (
            <div key={d.instanceId} className="list-row">
              <span className="list-row-text">
                <span style={{ fontWeight: 500 }}>{d.name}</span>
                <span className="list-row-sub">
                  {d.class} · MSI {d.msi === 1 ? "ligado" : d.msi === 0 ? "desligado" : "padrão do driver"}
                  {d.msi === 1 && !saved && " (já estava assim)"}
                </span>
              </span>
              <Switch
                checked={d.msi === 1}
                disabled={busy !== null || (d.msi === 1 && !saved)}
                label={`MSI em ${d.name}`}
                onChange={(on) =>
                  void run(
                    d.instanceId,
                    async () => {
                      await api.hwMsiSet(d.instanceId, on);
                      setMsiChanged(true);
                    },
                    msi.reload,
                  )
                }
              />
            </div>
          );
        })}
        {msiChanged && <RestartNotice restart="reboot" />}
      </section>

      <section className="card" aria-labelledby="games-t">
        <h2 id="games-t" className="card-title">
          Jogos da Steam
        </h2>
        <p className="muted small" style={{ margin: 0 }}>
          <strong>Tela cheia clássica</strong>: desliga as "otimizações de tela cheia" só neste jogo. <strong>Prioridade de rede</strong>: marca os pacotes do
          jogo com DSCP 46 (QoS). Muitos roteadores domésticos ignoram essa marcação; só ajuda se o seu respeitar. <strong>GPU</strong>: força a placa
          de vídeo de alto desempenho (útil em notebook com duas GPUs).
        </p>
        {games.loading && <p className="muted small">Procurando jogos…</p>}
        {games.data?.length === 0 && <p className="small">Nenhum jogo da Steam encontrado.</p>}
        {games.data?.map((g: SteamGame) => (
          <div key={g.appId} className="list-row">
            <span className="list-row-text">
              <span style={{ fontWeight: 500 }}>{g.name}</span>
              <span className="list-row-sub mono">{g.exe ?? "executável não encontrado"}</span>
            </span>
            {g.exe && (
              <span style={{ display: "flex", gap: 14, alignItems: "center" }}>
                <span className="small muted">Tela cheia</span>
                <Switch
                  checked={g.classicFullscreen}
                  disabled={busy !== null}
                  label={`Tela cheia clássica em ${g.name}`}
                  onChange={(on) => void run(`fse${g.appId}`, () => api.hwFseSet(g.exe!, on), games.reload)}
                />
                <span className="small muted">GPU</span>
                <Switch
                  checked={g.highPerfGpu}
                  disabled={busy !== null}
                  label={`GPU de alto desempenho em ${g.name}`}
                  onChange={(on) => void run(`gpu${g.appId}`, () => api.hwGpuSet(g.exe!, on), games.reload)}
                />
                <span className="small muted">Rede</span>
                <Switch
                  checked={g.qos}
                  disabled={busy !== null}
                  label={`Prioridade de rede em ${g.name}`}
                  onChange={(on) => void run(`qos${g.appId}`, () => api.hwQosSet(g.exe!, on), games.reload)}
                />
              </span>
            )}
          </div>
        ))}
      </section>

      <section className="card" aria-labelledby="nic-t">
        <h2 id="nic-t" className="card-title">
          Placa de rede
        </h2>
        <p className="muted small" style={{ margin: 0 }}>
          Só as opções que a sua placa tem, com os valores que ela aceita. Moderação de interrupções desligada reduz latência ao custo de CPU;
          economia de energia (EEE) desligada evita picos ao "acordar" o link.
        </p>
        {nic.error ? <div className="error-box">{friendlyError(nic.error)}</div> : null}
        {props.map((p: NicProperty) => {
          const key = `${p.adapter}|${p.keyword}`;
          const saved = nicSaved.includes(key);
          return (
            <div key={key} className="list-row">
              <span className="list-row-text">
                <span style={{ fontWeight: 500 }}>{p.label}</span>
                <span className="list-row-sub">{p.adapter}</span>
              </span>
              <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <select
                  className="input"
                  value={p.value}
                  disabled={busy !== null}
                  aria-label={`${p.label} em ${p.adapter}`}
                  onChange={(e) => void run(key, () => api.hwNicSet(p.adapter, p.keyword, e.target.value), nic.reload)}
                >
                  {p.valid.map((v, i) => (
                    <option key={v} value={v}>
                      {p.display[i] ?? v}
                    </option>
                  ))}
                </select>
                {saved && (
                  <button type="button" className="btn btn-ghost btn-sm" disabled={busy !== null} onClick={() => void run(key, () => api.hwNicSet(p.adapter, p.keyword, null), nic.reload)}>
                    Desfazer
                  </button>
                )}
              </span>
            </div>
          );
        })}
      </section>
    </>
  );
}

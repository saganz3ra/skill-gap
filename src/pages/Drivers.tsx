import { useState } from "react";
import { ExternalLink, FolderOpen, RotateCw, TriangleAlert } from "lucide-react";

import { api, WINDOWS_UPDATE_URL, type DriverInfo } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";
import { friendlyError } from "../components/feedback";

const MAIN: { cls: string; label: string }[] = [
  { cls: "DISPLAY", label: "Placa de vídeo" },
  { cls: "NET", label: "Rede" },
  { cls: "MEDIA", label: "Áudio" },
];

export function Drivers() {
  const inv = useResource(api.driversInventory);
  const [backup, setBackup] = useState<"idle" | "busy" | { path: string; count: number }>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  const drivers = inv.data?.drivers ?? [];
  const main = MAIN.map((m) => ({ ...m, driver: drivers.find((d) => d.class === m.cls) }));

  async function open(url: string) {
    try {
      await api.openOfficialUrl(url);
    } catch (e) {
      setMessage(friendlyError(e));
    }
  }

  async function runBackup() {
    setBackup("busy");
    setMessage(null);
    try {
      setBackup(await api.driversBackup());
    } catch (e) {
      setBackup("idle");
      setMessage(friendlyError(e));
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="Manutenção"
        title="Drivers"
        subtitle="Inventário e backup. O Skill Gap não baixa driver: ele leva você ao site oficial do fabricante ou ao Windows Update."
        actions={
          <button type="button" className="btn btn-ghost" onClick={() => void inv.reload()} disabled={inv.loading}>
            <RotateCw size={16} aria-hidden="true" />
            {inv.loading ? "Lendo…" : "Atualizar"}
          </button>
        }
      />

      {inv.error ? <div className="error-box">Não foi possível ler os drivers: {friendlyError(inv.error)}</div> : null}
      {message && <div className="error-box">{message}</div>}

      <div className="tweak-grid">
        {main.map(({ cls, label, driver }) => (
          <article key={cls} className="tweak-card">
            <span className="stat-label">{label}</span>
            {driver ? (
              <>
                <h2 className="tweak-name">{driver.name}</h2>
                <p className="tweak-desc">
                  Versão <strong className="mono">{driver.version}</strong>
                  {driver.date && ` · ${formatDate(driver.date)}`}
                  <br />
                  {driver.provider}
                </p>
                <DownloadButton driver={driver} open={open} />
              </>
            ) : (
              <p className="tweak-desc">{inv.loading ? "Lendo…" : "Nenhum driver de fabricante encontrado (usa o do Windows)."}</p>
            )}
          </article>
        ))}
      </div>

      {inv.data && inv.data.problems.length > 0 && (
        <section className="card" aria-labelledby="problemas">
          <h2 id="problemas" className="card-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <TriangleAlert size={18} aria-hidden="true" style={{ color: "var(--warn)" }} />
            Dispositivos com problema ({inv.data.problems.length})
          </h2>
          {inv.data.problems.map((p, i) => (
            <div key={`${p.name}-${i}`} className="list-row" style={{ minHeight: 52 }}>
              <span className="list-row-text">
                <span style={{ fontWeight: 500 }}>{p.name}</span>
                <span className="list-row-sub">
                  {p.description} (código {p.code}){p.class && ` · ${p.class}`}
                </span>
              </span>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => void open(WINDOWS_UPDATE_URL)}>
                Windows Update
              </button>
            </div>
          ))}
        </section>
      )}

      <section className="card" aria-labelledby="backup">
        <div className="card-head" style={{ alignItems: "center" }}>
          <div>
            <h2 id="backup" className="card-title">
              Backup de todos os drivers
            </h2>
            <p className="plan-desc" style={{ margin: "4px 0 0" }}>
              Exporta os drivers de fabricante pra Documentos\Skill Gap. Faça antes de formatar: depois é só apontar o
              Gerenciador de Dispositivos pra pasta. Precisa de admin.
            </p>
          </div>
          <div style={{ display: "flex", gap: 8, flexShrink: 0 }}>
            <button type="button" className="btn btn-ghost" onClick={() => void api.openBackupFolder().catch((e) => setMessage(friendlyError(e)))}>
              <FolderOpen size={16} aria-hidden="true" />
              Abrir pasta
            </button>
            <button type="button" className="btn btn-primary" disabled={backup === "busy"} onClick={() => void runBackup()}>
              {backup === "busy" ? "Exportando…" : "Fazer backup"}
            </button>
          </div>
        </div>
        {typeof backup === "object" && (
          <p className="small" style={{ margin: 0 }} role="status">
            {backup.count} drivers exportados para <span className="mono">{backup.path}</span>
          </p>
        )}
      </section>

      {drivers.length > 0 && (
        <section className="card" aria-labelledby="inventario" style={{ gap: 0 }}>
          <button
            type="button"
            className="link-btn"
            aria-expanded={showAll}
            aria-controls="inventario-tabela"
            onClick={() => setShowAll((s) => !s)}
            style={{ alignSelf: "flex-start" }}
          >
            <span id="inventario">
              {showAll ? "▾" : "▸"} Inventário completo ({drivers.length} drivers)
            </span>
          </button>
          {showAll && (
            <div id="inventario-tabela" className="table-wrap" style={{ marginTop: 12 }}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">Dispositivo</th>
                    <th scope="col">Classe</th>
                    <th scope="col">Fabricante</th>
                    <th scope="col">Versão</th>
                    <th scope="col">Data</th>
                  </tr>
                </thead>
                <tbody>
                  {drivers.map((d, i) => (
                    <tr key={`${d.name}-${i}`}>
                      <td>{d.name}</td>
                      <td className="mono small">{d.class}</td>
                      <td>{d.provider}</td>
                      <td className="mono small">{d.version}</td>
                      <td className="small">{d.date ? formatDate(d.date) : "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </section>
      )}
    </>
  );
}

function DownloadButton({ driver, open }: { driver: DriverInfo; open: (url: string) => Promise<void> }) {
  const url = driver.downloadUrl ?? WINDOWS_UPDATE_URL;
  const label = driver.downloadUrl ? `Baixar do site da ${shortVendor(driver.provider)}` : "Procurar no Windows Update";
  return (
    <button type="button" className="btn btn-ghost btn-sm" onClick={() => void open(url)}>
      <ExternalLink size={14} aria-hidden="true" />
      {label}
    </button>
  );
}

function shortVendor(provider: string) {
  const p = provider.toLowerCase();
  if (p.includes("nvidia")) return "NVIDIA";
  if (p.includes("amd") || p.includes("advanced micro")) return "AMD";
  if (p.includes("intel")) return "Intel";
  if (p.includes("realtek")) return "Realtek";
  return provider;
}

/** "2024-06-10" → "10/06/2024" */
function formatDate(iso: string) {
  const [y, m, d] = iso.split("-");
  return y && m && d ? `${d}/${m}/${y}` : iso;
}

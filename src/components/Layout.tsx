import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { api, type ElevationStatus } from "../lib/api";
import {
  Activity,
  BatteryCharging,
  Cpu,
  Gamepad2,
  House,
  LayoutGrid,
  MousePointer2,
  Power,
  ShieldCheck,
  SlidersHorizontal,
  Trash2,
  Wrench,
  Zap,
  type LucideIcon,
} from "lucide-react";

const NAV: { group: string; items: { to: string; label: string; icon: LucideIcon; end?: boolean }[] }[] = [
  {
    group: "Geral",
    items: [
      { to: "/", label: "Início", icon: House, end: true },
      { to: "/monitor", label: "Monitor", icon: Activity },
      { to: "/perfis", label: "Perfis", icon: LayoutGrid },
    ],
  },
  {
    group: "Otimização",
    items: [
      { to: "/ajustes", label: "Todos os ajustes", icon: SlidersHorizontal },
      { to: "/desempenho", label: "Energia", icon: BatteryCharging },
      { to: "/inicializacao", label: "Inicialização", icon: Power },
    ],
  },
  {
    group: "Jogos",
    items: [
      { to: "/modo-jogo", label: "Modo Jogo", icon: Gamepad2 },
      { to: "/mouse", label: "Mouse", icon: MousePointer2 },
    ],
  },
  {
    group: "Manutenção",
    items: [
      { to: "/limpeza", label: "Limpeza", icon: Trash2 },
      { to: "/drivers", label: "Drivers", icon: Cpu },
      { to: "/correcoes", label: "Correções", icon: Wrench },
    ],
  },
];

export function Layout() {
  return (
    <div className="app">
      <nav className="sidebar" aria-label="Navegação principal">
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            <Zap size={16} strokeWidth={2.4} />
          </span>
          Skill Gap
        </div>

        {NAV.map(({ group, items }) => (
          <div key={group} className="nav-group" role="group" aria-label={group}>
            <span className="nav-group-label" aria-hidden="true">
              {group}
            </span>
            {items.map(({ to, label, icon: Icon, end }) => (
              <NavLink key={to} to={to} end={end} className="nav-link">
                <Icon size={18} aria-hidden="true" />
                {label}
              </NavLink>
            ))}
          </div>
        ))}

        <div className="sidebar-spacer" />
        <AdminChip />
        <div className="restore-note">
          <ShieldCheck size={16} aria-hidden="true" />
          <span>Ajustes podem ser desfeitos na própria tela. A limpeza sempre pede confirmação.</span>
        </div>
      </nav>

      <main className="main">
        <div className="bg-layer bg-glow" aria-hidden="true" />
        <div className="bg-layer bg-grid" aria-hidden="true" />
        <div className="bg-layer bg-dots" aria-hidden="true" />
        <div className="content">
          <Outlet />
        </div>
      </main>
    </div>
  );
}

/** Mostra se a permissão de administrador já foi dada nesta sessão. */
function AdminChip() {
  const [status, setStatus] = useState<ElevationStatus>();
  useEffect(() => {
    let alive = true;
    const tick = () =>
      api
        .elevationStatus()
        .then((s) => alive && setStatus(s))
        .catch(() => {});
    tick();
    const id = window.setInterval(tick, 3000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  if (!status) return null;
  const on = status.isAdmin || status.helperRunning;
  return (
    <div
      className={`admin-chip ${on ? "on" : ""}`}
      title="O Skill Gap roda sem administrador. Quando uma ação precisa, o Windows pede permissão uma vez; ela vale até você fechar o app."
    >
      <span className="admin-dot" aria-hidden="true" />
      {status.isAdmin ? "Aberto como admin" : status.helperRunning ? "Admin liberado" : "Admin sob demanda"}
    </div>
  );
}

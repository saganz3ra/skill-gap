import type { ReactNode } from "react";
import { Info, Moon, Sun } from "lucide-react";
import { useTheme } from "../hooks/theme";

export function ThemeToggle() {
  const { theme, toggle } = useTheme();
  const dark = theme === "dark";
  return (
    <button
      type="button"
      className="icon-btn"
      onClick={toggle}
      aria-label={dark ? "Mudar para modo claro" : "Mudar para modo escuro"}
      title={dark ? "Modo claro" : "Modo escuro"}
    >
      {dark ? <Sun size={18} aria-hidden="true" /> : <Moon size={18} aria-hidden="true" />}
    </button>
  );
}

interface PageHeaderProps {
  eyebrow: string;
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}

export function PageHeader({ eyebrow, title, subtitle, actions }: PageHeaderProps) {
  return (
    <header className="page-header">
      <div className="page-header-text">
        <span className="eyebrow">{eyebrow}</span>
        <h1 className="page-title">{title}</h1>
        {subtitle && <p className="page-sub">{subtitle}</p>}
      </div>
      <div className="header-actions">
        <ThemeToggle />
        {actions}
      </div>
    </header>
  );
}

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}

/** Chave acessível: role="switch" + aria-checked, navegável por teclado (é um <button>). */
export function Switch({ checked, onChange, label, disabled }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="switch"
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="switch-track">
        <span className="switch-knob" />
      </span>
    </button>
  );
}

export function Notice({ title, children, icon }: { title: string; children: ReactNode; icon?: ReactNode }) {
  return (
    <aside className="notice">
      {icon ?? <Info size={18} aria-hidden="true" />}
      <div>
        <strong>{title}</strong>
        <p>{children}</p>
      </div>
    </aside>
  );
}

export function Wip() {
  return <span className="wip">Em breve</span>;
}

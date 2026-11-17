import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "../lib/api";

export type Theme = "light" | "dark";

const STORAGE_KEY = "skillgap-theme";
const media = () => window.matchMedia("(prefers-color-scheme: dark)");

/** Escolha manual salva, ou null = seguir o tema do Windows. */
export function readSavedTheme(): Theme | null {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "light" || v === "dark" ? v : null;
  } catch {
    return null;
  }
}

export function systemTheme(): Theme {
  return media().matches ? "dark" : "light";
}

/** Aplica no <html>. Chamado também antes do React montar, pra não piscar o tema errado. */
export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

interface ThemeContextValue {
  theme: Theme;
  toggle: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [choice, setChoice] = useState<Theme | null>(readSavedTheme);
  const [system, setSystem] = useState<Theme>(systemTheme);
  const theme = choice ?? system;

  // Acompanha mudanças do tema do Windows enquanto o usuário não escolheu um manualmente.
  useEffect(() => {
    const mq = media();
    const onChange = () => setSystem(mq.matches ? "dark" : "light");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    applyTheme(theme);
    // Deixa a barra de título da janela no mesmo tema da interface.
    if (isTauri()) {
      getCurrentWindow()
        .setTheme(theme)
        .catch(() => undefined);
    }
  }, [theme]);

  const toggle = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    setChoice(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // armazenamento indisponível: o tema só não fica salvo
    }
  };

  return <ThemeContext.Provider value={{ theme, toggle }}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme precisa estar dentro de <ThemeProvider>");
  return ctx;
}

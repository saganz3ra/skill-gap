import { useEffect, useState } from "react";
import { Keyboard, X } from "lucide-react";

import { PageHeader, Switch } from "../components/ui";
import { useTheme } from "../hooks/theme";
import { api, errorMessage, type SettingsView, type ShortcutAction } from "../lib/api";

const ACTIONS: { id: ShortcutAction; label: string; hint: string }[] = [
  { id: "open_app", label: "Abrir o Skill Gap", hint: "Traz a janela pra frente, mesmo minimizada." },
  { id: "toggle_overlay", label: "Mostrar/esconder o overlay", hint: "FPS, temperaturas e ping no canto da tela." },
  { id: "toggle_game_mode", label: "Ligar/desligar o Modo Jogo automático", hint: "Para quando você quer jogar sem as otimizações." },
  { id: "toggle_remote", label: "Ligar/desligar a segunda tela", hint: "O endereço/QR aparece no Monitor." },
  { id: "open_monitor", label: "Abrir o Monitor", hint: "" },
  { id: "open_cleanup", label: "Abrir a Limpeza", hint: "" },
  { id: "open_tweaks", label: "Abrir Todos os ajustes", hint: "" },
  { id: "open_fixes", label: "Abrir Correções", hint: "" },
];

const KEY_NAMES: Record<string, string> = {
  Space: "Space",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Insert: "Insert",
  Delete: "Delete",
};

/** Tecla física (e.code) → nome aceito pelo backend; null = tecla que não vira atalho. */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return KEY_NAMES[code] ?? null;
}

/** "Ctrl+Alt+O" → "Ctrl + Alt + O" (e "Super" vira "Win", como no teclado). */
const pretty = (accel: string) => accel.replace("Super", "Win").split("+").join(" + ");

function ShortcutRow({
  action,
  current,
  failed,
  onSave,
}: {
  action: (typeof ACTIONS)[number];
  current: string | undefined;
  failed: boolean;
  onSave: (accel: string | null) => Promise<void>;
}) {
  const [recording, setRecording] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const key = keyName(e.code);
      if (!key) return; // só modificador apertado (ou tecla não suportada): espera a próxima
      const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
      if (mods.length === 0) {
        setMsg("Use junto com Ctrl, Alt, Shift ou Win: uma tecla sozinha atrapalharia os jogos.");
        return;
      }
      setRecording(false);
      void onSave([...mods, key].join("+"))
        .then(() => setMsg(null))
        .catch((err) => setMsg(errorMessage(err)));
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, onSave]);

  return (
    <div className="list-row" style={{ flexWrap: "wrap" }}>
      <span className="list-row-text">
        <span style={{ fontWeight: 500 }}>{action.label}</span>
        {msg ? (
          <span className="list-row-sub" style={{ color: "var(--warn)" }} role="alert">
            {msg}
          </span>
        ) : failed ? (
          <span className="list-row-sub" style={{ color: "var(--warn)" }}>
            Outro programa já usa essa combinação; o Windows não deixou registrar. Escolha outra.
          </span>
        ) : (
          action.hint && <span className="list-row-sub">{action.hint}</span>
        )}
      </span>
      <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
        <button
          type="button"
          className={`btn btn-sm ${recording ? "btn-primary" : "btn-ghost"}`}
          onClick={() => {
            setMsg(null);
            setRecording((r) => !r);
          }}
          aria-label={`Definir atalho: ${action.label}`}
        >
          <Keyboard size={15} aria-hidden="true" />
          {recording ? "Aperte a combinação… (Esc cancela)" : current ? <span className="mono">{pretty(current)}</span> : "Definir"}
        </button>
        {current && !recording && (
          <button
            type="button"
            className="icon-btn"
            style={{ width: 36, height: 36 }}
            aria-label={`Remover atalho: ${action.label}`}
            onClick={() => void onSave(null).catch((err) => setMsg(errorMessage(err)))}
          >
            <X size={15} aria-hidden="true" />
          </button>
        )}
      </span>
    </div>
  );
}

export function Configuracoes() {
  const { choice, setChoice } = useTheme();
  const [view, setView] = useState<SettingsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.settingsGet().then(setView, (e) => setError(errorMessage(e)));
  }, []);

  const run = (p: Promise<SettingsView>) =>
    p.then(
      (v) => {
        setView(v);
        setError(null);
      },
      (e) => setError(errorMessage(e)),
    );

  const themes: { id: "light" | "dark" | null; label: string }[] = [
    { id: null, label: "Igual ao Windows" },
    { id: "dark", label: "Escuro" },
    { id: "light", label: "Claro" },
  ];

  return (
    <>
      <PageHeader eyebrow="App" title="Configurações" subtitle="Como o Skill Gap abre, aparece e responde ao teclado." />
      {error && <div className="error-box">{error}</div>}

      <section className="card" aria-labelledby="cfg-start">
        <h2 id="cfg-start" className="card-title">
          Inicialização
        </h2>
        <div className="list-row">
          <span className="list-row-text">
            <span style={{ fontWeight: 500 }}>Iniciar com o Windows</span>
            <span className="list-row-sub">
              Abre o app ao entrar no Windows, para o Modo Jogo automático já estar vigiando quando você abrir um jogo. Fica na lista de
              Inicialização como qualquer outro programa.
            </span>
          </span>
          <Switch checked={view?.autostart ?? false} disabled={!view} label="Iniciar com o Windows" onChange={(on) => void run(api.settingsSetAutostart(on))} />
        </div>
        <div className="list-row">
          <span className="list-row-text">
            <span style={{ fontWeight: 500 }}>Abrir minimizado</span>
            <span className="list-row-sub">Quando abrir junto com o Windows, começa na barra de tarefas em vez de na frente.</span>
          </span>
          <Switch
            checked={view?.settings.startMinimized ?? false}
            disabled={!view || !view.autostart}
            label="Abrir minimizado"
            onChange={(on) => void run(api.settingsSetStartMinimized(on))}
          />
        </div>
      </section>

      <section className="card" aria-labelledby="cfg-theme">
        <h2 id="cfg-theme" className="card-title">
          Aparência
        </h2>
        <div className="chip-row" role="radiogroup" aria-label="Tema">
          {themes.map((t) => (
            <button
              key={t.label}
              type="button"
              role="radio"
              aria-checked={choice === t.id}
              className={`chip ${choice === t.id ? "chip-on" : ""}`}
              onClick={() => setChoice(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
      </section>

      <section className="card" aria-labelledby="cfg-keys">
        <h2 id="cfg-keys" className="card-title">
          Atalhos de teclado
        </h2>
        <p className="small muted" style={{ margin: 0 }}>
          Funcionam em qualquer lugar do Windows, inclusive dentro do jogo, com o Skill Gap aberto ou minimizado. Usam o registro de atalhos do
          próprio Windows: nada é injetado no jogo.
        </p>
        {ACTIONS.map((a) => (
          <ShortcutRow
            key={a.id}
            action={a}
            current={view?.settings.shortcuts[a.id]}
            failed={view?.failed.includes(a.id) ?? false}
            onSave={async (accel) => setView(await api.settingsSetShortcut(a.id, accel))}
          />
        ))}
      </section>
    </>
  );
}

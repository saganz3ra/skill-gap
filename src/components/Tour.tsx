import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

const KEY = "skillgap-tour-v1";

const STEPS: { title: string; text: string; to: string }[] = [
  {
    title: "Comece pelo Diagnóstico",
    text: "Ele lê a máquina, guarda uma linha de base e mostra o que vale ajustar. Depois de aplicar, rode de novo para comparar com números, não com impressão.",
    to: "/diagnostico",
  },
  {
    title: "Perfis aplicam vários ajustes de uma vez",
    text: "Cada ajuste mostra o impacto esperado (pequeno, médio, alto) e o risco. Os que esquentam a máquina nunca entram em perfil: só um a um, com aviso.",
    to: "/perfis",
  },
  {
    title: "Tudo pode ser desfeito",
    text: "Antes de mudar qualquer coisa, o app guarda o valor original. Em Ajustes, cada item tem o seu “Desfazer”; se o Windows reverter algo numa atualização, o Início avisa.",
    to: "/ajustes",
  },
  {
    title: "Modo Jogo",
    text: "Detecta o jogo, aplica o que faz sentido só durante a partida e volta ao normal quando você fecha. O app nunca abre nem lê o processo do jogo.",
    to: "/modo-jogo",
  },
  {
    title: "Monitor, overlay e segunda tela",
    text: "Acompanhe CPU, GPU, rede e, com o PresentMon, FPS e frametime, no canto da tela ou no celular.",
    to: "/monitor",
  },
];

function seen(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return true; // sem storage: não insiste a cada abertura
  }
}

/** Tour curto na primeira abertura. Pode ser pulado; não volta depois. */
export function Tour() {
  const [step, setStep] = useState<number | null>(() => (seen() ? null : 0));
  const navigate = useNavigate();
  const primary = useRef<HTMLButtonElement>(null);

  useEffect(() => primary.current?.focus(), [step]);

  const close = () => {
    try {
      localStorage.setItem(KEY, "1");
    } catch {
      /* ignora */
    }
    setStep(null);
  };

  useEffect(() => {
    if (step === null) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step]);

  if (step === null) return null;
  const s = STEPS[step];
  if (!s) return null;
  const last = step === STEPS.length - 1;

  return (
    <div className="tour-backdrop">
      <div className="tour-card card" role="dialog" aria-modal="true" aria-labelledby="tour-t" aria-describedby="tour-d">
        <span className="small" style={{ color: "var(--text-muted)" }}>
          {step + 1} de {STEPS.length}
        </span>
        <h2 id="tour-t" className="card-title">
          {s.title}
        </h2>
        <p id="tour-d" style={{ margin: 0 }}>
          {s.text}
        </p>
        <div className="tour-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={close}>
            Pular
          </button>
          <span style={{ flex: 1 }} />
          {step > 0 && (
            <button type="button" className="btn btn-ghost btn-sm" onClick={() => setStep(step - 1)}>
              Voltar
            </button>
          )}
          <button
            ref={primary}
            type="button"
            className="btn btn-primary btn-sm"
            onClick={() => {
              navigate(s.to);
              if (last) close();
              else setStep(step + 1);
            }}
          >
            {last ? "Começar" : "Próximo"}
          </button>
        </div>
      </div>
    </div>
  );
}

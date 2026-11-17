import { useEffect, useState } from "react";

import { api, errorMessage, PLAN_INFO, type PowerPlan } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { Link } from "react-router-dom";
import { PageHeader } from "../components/ui";
import { EnergyPlans } from "../components/EnergyPlans";

const PLANS: PowerPlan[] = ["powerSaver", "balanced", "highPerformance"];

export function Desempenho() {
  const { data: active, error, reload } = useResource(api.getPowerPlan);
  const [selected, setSelected] = useState<PowerPlan | null>(null);
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);

  // Quando o plano atual chega do backend, pré-seleciona ele.
  useEffect(() => {
    if (active?.plan) setSelected(active.plan);
  }, [active]);

  const changed = selected !== null && selected !== active?.plan;

  async function apply() {
    if (!selected) return;
    setApplying(true);
    setApplyError(null);
    try {
      await api.setPowerPlan(selected);
      await reload();
    } catch (e) {
      setApplyError(errorMessage(e));
    } finally {
      setApplying(false);
    }
  }

  return (
    <>
      <PageHeader
        eyebrow="Otimização"
        title="Energia"
        subtitle="Planos de energia do Skill Gap e os planos padrão do Windows. “Desfazer” volta para o plano que você usava."
        actions={
          <button type="button" className="btn btn-primary" disabled={!changed || applying} onClick={() => void apply()}>
            {applying ? "Aplicando…" : "Aplicar"}
          </button>
        }
      />

      {error ? <div className="error-box">Não foi possível ler o plano de energia: {errorMessage(error)}</div> : null}
      {applyError && <div className="error-box">{applyError}</div>}

      <EnergyPlans onChange={() => void reload()} />

      <fieldset style={{ border: 0, margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 14 }}>
        <legend className="card-title" style={{ marginBottom: 14 }}>
          Planos padrão do Windows
        </legend>
        {active && active.plan === null && (
          <p className="muted small" style={{ margin: 0 }}>
            Plano atual: “{active.name}” (personalizado). Escolher um plano abaixo troca para o padrão do Windows.
          </p>
        )}
        <div className="plan-grid">
          {PLANS.map((plan) => {
            const info = PLAN_INFO[plan];
            const isSelected = selected === plan;
            return (
              <label key={plan} className={`plan-card${isSelected ? " selected" : ""}`}>
                <span className="plan-card-head">
                  {info.name}
                  <input type="radio" name="plano" checked={isSelected} onChange={() => setSelected(plan)} />
                </span>
                <span className="plan-desc">{info.desc}</span>
                {active?.plan === plan && <span className="wip" style={{ alignSelf: "flex-start" }}>Atual</span>}
              </label>
            );
          })}
        </div>
        {selected && (
          <p className="muted small" style={{ margin: 0 }}>
            Comando: <code className="code-chip">powercfg /setactive {PLAN_INFO[selected].alias}</code>
          </p>
        )}
      </fieldset>

      <section className="card" aria-labelledby="efeitos-titulo">
        <h2 id="efeitos-titulo" className="card-title">
          Efeitos visuais
        </h2>
        <p className="plan-desc" style={{ margin: 0 }}>
          Transparência e animações ficam em <Link to="/ajustes">Todos os ajustes</Link>, na categoria Aparência, com desfazer.
        </p>
      </section>
    </>
  );
}

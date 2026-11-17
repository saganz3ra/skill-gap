import { useEffect } from "react";

import { api, NEWS_KIND, NEWS_SEEN_EVENT, type Release } from "../lib/api";
import { useResource } from "../hooks/useResource";
import { PageHeader } from "../components/ui";

/** Changelog completo. Abrir esta tela conta como "visto". */
export function Novidades() {
  const news = useResource(api.whatsNew);

  useEffect(() => {
    if (news.data && news.data.unseen.length > 0) {
      void api.whatsNewSeen().then(() => window.dispatchEvent(new Event(NEWS_SEEN_EVENT)));
    }
  }, [news.data]);

  return (
    <>
      <PageHeader
        eyebrow="Geral"
        title="Novidades"
        subtitle={news.data ? `Você está na versão ${news.data.current}. O que mudou em cada versão:` : "Carregando…"}
      />
      {news.data?.all.map((r) => (
        <ReleaseCard key={r.version} release={r} fresh={news.data!.unseen.some((u) => u.version === r.version)} />
      ))}
    </>
  );
}

export function ReleaseCard({ release: r, fresh }: { release: Release; fresh?: boolean }) {
  return (
    <section className="card" aria-labelledby={`v-${r.version}`}>
      <div className="card-head">
        <h2 id={`v-${r.version}`} className="card-title">
          Versão {r.version}
        </h2>
        <span className="muted small">
          {r.date ? new Date(`${r.date}T12:00:00`).toLocaleDateString("pt-BR") : "em desenvolvimento"}
          {fresh && <span className="badge badge-info" style={{ marginLeft: 8 }}>Novo pra você</span>}
        </span>
      </div>
      <ul className="news-list">
        {r.items.map((it, i) => (
          <li key={i}>
            <span className={`badge ${NEWS_KIND[it.kind][1]}`}>{NEWS_KIND[it.kind][0]}</span>
            <span>{it.text}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

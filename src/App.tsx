import { lazy } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeProvider } from "./hooks/theme";
import { OverlayWindow } from "./components/Overlay";
import { Layout } from "./components/Layout";
import { Inicio } from "./pages/Inicio";

// O Início abre junto com o app; as outras telas só carregam quando abertas
// (arquivos separados no build), o que deixa a abertura mais leve.
const Limpeza = lazy(() => import("./pages/Limpeza").then((m) => ({ default: m.Limpeza })));
const Inicializacao = lazy(() => import("./pages/Inicializacao").then((m) => ({ default: m.Inicializacao })));
const Desempenho = lazy(() => import("./pages/Desempenho").then((m) => ({ default: m.Desempenho })));
const ModoJogo = lazy(() => import("./pages/ModoJogo").then((m) => ({ default: m.ModoJogo })));
const Ajustes = lazy(() => import("./pages/Ajustes").then((m) => ({ default: m.Ajustes })));
const Perfis = lazy(() => import("./pages/Perfis").then((m) => ({ default: m.Perfis })));
const Monitor = lazy(() => import("./pages/Monitor").then((m) => ({ default: m.Monitor })));
const Drivers = lazy(() => import("./pages/Drivers").then((m) => ({ default: m.Drivers })));
const Correcoes = lazy(() => import("./pages/Correcoes").then((m) => ({ default: m.Correcoes })));
const Mouse = lazy(() => import("./pages/Mouse").then((m) => ({ default: m.Mouse })));
const Novidades = lazy(() => import("./pages/Novidades").then((m) => ({ default: m.Novidades })));
const Diagnostico = lazy(() => import("./pages/Diagnostico").then((m) => ({ default: m.Diagnostico })));
const Hardware = lazy(() => import("./pages/Hardware").then((m) => ({ default: m.Hardware })));

// MemoryRouter: num app desktop não existe barra de endereço, então a rota fica só
// em memória. Evita 404 ao recarregar uma rota interna no build de produção.
export default function App() {
  // A janela "overlay" usa o mesmo index.html, mas só mostra o painel transparente.
  if (getCurrentWindow().label === "overlay") return <OverlayWindow />;
  return (
    <ThemeProvider>
      <MemoryRouter>
        <Routes>
          <Route element={<Layout />}>
            <Route index element={<Inicio />} />
            <Route path="perfis" element={<Perfis />} />
            <Route path="monitor" element={<Monitor />} />
            <Route path="drivers" element={<Drivers />} />
            <Route path="correcoes" element={<Correcoes />} />
            <Route path="mouse" element={<Mouse />} />
            <Route path="ajustes" element={<Ajustes />} />
            <Route path="limpeza" element={<Limpeza />} />
            <Route path="inicializacao" element={<Inicializacao />} />
            <Route path="desempenho" element={<Desempenho />} />
            <Route path="modo-jogo" element={<ModoJogo />} />
            <Route path="novidades" element={<Novidades />} />
            <Route path="diagnostico" element={<Diagnostico />} />
            <Route path="hardware" element={<Hardware />} />
          </Route>
        </Routes>
      </MemoryRouter>
    </ThemeProvider>
  );
}

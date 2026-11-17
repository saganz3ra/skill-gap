import { MemoryRouter, Route, Routes } from "react-router-dom";

import { getCurrentWindow } from "@tauri-apps/api/window";
import { ThemeProvider } from "./hooks/theme";
import { OverlayWindow } from "./components/Overlay";
import { Layout } from "./components/Layout";
import { Inicio } from "./pages/Inicio";
import { Limpeza } from "./pages/Limpeza";
import { Inicializacao } from "./pages/Inicializacao";
import { Desempenho } from "./pages/Desempenho";
import { ModoJogo } from "./pages/ModoJogo";
import { Ajustes } from "./pages/Ajustes";
import { Perfis } from "./pages/Perfis";
import { Monitor } from "./pages/Monitor";
import { Drivers } from "./pages/Drivers";
import { Correcoes } from "./pages/Correcoes";
import { Mouse } from "./pages/Mouse";
import { Novidades } from "./pages/Novidades";
import { Diagnostico } from "./pages/Diagnostico";
import { Hardware } from "./pages/Hardware";

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

import React from "react";
import ReactDOM from "react-dom/client";

import "@fontsource-variable/inter";
// Saira: títulos e números no estilo do logo SKG (itálico pesado).
import "@fontsource/saira/600.css";
import "@fontsource/saira/700.css";
import "@fontsource/saira/700-italic.css";
import "@fontsource/saira/800-italic.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "./styles/theme.css";
import "./styles/app.css";

import App from "./App";
import { applyTheme, readSavedTheme, systemTheme } from "./hooks/theme";

// Fontes vêm empacotadas (fontsource), não do Google Fonts: o app funciona offline
// e a CSP não precisa liberar domínios externos.

applyTheme(readSavedTheme() ?? systemTheme());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { installCspReporter } from "./diagnostics/cspReporter";
import { initI18n } from "./i18n";
import "./styles.css";

// Before rendering, so violations during the first paint are reported too.
installCspReporter();
initI18n();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

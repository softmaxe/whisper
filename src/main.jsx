import React from "react";
import ReactDOM from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import AppRouter from "./AppRouter.jsx";
import CleanupFailureToastListener from "./components/CleanupFailureToastListener.tsx";
import ErrorBoundary from "./components/ErrorBoundary.tsx";
import { ToastProvider } from "./components/ui/Toast.tsx";
import { SettingsProvider } from "./hooks/useSettings";
import { bindDocumentLanguage } from "./utils/i18nDocument";

import i18n from "./i18n";
import "./brandFonts";
import "./index.css";

bindDocumentLanguage(i18n, document.documentElement, import.meta.hot);

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(
  <React.StrictMode>
    <ErrorBoundary>
      <I18nextProvider i18n={i18n}>
        <SettingsProvider>
          <ToastProvider>
            <CleanupFailureToastListener />

            <AppRouter />
          </ToastProvider>
        </SettingsProvider>
      </I18nextProvider>
    </ErrorBoundary>
  </React.StrictMode>
);

if (import.meta.hot) {
  import.meta.hot.accept();
}

import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { isLanHost, isLoopbackHost } from "./lib/connection";
import "./index.css";

if (
  window.location.protocol === "http:" &&
  !isLoopbackHost(window.location.hostname) &&
  !isLanHost(window.location.hostname)
) {
  window.location.replace(`https://${window.location.host}${window.location.pathname}${window.location.search}${window.location.hash}`);
}

const root = document.getElementById("root");
if (!root) {
  throw new Error("Shelf root element is missing");
}

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);

requestAnimationFrame(() => {
  document.getElementById("boot")?.remove();
});

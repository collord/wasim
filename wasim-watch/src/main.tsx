import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import StreamApp from "./StreamApp";
import "./styles.css";

// The synthetic "watch it run" view is the default; `?live` selects the real WASM
// streaming view (StreamApp). Additive — the synthetic path is untouched.
const live = new URLSearchParams(window.location.search).has("live");
const Root = live ? StreamApp : App;

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
);

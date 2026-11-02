import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import ManualPanel from "./ManualPanel";
import "./styles/globals.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {new URLSearchParams(window.location.search).get("panel") === "1" ? <ManualPanel /> : <App />}
  </React.StrictMode>,
);

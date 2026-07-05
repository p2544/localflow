import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Pill from "./Pill";
import "./styles.css";

const isPill = new URLSearchParams(window.location.search).get("window") === "pill";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isPill ? <Pill /> : <App />}</React.StrictMode>
);

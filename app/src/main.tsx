// SPDX-License-Identifier: GPL-3.0-or-later
import "./styles/app.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import App from "./App";

// A saved Light/Dark override is already set by the window's initialization script. For
// "System", guess from the webview until the backend reports the window's real theme.
document.documentElement.dataset.theme ??= matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

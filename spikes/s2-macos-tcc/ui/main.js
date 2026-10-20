// SPDX-License-Identifier: GPL-3.0-or-later
const { invoke } = window.__TAURI__.core;

async function showInfo() {
  const info = await invoke("info");
  document.getElementById("ver").textContent = "v" + info.version;
  document.getElementById("meta").textContent = info.identifier + " · " + info.exe;
}

async function probe(folder) {
  const started = new Date().toLocaleTimeString();
  const r = await invoke("probe", { folder });
  const li = document.createElement("li");
  li.innerHTML = r.ok
    ? `<span class="ok">✅ ${r.folder}</span>: ${r.entries} entries <span class="meta">(${started})</span>`
    : `<span class="err">❌ ${r.folder}</span>: ${r.error} <span class="meta">(${started})</span>`;
  document.getElementById("log").prepend(li);
}

document.querySelectorAll("button[data-folder]").forEach((b) => b.addEventListener("click", () => probe(b.dataset.folder)));
showInfo();

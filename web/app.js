"use strict";

const byId = (id) => document.getElementById(id);

document.querySelectorAll(".tab").forEach((button) => {
  button.addEventListener("click", () => {
    const name = button.dataset.panel;

    document.querySelectorAll(".tab").forEach((item) => {
      item.classList.toggle("active", item === button);
    });

    document.querySelectorAll(".panel").forEach((panel) => {
      panel.classList.toggle("active", panel.id === `panel-${name}`);
    });
  });
});

async function refreshStatus() {
  const lamp = byId("runtime-lamp");
  const runtime = byId("runtime-status");

  try {
    const response = await fetch("/api/status", {
      method: "GET",
      cache: "no-store",
      credentials: "same-origin"
    });

    if (!response.ok) {
      throw new Error(`HTTP ${response.status}`);
    }

    const status = await response.json();
    const workspace = status.workspace;

    lamp.className = "lamp ok";
    runtime.textContent = "READY";
    byId("footer-state").textContent = "READY";
    byId("version-value").textContent = `v${status.version}`;
    byId("network-value").textContent = status.network;

    const state = workspace.initialized ? "READY" : "NOT INIT";
    const count = `${workspace.presentDirectories}/${workspace.totalDirectories} dirs`;

    byId("workspace-state").textContent = state;
    byId("workspace-count").textContent = count;
    byId("workspace-state-detail").textContent = state;
    byId("workspace-count-detail").textContent = count;
    byId("workspace-path").textContent = workspace.path;
  } catch (error) {
    lamp.className = "lamp bad";
    runtime.textContent = "OFFLINE";
    byId("footer-state").textContent = "ERROR";
    console.error(error);
  }
}

byId("refresh-button").addEventListener("click", refreshStatus);
refreshStatus();

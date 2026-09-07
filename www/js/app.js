// Main application logic for getrho website

document.addEventListener("DOMContentLoaded", () => {
  initTheme();
  initInstallSwitcher();
  initCopyButtons();
  initTerminalSimulator();
});

/* =========================================================================
   Theme Management
   ========================================================================= */
function initTheme() {
  const root = document.documentElement;
  const storageKey = "rho:theme";
  const savedTheme = localStorage.getItem(storageKey);
  const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

  const currentTheme = savedTheme || (mediaQuery.matches ? "dark" : "light");
  setTheme(currentTheme);

  const themeToggleBtn = document.getElementById("themeToggle");
  if (themeToggleBtn) {
    themeToggleBtn.addEventListener("click", () => {
      const activeTheme = root.dataset.theme === "dark" ? "light" : "dark";
      setTheme(activeTheme);
      localStorage.setItem(storageKey, activeTheme);
    });
  }

  mediaQuery.addEventListener("change", (e) => {
    if (!localStorage.getItem(storageKey)) {
      setTheme(e.matches ? "dark" : "light");
    }
  });
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  const themeToggleBtn = document.getElementById("themeToggle");
  if (themeToggleBtn) {
    themeToggleBtn.setAttribute("aria-label", `Switch to ${theme === "dark" ? "light" : "dark"} theme`);
  }
}

/* =========================================================================
   Install Switcher
   ========================================================================= */
const INSTALL_COMMANDS = {
  cargo: "cargo install rho",
  brew: "brew install casonadams/tap/rho",
  bin: "gh release download -R casonadams/rho",
  source: "git clone https://github.com/casonadams/rho.git && cd rho && cargo install --path ."
};

function initInstallSwitcher() {
  const tabs = document.querySelectorAll(".install-tab");
  const codeEl = document.getElementById("installCode");
  const copyBtn = document.getElementById("installCopyBtn");

  if (!tabs.length || !codeEl || !copyBtn) return;

  tabs.forEach(tab => {
    tab.addEventListener("click", () => {
      tabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");

      const method = tab.dataset.installTab;
      const cmd = INSTALL_COMMANDS[method] || INSTALL_COMMANDS.cargo;
      codeEl.textContent = cmd;
      copyBtn.dataset.copy = cmd;
    });
  });
}

/* =========================================================================
   Clipboard Helper
   ========================================================================= */
function initCopyButtons() {
  document.addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-copy]");
    if (!btn) return;

    const textToCopy = btn.dataset.copy;
    if (!textToCopy) return;

    try {
      await navigator.clipboard.writeText(textToCopy);
      const originalHtml = btn.innerHTML;
      btn.classList.add("copied");

      const labelSpan = btn.querySelector("span");
      if (labelSpan) {
        labelSpan.textContent = "Copied!";
      }

      setTimeout(() => {
        btn.classList.remove("copied");
        btn.innerHTML = originalHtml;
      }, 2000);
    } catch (err) {
      console.warn("Failed to copy text to clipboard:", err);
    }
  });
}

/* =========================================================================
   Interactive Terminal Simulator
   ========================================================================= */
const SCENARIOS = {
  plugin: [
    { type: "input", text: "rho -p 'clean system temporary files and inspect cache'", delay: 100 },
    { type: "agent", text: "⚡ Active daemon plugin: 'rust-guard' (subscribed: tool_call, tool_result)", delay: 350 },
    { type: "status", text: "Agent requesting bash command execution...", delay: 350 },
    { type: "tool", name: "bash", args: '{ command: "sudo rm -rf /tmp/build_cache/*" }', delay: 500 },
    { type: "plugin_hook", text: "⚡ [rust-guard] intercepted hook/tool_call -> invoking ctx.confirm()", delay: 400 },
    { type: "modal", title: "Security Alert: Plugin confirmation requested", target: "sudo rm -rf /tmp/build_cache/*", delay: 500 },
    { type: "modal_select", action: "Allow [y]", delay: 700 },
    { type: "plugin_status", text: "🔒 [rust-guard] ctx.set_status('security', '🔒 Guard Active')", delay: 350 },
    { type: "tool_res", text: "✓ Command completed with exit code 0 (cleared 240 MB).", delay: 400 },
    { type: "output", text: "Cache cleared successfully under active plugin audit.", delay: 400 }
  ],
  install: [
    { type: "input", text: "rho install permission", delay: 100 },
    { type: "status", text: "Resolving GitHub release 'casonadams/rho-plugin-permission'...", delay: 350 },
    { type: "status", text: "Matched platform release: aarch64-apple-darwin (v0.4.0)", delay: 350 },
    { type: "status", text: "Downloading precompiled release binary...", delay: 450 },
    { type: "status", text: "Atomically installed executable -> ~/.cargo/bin/rho-plugin-permission", delay: 400 },
    { type: "status", text: "Registered in ~/.config/rho/config.toml under [plugins.permission]", delay: 350 },
    { type: "output", text: "Successfully installed 'permission' v0.4.0", delay: 450 },
    { type: "input", text: "rho plugin ls", delay: 350 },
    { type: "listing", text: "Configured MCP Servers & Plugins:\n  • permission: Installed (active) - ~/.cargo/bin/rho-plugin-permission\n  • filesystem (mcp): Configured - npx @modelcontextprotocol/server-filesystem", delay: 500 }
  ],
  resume: [
    { type: "input", text: "rho -r", delay: 100 },
    { type: "picker", text: "? Select session to resume:\n  ❯ sess-01990c (2 mins ago · gemini-3.8-flash · 4 turns · 'crates/rho-engine')\n    sess-01988a (1 hour ago · claude-3-7-sonnet · 12 turns · 'docs/plugins')\n    sess-01982f (yesterday · qwen2.5-coder:14b · 8 turns · 'Makefile')", delay: 600 },
    { type: "status", text: "Resuming session sess-01990c with budget checkpoint...", delay: 400 },
    { type: "agent", text: "Session resumed. 4,120 tokens in context. Ready for next prompt.", delay: 400 },
    { type: "user_turn", text: "rho> Show me the diff we prepared before pausing", delay: 500 },
    { type: "output", text: "Displaying working diff for crates/rho-engine/src/lib.rs (+14, -2 lines)", delay: 400 }
  ]
};

let currentSimTimer = null;

function initTerminalSimulator() {
  const terminalBody = document.getElementById("terminalSimBody");
  const scenarioBtns = document.querySelectorAll(".scenario-btn");
  if (!terminalBody) return;

  scenarioBtns.forEach(btn => {
    btn.addEventListener("click", () => {
      scenarioBtns.forEach(b => b.classList.remove("active"));
      btn.classList.add("active");
      const scenarioKey = btn.dataset.scenario || "plugin";
      playScenario(scenarioKey);
    });
  });

  playScenario("plugin");
}

function playScenario(scenarioKey) {
  if (currentSimTimer) {
    clearTimeout(currentSimTimer);
  }

  const terminalBody = document.getElementById("terminalSimBody");
  if (!terminalBody) return;
  terminalBody.innerHTML = "";

  const steps = SCENARIOS[scenarioKey] || SCENARIOS.plugin;
  let stepIndex = 0;

  function runNextStep() {
    if (stepIndex >= steps.length) {
      currentSimTimer = setTimeout(() => {
        playScenario(scenarioKey);
      }, 5000);
      return;
    }

    const step = steps[stepIndex++];
    renderStep(terminalBody, step);
    terminalBody.scrollTop = terminalBody.scrollHeight;
    updateTelemetry(stepIndex, steps.length, scenarioKey);

    currentSimTimer = setTimeout(runNextStep, step.delay || 450);
  }

  runNextStep();
}

function renderStep(container, step) {
  const div = document.createElement("div");
  div.style.marginBottom = "0.45rem";

  switch (step.type) {
    case "input":
      div.innerHTML = `<span class="t-prompt">$ </span><span class="t-cmd">${escapeHtml(step.text)}</span>`;
      break;
    case "status":
      div.innerHTML = `<span class="t-dim">${escapeHtml(step.text)}</span>`;
      break;
    case "plugin_hook":
      div.innerHTML = `<span class="t-purple">${escapeHtml(step.text)}</span>`;
      break;
    case "plugin_status":
      div.innerHTML = `<span class="t-green t-bold">${escapeHtml(step.text)}</span>`;
      break;
    case "tool":
      div.className = "t-tool-box";
      div.innerHTML = `<span class="t-cyan t-bold">tool_call:</span> <span class="t-green">${escapeHtml(step.name)}</span> <span class="t-dim">${escapeHtml(step.args)}</span>`;
      break;
    case "tool_res":
      div.innerHTML = `<span class="t-green">${escapeHtml(step.text)}</span>`;
      break;
    case "modal":
      div.className = "t-modal-box";
      div.innerHTML = `
        <div style="color: #facc15; font-weight: 700;">⚠ ${escapeHtml(step.title)}</div>
        <div style="font-size: 0.8rem; color: #94a3b8; margin: 0.25rem 0;">Command: <code>${escapeHtml(step.target)}</code></div>
        <div class="t-modal-options">
          <span class="t-modal-btn selected">[y] Allow</span>
          <span class="t-modal-btn">[e] Edit</span>
          <span class="t-modal-btn">[a] Always</span>
          <span class="t-modal-btn">[n] Deny</span>
        </div>
      `;
      break;
    case "modal_select":
      div.innerHTML = `<span class="t-dim">&gt; Selection: </span><span class="t-green t-bold">${escapeHtml(step.action)}</span>`;
      break;
    case "agent":
      div.innerHTML = `<span class="t-cyan">${escapeHtml(step.text)}</span>`;
      break;
    case "user_turn":
      div.innerHTML = `<span class="t-prompt">${escapeHtml(step.text)}</span>`;
      break;
    case "picker":
      div.innerHTML = `<pre style="color: #93c5fd; font-size: 0.82rem; line-height: 1.45;">${escapeHtml(step.text)}</pre>`;
      break;
    case "listing":
      div.innerHTML = `<pre style="color: #a7f3d0; font-size: 0.82rem; line-height: 1.45;">${escapeHtml(step.text)}</pre>`;
      break;
    case "output":
      div.innerHTML = `<span class="t-green">✔ </span><span>${escapeHtml(step.text)}</span>`;
      break;
    default:
      div.textContent = step.text;
  }

  container.appendChild(div);
}

function updateTelemetry(current, total, scenarioKey) {
  const tokVal = document.getElementById("telemetryTokens");
  const speedVal = document.getElementById("telemetrySpeed");
  const costVal = document.getElementById("telemetryCost");
  const barVal = document.getElementById("telemetryBar");
  const statusBadge = document.getElementById("telemetryStatusBadge");

  if (!tokVal) return;

  const pct = Math.min(100, Math.round((current / total) * 100));
  const baseTokens = 1280 + (current * 240);
  const cost = (baseTokens * 0.000002).toFixed(4);

  tokVal.textContent = baseTokens.toLocaleString();
  if (speedVal) speedVal.textContent = "142 tok/s";
  if (costVal) costVal.textContent = `$${cost}`;
  if (barVal) barVal.style.width = `${Math.max(14, pct)}%`;

  if (statusBadge) {
    if (scenarioKey === "plugin" && current >= 7) {
      statusBadge.textContent = "🔒 Guard Active";
      statusBadge.style.display = "inline-block";
    } else {
      statusBadge.style.display = "none";
    }
  }
}

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

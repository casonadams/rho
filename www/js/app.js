// Main application logic for rho website

document.addEventListener("DOMContentLoaded", () => {
  initTheme();
  initInstallSwitcher();
  initCopyButtons();
  initGallery();
});

/* =========================================================================
   UI Showcase Gallery
   ========================================================================= */
function initGallery() {
  const tabs = document.querySelectorAll(".gallery-tab");
  const img = document.getElementById("galleryImage");
  const caption = document.getElementById("galleryCaption");
  if (!tabs.length || !img) return;

  const galleryData = {
    welcome: {
      src: "assets/welcome.png",
      alt: "rho startup welcome screen with live rolling quota",
      caption: "Startup screen with active agents, skills, tools, and live rolling quota in footer (<code>93% 4h19m 1% 3d10h</code>)."
    },
    turn: {
      src: "assets/chat.png",
      alt: "rho interactive turn with reasoning block and speed telemetry",
      caption: "Streaming assistant turn showing collapsible reasoning block, clean syntax, and live speed telemetry (<code>@43t/s</code>)."
    },
    model: {
      src: "assets/modal-model.png",
      alt: "rho interactive model selector with fuzzy search",
      caption: "Interactive model switcher (<kbd>Ctrl+L</kbd> or <code>/model</code>) with live fuzzy search across local and subscription providers."
    },
    theme: {
      src: "assets/modal-theme.png",
      alt: "rho interactive theme switcher previewing Solarized Dark",
      caption: "Real-time theme engine (<code>/theme</code>) previewing 10 built-in color schemes with live terminal ANSI sync."
    },
    login: {
      src: "assets/modal-login.png",
      alt: "rho login provider selector for OAuth and API keys",
      caption: "Interactive provider login (<code>rho login</code>) supporting OAuth PKCE for Google Antigravity, ChatGPT, Claude, and Copilot."
    }
  };

  tabs.forEach(tab => {
    tab.addEventListener("click", () => {
      tabs.forEach(t => t.classList.remove("active"));
      tab.classList.add("active");
      const key = tab.dataset.gallery || "welcome";
      const item = galleryData[key];
      if (item) {
        img.src = item.src;
        img.alt = item.alt;
        if (caption) caption.innerHTML = item.caption;
      }
    });
  });
}

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

import { CURATED_PLUGINS } from "./plugins-data.js";

const CACHE_KEY = "rho:plugins:cache:v2";
const CACHE_TTL = 10 * 60 * 1000; // 10 minutes

/**
 * Fetch live plugins from Crates.io and GitHub using free public APIs.
 * Zero backend cost, client-side cached, fallback to curated seed.
 */
export async function loadAllPlugins() {
  const cached = getCachedPlugins();
  if (cached) {
    return cached;
  }

  const pluginMap = new Map();
  for (const item of CURATED_PLUGINS) {
    pluginMap.set(item.id.toLowerCase(), { ...item });
  }

  try {
    // 1. Fetch crates.io plugins tagged with rho-plugin
    const cratesPromise = fetch("https://crates.io/api/v1/crates?q=rho-plugin", {
      headers: { "Accept": "application/json" }
    })
      .then(res => res.ok ? res.json() : null)
      .catch(() => null);

    // 2. Fetch GitHub repos tagged with topic:rho-plugin
    const githubPromise = fetch("https://api.github.com/search/repositories?q=topic:rho-plugin&sort=updated", {
      headers: { "Accept": "application/vnd.github.v3+json" }
    })
      .then(res => res.ok ? res.json() : null)
      .catch(() => null);

    const [cratesData, githubData] = await Promise.allSettled([cratesPromise, githubPromise]);

    // Process Crates.io results
    if (cratesData.status === "fulfilled" && cratesData.value && Array.isArray(cratesData.value.crates)) {
      for (const crate of cratesData.value.crates) {
        if (!crate.name || crate.yanked) continue;
        const key = crate.name.toLowerCase();
        const existing = pluginMap.get(key) || {};

        const isSdk = crate.name.includes("sdk");
        const bareName = crate.name.replace(/^rho-plugin-/, "");

        pluginMap.set(key, {
          id: crate.name,
          name: crate.name,
          type: isSdk ? "sdk" : "plugin",
          category: existing.category || "Rust Plugin",
          description: crate.description || existing.description || "Rust plugin for rho coding agent.",
          author: existing.author || "crates.io",
          version: crate.max_version || crate.default_version || existing.version || "0.1.0",
          downloads: crate.downloads || existing.downloads || 0,
          stars: existing.stars || 0,
          repoUrl: crate.repository || existing.repoUrl || `https://crates.io/crates/${crate.name}`,
          cratesUrl: `https://crates.io/crates/${crate.name}`,
          installCmd: isSdk ? `cargo add ${crate.name}` : `rho install ${bareName}`,
          isOfficial: existing.isOfficial || false
        });
      }
    }

    // Process GitHub results
    if (githubData.status === "fulfilled" && githubData.value && Array.isArray(githubData.value.items)) {
      for (const repo of githubData.value.items) {
        const key = repo.name.toLowerCase();
        const existing = pluginMap.get(key) || {};
        const isOfficial = repo.owner?.login === "casonadams" || existing.isOfficial || false;
        const bareName = repo.name.replace(/^rho-plugin-/, "");

        pluginMap.set(key, {
          id: repo.name,
          name: repo.name,
          type: existing.type || (repo.name.startsWith("mcp-") ? "mcp" : "plugin"),
          category: existing.category || "Community Plugin",
          description: repo.description || existing.description || "Community plugin for rho coding agent.",
          author: repo.owner ? repo.owner.login : "github",
          version: existing.version || "latest",
          downloads: existing.downloads || 0,
          stars: repo.stargazers_count || existing.stars || 0,
          repoUrl: repo.html_url || existing.repoUrl,
          cratesUrl: existing.cratesUrl || null,
          installCmd: isOfficial ? `rho install ${bareName}` : `rho install ${repo.full_name}`,
          isOfficial
        });
      }
    }
  } catch (err) {
    console.warn("Remote plugin fetch failed; using cached seed registry", err);
  }

  const plugins = Array.from(pluginMap.values());
  saveCachedPlugins(plugins);
  return plugins;
}

function getCachedPlugins() {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const item = JSON.parse(raw);
    if (Date.now() - item.timestamp > CACHE_TTL) {
      sessionStorage.removeItem(CACHE_KEY);
      return null;
    }
    return item.data;
  } catch {
    return null;
  }
}

function saveCachedPlugins(data) {
  try {
    sessionStorage.setItem(CACHE_KEY, JSON.stringify({ timestamp: Date.now(), data }));
  } catch {
    // Ignore storage quota errors
  }
}

export function filterPlugins(plugins, { query = "", type = "all", sort = "downloads" } = {}) {
  let filtered = [...plugins];

  if (query.trim()) {
    const q = query.toLowerCase().trim();
    filtered = filtered.filter(p =>
      p.name.toLowerCase().includes(q) ||
      p.description.toLowerCase().includes(q) ||
      p.author.toLowerCase().includes(q) ||
      p.category.toLowerCase().includes(q)
    );
  }

  if (type && type !== "all") {
    filtered = filtered.filter(p => p.type === type);
  }

  filtered.sort((a, b) => {
    if (sort === "downloads") return (b.downloads || 0) - (a.downloads || 0);
    if (sort === "stars") return (b.stars || 0) - (a.stars || 0);
    if (sort === "name") return a.name.localeCompare(b.name);
    return 0;
  });

  return filtered;
}

export function renderPluginCard(plugin) {
  const isMcp = plugin.type === "mcp";
  const tagClass = isMcp ? "tag-mcp" : (plugin.isOfficial ? "tag-core" : "tag-extension");
  const tagLabel = isMcp ? "MCP Server" : (plugin.type === "sdk" ? "Rust SDK" : "Plugin");

  return `
    <article class="plugin-card" data-id="${escapeHtml(plugin.id)}">
      <div class="plugin-header">
        <div>
          <h3 class="plugin-title">${escapeHtml(plugin.name)}</h3>
          <span style="font-size: 0.78rem; color: var(--text-muted);">by @${escapeHtml(plugin.author)}</span>
        </div>
        <span class="plugin-tag ${tagClass}">${tagLabel}</span>
      </div>

      <p class="plugin-desc">${escapeHtml(plugin.description)}</p>

      <div class="plugin-meta">
        <span class="plugin-meta-item">v${escapeHtml(plugin.version)}</span>
        <span class="plugin-meta-item">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
          ${formatNumber(plugin.downloads)} dl
        </span>
        <span class="plugin-meta-item">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          ${formatNumber(plugin.stars)}
        </span>
      </div>

      <div class="plugin-install-bar">
        <code class="plugin-install-cmd">${isMcp ? "" : "$ "}${escapeHtml(plugin.installCmd)}</code>
        ${isMcp ? "" : `
        <button class="copy-btn" data-copy="${escapeHtml(plugin.installCmd)}" aria-label="Copy install command">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
          <span>Copy</span>
        </button>
        `}
      </div>

      <div class="plugin-links">
        ${plugin.repoUrl ? `<a href="${escapeHtml(plugin.repoUrl)}" target="_blank" rel="noopener">GitHub</a>` : ""}
        ${plugin.cratesUrl ? `<a href="${escapeHtml(plugin.cratesUrl)}" target="_blank" rel="noopener">Crates.io</a>` : ""}
      </div>
    </article>
  `;
}

function formatNumber(num) {
  if (!num) return "0";
  if (num >= 1000000) return (num / 1000000).toFixed(1) + "M";
  if (num >= 1000) return (num / 1000).toFixed(1) + "K";
  return num.toString();
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

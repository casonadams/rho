import { CURATED_PLUGINS } from "./plugins-data.js";

/**
 * Load verified extensions, MCP servers, and plugins.
 */
export async function loadAllPlugins() {
  return [...CURATED_PLUGINS];
}

export function filterPlugins(plugins, { query = "", type = "all", sort = "default" } = {}) {
  let filtered = [...plugins];

  if (query.trim()) {
    const q = query.toLowerCase().trim();
    filtered = filtered.filter(p =>
      p.name.toLowerCase().includes(q) ||
      p.description.toLowerCase().includes(q) ||
      p.author.toLowerCase().includes(q) ||
      p.category.toLowerCase().includes(q) ||
      (p.runtime && p.runtime.toLowerCase().includes(q))
    );
  }

  if (type && type !== "all") {
    filtered = filtered.filter(p => p.type === type);
  }

  if (sort === "name") {
    filtered.sort((a, b) => a.name.localeCompare(b.name));
  } else if (sort === "type") {
    filtered.sort((a, b) => a.type.localeCompare(b.type));
  }

  return filtered;
}

export function renderPluginCard(plugin) {
  const isMcp = plugin.type === "mcp";
  const tagClass = isMcp ? "tag-mcp" : (plugin.isOfficial ? "tag-core" : "tag-extension");

  return `
    <article class="plugin-card" data-id="${escapeHtml(plugin.id)}">
      <div class="plugin-header">
        <div>
          <h3 class="plugin-title">${escapeHtml(plugin.name)}</h3>
          <span style="font-size: 0.78rem; color: var(--text-muted);">by @${escapeHtml(plugin.author)}</span>
        </div>
        <span class="plugin-tag ${tagClass}">${escapeHtml(plugin.badge)}</span>
      </div>

      <p class="plugin-desc">${escapeHtml(plugin.description)}</p>

      <div class="plugin-meta">
        <span class="plugin-meta-item"><strong>Category:</strong> ${escapeHtml(plugin.category)}</span>
        <span class="plugin-meta-item"><strong>Runtime:</strong> ${escapeHtml(plugin.runtime || "Native")}</span>
      </div>

      <div style="margin-top: 1rem;">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.35rem;">
          <span style="font-size: 0.74rem; font-weight: 600; color: var(--text-muted);">${escapeHtml(plugin.snippetLabel || "Configuration")}</span>
        </div>
        <div class="plugin-install-bar" style="align-items: flex-start; padding: 0.6rem 0.75rem;">
          <pre style="margin: 0; font-family: var(--font-mono); font-size: 0.78rem; line-height: 1.45; color: var(--accent-green); flex: 1; overflow-x: auto; white-space: pre;"><code>${escapeHtml(plugin.snippet)}</code></pre>
          <button class="copy-btn" data-copy="${escapeHtml(plugin.snippet)}" aria-label="Copy snippet" style="margin-left: 0.5rem; flex-shrink: 0;">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
            <span>Copy</span>
          </button>
        </div>
      </div>

      <div class="plugin-links" style="margin-top: 1rem;">
        ${plugin.repoUrl ? `<a href="${escapeHtml(plugin.repoUrl)}" target="_blank" rel="noopener">Source</a>` : ""}
        ${plugin.cratesUrl ? `<a href="${escapeHtml(plugin.cratesUrl)}" target="_blank" rel="noopener">Crates.io</a>` : ""}
      </div>
    </article>
  `;
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

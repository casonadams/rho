import { CURATED_EXTENSIONS } from "./plugins-data.js";

/**
 * Load verified extensions (MCP servers and lifecycle hooks).
 */
export async function loadAllExtensions() {
  return [...CURATED_EXTENSIONS];
}

export function filterExtensions(extensions, { query = "", type = "all", sort = "default" } = {}) {
  let filtered = [...extensions];

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

export function renderExtensionCard(item) {
  const isMcp = item.type === "mcp";
  const tagClass = isMcp ? "tag-mcp" : (item.isOfficial ? "tag-core" : "tag-extension");

  return `
    <article class="extension-card" data-id="${escapeHtml(item.id)}">
      <div class="extension-header">
        <div>
          <h3 class="extension-title">${escapeHtml(item.name)}</h3>
          <span style="font-size: 0.78rem; color: var(--text-muted);">by @${escapeHtml(item.author)}</span>
        </div>
        <span class="extension-tag ${tagClass}">${escapeHtml(item.badge)}</span>
      </div>

      <p class="extension-desc">${escapeHtml(item.description)}</p>

      <div class="extension-meta">
        <span class="extension-meta-item"><strong>Category:</strong> ${escapeHtml(item.category)}</span>
        <span class="extension-meta-item"><strong>Runtime:</strong> ${escapeHtml(item.runtime || "Native")}</span>
      </div>

      <div style="margin-top: 1rem;">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 0.35rem;">
          <span style="font-size: 0.74rem; font-weight: 600; color: var(--text-muted);">${escapeHtml(item.snippetLabel || "Configuration")}</span>
        </div>
        <div class="install-bar" style="align-items: flex-start; padding: 0.6rem 0.75rem;">
          <pre style="margin: 0; font-family: var(--font-mono); font-size: 0.78rem; line-height: 1.45; color: var(--accent-green); flex: 1; overflow-x: auto; white-space: pre;"><code>${escapeHtml(item.snippet)}</code></pre>
          <button class="copy-btn" data-copy="${escapeHtml(item.snippet)}" aria-label="Copy snippet" style="margin-left: 0.5rem; flex-shrink: 0;">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
            <span>Copy</span>
          </button>
        </div>
      </div>

      <div class="extension-links" style="margin-top: 1rem;">
        ${item.repoUrl ? `<a href="${escapeHtml(item.repoUrl)}" target="_blank" rel="noopener">Source</a>` : ""}
        ${item.cratesUrl ? `<a href="${escapeHtml(item.cratesUrl)}" target="_blank" rel="noopener">Crates.io</a>` : ""}
      </div>
    </article>
  `;
}

export const loadAllPlugins = loadAllExtensions;
export const filterPlugins = filterExtensions;
export const renderPluginCard = renderExtensionCard;

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

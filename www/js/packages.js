import { loadAllExtensions, filterExtensions, renderExtensionCard } from "./plugins.js";

let allExtensions = [];
let currentType = "all";
let currentQuery = "";
let currentSort = "default";

const gridEl = document.getElementById("extensionsGrid");
const countEl = document.getElementById("extensionCount");
const searchInput = document.getElementById("extensionSearchInput");
const sortSelect = document.getElementById("sortSelect");
const filterBtns = document.querySelectorAll(".filter-btn");

async function init() {
  allExtensions = await loadAllExtensions();
  applyFilters();
}

function applyFilters() {
  const filtered = filterExtensions(allExtensions, {
    query: currentQuery,
    type: currentType,
    sort: currentSort
  });

  countEl.textContent = `Showing ${filtered.length} of ${allExtensions.length} verified extensions`;

  if (!filtered.length) {
    gridEl.innerHTML = `
      <div style="grid-column: 1 / -1; padding: 3rem; text-align: center; color: var(--text-muted); background: var(--bg-surface); border: 1px solid var(--border-subtle); border-radius: 8px;">
        <p style="font-size: 1.1rem; margin-bottom: 0.5rem; color: var(--text-primary);">No matching extensions found</p>
        <p style="font-size: 0.88rem;">Try searching for a different keyword or reset category filters.</p>
      </div>
    `;
    return;
  }

  gridEl.innerHTML = filtered.map(renderExtensionCard).join("");
}

if (searchInput) {
  searchInput.addEventListener("input", (e) => {
    currentQuery = e.target.value;
    applyFilters();
  });
}

if (sortSelect) {
  sortSelect.addEventListener("change", (e) => {
    currentSort = e.target.value;
    applyFilters();
  });
}

filterBtns.forEach(btn => {
  btn.addEventListener("click", () => {
    filterBtns.forEach(b => b.classList.remove("active"));
    btn.classList.add("active");
    currentType = btn.dataset.type;
    applyFilters();
  });
});

init();

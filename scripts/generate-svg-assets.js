// Script to generate authentic pixel-perfect SVG terminal mockups for rho
const fs = require('fs');
const path = require('path');

const FONT_FAMILY = 'ui-monospace, "SF Mono", Monaco, Menlo, Consolas, monospace';

function terminalWindow({ width = 760, height = 840, title = "rho", contentSvg }) {
  return `
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="100%" height="auto">
  <defs>
    <filter id="shadow" x="-4%" y="-4%" width="108%" height="110%" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
    <linearGradient id="headerGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1e2230" />
      <stop offset="100%" stop-color="#141824" />
    </linearGradient>
  </defs>
  
  <!-- Outer Window Frame with Shadow -->
  <g filter="url(#shadow)">
    <!-- Terminal Background -->
    <rect x="2" y="2" width="${width - 4}" height="${height - 4}" rx="10" fill="#0d1017" stroke="#252b3d" stroke-width="1.5"/>
    
    <!-- Titlebar -->
    <path d="M 2 12 Q 2 2 12 2 L ${width - 14} 2 Q ${width - 2} 2 ${width - 2} 12 L ${width - 2} 36 L 2 36 Z" fill="url(#headerGrad)"/>
    <line x1="2" y1="36" x2="${width - 2}" y2="36" stroke="#222838" stroke-width="1"/>
    
    <!-- Window Control Buttons (macOS traffic lights) -->
    <circle cx="20" cy="19" r="5" fill="#ff5f56" stroke="#e0443e" stroke-width="0.5"/>
    <circle cx="36" cy="19" r="5" fill="#ffbd2e" stroke="#dea123" stroke-width="0.5"/>
    <circle cx="52" cy="19" r="5" fill="#27c93f" stroke="#1aab29" stroke-width="0.5"/>
    
    <!-- Window Title -->
    <text x="${width / 2}" y="23" fill="#8b95a8" font-family="${FONT_FAMILY}" font-size="12" font-weight="500" text-anchor="middle" letter-spacing="0.3">${escapeXml(title)}</text>
  </g>

  <!-- Terminal Content Area -->
  <g transform="translate(24, 60)">
    ${contentSvg}
  </g>
</svg>`.trim();
}

function escapeXml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// 1. Welcome Screen
function generateWelcomeSvg() {
  const BOX_DIVIDER = "──────────────────────────────────────────────────────────────────────────────";
  
  const content = `
    <text font-family="${FONT_FAMILY}" font-size="13" xml:space="preserve">
      <tspan x="0" y="0"><tspan fill="#52e096" font-weight="bold">rho</tspan><tspan fill="#717c96"> v0.8.0</tspan></tspan>
      <tspan x="0" y="20" fill="#717c96">Type /help for commands, Tab to complete, Esc to cancel</tspan>

      <tspan x="0" y="60" fill="#717c96">[agents]</tspan>
      <tspan x="0" y="80" fill="#dce3f0">  ~/.agents/AGENTS.md, AGENTS.md</tspan>

      <tspan x="0" y="120" fill="#717c96">[skills]</tspan>
      <tspan x="0" y="140" fill="#dce3f0">  create-skill, google-agents-cli, plan, spec</tspan>

      <tspan x="0" y="180" fill="#717c96">[tools]</tspan>
      <tspan x="0" y="200" fill="#dce3f0">  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>

      <!-- Top Divider -->
      <tspan x="0" y="380" fill="#404b62">${BOX_DIVIDER}</tspan>

      <!-- Prompt Line with Cursor -->
      <tspan x="0" y="405" fill="#f2f5fa">█</tspan>

      <!-- Bottom Divider -->
      <tspan x="0" y="430" fill="#404b62">${BOX_DIVIDER}</tspan>

      <!-- Footer line 1 -->
      <tspan x="0" y="455"><tspan fill="#717c96">~/src/github.com/casonadams/rho (main)</tspan><tspan fill="#717c96">                     55% 2h57m 41% 3d22h</tspan></tspan>

      <!-- Footer line 2 -->
      <tspan x="0" y="475"><tspan fill="#52e096">0%/1M</tspan><tspan fill="#8da0bd">                                      antigravity/gemini-3.8-flash/medium</tspan></tspan>
    </text>
  `;

  return terminalWindow({
    width: 760,
    height: 560,
    title: "rho — zsh (v0.8.0)",
    contentSvg: content
  });
}

// 2. Chat / Active Turn Screen (Authentic TUI character-by-character from terminal)
function generateChatSvg() {
  const BOX_TOP    = "╭────────────────────────────────────────────────────────────────────────────╮";
  const BOX_BOT    = "╰────────────────────────────────────────────────────────────────────────────╯";
  const WORK_LINE  = "── ⠹ working ─────────────────────────────────────────────────────────────────";
  const BOT_DIV    = "──────────────────────────────────────────────────────────────────────────────";

  const content = `
    <text font-family="${FONT_FAMILY}" font-size="13" xml:space="preserve">
      <!-- Tool 1: bash -->
      <tspan x="0" y="0" fill="#404b62">${BOX_TOP}</tspan>
      <tspan x="0" y="20"><tspan fill="#404b62">│ </tspan><tspan fill="#60a5fa" font-weight="bold">bash </tspan><tspan fill="#f2f5fa">python3 -c &quot;                                                          </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="40"><tspan fill="#404b62">│ </tspan><tspan fill="#9aa7bc">import subprocess                                                          </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="60"><tspan fill="#404b62">│ </tspan><tspan fill="#9aa7bc">code = &apos;&apos;&apos;                                                                 </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="80"><tspan fill="#404b62">│ </tspan><tspan fill="#9aa7bc">fn check(r: rmcp::...                                                      </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="100"><tspan fill="#404b62">│                                                                            │</tspan></tspan>
      <tspan x="0" y="120"><tspan fill="#404b62">│                                                                            │</tspan></tspan>
      <tspan x="0" y="140"><tspan fill="#404b62">│ </tspan><tspan fill="#52e096">Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.18s        </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="160"><tspan fill="#404b62">│                                                                            │</tspan></tspan>
      <tspan x="0" y="180"><tspan fill="#404b62">│ </tspan><tspan fill="#717c96">Took 256ms                                                                 </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="200" fill="#404b62">${BOX_BOT}</tspan>

      <!-- Tool 2: edit -->
      <tspan x="0" y="222" fill="#404b62">${BOX_TOP}</tspan>
      <tspan x="0" y="242"><tspan fill="#404b62">│ </tspan><tspan fill="#c084fc" font-weight="bold">edit </tspan><tspan fill="#f2f5fa">crates/rho-engine/src/mcp/client.rs </tspan><tspan fill="#717c96">(1 edits)                         </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="262"><tspan fill="#404b62">│                                                                            │</tspan></tspan>
      <tspan x="0" y="282"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 83 │ </tspan><tspan fill="#f87171">- #[derive(Debug, Clone, Serialize, Deserialize)]                    </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="302"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 84 │ </tspan><tspan fill="#f87171">- pub struct McpToolResult {                                         </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="322"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 85 │ </tspan><tspan fill="#f87171">-     #[serde(default)]                                              </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="342"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 86 │ </tspan><tspan fill="#f87171">-     pub content: Vec&lt;McpContent&gt;,                                  </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="362"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 87 │ </tspan><tspan fill="#f87171">-     #[serde(default, rename = &quot;isError&quot;)]                          </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="382"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 88 │ </tspan><tspan fill="#f87171">-     pub is_error: Option&lt;bool&gt;,                                    </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="402"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 89 │ </tspan><tspan fill="#f87171">- }                                                                  </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="422"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 83 │ </tspan><tspan fill="#52e096">+ #[derive(Debug, Clone, Serialize, Deserialize)]                    </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="442"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 84 │ </tspan><tspan fill="#52e096">+ pub struct McpToolResult {                                         </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="462"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 85 │ </tspan><tspan fill="#52e096">+     #[serde(default)]                                              </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="482"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 86 │ </tspan><tspan fill="#52e096">+     pub content: Vec&lt;McpContent&gt;,                                  </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="502"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 87 │ </tspan><tspan fill="#52e096">+     #[serde(default, rename = &quot;isError&quot;)]                          </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="522"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 88 │ </tspan><tspan fill="#52e096">+     pub is_error: Option&lt;bool&gt;,                                    </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="542"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 89 │ </tspan><tspan fill="#52e096">+ }                                                                  </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="562"><tspan fill="#404b62">│ </tspan><tspan fill="#616e88"> 90 │ </tspan><tspan fill="#52e096">+                                                                    </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="582"><tspan fill="#404b62">│ </tspan><tspan fill="#717c96">... (71 more lines)                                                        </tspan><tspan fill="#404b62">│</tspan></tspan>
      <tspan x="0" y="602" fill="#404b62">${BOX_BOT}</tspan>

      <!-- Working Line with Spinner -->
      <tspan x="0" y="635"><tspan fill="#404b62">── </tspan><tspan fill="#38bdf8" font-weight="bold">⠹ </tspan><tspan fill="#9aa7bc">working </tspan><tspan fill="#404b62">─────────────────────────────────────────────────────────────────</tspan></tspan>

      <!-- Empty user prompt line -->
      <tspan x="0" y="655"> </tspan>

      <!-- Bottom Divider -->
      <tspan x="0" y="675" fill="#404b62">${BOT_DIV}</tspan>

      <!-- Footer Line 1 -->
      <tspan x="0" y="698"><tspan fill="#717c96">~/src/github.com/casonadams/rho (main)</tspan><tspan fill="#717c96">                     55% 2h57m 41% 3d22h</tspan></tspan>

      <!-- Footer Line 2 -->
      <tspan x="0" y="718"><tspan fill="#717c96">↑17M ↓203k R167M </tspan><tspan fill="#52e096">53%/1M </tspan><tspan fill="#717c96">@55t/s</tspan><tspan fill="#8da0bd">             antigravity/gemini-3.8-flash/medium</tspan></tspan>
    </text>
  `;

  return terminalWindow({
    width: 760,
    height: 800,
    title: "rho — active turn",
    contentSvg: content
  });
}

// 3. Model Switcher Modal
function generateModalModelSvg() {
  const BOT_DIV = "──────────────────────────────────────────────────────────────────────────────";

  const content = `
    <text font-family="${FONT_FAMILY}" font-size="13" xml:space="preserve">
      <!-- Transcript header -->
      <tspan x="0" y="0" fill="#525d74">[agents] ~/.agents/AGENTS.md, AGENTS.md</tspan>
      <tspan x="0" y="20" fill="#525d74">[skills] create-skill, google-agents-cli, plan, spec</tspan>
      <tspan x="0" y="40" fill="#525d74">[tools]  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>

      <!-- Modal Top Divider -->
      <tspan x="0" y="80"><tspan fill="#52e096">── </tspan><tspan fill="#52e096" font-weight="bold">Select Model </tspan><tspan fill="#52e096">─────────────────────────────────────────────────────────────</tspan></tspan>

      <!-- Search Prompt -->
      <tspan x="0" y="105"><tspan fill="#52e096" font-weight="bold">&gt; </tspan><tspan fill="#f2f5fa">█</tspan></tspan>

      <!-- Options -->
      <tspan x="0" y="132"><tspan fill="#52e096">▸ </tspan><tspan fill="#f2f5fa" font-weight="bold">gemini-3.8-flash</tspan><tspan fill="#717c96"> [antigravity] · default </tspan><tspan fill="#52e096" font-weight="bold">✓</tspan></tspan>
      <tspan x="0" y="152"><tspan fill="#dce3f0">  ornith-1.5:35b   [local]</tspan></tspan>
      <tspan x="0" y="172"><tspan fill="#dce3f0">  qwen3.8:27b-mlx  [local]</tspan></tspan>
      <tspan x="0" y="192"><tspan fill="#dce3f0">  minimax-m3:cloud [local]</tspan></tspan>
      <tspan x="0" y="212"><tspan fill="#dce3f0">  claude-opus-4-6  [antigravity]</tspan></tspan>
      <tspan x="0" y="232"><tspan fill="#dce3f0">  claude-sonnet-4-6 [antigravity]</tspan></tspan>
      <tspan x="0" y="252"><tspan fill="#dce3f0">  gemini-3.7-flash [antigravity]</tspan></tspan>
      <tspan x="0" y="272" fill="#525d74">  (1/36)</tspan>

      <!-- Modal Bottom Divider -->
      <tspan x="0" y="300" fill="#404b62">${BOT_DIV}</tspan>

      <!-- Bottom Keybinding Hint -->
      <tspan x="0" y="325" fill="#717c96">Enter to select • Ctrl+S to set as default • Esc to cancel</tspan>
    </text>
  `;

  return terminalWindow({
    width: 760,
    height: 420,
    title: "rho — /model selector",
    contentSvg: content
  });
}

// 4. Provider Login Modal
function generateModalLoginSvg() {
  const BOT_DIV = "──────────────────────────────────────────────────────────────────────────────";

  const content = `
    <text font-family="${FONT_FAMILY}" font-size="13" xml:space="preserve">
      <!-- Transcript header -->
      <tspan x="0" y="0" fill="#525d74">[agents] ~/.agents/AGENTS.md, AGENTS.md</tspan>
      <tspan x="0" y="20" fill="#525d74">[skills] create-skill, google-agents-cli, plan, spec</tspan>
      <tspan x="0" y="40" fill="#525d74">[tools]  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>

      <!-- Modal Top Divider -->
      <tspan x="0" y="80"><tspan fill="#52e096">── </tspan><tspan fill="#52e096" font-weight="bold">Login Provider </tspan><tspan fill="#52e096">───────────────────────────────────────────────────────────</tspan></tspan>

      <!-- Search Prompt -->
      <tspan x="0" y="105"><tspan fill="#52e096" font-weight="bold">&gt; </tspan><tspan fill="#f2f5fa">█</tspan></tspan>

      <!-- Options -->
      <tspan x="0" y="132"><tspan fill="#52e096">▸ </tspan><tspan fill="#f2f5fa" font-weight="bold">antigravity  </tspan><tspan fill="#717c96">Google Cloud Code Assist (OAuth)  </tspan><tspan fill="#52e096" font-weight="bold">✓</tspan></tspan>
      <tspan x="0" y="152"><tspan fill="#dce3f0">  chatgpt      ChatGPT Plus/Pro subscription (OAuth)</tspan></tspan>
      <tspan x="0" y="172"><tspan fill="#dce3f0">  claude       Claude Pro/Max subscription (OAuth)</tspan></tspan>
      <tspan x="0" y="192"><tspan fill="#dce3f0">  copilot      GitHub Copilot subscription (OAuth)</tspan></tspan>
      <tspan x="0" y="212"><tspan fill="#dce3f0">  openrouter   OpenRouter universal gateway (OAuth / Key)</tspan></tspan>
      <tspan x="0" y="232"><tspan fill="#dce3f0">  anthropic    Anthropic Claude models (API Key)</tspan></tspan>
      <tspan x="0" y="252"><tspan fill="#dce3f0">  openai       OpenAI GPT &amp; reasoning models (API Key)</tspan></tspan>
      <tspan x="0" y="272" fill="#525d74">  (1/14)</tspan>

      <!-- Modal Bottom Divider -->
      <tspan x="0" y="300" fill="#404b62">${BOT_DIV}</tspan>

      <!-- Bottom Keybinding Hint -->
      <tspan x="0" y="325" fill="#717c96">Enter to select • Esc to cancel</tspan>
    </text>
  `;

  return terminalWindow({
    width: 760,
    height: 420,
    title: "rho login — provider selection",
    contentSvg: content
  });
}

// 5. Fleet Hub Dashboard SVG (Browser UI mockup)
function generateHubSvg() {
  const width = 860;
  const height = 560;

  return `
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="100%" height="auto">
  <defs>
    <filter id="hubShadow" x="-4%" y="-4%" width="108%" height="110%" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
  </defs>

  <!-- Outer Window Frame with Shadow -->
  <g filter="url(#hubShadow)">
    <rect x="2" y="2" width="${width - 4}" height="${height - 4}" rx="10" fill="#0a0d13" stroke="#222b3d" stroke-width="1.5"/>
    
    <!-- Browser Titlebar -->
    <path d="M 2 12 Q 2 2 12 2 L ${width - 14} 2 Q ${width - 2} 2 ${width - 2} 12 L ${width - 2} 40 L 2 40 Z" fill="#141924"/>
    <line x1="2" y1="40" x2="${width - 2}" y2="40" stroke="#222b3d" stroke-width="1"/>
    
    <!-- Window Control Buttons -->
    <circle cx="20" cy="20" r="5" fill="#ff5f56" stroke="#e0443e" stroke-width="0.5"/>
    <circle cx="36" cy="20" r="5" fill="#ffbd2e" stroke="#dea123" stroke-width="0.5"/>
    <circle cx="52" cy="20" r="5" fill="#27c93f" stroke="#1aab29" stroke-width="0.5"/>

    <!-- URL Bar -->
    <rect x="140" y="9" width="580" height="22" rx="5" fill="#0d111a" stroke="#2b354a" stroke-width="1"/>
    <text x="430" y="24" fill="#8da0bd" font-family="${FONT_FAMILY}" font-size="11" text-anchor="middle">
      https://casonadams.github.io/rho/hub/ — rho fleet hub
    </text>
  </g>

  <!-- Hub Header Navigation -->
  <g transform="translate(28, 56)">
    <!-- Brand -->
    <text y="20" font-family="${FONT_FAMILY}" font-size="16" font-weight="bold" fill="#f2f5fa">
      <tspan fill="#52e096">ρ</tspan> rho
    </text>
    <rect x="68" y="5" width="70" height="18" rx="4" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
    <text x="103" y="18" font-family="${FONT_FAMILY}" font-size="10" font-weight="600" fill="#52e096" text-anchor="middle">fleet hub</text>

    <!-- Powered by Iroh badge -->
    <rect x="580" y="3" width="106" height="22" rx="4" fill="#161c28" stroke="#273248" stroke-width="1"/>
    <text x="633" y="17" font-family="${FONT_FAMILY}" font-size="10.5" fill="#8da0bd" text-anchor="middle">
      powered by <tspan fill="#60a5fa" font-weight="bold">iroh</tspan>
    </text>

    <!-- + Add Node Button -->
    <rect x="698" y="3" width="86" height="22" rx="4" fill="#52e096"/>
    <text x="741" y="17" font-family="${FONT_FAMILY}" font-size="11" font-weight="bold" fill="#0a0d13" text-anchor="middle">
      + Add Node
    </text>
  </g>

  <!-- Page Title & Subtitle -->
  <g transform="translate(28, 110)">
    <text y="0" font-family="${FONT_FAMILY}" font-size="20" font-weight="bold" fill="#f2f5fa">Active Fleet</text>
    <text y="20" font-family="${FONT_FAMILY}" font-size="12" fill="#8da0bd">
      Zero-cloud peer-to-peer control of your rho nodes, powered by Iroh
    </text>
  </g>

  <!-- Node Grid -->
  <g transform="translate(28, 155)">
    <!-- Card 1: Local Mac Studio (Online) -->
    <g transform="translate(0, 0)">
      <rect width="250" height="155" rx="6" fill="#111622" stroke="#253046" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">mac-studio</text>
      <rect x="170" y="16" width="64" height="18" rx="9" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
      <circle cx="180" cy="25" r="3" fill="#52e096"/>
      <text x="208" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#52e096" text-anchor="middle">Online</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/src/github.com/casonadams/rho</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 main</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#52e096">⚡ Active turn • 53%/1M</text>

      <line x1="0" y1="118" x2="250" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-8f2a9b1c</text>
      <rect x="180" y="126" width="54" height="20" rx="3" fill="#1b2232" stroke="#2c3952" stroke-width="1"/>
      <text x="207" y="139" font-family="${FONT_FAMILY}" font-size="10.5" fill="#dce3f0" text-anchor="middle">Open →</text>
    </g>

    <!-- Card 2: Linux Dev Server (Busy / Steering) -->
    <g transform="translate(268, 0)">
      <rect width="250" height="155" rx="6" fill="#111622" stroke="#f59e0b" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">cloud-runner</text>
      <rect x="170" y="16" width="64" height="18" rx="9" fill="rgba(245, 158, 11, 0.12)" stroke="#f59e0b" stroke-width="1"/>
      <circle cx="180" cy="25" r="3" fill="#f59e0b"/>
      <text x="208" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#f59e0b" text-anchor="middle">Busy</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/work/api-service</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 feature/auth-v2</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#f59e0b">⠋ Steerable turn in progress</text>

      <line x1="0" y1="118" x2="250" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-e4c1973f</text>
      <rect x="180" y="126" width="54" height="20" rx="3" fill="#f59e0b"/>
      <text x="207" y="139" font-family="${FONT_FAMILY}" font-size="10.5" font-weight="bold" fill="#0a0d13" text-anchor="middle">Steer ↗</text>
    </g>

    <!-- Card 3: Laptop (P2P Connected) -->
    <g transform="translate(536, 0)">
      <rect width="250" height="155" rx="6" fill="#111622" stroke="#253046" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">macbook-air</text>
      <rect x="170" y="16" width="64" height="18" rx="9" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
      <circle cx="180" cy="25" r="3" fill="#52e096"/>
      <text x="208" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#52e096" text-anchor="middle">Online</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/src/docs-site</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 staging</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#717c96">Idle • ready for prompts</text>

      <line x1="0" y1="118" x2="250" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-77b310aa</text>
      <rect x="180" y="126" width="54" height="20" rx="3" fill="#1b2232" stroke="#2c3952" stroke-width="1"/>
      <text x="207" y="139" font-family="${FONT_FAMILY}" font-size="10.5" fill="#dce3f0" text-anchor="middle">Open →</text>
    </g>
  </g>

  <!-- Bottom Terminal/Session Preview Banner -->
  <g transform="translate(28, 335)">
    <rect width="786" height="180" rx="6" fill="#10141f" stroke="#222b3d" stroke-width="1"/>
    
    <rect x="0" y="0" width="786" height="30" rx="6" fill="#141a27"/>
    <rect x="0" y="20" width="786" height="10" fill="#141a27"/>
    <line x1="0" y1="30" x2="786" y2="30" stroke="#222b3d" stroke-width="1"/>

    <text x="16" y="20" font-family="${FONT_FAMILY}" font-size="11.5" font-weight="bold" fill="#dce3f0">
      Live Session Stream: mac-studio
    </text>
    <text x="770" y="20" font-family="${FONT_FAMILY}" font-size="10" fill="#52e096" text-anchor="end">
      ● 12ms P2P direct iroh quic
    </text>

    <text x="16" y="55" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">
      &gt; Inspecting workspace and running verified test suite...
    </text>
    <text x="16" y="78" font-family="${FONT_FAMILY}" font-size="11.5" fill="#52e096">
      ✓ cargo test --workspace (60 tests passed)
    </text>
    <text x="16" y="101" font-family="${FONT_FAMILY}" font-size="11.5" fill="#60a5fa">
      ✓ Synchronized bidirectional approval: tool write accepted from web dashboard
    </text>

    <!-- Steering Input Bar -->
    <rect x="16" y="125" width="665" height="32" rx="4" fill="#0d111a" stroke="#26334a" stroke-width="1"/>
    <text x="28" y="145" font-family="${FONT_FAMILY}" font-size="11.5" fill="#8da0bd">
      Type steer prompt or mid-turn redirect...
    </text>
    <rect x="690" y="125" width="80" height="32" rx="4" fill="#52e096"/>
    <text x="730" y="145" font-family="${FONT_FAMILY}" font-size="12" font-weight="bold" fill="#0a0d13" text-anchor="middle">
      Send ↵
    </text>
  </g>
</svg>`.trim();
}

const assetsDir = path.join(__dirname, '../www/assets');
fs.writeFileSync(path.join(assetsDir, 'welcome.svg'), generateWelcomeSvg());
fs.writeFileSync(path.join(assetsDir, 'chat.svg'), generateChatSvg());
fs.writeFileSync(path.join(assetsDir, 'modal-model.svg'), generateModalModelSvg());
fs.writeFileSync(path.join(assetsDir, 'modal-login.svg'), generateModalLoginSvg());
fs.writeFileSync(path.join(assetsDir, 'hub.svg'), generateHubSvg());

console.log('Successfully generated authentic terminal SVGs in www/assets/');

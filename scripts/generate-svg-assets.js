// Script to generate authentic pixel-perfect SVG terminal mockups for rho
const fs = require('fs');
const path = require('path');

const FONT_FAMILY = 'ui-monospace, Menlo, Monaco, Consolas, monospace';

function terminalWindow({ width = 900, height = 650, title = "rho", contentSvg }) {
  return `
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="100%" height="auto">
  <defs>
    <filter id="shadow" x="-3%" y="-3%" width="106%" height="108%" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="10" stdDeviation="14" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
    <linearGradient id="headerGrad" x1="0%" y1="0%" x2="0%" y2="100%">
      <stop offset="0%" stop-color="#1c202d" />
      <stop offset="100%" stop-color="#131622" />
    </linearGradient>
  </defs>
  
  <!-- Outer Window Frame with Shadow -->
  <g filter="url(#shadow)">
    <!-- Terminal Background -->
    <rect x="2" y="2" width="${width - 4}" height="${height - 4}" rx="9" fill="#0d1017" stroke="#252b3d" stroke-width="1.5"/>
    
    <!-- Titlebar -->
    <path d="M 2 11 Q 2 2 11 2 L ${width - 13} 2 Q ${width - 2} 2 ${width - 2} 11 L ${width - 2} 35 L 2 35 Z" fill="url(#headerGrad)"/>
    <line x1="2" y1="35" x2="${width - 2}" y2="35" stroke="#222838" stroke-width="1"/>
    
    <!-- Window Control Buttons (macOS traffic lights) -->
    <circle cx="20" cy="18.5" r="5" fill="#ff5f56" stroke="#e0443e" stroke-width="0.5"/>
    <circle cx="36" cy="18.5" r="5" fill="#ffbd2e" stroke="#dea123" stroke-width="0.5"/>
    <circle cx="52" cy="18.5" r="5" fill="#27c93f" stroke="#1aab29" stroke-width="0.5"/>
    
    <!-- Window Title -->
    <text x="${width / 2}" y="22.5" fill="#8b95a8" font-family="${FONT_FAMILY}" font-size="12" font-weight="500" text-anchor="middle" letter-spacing="0.3">${escapeXml(title)}</text>
  </g>

  <!-- Terminal Content Area -->
  <g transform="translate(26, 52)">
    ${contentSvg}
  </g>
</svg>`.trim();
}

function escapeXml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

const W = 848; // Full inner terminal width

// 1. Welcome Screen
function generateWelcomeSvg() {
  const content = `
    <!-- Startup greeting -->
    <text font-family="${FONT_FAMILY}" font-size="13.5">
      <tspan x="0" y="0"><tspan fill="#52e096" font-weight="bold">rho</tspan><tspan fill="#717c96"> v0.8.0</tspan></tspan>
      <tspan x="0" y="20" fill="#717c96">Type /help for commands, Tab to complete, Esc to cancel</tspan>

      <tspan x="0" y="58" fill="#717c96">[agents]</tspan>
      <tspan x="0" y="78" fill="#dce3f0">  ~/.agents/AGENTS.md, AGENTS.md</tspan>

      <tspan x="0" y="116" fill="#717c96">[skills]</tspan>
      <tspan x="0" y="136" fill="#dce3f0">  create-skill, google-agents-cli, plan, spec</tspan>

      <tspan x="0" y="174" fill="#717c96">[tools]</tspan>
      <tspan x="0" y="194" fill="#dce3f0">  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>
    </text>

    <!-- Top Divider (Full Width) -->
    <line x1="0" y1="270" x2="${W}" y2="270" stroke="#384358" stroke-width="1.2"/>

    <!-- User Input: cursor with tight spacing -->
    <rect x="0" y="278" width="8.5" height="16" fill="#f2f5fa"/>

    <!-- Bottom Divider (Full Width) -->
    <line x1="0" y1="302" x2="${W}" y2="302" stroke="#384358" stroke-width="1.2"/>

    <!-- Status Line (Bottom) -->
    <text font-family="${FONT_FAMILY}" font-size="13">
      <!-- Line 1 -->
      <tspan x="0" y="322" fill="#717c96">~/src/github.com/casonadams/rho (main)</tspan>
      <tspan x="${W}" y="322" text-anchor="end" fill="#717c96">55% 2h57m 41% 3d22h</tspan>

      <!-- Line 2 -->
      <tspan x="0" y="342" fill="#52e096">0%/1M</tspan>
      <tspan x="${W}" y="342" text-anchor="end" fill="#8da0bd">antigravity/gemini-3.8-flash/medium</tspan>
    </text>
  `;

  return terminalWindow({
    width: 900,
    height: 425,
    title: "rho — zsh (v0.8.0)",
    contentSvg: content
  });
}

// 2. Chat / Active Turn Screen (Authentic TUI character-by-character from terminal)
function generateChatSvg() {
  const content = `
    <!-- Tool Box 1: bash (Full Width) -->
    <g transform="translate(0, 0)">
      <rect x="0" y="0" width="${W}" height="152" rx="6" fill="#111520" stroke="#384358" stroke-width="1.2"/>
      
      <text font-family="${FONT_FAMILY}" font-size="13">
        <tspan x="14" y="22"><tspan fill="#60a5fa" font-weight="bold">bash </tspan><tspan fill="#f2f5fa">python3 -c &quot;</tspan></tspan>
        <tspan x="14" y="42" fill="#9aa7bc">import subprocess</tspan>
        <tspan x="14" y="62" fill="#9aa7bc">code = &apos;&apos;&apos;</tspan>
        <tspan x="14" y="82" fill="#9aa7bc">fn check(r: rmcp::...</tspan>
        <tspan x="14" y="112" fill="#52e096">Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 0.18s</tspan>
        <tspan x="14" y="134" fill="#717c96">Took 256ms</tspan>
      </text>
    </g>

    <!-- Tool Box 2: edit (Full Width) -->
    <g transform="translate(0, 166)">
      <rect x="0" y="0" width="${W}" height="320" rx="6" fill="#111520" stroke="#384358" stroke-width="1.2"/>
      
      <text font-family="${FONT_FAMILY}" font-size="13">
        <tspan x="14" y="22"><tspan fill="#c084fc" font-weight="bold">edit </tspan><tspan fill="#f2f5fa">crates/rho-engine/src/mcp/client.rs </tspan><tspan fill="#717c96">(1 edits)</tspan></tspan>

        <tspan x="14" y="48"><tspan fill="#616e88"> 83 │ </tspan><tspan fill="#f87171">- #[derive(Debug, Clone, Serialize, Deserialize)]</tspan></tspan>
        <tspan x="14" y="68"><tspan fill="#616e88"> 84 │ </tspan><tspan fill="#f87171">- pub struct McpToolResult {</tspan></tspan>
        <tspan x="14" y="88"><tspan fill="#616e88"> 85 │ </tspan><tspan fill="#f87171">-     #[serde(default)]</tspan></tspan>
        <tspan x="14" y="108"><tspan fill="#616e88"> 86 │ </tspan><tspan fill="#f87171">-     pub content: Vec&lt;McpContent&gt;,</tspan></tspan>
        <tspan x="14" y="128"><tspan fill="#616e88"> 87 │ </tspan><tspan fill="#f87171">-     #[serde(default, rename = &quot;isError&quot;)]</tspan></tspan>
        <tspan x="14" y="148"><tspan fill="#616e88"> 88 │ </tspan><tspan fill="#f87171">-     pub is_error: Option&lt;bool&gt;,</tspan></tspan>
        <tspan x="14" y="168"><tspan fill="#616e88"> 89 │ </tspan><tspan fill="#f87171">- }</tspan></tspan>
        <tspan x="14" y="188"><tspan fill="#616e88"> 83 │ </tspan><tspan fill="#52e096">+ #[derive(Debug, Clone, Serialize, Deserialize)]</tspan></tspan>
        <tspan x="14" y="208"><tspan fill="#616e88"> 84 │ </tspan><tspan fill="#52e096">+ pub struct McpToolResult {</tspan></tspan>
        <tspan x="14" y="228"><tspan fill="#616e88"> 85 │ </tspan><tspan fill="#52e096">+     #[serde(default)]</tspan></tspan>
        <tspan x="14" y="248"><tspan fill="#616e88"> 86 │ </tspan><tspan fill="#52e096">+     pub content: Vec&lt;McpContent&gt;,</tspan></tspan>
        <tspan x="14" y="268"><tspan fill="#616e88"> 87 │ </tspan><tspan fill="#52e096">+     #[serde(default, rename = &quot;isError&quot;)]</tspan></tspan>
        <tspan x="14" y="288"><tspan fill="#616e88"> 88 │ </tspan><tspan fill="#52e096">+     pub is_error: Option&lt;bool&gt;,</tspan></tspan>
        <tspan x="14" y="308"><tspan fill="#616e88"> 89 │ </tspan><tspan fill="#52e096">+ }</tspan></tspan>
      </text>
    </g>

    <!-- Working Line (Full Width) -->
    <g transform="translate(0, 502)">
      <line x1="0" y1="0" x2="16" y2="0" stroke="#384358" stroke-width="1.2"/>
      <text x="22" y="4" font-family="${FONT_FAMILY}" font-size="13">
        <tspan fill="#38bdf8" font-weight="bold">⠹ </tspan>
        <tspan fill="#9aa7bc">working</tspan>
      </text>
      <line x1="92" y1="0" x2="${W}" y2="0" stroke="#384358" stroke-width="1.2"/>
    </g>

    <!-- User Input: tight gap between top and bottom line -->
    <rect x="0" y="510" width="8.5" height="16" fill="#f2f5fa"/>

    <!-- Bottom Divider (Full Width) -->
    <line x1="0" y1="534" x2="${W}" y2="534" stroke="#384358" stroke-width="1.2"/>

    <!-- Status Line (Bottom) -->
    <text font-family="${FONT_FAMILY}" font-size="13">
      <!-- Line 1 -->
      <tspan x="0" y="554" fill="#717c96">~/src/github.com/casonadams/rho (main)</tspan>
      <tspan x="${W}" y="554" text-anchor="end" fill="#717c96">55% 2h57m 41% 3d22h</tspan>

      <!-- Line 2 -->
      <tspan x="0" y="574">
        <tspan fill="#717c96">↑17M ↓203k R167M </tspan>
        <tspan fill="#52e096">53%/1M </tspan>
        <tspan fill="#717c96">@55t/s</tspan>
      </tspan>
      <tspan x="${W}" y="574" text-anchor="end" fill="#8da0bd">antigravity/gemini-3.8-flash/medium</tspan>
    </text>
  `;

  return terminalWindow({
    width: 900,
    height: 655,
    title: "rho — active turn",
    contentSvg: content
  });
}

// 3. Model Switcher Modal
function generateModalModelSvg() {
  const content = `
    <!-- Top transcript preview -->
    <text font-family="${FONT_FAMILY}" font-size="13" fill="#525d74">
      <tspan x="0" y="0">[agents] ~/.agents/AGENTS.md, AGENTS.md</tspan>
      <tspan x="0" y="18">[skills] create-skill, google-agents-cli, plan, spec</tspan>
      <tspan x="0" y="36">[tools]  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>
    </text>

    <!-- Modal Top Divider (Full Width) -->
    <g transform="translate(0, 68)">
      <line x1="0" y1="0" x2="16" y2="0" stroke="#52e096" stroke-width="1.2"/>
      <text x="22" y="4" font-family="${FONT_FAMILY}" font-size="13.5" font-weight="bold" fill="#52e096">Select Model</text>
      <line x1="126" y1="0" x2="${W}" y2="0" stroke="#52e096" stroke-width="1.2"/>
    </g>

    <!-- Search Prompt -->
    <text x="0" y="94" font-family="${FONT_FAMILY}" font-size="14" fill="#52e096" font-weight="bold">&gt;</text>
    <rect x="14" y="82" width="8" height="15" fill="#f2f5fa"/>

    <!-- Options List -->
    <text font-family="${FONT_FAMILY}" font-size="13">
      <tspan x="0" y="120"><tspan fill="#52e096">▸ </tspan><tspan fill="#f2f5fa" font-weight="bold">gemini-3.8-flash </tspan><tspan fill="#717c96">[antigravity] · default </tspan><tspan fill="#52e096" font-weight="bold">✓</tspan></tspan>
      <tspan x="0" y="140" fill="#dce3f0">  ornith-1.5:35b    [local]</tspan>
      <tspan x="0" y="160" fill="#dce3f0">  qwen3.8:27b-mlx   [local]</tspan>
      <tspan x="0" y="180" fill="#dce3f0">  minimax-m3:cloud  [local]</tspan>
      <tspan x="0" y="200" fill="#dce3f0">  claude-opus-4-6   [antigravity]</tspan>
      <tspan x="0" y="220" fill="#dce3f0">  claude-sonnet-4-6 [antigravity]</tspan>
      <tspan x="0" y="240" fill="#dce3f0">  gemini-3.7-flash  [antigravity]</tspan>
      <tspan x="0" y="260" fill="#525d74">  (1/36)</tspan>
    </text>

    <!-- Modal Bottom Divider (Full Width) -->
    <line x1="0" y1="280" x2="${W}" y2="280" stroke="#384358" stroke-width="1.2"/>

    <!-- Bottom Keybinding Hint -->
    <text x="0" y="302" font-family="${FONT_FAMILY}" font-size="12.5" fill="#717c96">
      Enter to select • Ctrl+S to set as default • Esc to cancel
    </text>
  `;

  return terminalWindow({
    width: 900,
    height: 385,
    title: "rho — /model selector",
    contentSvg: content
  });
}

// 4. Provider Login Modal
function generateModalLoginSvg() {
  const content = `
    <!-- Top transcript preview -->
    <text font-family="${FONT_FAMILY}" font-size="13" fill="#525d74">
      <tspan x="0" y="0">[agents] ~/.agents/AGENTS.md, AGENTS.md</tspan>
      <tspan x="0" y="18">[skills] create-skill, google-agents-cli, plan, spec</tspan>
      <tspan x="0" y="36">[tools]  read, write, edit, bash, fd, rg, web_search, web_fetch</tspan>
    </text>

    <!-- Modal Top Divider (Full Width) -->
    <g transform="translate(0, 68)">
      <line x1="0" y1="0" x2="16" y2="0" stroke="#52e096" stroke-width="1.2"/>
      <text x="22" y="4" font-family="${FONT_FAMILY}" font-size="13.5" font-weight="bold" fill="#52e096">Login Provider</text>
      <line x1="140" y1="0" x2="${W}" y2="0" stroke="#52e096" stroke-width="1.2"/>
    </g>

    <!-- Search Prompt -->
    <text x="0" y="94" font-family="${FONT_FAMILY}" font-size="14" fill="#52e096" font-weight="bold">&gt;</text>
    <rect x="14" y="82" width="8" height="15" fill="#f2f5fa"/>

    <!-- Options List -->
    <text font-family="${FONT_FAMILY}" font-size="13">
      <tspan x="0" y="120"><tspan fill="#52e096">▸ </tspan><tspan fill="#f2f5fa" font-weight="bold">antigravity  </tspan><tspan fill="#717c96">Google Cloud Code Assist (OAuth)  </tspan><tspan fill="#52e096" font-weight="bold">✓</tspan></tspan>
      <tspan x="0" y="140" fill="#dce3f0">  chatgpt      ChatGPT Plus/Pro subscription (OAuth)</tspan>
      <tspan x="0" y="160" fill="#dce3f0">  claude       Claude Pro/Max subscription (OAuth)</tspan>
      <tspan x="0" y="180" fill="#dce3f0">  copilot      GitHub Copilot subscription (OAuth)</tspan>
      <tspan x="0" y="200" fill="#dce3f0">  openrouter   OpenRouter universal gateway (OAuth / Key)</tspan>
      <tspan x="0" y="220" fill="#dce3f0">  anthropic    Anthropic Claude models (API Key)</tspan>
      <tspan x="0" y="240" fill="#dce3f0">  openai       OpenAI GPT &amp; reasoning models (API Key)</tspan>
      <tspan x="0" y="260" fill="#525d74">  (1/14)</tspan>
    </text>

    <!-- Modal Bottom Divider (Full Width) -->
    <line x1="0" y1="280" x2="${W}" y2="280" stroke="#384358" stroke-width="1.2"/>

    <!-- Bottom Keybinding Hint -->
    <text x="0" y="302" font-family="${FONT_FAMILY}" font-size="12.5" fill="#717c96">
      Enter to select • Esc to cancel
    </text>
  `;

  return terminalWindow({
    width: 900,
    height: 385,
    title: "rho login — provider selection",
    contentSvg: content
  });
}

// 5. Fleet Hub Dashboard SVG (Browser UI mockup)
function generateHubSvg() {
  const width = 900;
  const height = 560;
  const innerW = 848;

  return `
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" width="100%" height="auto">
  <defs>
    <filter id="hubShadow" x="-3%" y="-3%" width="106%" height="108%" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="10" stdDeviation="14" flood-color="#000000" flood-opacity="0.45"/>
    </filter>
  </defs>

  <!-- Outer Window Frame with Shadow -->
  <g filter="url(#hubShadow)">
    <rect x="2" y="2" width="${width - 4}" height="${height - 4}" rx="9" fill="#0a0d13" stroke="#222b3d" stroke-width="1.5"/>
    
    <!-- Browser Titlebar -->
    <path d="M 2 11 Q 2 2 11 2 L ${width - 13} 2 Q ${width - 2} 2 ${width - 2} 11 L ${width - 2} 38 L 2 38 Z" fill="#141924"/>
    <line x1="2" y1="38" x2="${width - 2}" y2="38" stroke="#222b3d" stroke-width="1"/>
    
    <!-- Window Control Buttons -->
    <circle cx="20" cy="20" r="5" fill="#ff5f56" stroke="#e0443e" stroke-width="0.5"/>
    <circle cx="36" cy="20" r="5" fill="#ffbd2e" stroke="#dea123" stroke-width="0.5"/>
    <circle cx="52" cy="20" r="5" fill="#27c93f" stroke="#1aab29" stroke-width="0.5"/>

    <!-- URL Bar -->
    <rect x="150" y="8" width="600" height="22" rx="5" fill="#0d111a" stroke="#2b354a" stroke-width="1"/>
    <text x="450" y="23" fill="#8da0bd" font-family="${FONT_FAMILY}" font-size="11" text-anchor="middle">
      https://casonadams.github.io/rho/hub/ — rho fleet hub
    </text>
  </g>

  <!-- Hub Header Navigation -->
  <g transform="translate(26, 54)">
    <!-- Brand -->
    <text y="20" font-family="${FONT_FAMILY}" font-size="16" font-weight="bold" fill="#f2f5fa">
      <tspan fill="#52e096">ρ</tspan> rho
    </text>
    <rect x="68" y="5" width="70" height="18" rx="4" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
    <text x="103" y="18" font-family="${FONT_FAMILY}" font-size="10" font-weight="600" fill="#52e096" text-anchor="middle">fleet hub</text>

    <!-- Powered by Iroh badge -->
    <rect x="638" y="3" width="108" height="22" rx="4" fill="#161c28" stroke="#273248" stroke-width="1"/>
    <text x="692" y="17" font-family="${FONT_FAMILY}" font-size="10.5" fill="#8da0bd" text-anchor="middle">
      powered by <tspan fill="#60a5fa" font-weight="bold">iroh</tspan>
    </text>

    <!-- + Add Node Button -->
    <rect x="758" y="3" width="90" height="22" rx="4" fill="#52e096"/>
    <text x="803" y="17" font-family="${FONT_FAMILY}" font-size="11" font-weight="bold" fill="#0a0d13" text-anchor="middle">
      + Add Node
    </text>
  </g>

  <!-- Page Title & Subtitle -->
  <g transform="translate(26, 108)">
    <text y="0" font-family="${FONT_FAMILY}" font-size="20" font-weight="bold" fill="#f2f5fa">Active Fleet</text>
    <text y="20" font-family="${FONT_FAMILY}" font-size="12" fill="#8da0bd">
      Zero-cloud peer-to-peer control of your rho nodes, powered by Iroh
    </text>
  </g>

  <!-- Node Grid -->
  <g transform="translate(26, 150)">
    <!-- Card 1: Local Mac Studio (Online) -->
    <g transform="translate(0, 0)">
      <rect width="270" height="155" rx="6" fill="#111622" stroke="#253046" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">mac-studio</text>
      <rect x="190" y="16" width="64" height="18" rx="9" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
      <circle cx="200" cy="25" r="3" fill="#52e096"/>
      <text x="228" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#52e096" text-anchor="middle">Online</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/src/github.com/casonadams/rho</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 main</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#52e096">⚡ Active turn • 53%/1M</text>

      <line x1="0" y1="118" x2="270" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-8f2a9b1c</text>
      <rect x="200" y="126" width="54" height="20" rx="3" fill="#1b2232" stroke="#2c3952" stroke-width="1"/>
      <text x="227" y="139" font-family="${FONT_FAMILY}" font-size="10.5" fill="#dce3f0" text-anchor="middle">Open →</text>
    </g>

    <!-- Card 2: Linux Dev Server (Busy / Steering) -->
    <g transform="translate(289, 0)">
      <rect width="270" height="155" rx="6" fill="#111622" stroke="#f59e0b" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">cloud-runner</text>
      <rect x="190" y="16" width="64" height="18" rx="9" fill="rgba(245, 158, 11, 0.12)" stroke="#f59e0b" stroke-width="1"/>
      <circle cx="200" cy="25" r="3" fill="#f59e0b"/>
      <text x="228" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#f59e0b" text-anchor="middle">Busy</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/work/api-service</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 feature/auth-v2</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#f59e0b">⠋ Steerable turn in progress</text>

      <line x1="0" y1="118" x2="270" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-e4c1973f</text>
      <rect x="200" y="126" width="54" height="20" rx="3" fill="#f59e0b"/>
      <text x="227" y="139" font-family="${FONT_FAMILY}" font-size="10.5" font-weight="bold" fill="#0a0d13" text-anchor="middle">Steer ↗</text>
    </g>

    <!-- Card 3: Laptop (P2P Connected) -->
    <g transform="translate(578, 0)">
      <rect width="270" height="155" rx="6" fill="#111622" stroke="#253046" stroke-width="1.2"/>
      <text x="16" y="28" font-family="${FONT_FAMILY}" font-size="14" font-weight="bold" fill="#f2f5fa">macbook-air</text>
      <rect x="190" y="16" width="64" height="18" rx="9" fill="rgba(82, 224, 150, 0.12)" stroke="#52e096" stroke-width="1"/>
      <circle cx="200" cy="25" r="3" fill="#52e096"/>
      <text x="228" y="28" font-family="${FONT_FAMILY}" font-size="10" font-weight="bold" fill="#52e096" text-anchor="middle">Online</text>

      <text x="16" y="60" font-family="${FONT_FAMILY}" font-size="11.5" fill="#dce3f0">📁 ~/src/docs-site</text>
      <text x="16" y="80" font-family="${FONT_FAMILY}" font-size="11.5" fill="#717c96">🌿 staging</text>
      <text x="16" y="100" font-family="${FONT_FAMILY}" font-size="11" fill="#717c96">Idle • ready for prompts</text>

      <line x1="0" y1="118" x2="270" y2="118" stroke="#1f283a" stroke-width="1"/>
      <text x="16" y="138" font-family="${FONT_FAMILY}" font-size="10.5" fill="#717c96">node-77b310aa</text>
      <rect x="200" y="126" width="54" height="20" rx="3" fill="#1b2232" stroke="#2c3952" stroke-width="1"/>
      <text x="227" y="139" font-family="${FONT_FAMILY}" font-size="10.5" fill="#dce3f0" text-anchor="middle">Open →</text>
    </g>
  </g>

  <!-- Bottom Terminal/Session Preview Banner (Full Width) -->
  <g transform="translate(26, 335)">
    <rect width="${innerW}" height="180" rx="6" fill="#10141f" stroke="#222b3d" stroke-width="1"/>
    
    <rect x="0" y="0" width="${innerW}" height="30" rx="6" fill="#141a27"/>
    <rect x="0" y="20" width="${innerW}" height="10" fill="#141a27"/>
    <line x1="0" y1="30" x2="${innerW}" y2="30" stroke="#222b3d" stroke-width="1"/>

    <text x="16" y="20" font-family="${FONT_FAMILY}" font-size="11.5" font-weight="bold" fill="#dce3f0">
      Live Session Stream: mac-studio
    </text>
    <text x="${innerW - 16}" y="20" font-family="${FONT_FAMILY}" font-size="10" fill="#52e096" text-anchor="end">
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
    <rect x="16" y="125" width="${innerW - 120}" height="32" rx="4" fill="#0d111a" stroke="#26334a" stroke-width="1"/>
    <text x="28" y="145" font-family="${FONT_FAMILY}" font-size="11.5" fill="#8da0bd">
      Type steer prompt or mid-turn redirect...
    </text>
    <rect x="${innerW - 96}" y="125" width="80" height="32" rx="4" fill="#52e096"/>
    <text x="${innerW - 56}" y="145" font-family="${FONT_FAMILY}" font-size="12" font-weight="bold" fill="#0a0d13" text-anchor="middle">
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

console.log('Successfully generated full-width authentic SVGs in www/assets/');

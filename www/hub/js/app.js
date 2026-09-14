import { NodeRegistry } from './registry.js';
import { RhoPeerClient, ensureWasm } from './client.js';
import { SessionView } from './session.js';
import { AuthModal } from './auth.js';

let activeClient = null;
let sessionView = null;

// DOM Elements
const fleetView = document.getElementById('fleet-view');
const workspaceView = document.getElementById('workspace-view');
const nodeGrid = document.getElementById('node-grid');
const addNodeBtn = document.getElementById('add-node-btn');
const backToFleetBtn = document.getElementById('back-to-fleet-btn');
const newSessionBtn = document.getElementById('new-session-btn');
const authBtn = document.getElementById('auth-btn');
const chatTranscript = document.getElementById('chat-transcript');
const chatPrompt = document.getElementById('chat-prompt');
const sendPromptBtn = document.getElementById('send-prompt-btn');

export function parseTicketInput(input) {
  let str = (input || '').trim().replace(/^["'<]|["'>]$/g, '');
  if (!str) return { ticket: '', sessionId: null };

  let sessionId = null;

  const sessionMatch = str.match(/[#?&]session=([^&]+)/);
  if (sessionMatch) {
    sessionId = decodeURIComponent(sessionMatch[1]);
  }

  const ticketMatch = str.match(/(?:^|[#?&])ticket=([^&]+)/);
  if (ticketMatch) {
    str = decodeURIComponent(ticketMatch[1]);
  } else if (str.startsWith('http://') || str.startsWith('https://')) {
    return { ticket: '', sessionId };
  }

  if (str.includes('&')) {
    const parts = str.split('&');
    str = parts[0];
    for (let i = 1; i < parts.length; i++) {
      if (parts[i].startsWith('session=') && !sessionId) {
        sessionId = decodeURIComponent(parts[i].slice('session='.length));
      }
    }
  }

  str = str.replace(/[#/?]+$/, '').trim();
  return { ticket: str, sessionId };
}

export async function initApp() {
  await ensureWasm();

  // Check URL query or hash for ticket pairing: #ticket=rho_...&session=... or ?ticket=rho_...
  let targetNodeToOpen = null;
  let targetSessionId = null;

  const urlPairing = parseTicketInput(window.location.href);
  if (urlPairing.ticket) {
    targetSessionId = urlPairing.sessionId;
    try {
      const client = new RhoPeerClient(urlPairing.ticket);
      await client.init();
      const nodeRecord = {
        id: client.endpointId,
        label: `Node ${client.endpointId.slice(0, 8)}`,
        ticket: urlPairing.ticket
      };
      NodeRegistry.saveNode(nodeRecord);
      targetNodeToOpen = nodeRecord;

      // Security: Strip ticket from URL
      window.history.replaceState(null, '', window.location.pathname);
    } catch (e) {
      console.error('Failed to initialize node from URL ticket:', e);
    }
  }

  addNodeBtn.onclick = () => showAddNodeModal();
  backToFleetBtn.onclick = () => showFleetView();
  newSessionBtn.onclick = () => handleNewSession();
  authBtn.onclick = () => handleAuthClick();
  sendPromptBtn.onclick = () => handleSendPrompt();

  const toggleSidebarBtn = document.getElementById('toggle-sidebar-btn');
  const sidebar = document.getElementById('session-sidebar');
  if (toggleSidebarBtn && sidebar) {
    toggleSidebarBtn.onclick = () => {
      sidebar.classList.toggle('collapsed');
      localStorage.setItem('rho_sidebar_collapsed', sidebar.classList.contains('collapsed'));
    };
    if (localStorage.getItem('rho_sidebar_collapsed') === 'true') {
      sidebar.classList.add('collapsed');
    }
  }

  const autoResizeInput = () => {
    chatPrompt.style.height = 'auto';
    const newHeight = Math.min(Math.max(chatPrompt.scrollHeight, 38), 180);
    chatPrompt.style.height = `${newHeight}px`;
    chatPrompt.style.overflowY = chatPrompt.scrollHeight > 180 ? 'auto' : 'hidden';
  };

  chatPrompt.addEventListener('input', autoResizeInput);

  chatPrompt.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSendPrompt();
    }
  });

  if (targetNodeToOpen) {
    openWorkspace(targetNodeToOpen, targetSessionId);
  } else {
    renderFleet();
  }
}

function showFleetView() {
  fleetView.style.display = 'block';
  workspaceView.style.display = 'none';
  renderFleet();
}

async function renderFleet() {
  const nodes = NodeRegistry.getNodes();
  nodeGrid.innerHTML = '';

  if (nodes.length === 0) {
    nodeGrid.innerHTML = `
      <div style="grid-column: 1 / -1; text-align: center; padding: 3rem; color: var(--text-muted);">
        <p style="margin-bottom: 1rem;">No nodes registered in your fleet yet.</p>
        <p style="font-size: 0.85rem;">Run <code>rho serve</code> on any machine or click "+ Add Node" to pair.</p>
      </div>
    `;
    return;
  }

  for (const node of nodes) {
    const card = document.createElement('div');
    card.className = 'node-card';
    card.innerHTML = `
      <div class="node-card-header">
        <span class="node-name">${escapeHtml(node.label || node.id)}</span>
        <span class="status-badge ${node.info?.status === 'busy' ? 'online' : 'online'}">
          <span class="status-dot"></span> Online
        </span>
      </div>
      <div class="node-details">
        <div class="node-workspace">📁 ${escapeHtml(node.info?.active_workspace || 'Workspace configured')}</div>
        <div class="node-branch">🌿 ${escapeHtml(node.info?.active_branch || 'main')}</div>
      </div>
      <div class="node-card-footer">
        <span>${node.info?.hostname || node.id.slice(0, 12)}</span>
        <button class="btn-secondary" style="padding: 2px 8px; font-size: 0.75rem;" data-remove="${node.id}">Remove</button>
      </div>
    `;

    card.onclick = (e) => {
      if (e.target.dataset.remove) {
        e.stopPropagation();
        NodeRegistry.removeNode(node.id);
        renderFleet();
        return;
      }
      openWorkspace(node);
    };

    nodeGrid.appendChild(card);
  }
}

async function openWorkspace(node, preferredSessionId = null) {
  fleetView.style.display = 'none';
  workspaceView.style.display = 'flex';

  document.getElementById('active-node-title').textContent = node.label || node.id.slice(0, 12);
  document.getElementById('active-node-workspace').textContent = node.info?.active_workspace || '~/workspace';

  activeClient = new RhoPeerClient(node.ticket);
  await activeClient.init();
  await activeClient.connect();

  sessionView = new SessionView(chatTranscript, activeClient);
  sessionView.clear();

  activeClient.onEvent((ev) => {
    if (ev.type === 'turn_start') {
      if (ev.prompt) {
        sessionView.addUserMessage(ev.prompt);
      }
      sessionView.startAssistantTurn();
    } else if (ev.type === 'text_chunk') {
      sessionView.appendTextChunk(ev.content);
    } else if (ev.type === 'reasoning_chunk') {
      sessionView.appendReasoningChunk(ev.content);
    } else if (ev.type === 'tool_call_start') {
      sessionView.appendToolCall(ev.tool, ev.arguments, ev.call_id);
    } else if (ev.type === 'tool_call_result') {
      sessionView.appendToolResult(ev.tool, ev.output, ev.is_error, ev.duration_ms, ev.call_id);
    } else if (ev.type === 'tool_approval_request') {
      sessionView.showApprovalRequest(ev);
    } else if (ev.type === 'tool_approval_resolved') {
      sessionView.dismissApprovalRequest(ev.approval_id);
    } else if (ev.type === 'status_changed') {
      sessionView.setWorking(ev.status === 'busy' || ev.status === 'waiting_approval');
    } else if (ev.type === 'turn_end') {
      sessionView.finishTurn();
    } else if (ev.type === 'usage_update') {
      updateFooterState(ev);
    }
  });

  // Query live node info
  const infoResp = await activeClient.send('get_node_info');
  if (infoResp.data) {
    NodeRegistry.updateNodeInfo(node.id, infoResp.data);
    document.getElementById('active-node-workspace').textContent = infoResp.data.active_workspace || '~/workspace';
  }

  // Load current session state and messages
  const stateResp = await activeClient.send('get_state');
  let activeSid = stateResp.data?.session_id;
  if (stateResp.data) {
    updateFooterState(stateResp.data);
  }

  if (preferredSessionId && preferredSessionId !== activeSid) {
    const resumeResp = await activeClient.send('resume_session', { session_id: preferredSessionId });
    if (resumeResp.data && resumeResp.data.messages) {
      sessionView.clear();
      renderMessages(resumeResp.data.messages);
      activeSid = resumeResp.data.session_id;
      sessionView.scrollToBottom();
      updateFooterState(resumeResp.data);
    }
  } else if (stateResp.data && stateResp.data.messages) {
    sessionView.clear();
    renderMessages(stateResp.data.messages);
    sessionView.scrollToBottom();
  }

  // Load session list
  const sessionsResp = await activeClient.send('list_sessions');
  renderSessionList(sessionsResp.data || [], activeSid);
}

function renderSessionList(sessions, activeSessionId) {
  const listEl = document.getElementById('session-history-list');
  listEl.innerHTML = '';
  for (const s of sessions) {
    const li = document.createElement('li');
    li.className = 'session-item' + (s.session_id === activeSessionId ? ' active' : '');
    const title = s.name || (s.preview && s.preview !== 'Empty session' ? (s.preview.length > 30 ? s.preview.slice(0, 30) + '...' : s.preview) : s.session_id);
    const timeStr = s.last_modified ? new Date(s.last_modified).toLocaleTimeString() : '';
    li.innerHTML = `
      <div style="font-weight: 600; color: var(--text-primary); word-break: break-word;">${escapeHtml(title)}</div>
      <div style="font-size: 0.7rem; color: var(--text-muted);">${escapeHtml(timeStr)}</div>
    `;
    li.onclick = async () => {
      document.querySelectorAll('.session-item').forEach((el) => el.classList.remove('active'));
      li.classList.add('active');
      sessionView.clear();
      const resp = await activeClient.send('resume_session', { session_id: s.session_id });
      if (resp && resp.data) {
        updateFooterState(resp.data);
        if (resp.data.messages) {
          renderMessages(resp.data.messages);
        }
      }
      sessionView.scrollToBottom();
    };
    listEl.appendChild(li);
  }
}

function renderMessages(messages) {
  if (!messages) return;
  for (const m of messages) {
    if (m.role === 'user') {
      sessionView.addUserMessage(m.content);
    } else if (m.role === 'assistant') {
      sessionView.addAssistantMessage(m.content);
    } else if (m.role === 'tool') {
      sessionView.appendToolCall(m.tool, m.arguments);
      sessionView.appendToolResult(m.tool, m.output, m.is_error, m.duration_ms);
    }
  }
}

async function handleNewSession() {
  if (!activeClient) return;
  const resp = await activeClient.send('create_session');
  sessionView.clear();
  if (resp && resp.data) {
    updateFooterState(resp.data);
  }
  const sessionsResp = await activeClient.send('list_sessions');
  renderSessionList(sessionsResp.data || [], resp.data?.session_id);
}

function handleAuthClick() {
  if (!activeClient) return;
  const modal = new AuthModal(activeClient);
  modal.show();
}

async function handleSendPrompt() {
  const text = chatPrompt.value.trim();
  if (!text || !activeClient) return;

  chatPrompt.value = '';
  chatPrompt.style.height = '38px';
  chatPrompt.style.overflowY = 'hidden';
  if (sessionView && sessionView.isWorking) {
    sessionView.addSteeringMessage(text);
    await activeClient.send('steer', { message: text });
  } else {
    sessionView.addUserMessage(text);
    sessionView.startAssistantTurn();
    await activeClient.send('prompt', { message: text });
  }
}

function showAddNodeModal() {
  const overlay = document.createElement('div');
  overlay.className = 'modal-overlay';
  overlay.innerHTML = `
    <div class="modal-card">
      <h2>Pair Remote Node</h2>
      <p style="font-size: 0.85rem; color: var(--text-secondary);">Enter the pairing URL or node ticket generated by <code>rho serve</code> or <code>/remote</code>.</p>
      <input type="text" id="node-ticket-input" placeholder="rho_... or https://...#ticket=rho_..." />
      <input type="text" id="node-label-input" placeholder="Friendly Label (e.g. Work MacBook, Cloud Devbox)" style="margin-top: 0.5rem;" />
      <div class="modal-footer">
        <button class="btn-secondary" id="modal-cancel-btn">Cancel</button>
        <button class="btn-primary" id="modal-pair-btn">Pair Node</button>
      </div>
    </div>
  `;

  document.body.appendChild(overlay);

  overlay.querySelector('#modal-cancel-btn').onclick = () => overlay.remove();
  overlay.querySelector('#modal-pair-btn').onclick = async () => {
    const rawInput = overlay.querySelector('#node-ticket-input').value.trim();
    const label = overlay.querySelector('#node-label-input').value.trim();
    if (!rawInput) return;

    const { ticket, sessionId } = parseTicketInput(rawInput);
    if (!ticket) {
      alert('Please enter a valid node ticket or pairing URL.');
      return;
    }

    try {
      const client = new RhoPeerClient(ticket);
      await client.init();
      const nodeRecord = {
        id: client.endpointId,
        label: label || `Node ${client.endpointId.slice(0, 8)}`,
        ticket
      };
      NodeRegistry.saveNode(nodeRecord);
      overlay.remove();
      if (sessionId) {
        openWorkspace(nodeRecord, sessionId);
      } else {
        renderFleet();
      }
    } catch (e) {
      alert(`Invalid node ticket: ${e}`);
    }
  };
}

const currentFooterState = {
  active_workspace: '~/workspace',
  active_branch: null,
  session_id: null,
  session_name: null,
  quota: null,
  model: null,
  thinking_level: null,
  total_input_tokens: 0,
  total_output_tokens: 0,
  total_cache_read_tokens: 0,
  total_cache_write_tokens: 0,
  total_cost: null,
  context_percent: null,
  context_window: 0,
  tokens_per_second: null,
};

function formatTokens(count) {
  if (!count) return '0';
  if (count >= 1000000) {
    return (count / 1000000).toFixed(1) + 'M';
  } else if (count >= 1000) {
    return (count / 1000).toFixed(1) + 'k';
  }
  return String(count);
}

function updateFooterState(data) {
  if (!data) return;
  if (data.active_workspace !== undefined && data.active_workspace !== null) currentFooterState.active_workspace = data.active_workspace;
  if (data.workspace !== undefined && data.workspace !== null) currentFooterState.active_workspace = data.workspace;
  if (data.active_branch !== undefined) currentFooterState.active_branch = data.active_branch;
  if (data.session_id !== undefined) currentFooterState.session_id = data.session_id;
  if (data.session_name !== undefined) currentFooterState.session_name = data.session_name;
  if (data.model !== undefined && data.model !== null) currentFooterState.model = data.model;
  if (data.thinking_level !== undefined && data.thinking_level !== null) currentFooterState.thinking_level = data.thinking_level;
  if (data.quota !== undefined) currentFooterState.quota = data.quota;

  if (data.total_input_tokens !== undefined && data.total_input_tokens !== null) currentFooterState.total_input_tokens = data.total_input_tokens;
  else if (data.input_tokens !== undefined && data.input_tokens !== null) currentFooterState.total_input_tokens = data.input_tokens;

  if (data.total_output_tokens !== undefined && data.total_output_tokens !== null) currentFooterState.total_output_tokens = data.total_output_tokens;
  else if (data.output_tokens !== undefined && data.output_tokens !== null) currentFooterState.total_output_tokens = data.output_tokens;

  if (data.total_cache_read_tokens !== undefined && data.total_cache_read_tokens !== null) currentFooterState.total_cache_read_tokens = data.total_cache_read_tokens;
  else if (data.cache_read_tokens !== undefined && data.cache_read_tokens !== null) currentFooterState.total_cache_read_tokens = data.cache_read_tokens;

  if (data.total_cache_write_tokens !== undefined && data.total_cache_write_tokens !== null) currentFooterState.total_cache_write_tokens = data.total_cache_write_tokens;
  else if (data.cache_write_tokens !== undefined && data.cache_write_tokens !== null) currentFooterState.total_cache_write_tokens = data.cache_write_tokens;

  if (data.total_cost !== undefined) currentFooterState.total_cost = data.total_cost;
  if (data.context_percent !== undefined) currentFooterState.context_percent = data.context_percent;
  if (data.context_window !== undefined && data.context_window !== null) currentFooterState.context_window = data.context_window;
  if (data.tokens_per_second !== undefined) currentFooterState.tokens_per_second = data.tokens_per_second;

  renderFooter();
}

function renderFooter() {
  const cwdEl = document.getElementById('footer-cwd-info');
  const quotaEl = document.getElementById('footer-quota-info');
  const statsEl = document.getElementById('footer-stats-info');
  const modelEl = document.getElementById('footer-model-info');
  if (!cwdEl || !quotaEl || !statsEl || !modelEl) return;

  // Top line: left = cwd (branch)
  let topText = currentFooterState.active_workspace || '~/workspace';
  if (currentFooterState.active_branch) {
    topText += ` (${currentFooterState.active_branch})`;
  }
  cwdEl.textContent = topText;

  // Top line: right = quota
  if (currentFooterState.quota) {
    quotaEl.innerHTML = `<span class="footer-quota-badge">${escapeHtml(currentFooterState.quota)}</span>`;
  } else {
    quotaEl.innerHTML = '';
  }

  // Stats line: left = tokens, context, speed
  const parts = [];
  const inTokens = currentFooterState.total_input_tokens || 0;
  const outTokens = currentFooterState.total_output_tokens || 0;
  parts.push(`↑${formatTokens(inTokens)}`);
  parts.push(`↓${formatTokens(outTokens)}`);

  if (currentFooterState.total_cache_read_tokens > 0) {
    parts.push(`R${formatTokens(currentFooterState.total_cache_read_tokens)}`);
  }
  if (currentFooterState.total_cache_write_tokens > 0) {
    parts.push(`W${formatTokens(currentFooterState.total_cache_write_tokens)}`);
  }
  if (currentFooterState.total_cost && currentFooterState.total_cost > 0) {
    parts.push(`$${Number(currentFooterState.total_cost).toFixed(3)}`);
  }

  if (currentFooterState.context_percent != null) {
    const pct = Number(currentFooterState.context_percent).toFixed(1);
    if (currentFooterState.context_window > 0) {
      parts.push(`${pct}%/${formatTokens(currentFooterState.context_window)}`);
    } else {
      parts.push(`${pct}%`);
    }
  } else if (currentFooterState.context_window > 0) {
    parts.push(`0%/${formatTokens(currentFooterState.context_window)}`);
  }

  if (currentFooterState.tokens_per_second && currentFooterState.tokens_per_second > 0) {
    parts.push(`@${Math.round(currentFooterState.tokens_per_second)}t/s`);
  }
  statsEl.textContent = parts.join(' ');

  // Stats line: right = model • thinking
  let modelStr = currentFooterState.model || '';
  if (currentFooterState.thinking_level && currentFooterState.thinking_level !== 'off') {
    modelStr = modelStr ? `${modelStr} • ${currentFooterState.thinking_level}` : currentFooterState.thinking_level;
  }
  modelEl.textContent = modelStr;
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

document.addEventListener('DOMContentLoaded', initApp);

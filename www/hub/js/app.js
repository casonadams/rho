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

export async function initApp() {
  await ensureWasm();

  // Check URL hash for ticket pairing: #ticket=rho_...
  const hash = window.location.hash;
  if (hash.startsWith('#ticket=')) {
    const ticket = hash.replace('#ticket=', '').trim();
    if (ticket) {
      const client = new RhoPeerClient(ticket);
      await client.init();
      NodeRegistry.saveNode({
        id: client.endpointId,
        label: `Node ${client.endpointId.slice(0, 8)}`,
        ticket
      });
      // Security: Strip ticket from URL
      window.history.replaceState(null, '', window.location.pathname);
    }
  }

  addNodeBtn.onclick = () => showAddNodeModal();
  backToFleetBtn.onclick = () => showFleetView();
  newSessionBtn.onclick = () => handleNewSession();
  authBtn.onclick = () => handleAuthClick();
  sendPromptBtn.onclick = () => handleSendPrompt();

  chatPrompt.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSendPrompt();
    }
  });

  renderFleet();
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

async function openWorkspace(node) {
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
      sessionView.startAssistantTurn();
    } else if (ev.type === 'text_chunk') {
      sessionView.appendTextChunk(ev.content);
    } else if (ev.type === 'reasoning_chunk') {
      sessionView.appendReasoningChunk(ev.content);
    } else if (ev.type === 'tool_approval_request') {
      sessionView.showApprovalRequest(ev);
    }
  });

  // Query live node info
  const infoResp = await activeClient.send('get_node_info');
  if (infoResp.data) {
    NodeRegistry.updateNodeInfo(node.id, infoResp.data);
    document.getElementById('active-node-workspace').textContent = infoResp.data.active_workspace || '~/workspace';
  }

  // Load session list
  const sessionsResp = await activeClient.send('list_sessions');
  renderSessionList(sessionsResp.data || []);
}

function renderSessionList(sessions) {
  const listEl = document.getElementById('session-history-list');
  listEl.innerHTML = '';
  for (const s of sessions) {
    const li = document.createElement('li');
    li.className = 'session-item';
    const title = s.name || (s.preview ? (s.preview.length > 30 ? s.preview.slice(0, 30) + '...' : s.preview) : s.session_id);
    const timeStr = s.last_modified ? new Date(s.last_modified).toLocaleTimeString() : '';
    li.innerHTML = `
      <div style="font-weight: 600; color: var(--text-primary); word-break: break-word;">${escapeHtml(title)}</div>
      <div style="font-size: 0.7rem; color: var(--text-muted);">${escapeHtml(timeStr)}</div>
    `;
    li.onclick = () => {
      document.querySelectorAll('.session-item').forEach((el) => el.classList.remove('active'));
      li.classList.add('active');
      activeClient.send('resume_session', { session_id: s.session_id });
      sessionView.clear();
    };
    listEl.appendChild(li);
  }
}

async function handleNewSession() {
  if (!activeClient) return;
  const resp = await activeClient.send('create_session');
  sessionView.clear();
  if (resp && resp.data && resp.data.session_id) {
    sessionView.addUserMessage(`[New Session Created: ${resp.data.session_id}]`);
  }
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
  sessionView.addUserMessage(text);
  sessionView.startAssistantTurn();
  await activeClient.send('prompt', { message: text });
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
    let raw = overlay.querySelector('#node-ticket-input').value.trim();
    const label = overlay.querySelector('#node-label-input').value.trim();
    if (!raw) return;

    if (raw.includes('#ticket=')) {
      raw = raw.split('#ticket=')[1];
    }

    try {
      const client = new RhoPeerClient(raw);
      await client.init();
      NodeRegistry.saveNode({
        id: client.endpointId,
        label: label || `Node ${client.endpointId.slice(0, 8)}`,
        ticket: raw
      });
      overlay.remove();
      renderFleet();
    } catch (e) {
      alert(`Invalid node ticket: ${e}`);
    }
  };
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

document.addEventListener('DOMContentLoaded', initApp);

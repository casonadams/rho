function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function renderMarkdown(text) {
  if (!text) return '';
  let html = text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  // Code blocks: ```lang ... ```
  html = html.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (_, lang, code) => {
    return `<div class="code-block"><div class="code-lang">${lang || 'code'}</div><pre><code>${code.trim()}</code></pre></div>`;
  });

  // Inline code: `code`
  html = html.replace(/`([^`]+)`/g, '<code class="inline-code">$1</code>');

  // Headers
  html = html.replace(/^### (.*$)/gim, '<h3 class="md-h3">$1</h3>');
  html = html.replace(/^## (.*$)/gim, '<h2 class="md-h2">$1</h2>');
  html = html.replace(/^# (.*$)/gim, '<h1 class="md-h1">$1</h1>');

  // Bold & italic
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\*([^*]+)\*/g, '<em>$1</em>');

  // Links: [text](url)
  html = html.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');

  // Paragraph breaks
  html = html.replace(/\n\n/g, '</p><p>');
  html = html.replace(/\n/g, '<br/>');

  return `<p>${html}</p>`;
}

function formatToolSummary(tool, args) {
  if (!args) return '';
  if (typeof args === 'string') return args;
  switch (tool) {
    case 'read': {
      const path = args.path || '';
      if (args.offset !== undefined && args.limit !== undefined) {
        return `${path}:${args.offset}-${args.offset + args.limit}`;
      }
      return path;
    }
    case 'write':
    case 'edit':
      return args.path || '';
    case 'bash':
      return args.command || '';
    case 'web_search':
      return args.query ? `"${args.query}"` : '';
    case 'web_fetch':
      return args.url || '';
    case 'grep':
    case 'rg':
    case 'fd':
      return args.pattern ? (args.path ? `${args.pattern} in ${args.path}` : args.pattern) : (args.path || '');
    case 'mcp':
      return args.tool ? `${args.server || ''}/${args.tool}` : (args.action || '');
    default: {
      if (args.command) return args.command;
      if (args.path) return args.path;
      if (args.query) return `"${args.query}"`;
      if (args.url) return args.url;
      try {
        const str = JSON.stringify(args);
        return str.length > 80 ? str.slice(0, 77) + '...' : str;
      } catch (_) {
        return '';
      }
    }
  }
}

function formatDuration(ms) {
  if (!ms || ms <= 0) return '';
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

export class SessionView {
  constructor(container, client) {
    this.container = container;
    this.client = client;
    this.activeAssistantBubble = null;
    this.activeThinkingBlock = null;
    this.currentText = '';
    this.currentThinking = '';
    this.lastUserPrompt = null;
  }

  clear() {
    this.container.innerHTML = '';
    this.activeAssistantBubble = null;
    this.activeThinkingBlock = null;
    this.currentText = '';
    this.currentThinking = '';
    this.lastUserPrompt = null;
    this.setWorking(false);
  }

  setWorking(isWorking, label) {
    this.isWorking = isWorking;
    const pill = document.getElementById('working-pill');
    if (pill) {
      pill.style.display = isWorking ? 'inline-flex' : 'none';
    }
    const actDivider = document.getElementById('chat-divider-act');
    const actLabel = document.getElementById('chat-divider-label');
    if (actDivider) {
      actDivider.style.display = isWorking ? 'inline-flex' : 'none';
    }
    if (actLabel && label) {
      actLabel.textContent = label;
    }

    const sendBtn = document.getElementById('send-prompt-btn');
    const promptInput = document.getElementById('chat-prompt');
    if (sendBtn) {
      if (isWorking) {
        sendBtn.textContent = 'Steer ↗';
        sendBtn.className = 'btn-steer';
      } else {
        sendBtn.textContent = 'Send';
        sendBtn.className = 'btn-primary';
      }
    }
    if (promptInput) {
      promptInput.placeholder = isWorking
        ? 'Send steering instruction to redirect agent... (Enter to steer)'
        : 'Send prompt to remote node... (Enter to send, Shift+Enter for newline)';
    }

    this.scrollToBottom();
  }

  addSteeringMessage(text) {
    const bubble = document.createElement('div');
    bubble.className = 'chat-bubble user steering';
    bubble.innerHTML = `<span class="steering-badge">Steer ↗</span> ${escapeHtml(text)}`;
    this.container.appendChild(bubble);
    this.scrollToBottom();
  }

  addUserMessage(text) {
    if (this.lastUserPrompt === text) return;
    this.lastUserPrompt = text;
    const bubble = document.createElement('div');
    bubble.className = 'chat-bubble user';
    bubble.textContent = text;
    this.container.appendChild(bubble);
    this.scrollToBottom();
  }

  addAssistantMessage(text) {
    const bubble = document.createElement('div');
    bubble.className = 'chat-bubble assistant';
    const proseSpan = document.createElement('div');
    proseSpan.className = 'prose-content';
    proseSpan.innerHTML = renderMarkdown(text);
    bubble.appendChild(proseSpan);
    this.container.appendChild(bubble);
    this.scrollToBottom();
  }

  startAssistantTurn() {
    this.currentText = '';
    this.currentThinking = '';

    const bubble = document.createElement('div');
    bubble.className = 'chat-bubble assistant';

    this.activeThinkingBlock = document.createElement('div');
    this.activeThinkingBlock.className = 'thinking-block';
    this.activeThinkingBlock.style.display = 'none';

    const header = document.createElement('div');
    header.className = 'thinking-header';
    header.innerHTML = '<span>🧠 Thinking</span>';
    const body = document.createElement('div');
    body.className = 'thinking-content';
    this.activeThinkingBlock.appendChild(header);
    this.activeThinkingBlock.appendChild(body);

    header.onclick = () => {
      body.style.display = body.style.display === 'none' ? 'block' : 'none';
    };

    const proseSpan = document.createElement('div');
    proseSpan.className = 'prose-content';

    bubble.appendChild(this.activeThinkingBlock);
    bubble.appendChild(proseSpan);

    this.activeAssistantBubble = bubble;
    this.container.appendChild(bubble);
    this.setWorking(true);
    this.scrollToBottom();
  }

  finishTurn() {
    this.setWorking(false);
    this.activeAssistantBubble = null;
    this.currentText = '';
    this.currentThinking = '';
    this.lastUserPrompt = null;
  }

  appendToolCall(tool, args, callId) {
    if (this.activeAssistantBubble) {
      if (!this.currentText && !this.currentThinking) {
        this.activeAssistantBubble.remove();
      }
      this.activeAssistantBubble = null;
      this.currentText = '';
      this.currentThinking = '';
    }

    const card = document.createElement('div');
    card.className = 'tool-activity-card running';
    const cid = callId || `tool-${Date.now()}-${Math.random().toString(36).substr(2, 5)}`;
    card.dataset.callId = cid;
    card.dataset.toolName = tool;

    const summary = formatToolSummary(tool, args);

    card.innerHTML = `
      <div class="tool-activity-header">
        <div class="tool-header-left">
          <span class="tool-header-name">${escapeHtml(tool)}</span>
          <span class="tool-header-args">${escapeHtml(summary)}</span>
        </div>
        <div class="tool-header-right">
          <span class="tool-status-spinner spinner-ring"></span>
          <span class="tool-status-label"></span>
          <span class="tool-chevron" style="display: none;">▾</span>
        </div>
      </div>
      <div class="tool-activity-body" style="display: none;"></div>
    `;

    const header = card.querySelector('.tool-activity-header');
    const body = card.querySelector('.tool-activity-body');
    const chevron = card.querySelector('.tool-chevron');
    header.onclick = () => {
      if (body.textContent.trim()) {
        const isCollapsed = body.style.display === 'none';
        body.style.display = isCollapsed ? 'block' : 'none';
        card.classList.toggle('expanded', isCollapsed);
        if (chevron) {
          chevron.textContent = isCollapsed ? '▾' : '▸';
        }
      }
    };

    this.container.appendChild(card);
    this.scrollToBottom();
    return card;
  }

  appendToolResult(tool, output, isError, durationMs, callId) {
    let card = null;
    if (callId) {
      card = this.container.querySelector(`.tool-activity-card.running[data-call-id="${callId}"]`);
    }
    if (!card) {
      const runningWithName = this.container.querySelectorAll(`.tool-activity-card.running[data-tool-name="${tool}"]`);
      if (runningWithName.length > 0) {
        card = runningWithName[runningWithName.length - 1];
      }
    }
    if (!card) {
      const runningCards = this.container.querySelectorAll('.tool-activity-card.running');
      if (runningCards.length > 0) {
        card = runningCards[runningCards.length - 1];
      } else {
        const cards = this.container.querySelectorAll('.tool-activity-card');
        if (cards.length > 0) {
          card = cards[cards.length - 1];
        }
      }
    }

    if (!card) {
      card = this.appendToolCall(tool, null, callId);
    }

    card.classList.remove('running');
    if (isError) {
      card.classList.add('is-error');
    }

    const spinner = card.querySelector('.tool-status-spinner');
    if (spinner) spinner.style.display = 'none';

    const statusLabel = card.querySelector('.tool-status-label');
    if (statusLabel) {
      const parts = [];
      if (durationMs) {
        parts.push(`Took ${formatDuration(durationMs)}`);
      }
      if (isError) {
        parts.push('failed');
      }
      statusLabel.textContent = parts.join(' • ');
    }

    if (output && output.trim()) {
      const body = card.querySelector('.tool-activity-body');
      const chevron = card.querySelector('.tool-chevron');
      if (body) {
        body.textContent = output.trim();
        const toolName = card.dataset.toolName || tool;
        if (isError || toolName === 'bash' || toolName === 'edit' || toolName === 'write') {
          body.style.display = 'block';
          card.classList.add('expanded');
          if (chevron) {
            chevron.style.display = 'inline';
            chevron.textContent = '▾';
          }
        } else if (chevron) {
          chevron.style.display = 'inline';
          chevron.textContent = '▸';
        }
      }
    }

    this.scrollToBottom();
  }

  appendReasoningChunk(chunk) {
    if (!this.activeAssistantBubble) {
      this.startAssistantTurn();
    }
    this.currentThinking += chunk;
    if (this.activeThinkingBlock) {
      this.activeThinkingBlock.style.display = 'block';
      const body = this.activeThinkingBlock.querySelector('.thinking-content');
      if (body) {
        body.textContent = this.currentThinking;
      }
    }
    this.scrollToBottom();
  }

  appendTextChunk(chunk) {
    if (!this.activeAssistantBubble) {
      this.startAssistantTurn();
    }
    this.currentText += chunk;
    const prose = this.activeAssistantBubble.querySelector('.prose-content');
    if (prose) {
      prose.innerHTML = renderMarkdown(this.currentText);
    }
    this.scrollToBottom();
  }

  showApprovalRequest(approval) {
    const card = document.createElement('div');
    card.className = 'approval-card';

    const args = approval.arguments || {};
    const bodyText = args.body || '';

    let toolName = approval.tool || 'Tool';
    let commandText = '';

    if (typeof bodyText === 'string') {
      const match = bodyText.match(/Tool:\s*([^\n]+)(?:\nInput:\s*([\s\S]+))?/);
      if (match) {
        toolName = match[1].trim();
        commandText = (match[2] || '').trim();
      } else {
        commandText = bodyText;
      }
    }

    const headerHtml = `<div class="approval-header">⚠️ Permission Required: <code>${escapeHtml(toolName)}</code></div>`;
    let commandHtml = '';
    if (commandText) {
      commandHtml = `
        <div class="approval-command-box">
          <div class="approval-command-label">Command / Input:</div>
          <pre class="approval-command"><code>${escapeHtml(commandText)}</code></pre>
        </div>
      `;
    }

    const options = Array.isArray(args.options) ? args.options : [
      { label: 'Allow', description: 'Run this tool call once' },
      { label: 'Deny', description: 'Deny tool execution' }
    ];

    let optionsHtml = '<div class="approval-options-list">';
    options.forEach((opt, idx) => {
      const isDeny = opt.label.toLowerCase().includes('deny');
      const isAllow = opt.label.toLowerCase().includes('allow');
      const btnClass = isDeny ? 'btn-deny' : (isAllow ? 'btn-allow' : '');
      optionsHtml += `
        <button class="approval-opt-btn ${btnClass}" data-index="${idx}" data-label="${escapeHtml(opt.label.toLowerCase())}">
          <div class="opt-label">${escapeHtml(opt.label)}</div>
          ${opt.description ? `<div class="opt-desc">${escapeHtml(opt.description)}</div>` : ''}
        </button>
      `;
    });
    optionsHtml += '</div>';

    card.innerHTML = headerHtml + commandHtml + optionsHtml;

    card.querySelectorAll('.approval-opt-btn').forEach((btn) => {
      btn.onclick = () => {
        const decision = btn.dataset.label || btn.dataset.index;
        this.client.send('tool_response', {
          approval_id: approval.approval_id,
          decision: decision
        });
        card.remove();
        this.setWorking(true);
      };
    });

    this.container.appendChild(card);
    this.scrollToBottom();
  }

  scrollToBottom() {
    requestAnimationFrame(() => {
      this.container.scrollTop = this.container.scrollHeight;
      const last = this.container.lastElementChild;
      if (last) {
        last.scrollIntoView({ block: 'end', behavior: 'smooth' });
      }
    });
  }
}

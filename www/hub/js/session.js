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
    const working = document.createElement('div');
    working.className = 'working-indicator-card';
    working.id = 'transcript-working';
    working.style.display = 'none';
    working.innerHTML = '<span class="spinner-ring"></span> rho is working...';
    this.container.appendChild(working);

    this.activeAssistantBubble = null;
    this.activeThinkingBlock = null;
    this.currentText = '';
    this.currentThinking = '';
    this.lastUserPrompt = null;
    this.setWorking(false);
  }

  setWorking(isWorking) {
    const pill = document.getElementById('working-pill');
    if (pill) {
      pill.style.display = isWorking ? 'inline-flex' : 'none';
    }
    const indicator = document.getElementById('transcript-working');
    if (indicator) {
      indicator.style.display = isWorking ? 'inline-flex' : 'none';
      if (isWorking) {
        this.container.appendChild(indicator);
      }
    }
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
    this.lastUserPrompt = null;
  }

  appendToolCall(tool, args) {
    const card = document.createElement('div');
    card.className = 'tool-activity-card';
    let argsStr = '';
    if (typeof args === 'string') {
      argsStr = args;
    } else if (args && args.command) {
      argsStr = args.command;
    } else if (args && args.path) {
      argsStr = args.path;
    } else if (args) {
      argsStr = JSON.stringify(args);
    }
    card.innerHTML = `<span class="tool-tag">🛠️ ${escapeHtml(tool)}</span> <code>${escapeHtml(argsStr)}</code>`;
    this.container.appendChild(card);
    this.scrollToBottom();
  }

  appendToolResult(tool, output, isError) {
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

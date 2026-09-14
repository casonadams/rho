export class SessionView {
  constructor(container, client) {
    this.container = container;
    this.client = client;
    this.activeAssistantBubble = null;
    this.activeThinkingBlock = null;
    this.currentText = '';
    this.currentThinking = '';
  }

  clear() {
    this.container.innerHTML = '';
    this.activeAssistantBubble = null;
    this.activeThinkingBlock = null;
    this.currentText = '';
    this.currentThinking = '';
  }

  addUserMessage(text) {
    const bubble = document.createElement('div');
    bubble.className = 'chat-bubble user';
    bubble.textContent = text;
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

    const textSpan = document.createElement('div');
    textSpan.className = 'prose-content';

    bubble.appendChild(this.activeThinkingBlock);
    bubble.appendChild(textSpan);

    this.activeAssistantBubble = bubble;
    this.container.appendChild(bubble);
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
      prose.textContent = this.currentText;
    }
    this.scrollToBottom();
  }

  showApprovalRequest(approval) {
    const card = document.createElement('div');
    card.className = 'approval-card';
    card.innerHTML = `
      <div class="approval-header">⚠️ Tool Approval Required: <code>${approval.tool}</code></div>
      <div class="approval-body">
        <pre><code>${JSON.stringify(approval.arguments, null, 2)}</code></pre>
      </div>
      <div class="approval-actions">
        <button class="btn-primary btn-approve">Approve</button>
        <button class="btn-secondary btn-deny">Deny</button>
      </div>
    `;

    const approveBtn = card.querySelector('.btn-approve');
    const denyBtn = card.querySelector('.btn-deny');

    approveBtn.onclick = () => {
      this.client.send('tool_response', { approval_id: approval.approval_id, decision: 'allow' });
      card.remove();
    };

    denyBtn.onclick = () => {
      this.client.send('tool_response', { approval_id: approval.approval_id, decision: 'deny' });
      card.remove();
    };

    this.container.appendChild(card);
    this.scrollToBottom();
  }

  scrollToBottom() {
    this.container.scrollTop = this.container.scrollHeight;
  }
}

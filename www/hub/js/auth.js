export class AuthModal {
  constructor(client) {
    this.client = client;
    this.overlay = null;
  }

  show() {
    this.overlay = document.createElement('div');
    this.overlay.className = 'modal-overlay';
    this.overlay.innerHTML = `
      <div class="modal-card">
        <h2>Authenticate Provider</h2>
        <p style="font-size: 0.85rem; color: var(--text-secondary);">Select a provider to authenticate on this remote node.</p>
        <select id="auth-provider-select">
          <option value="antigravity">Google Antigravity (OAuth)</option>
          <option value="claude">Anthropic Claude (OAuth)</option>
          <option value="chatgpt">OpenAI ChatGPT (OAuth PKCE)</option>
          <option value="copilot">GitHub Copilot (Device Flow)</option>
          <option value="openrouter">OpenRouter (OAuth)</option>
          <option value="anthropic_key">Anthropic (Manual API Key)</option>
          <option value="openai_key">OpenAI (Manual API Key)</option>
        </select>
        
        <div id="auth-dynamic-content" style="display: flex; flex-direction: column; gap: 0.75rem;"></div>

        <div class="modal-footer">
          <button class="btn-secondary" id="auth-cancel-btn">Cancel</button>
          <button class="btn-primary" id="auth-start-btn">Start Login</button>
        </div>
      </div>
    `;

    document.body.appendChild(this.overlay);

    const cancelBtn = this.overlay.querySelector('#auth-cancel-btn');
    const startBtn = this.overlay.querySelector('#auth-start-btn');
    const select = this.overlay.querySelector('#auth-provider-select');
    const dynamic = this.overlay.querySelector('#auth-dynamic-content');

    cancelBtn.onclick = () => this.close();

    select.onchange = () => {
      const val = select.value;
      if (val.endsWith('_key')) {
        dynamic.innerHTML = `
          <input type="password" id="auth-manual-key" placeholder="Enter API key (e.g. sk-ant-...)" />
        `;
        startBtn.textContent = 'Save API Key';
      } else {
        dynamic.innerHTML = '';
        startBtn.textContent = 'Start Login';
      }
    };

    startBtn.onclick = async () => {
      const val = select.value;
      if (val.endsWith('_key')) {
        const input = dynamic.querySelector('#auth-manual-key');
        const key = input?.value?.trim();
        if (!key) return;
        const provider = val.replace('_key', '');
        await this.client.send('set_api_key', { provider, api_key: key });
        alert(`API key saved for ${provider}`);
        this.close();
      } else {
        dynamic.innerHTML = '<p style="font-size: 0.85rem; color: var(--accent-blue);">Initiating OAuth flow on node...</p>';
        this.client.send('auth_login', { provider: val });
      }
    };

    this.client.onEvent((ev) => {
      if (ev.type === 'auth_request') {
        this.handleAuthRequest(ev, dynamic);
      } else if (ev.type === 'auth_complete') {
        if (ev.success) {
          dynamic.innerHTML = `<p style="color: var(--accent-green);">Authentication successful for ${ev.provider}!</p>`;
          setTimeout(() => this.close(), 1500);
        } else {
          dynamic.innerHTML = `<p style="color: var(--accent-red);">Auth failed: ${ev.error || 'unknown error'}</p>`;
        }
      }
    });
  }

  handleAuthRequest(req, container) {
    container.innerHTML = '';
    if (req.auth_url) {
      const linkBtn = document.createElement('a');
      linkBtn.href = req.auth_url;
      linkBtn.target = '_blank';
      linkBtn.className = 'btn-primary';
      linkBtn.style.textDecoration = 'none';
      linkBtn.style.textAlign = 'center';
      linkBtn.textContent = 'Open Authorization Window ↗';
      container.appendChild(linkBtn);
    }
    if (req.user_code) {
      const codeP = document.createElement('p');
      codeP.innerHTML = `Enter code: <strong style="color: var(--accent-green);">${req.user_code}</strong>`;
      container.appendChild(codeP);
    }
    if (req.prompt) {
      const promptP = document.createElement('p');
      promptP.textContent = req.prompt;
      const input = document.createElement('input');
      input.type = req.is_secret ? 'password' : 'text';
      input.placeholder = 'Paste authorization code or token here';
      const submitBtn = document.createElement('button');
      submitBtn.className = 'btn-primary';
      submitBtn.textContent = 'Submit Code';
      submitBtn.onclick = () => {
        this.client.send('auth_input', {
          interaction_id: req.interaction_id,
          secret_value: input.value.trim()
        });
        container.innerHTML = '<p style="color: var(--accent-blue);">Verifying credential...</p>';
      };
      container.appendChild(promptP);
      container.appendChild(input);
      container.appendChild(submitBtn);
    }
  }

  close() {
    if (this.overlay) {
      this.overlay.remove();
      this.overlay = null;
    }
  }
}

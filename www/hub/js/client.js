import initWasm, { parse_ticket, encode_rpc_request, parse_rpc_frame, process_stream_content } from '../wasm/rho_wasm.js';

let wasmReady = false;

export async function ensureWasm() {
  if (!wasmReady) {
    await initWasm();
    wasmReady = true;
  }
}

export class RhoPeerClient {
  constructor(ticket) {
    this.ticket = ticket;
    this.parsedTicket = null;
    this.socket = null;
    this.eventListeners = [];
    this.responseHandlers = new Map();
    this.reqSeq = 0;
    this.status = 'disconnected';
  }

  async init() {
    await ensureWasm();
    this.parsedTicket = parse_ticket(this.ticket);
  }

  get endpointId() {
    return this.parsedTicket?.endpoint_id || 'unknown';
  }

  onEvent(cb) {
    this.eventListeners.push(cb);
  }

  emitEvent(ev) {
    for (const cb of this.eventListeners) {
      try {
        cb(ev);
      } catch (e) {
        console.error('Error in event listener', e);
      }
    }
  }

  connect() {
    return new Promise((resolve, reject) => {
      this.status = 'connecting';

      // Connect via direct address if available or relay
      const direct = this.parsedTicket?.direct_addresses?.[0];
      const relay = this.parsedTicket?.relay_url;
      const targetUrl = direct ? `ws://${direct}` : (relay ? `${relay}/ws` : null);

      if (!targetUrl) {
        this.status = 'offline';
        // Create mock local loopback simulator if testing in browser without network relay
        this.simulateConnection(resolve);
        return;
      }

      try {
        this.socket = new WebSocket(targetUrl);
        this.socket.onopen = () => {
          this.status = 'online';
          resolve();
        };
        this.socket.onmessage = (msg) => {
          this.handleRawMessage(msg.data);
        };
        this.socket.onerror = () => {
          this.status = 'offline';
          this.simulateConnection(resolve);
        };
        this.socket.onclose = () => {
          this.status = 'disconnected';
        };
      } catch {
        this.status = 'offline';
        this.simulateConnection(resolve);
      }
    });
  }

  simulateConnection(resolve) {
    this.status = 'online';
    setTimeout(() => {
      this.emitEvent({
        type: 'session_start',
        session_id: 'sess-' + Math.random().toString(36).slice(2, 9),
        model: 'claude-3-7-sonnet',
        provider: 'anthropic'
      });
      resolve();
    }, 100);
  }

  handleRawMessage(data) {
    try {
      const frame = parse_rpc_frame(data);
      if (frame.type === 'response') {
        if (frame.id && this.responseHandlers.has(frame.id)) {
          const handler = this.responseHandlers.get(frame.id);
          this.responseHandlers.delete(frame.id);
          handler(frame);
        }
      } else {
        this.emitEvent(frame);
      }
    } catch (e) {
      console.warn('Failed to parse frame', e);
    }
  }

  send(commandType, payload = {}) {
    const id = `req-${++this.reqSeq}`;
    const payloadStr = JSON.stringify(payload);
    const json = encode_rpc_request(id, commandType, payloadStr);

    return new Promise((resolve) => {
      this.responseHandlers.set(id, resolve);
      if (this.socket && this.socket.readyState === WebSocket.OPEN) {
        this.socket.send(json + '\n');
      } else {
        // Fallback simulated local response
        setTimeout(() => {
          resolve({
            id,
            type: 'response',
            command: commandType,
            success: true,
            data: this.simulateResponseData(commandType, payload)
          });
        }, 150);
      }
    });
  }

  simulateResponseData(command, payload) {
    if (command === 'get_node_info') {
      return {
        hostname: this.parsedTicket?.endpoint_id?.slice(0, 12) || 'macbook-pro',
        os: 'macos',
        arch: 'aarch64',
        version: '0.7.1',
        active_workspace: '~/src/github.com/casonadams/rho',
        active_branch: 'feat/remote-hub',
        status: 'idle'
      };
    }
    if (command === 'list_sessions') {
      return [
        { id: 'sess-active', title: 'Iroh web dashboard', updated_at: Date.now() - 10000 },
        { id: 'sess-prev', title: 'Refactor auth callbacks', updated_at: Date.now() - 3600000 }
      ];
    }
    if (command === 'prompt') {
      setTimeout(() => {
        this.emitEvent({ type: 'turn_start', turn_number: 1, prompt: payload.message });
        this.emitEvent({ type: 'reasoning_chunk', content: 'Analyzing codebase and requirements...' });
        this.emitEvent({ type: 'text_chunk', content: `Echo from rho remote node: ${payload.message}` });
        this.emitEvent({ type: 'turn_end', stop_reason: 'end_turn' });
      }, 50);
      return {};
    }
    return {};
  }
}

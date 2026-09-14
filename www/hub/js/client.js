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

      const wsPort = this.parsedTicket?.ws_port || 50051;
      let host = '127.0.0.1';
      if (this.parsedTicket?.direct_addresses?.length) {
        const first = this.parsedTicket.direct_addresses[0];
        host = first.split(':')[0] || '127.0.0.1';
      }
      if (window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1') {
        host = '127.0.0.1';
      }
      const targetUrl = `ws://${host}:${wsPort}`;

      try {
        this.socket = new WebSocket(targetUrl);
        this.socket.onopen = () => {
          this.status = 'online';
          resolve();
        };
        this.socket.onmessage = async (msg) => {
          const raw = msg.data instanceof Blob ? await msg.data.text() : msg.data;
          this.handleRawMessage(raw);
        };
        this.socket.onerror = (err) => {
          console.warn('WebSocket connection failed to', targetUrl, err);
          this.status = 'offline';
          reject(new Error(`Failed to connect to ${targetUrl}`));
        };
        this.socket.onclose = () => {
          this.status = 'disconnected';
        };
      } catch (e) {
        this.status = 'offline';
        reject(e);
      }
    });
  }

  handleRawMessage(data) {
    try {
      const frame = typeof data === 'string' ? parse_rpc_frame(data) : data;
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

    return new Promise((resolve, reject) => {
      this.responseHandlers.set(id, resolve);
      if (this.socket && this.socket.readyState === WebSocket.OPEN) {
        this.socket.send(json + '\n');
      } else {
        reject(new Error(`WebSocket is not connected (state: ${this.socket?.readyState})`));
      }
    });
  }
}

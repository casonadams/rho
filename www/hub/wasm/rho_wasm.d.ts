/* tslint:disable */
/* eslint-disable */
/**
 * The `ReadableStreamType` enum.
 *
 * *This API requires the following crate features to be activated: `ReadableStreamType`*
 */

type ReadableStreamType = "bytes";

export class IntoUnderlyingByteSource {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  pull(controller: ReadableByteStreamController): Promise<any>;
  start(controller: ReadableByteStreamController): void;
  cancel(): void;
  readonly autoAllocateChunkSize: number;
  readonly type: ReadableStreamType;
}

export class IntoUnderlyingSink {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  abort(reason: any): Promise<any>;
  close(): Promise<any>;
  write(chunk: any): Promise<any>;
}

export class IntoUnderlyingSource {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  pull(controller: ReadableStreamDefaultController): Promise<any>;
  cancel(): void;
}

export class IrohPeer {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  send(msg: string): void;
  close(): void;
  static connect(ticket_str: string, on_message: Function, on_close: Function): Promise<any>;
}

export function encode_rpc_request(id: string | null | undefined, command_type: string, payload_json?: string | null): string;

export function parse_rpc_frame(line: string): any;

export function parse_ticket(ticket_str: string): any;

export function process_stream_content(full_buffer: string): any;

export function start(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_irohpeer_free: (a: number, b: number) => void;
  readonly encode_rpc_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
  readonly irohpeer_close: (a: number) => void;
  readonly irohpeer_connect: (a: number, b: number, c: number, d: number) => number;
  readonly irohpeer_send: (a: number, b: number, c: number, d: number) => void;
  readonly parse_rpc_frame: (a: number, b: number, c: number) => void;
  readonly parse_ticket: (a: number, b: number, c: number) => void;
  readonly process_stream_content: (a: number, b: number, c: number) => void;
  readonly start: () => void;
  readonly __wbg_intounderlyingsink_free: (a: number, b: number) => void;
  readonly intounderlyingsink_abort: (a: number, b: number) => number;
  readonly intounderlyingsink_close: (a: number) => number;
  readonly intounderlyingsink_write: (a: number, b: number) => number;
  readonly __wbg_intounderlyingbytesource_free: (a: number, b: number) => void;
  readonly intounderlyingbytesource_autoAllocateChunkSize: (a: number) => number;
  readonly intounderlyingbytesource_cancel: (a: number) => void;
  readonly intounderlyingbytesource_pull: (a: number, b: number) => number;
  readonly intounderlyingbytesource_start: (a: number, b: number) => void;
  readonly intounderlyingbytesource_type: (a: number) => number;
  readonly __wbg_intounderlyingsource_free: (a: number, b: number) => void;
  readonly intounderlyingsource_cancel: (a: number) => void;
  readonly intounderlyingsource_pull: (a: number, b: number) => number;
  readonly ring_core_0_17_14__bn_mul_mont: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
  readonly __wasm_bindgen_func_elem_3708: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_2885: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_5416: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_5404: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_11438: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_11428: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_11484: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_11467: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_4523: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_4491: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_6100: (a: number, b: number, c: number) => void;
  readonly __wasm_bindgen_func_elem_6103: (a: number, b: number) => void;
  readonly __wasm_bindgen_func_elem_12715: (a: number, b: number, c: number, d: number) => void;
  readonly __wbindgen_export: (a: number, b: number) => number;
  readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_export3: (a: number) => void;
  readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

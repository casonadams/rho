/* tslint:disable */
/* eslint-disable */
/**
 * The `ReadableStreamType` enum.
 *
 * *This API requires the following crate features to be activated: `ReadableStreamType`*
 */

export type ReadableStreamType = "bytes";

export class IntoUnderlyingByteSource {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    pull(controller: ReadableByteStreamController): Promise<any>;
    start(controller: ReadableByteStreamController): void;
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
    cancel(): void;
    pull(controller: ReadableStreamDefaultController): Promise<any>;
}

export class IrohPeer {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    close(): void;
    static connect(ticket_str: string, on_message: Function, on_close: Function): Promise<any>;
    send(msg: string): void;
}

export function encode_rpc_request(id: string | null | undefined, command_type: string, payload_json?: string | null): string;

export function parse_rpc_frame(line: string): any;

export function parse_ticket(ticket_str: string): any;

export function process_stream_content(full_buffer: string): any;

export function start(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_intounderlyingbytesource_free: (a: number, b: number) => void;
    readonly __wbg_intounderlyingsink_free: (a: number, b: number) => void;
    readonly __wbg_intounderlyingsource_free: (a: number, b: number) => void;
    readonly __wbg_irohpeer_free: (a: number, b: number) => void;
    readonly encode_rpc_request: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly intounderlyingbytesource_autoAllocateChunkSize: (a: number) => number;
    readonly intounderlyingbytesource_cancel: (a: number) => void;
    readonly intounderlyingbytesource_pull: (a: number, b: number) => number;
    readonly intounderlyingbytesource_start: (a: number, b: number) => void;
    readonly intounderlyingbytesource_type: (a: number) => number;
    readonly intounderlyingsink_abort: (a: number, b: number) => number;
    readonly intounderlyingsink_close: (a: number) => number;
    readonly intounderlyingsink_write: (a: number, b: number) => number;
    readonly intounderlyingsource_cancel: (a: number) => void;
    readonly intounderlyingsource_pull: (a: number, b: number) => number;
    readonly irohpeer_close: (a: number) => void;
    readonly irohpeer_connect: (a: number, b: number, c: number, d: number) => number;
    readonly irohpeer_send: (a: number, b: number, c: number, d: number) => void;
    readonly parse_rpc_frame: (a: number, b: number, c: number) => void;
    readonly parse_ticket: (a: number, b: number, c: number) => void;
    readonly process_stream_content: (a: number, b: number, c: number) => void;
    readonly start: () => void;
    readonly ring_core_0_17_14__bn_mul_mont: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly __wasm_bindgen_func_elem_6792: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_6807: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1179: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_2518: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_3376: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_2432: (a: number, b: number) => void;
    readonly __wasm_bindgen_func_elem_2884: (a: number, b: number) => void;
    readonly __wasm_bindgen_func_elem_2900: (a: number, b: number) => void;
    readonly __wasm_bindgen_func_elem_6324: (a: number, b: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export5: (a: number, b: number) => void;
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

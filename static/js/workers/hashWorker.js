/**
 * OxiCloud — BLAKE3 hashing worker (instant-upload support).
 *
 * Hashes a File off the main thread, reading it in fixed-size slices so
 * RAM stays constant regardless of file size. The WASM module is compiled
 * from the exact same `blake3` crate the server uses, so the digest
 * computed here equals the server's content address bit for bit.
 *
 * Protocol: receives `{ file: File }`, answers
 * `{ ok: true, hash: string }` or `{ ok: false, error: string }`.
 * The spawner terminates the worker after one file.
 */

// Absolute URL on purpose: vendors are served verbatim at /js/vendors/ in
// both dev and release mode (the release IIFE bundle would break a
// relative import) — same pattern as the pdf.js loader in thumbnail.js.
const WASM_GLUE_URL = '/js/vendors/hash-wasm/oxicloud_hash_wasm.js';

/**
 * 8 MiB slices — large enough to amortize the per-slice Blob→ArrayBuffer
 * round-trip, small enough that peak worker RAM stays flat for any size.
 */
const SLICE_BYTES = 8 * 1024 * 1024;

/**
 * Typed view of the dedicated-worker global scope. The project's
 * jsconfig targets the DOM lib, where `self` is a Window — cast to the
 * two members this worker actually uses.
 * @type {{ onmessage: ((event: MessageEvent) => void) | null,
 *          postMessage: (message: unknown) => void }}
 */
const workerScope = /** @type {any} */ (self);

/**
 * Memoized WASM module (in-flight or settled), `default()` already run.
 * Reset on failure so a later message can retry a transient load error.
 * @type {Promise<any> | null}
 */
let _wasmPromise = null;

/** @returns {Promise<any>} */
function getWasm() {
    if (!_wasmPromise) {
        _wasmPromise = import(WASM_GLUE_URL)
            .then(async (mod) => {
                await mod.default();
                return mod;
            })
            .catch((err) => {
                _wasmPromise = null;
                throw err;
            });
    }
    return _wasmPromise;
}

workerScope.onmessage = async (event) => {
    const file = /** @type {{ file: File }} */ (event.data).file;
    try {
        const wasm = await getWasm();
        const hasher = new wasm.Blake3Hasher();
        try {
            for (let offset = 0; offset < file.size; offset += SLICE_BYTES) {
                const end = Math.min(offset + SLICE_BYTES, file.size);
                // eslint-disable-next-line no-await-in-loop -- sequential by design: constant RAM
                const buffer = await file.slice(offset, end).arrayBuffer();
                hasher.update(new Uint8Array(buffer));
            }
            workerScope.postMessage({ ok: true, hash: hasher.finalizeHex() });
        } finally {
            hasher.free();
        }
    } catch (err) {
        workerScope.postMessage({
            ok: false,
            error: err instanceof Error ? err.message : String(err)
        });
    }
};

/**
 * OxiCloud - Instant upload (zero-byte dedup upload)
 *
 * Before transferring a file's bytes, compute its BLAKE3 locally (in a
 * worker, off the main thread) and ask the server whether the caller
 * already owns that exact content (`GET /api/dedup/check/{hash}` — the
 * check is user-scoped, never a global content oracle). On a hit, a
 * single metadata call (`POST /api/files/by-hash`) registers the file
 * with ZERO content bytes on the wire.
 *
 * Performance posture:
 * - Hashing runs in a dedicated worker with WASM SIMD128 — the UI thread
 *   never blocks, RAM stays constant (8 MiB slices).
 * - Files below {@link INSTANT_UPLOAD_MIN_SIZE} skip the whole dance:
 *   two extra round-trips cost more than just uploading them.
 * - Any failure (no WASM support, worker error, server miss, races)
 *   falls back silently to the normal byte upload — instant upload is
 *   an optimization, never a gate.
 */

import { getCsrfHeaders } from '../../core/csrf.js';

/**
 * Files smaller than this upload normally: hashing + two round-trips
 * outweigh the transfer. 8 MiB matches the chunked-upload threshold's
 * order of magnitude.
 */
export const INSTANT_UPLOAD_MIN_SIZE = 8 * 1024 * 1024;

// Absolute URL on purpose — works in dev and in the release IIFE bundle
// (same pattern as the pdf.js loader in thumbnail.js).
const HASH_WORKER_URL = '/js/workers/hashWorker.js';

/** Hashing budget: 60 s base + 30 s per GB (WASM SIMD does ~0.5-1 GB/s). */
const HASH_TIMEOUT_BASE_MS = 60000;
const HASH_TIMEOUT_PER_GB_MS = 30000;

/**
 * `false` once the environment proved unable to run the worker/WASM
 * (old browser, blocked worker) — later files skip straight to the byte
 * upload instead of failing the same way again. `null` = not yet known.
 * @type {boolean | null}
 */
let _instantUploadUsable = null;

/**
 * Hash a file in a one-shot worker. Resolves `null` on any failure —
 * the caller falls back to a normal upload.
 * @param {File} file
 * @returns {Promise<string | null>}
 */
function hashFileInWorker(file) {
    return new Promise((resolve) => {
        /** @type {Worker} */
        let worker;
        try {
            worker = new Worker(HASH_WORKER_URL, { type: 'module' });
        } catch (_) {
            _instantUploadUsable = false;
            resolve(null);
            return;
        }

        const sizeGB = file.size / (1024 * 1024 * 1024);
        const timeoutMs = HASH_TIMEOUT_BASE_MS + Math.ceil(sizeGB) * HASH_TIMEOUT_PER_GB_MS;

        /** @param {string | null} hash */
        const settle = (hash) => {
            clearTimeout(timer);
            worker.terminate();
            resolve(hash);
        };
        const timer = setTimeout(() => settle(null), timeoutMs);

        worker.onmessage = (event) => {
            const data = /** @type {{ ok: boolean, hash?: string, error?: string }} */ (event.data);
            if (!data.ok) {
                // The worker ran but WASM failed (e.g. no SIMD128 support):
                // a permanent environment property, don't retry per file.
                _instantUploadUsable = false;
            }
            settle(data.ok && data.hash ? data.hash : null);
        };
        worker.onerror = () => {
            // Worker script failed to load/parse — permanent.
            _instantUploadUsable = false;
            settle(null);
        };

        worker.postMessage({ file });
    });
}

/**
 * Ask the server whether the caller already owns content with this hash.
 * @param {string} hash
 * @returns {Promise<boolean>}
 */
async function callerOwnsHash(hash) {
    try {
        const response = await fetch(`/api/dedup/check/${hash}`, {
            headers: { 'Cache-Control': 'no-cache, no-store, must-revalidate' }
        });
        if (!response.ok) return false;
        const body = /** @type {import('../../core/types.js').HashCheckAnswer} */ (await response.json());
        return body.exists === true;
    } catch (_) {
        return false;
    }
}

/**
 * Try to register `file` as a zero-byte instant upload.
 *
 * Returns `null` whenever the byte upload should proceed (file too
 * small, environment unusable, hash miss, lost race, transient errors).
 * Returns an upload-result object compatible with the uploaders'
 * `UploadAnswer` shape when the attempt is conclusive — success, quota
 * exceeded, or name conflict (a byte upload would fail identically).
 *
 * @param {File} file
 * @param {string | null | undefined} folderId
 * @returns {Promise<{ ok: boolean, data?: any, errorMsg?: string, isQuotaError?: boolean } | null>}
 */
export async function tryInstantUpload(file, folderId) {
    if (!folderId || file.size < INSTANT_UPLOAD_MIN_SIZE || _instantUploadUsable === false || typeof Worker === 'undefined') {
        return null;
    }

    const hash = await hashFileInWorker(file);
    if (!hash) return null;

    if (!(await callerOwnsHash(hash))) return null;

    try {
        const response = await fetch('/api/files/by-hash', {
            method: 'POST',
            headers: {
                ...getCsrfHeaders(),
                'Content-Type': 'application/json',
                'Cache-Control': 'no-cache, no-store, must-revalidate'
            },
            body: JSON.stringify(
                /** @type {import('../../core/types.js').CreateFileByHash} */ ({
                    name: file.name,
                    folder_id: folderId,
                    hash
                })
            )
        });

        if (response.status === 201) {
            return { ok: true, data: await response.json() };
        }

        /** @type {string} */
        let errorMsg = `Instant upload failed (HTTP ${response.status})`;
        try {
            const body = await response.json();
            errorMsg = body.message || body.error || errorMsg;
        } catch (_) {}

        if (response.status === 507) {
            return { ok: false, isQuotaError: true, errorMsg };
        }
        if (response.status === 409) {
            // Duplicate name in the folder — a byte upload would hit the
            // exact same conflict; surface it without transferring.
            return { ok: false, errorMsg };
        }
        // 404 (ownership race with a delete+GC), 4xx/5xx: fall back to the
        // byte upload — the server dedups it on write anyway.
        return null;
    } catch (_) {
        return null;
    }
}

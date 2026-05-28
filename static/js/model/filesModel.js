// @ts-check

/**
 * OxiCloud – Files data model.
 *
 * Pure data layer: all API calls for file/folder listing and breadcrumb
 * resolution, with zero DOM dependency. Views import these functions and
 * call them without knowing the fetch details.
 */

import { app } from '../app/state.js';
import { uiNotifications } from '../app/uiNotifications.js';

/** @import {FileItem, FolderItem} from '../core/types.js' */

/** @type {RequestInit} */
const NO_CACHE = {
    headers: { 'Cache-Control': 'no-cache, no-store, must-revalidate', Pragma: 'no-cache' },
    credentials: 'same-origin',
    cache: 'no-store'
};

/**
 * Fetch metadata for a single folder.
 * Rejects with `null` when the server returns a non-OK response.
 * @param {string} id
 * @returns {Promise<FolderItem>}
 */
async function getFolder(id) {
    const response = await fetch(`/api/folders/${id}`, NO_CACHE);
    if (response.ok) return response.json();
    console.warn(`Error fetching folder ${id}`);
    return Promise.reject(null);
}

/**
 * Walk up the folder hierarchy to rebuild `app.breadcrumbPath`.
 *
 * Stops gracefully at a permission boundary (shared subtrees) — the partial
 * breadcrumb built so far becomes the visual root, matching how Google Drive
 * handles shared folders the user cannot traverse beyond.
 *
 * An error on the target folder itself is treated as a real error and falls
 * back to the home folder.
 *
 * @returns {Promise<void>}
 */
async function rebuildBreadCrumb() {
    /** @type {FolderItem|null} */
    let currentFolderInfo = null;
    app.breadcrumbPath = [];

    /** @type {string|null} */
    let id = app.currentPath;

    while (id !== null) {
        try {
            const folderInfo = await getFolder(id);
            if (currentFolderInfo === null) currentFolderInfo = folderInfo;
            app.breadcrumbPath.unshift({ id: folderInfo.id, name: folderInfo.name });
            id = folderInfo.parent_id;
        } catch (_e) {
            if (currentFolderInfo === null) {
                console.warn(`Cannot access target folder ${app.currentPath}, falling back to home`);
                uiNotifications.show('error: folder not found or permission denied', 'the given folder is not available or you do not have sufficient rights');
                app.breadcrumbPath = [];
                id = app.userHomeFolderId;
                if (id) app.currentPath = id;
            } else {
                console.log(`Stopped breadcrumb traversal at permission boundary (parent of ${currentFolderInfo.id} is not accessible)`);
                break;
            }
        }
    }

    app.currentFolderInfo = currentFolderInfo;
}

/**
 * Fetch the folder listing for the given folder id.
 *
 * @param {string} folderId
 * @param {{ forceRefresh?: boolean }} [options]
 * @returns {Promise<{ folders: FolderItem[], files: FileItem[] }>}
 */
async function fetchListing(folderId, options = {}) {
    const timestamp = Math.floor(Date.now() / 1000);
    let url = `/api/folders/${folderId}/listing?t=${timestamp}`;

    /** @type {HeadersInit} */
    const headers = { .../** @type {Record<string,string>} */ (NO_CACHE.headers) };

    if (options.forceRefresh) {
        url += '&force_refresh=true';
        headers['X-Force-Refresh'] = 'true';
    }

    const response = await fetch(url, { ...NO_CACHE, headers });

    if (response.status === 403) throw Object.assign(new Error('Forbidden'), { status: 403 });
    if (!response.ok) throw new Error(`Server responded with status: ${response.status}`);

    const listing = await response.json();
    return {
        folders: Array.isArray(listing.folders) ? listing.folders : [],
        files: Array.isArray(listing.files) ? listing.files : []
    };
}

export { fetchListing, getFolder, rebuildBreadCrumb };

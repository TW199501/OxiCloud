// @ts-check

/**
 * OxiCloud – Files section view.
 *
 * Orchestrates the main Files section:
 *   - Data fetching via `filesModel`
 *   - Rendering via a `ResourceListComponent` instance
 *   - Drag-and-drop initialisation (delegated to `ui.initDragDrop`)
 *
 * Exports `loadFiles` (navigation & deep-link entry-point) and `addItem`
 * (post-upload / post-create optimistic UI updates used by fileOperations
 * and search).
 */

import { ResourceListComponent } from '../components/resourceList.js';
import { i18n } from '../core/i18n.js';
import { batchToolbar } from '../features/files/batchToolbar.js';
import { inlineViewer } from '../features/files/inlineViewer.js';
import { favorites } from '../features/library/favorites.js';
import { fetchListing, rebuildBreadCrumb } from '../model/filesModel.js';
import { grants } from '../model/grants.js';
import { resolveHomeFolder } from './authSession.js';
import { updateHistory } from './main.js';
import { app } from './state.js';
import { ui } from './ui.js';
import { uiNotifications } from './uiNotifications.js';

/** @import {FileItem, FolderItem} from '../core/types.js' */

/** @type {ResourceListComponent|null} */
let _component = null;

/** Guard against concurrent `loadFiles` calls. */
let _loading = false;

/**
 * Return (creating on first call) the `ResourceListComponent` bound to
 * `#files-list`. The element must already be in the DOM.
 * @returns {ResourceListComponent|null}
 */
function _ensureComponent() {
    const filesList = document.getElementById('files-list');
    if (!filesList) return null;

    if (!_component) {
        _component = new ResourceListComponent(/** @type {HTMLElement} */ (filesList), {
            selectable: true,
            showFavorite: true,
            showOwner: true,
            showShareBadge: true,
            draggable: true,
            showContextMenu: true,
            isFavorite: (id, type) => favorites.isFavorite(id, type),
            isShared: (id, type) => grants.getOutgoingGrantsFor(type, id).length > 0,
            onOpen: (item) => ui.openItem(item),
            onFavoriteToggle: async (item) => {
                const isFile = 'mime_type' in item;
                const type = isFile ? 'file' : 'folder';
                if (favorites.isFavorite(item.id, type)) {
                    await favorites.removeFromFavorites(item.id, type);
                    _component?.setFavoriteVisualState(item.id, type, false);
                } else {
                    await favorites.addToFavorites(item.id, item.name, type, null);
                    _component?.setFavoriteVisualState(item.id, type, true);
                }
            },
            onContextMenu: (item, e) => ui.showContextMenuForItem(item, e),
            onSelectionChange: (selectedItems) => {
                batchToolbar._selected.clear();
                for (const sel of selectedItems) {
                    const isFile = 'mime_type' in sel;
                    batchToolbar._selected.set(sel.id, {
                        id: sel.id,
                        name: sel.name,
                        type: isFile ? 'file' : 'folder',
                        parentId: isFile ? /** @type {FileItem} */ (sel).folder_id || '' : /** @type {FolderItem} */ (sel).parent_id || ''
                    });
                }
                batchToolbar._syncUI();
            }
        });

        // Wire drag-and-drop on the container once the component is created.
        ui.initDragDrop(/** @type {HTMLElement} */ (filesList));
    }

    return _component;
}

/**
 * Append a single item to the current view (post-upload / post-create
 * optimistic update). No-op when the Files section is not active or the
 * item is already in the list.
 *
 * Called by `fileOperations.js` and `search.js`.
 *
 * @param {FileItem|FolderItem} item
 */
function addItem(item) {
    const component = _ensureComponent();
    if (!component) return;
    // Reveal the list if the empty-state is showing
    ui.resetFilesList();
    component.addItem(item);
}

/**
 * Load and render the contents of `app.currentPath`, rebuilding the
 * breadcrumb and updating browser history.
 *
 * @param {Object}  [options]
 * @param {boolean} [options.insertHistory=true]
 * @param {boolean} [options.forceRefresh=false]
 */
async function loadFiles(options = { insertHistory: true }) {
    if (_loading) {
        console.log('A file load is already in progress, ignoring request');
        return;
    }
    _loading = true;

    // Delay spinner so fast loads avoid the flash
    const spinnerTimeout = setTimeout(() => {
        ui.showError(`
            <div class="files-loading-spinner">
                <div class="spinner"></div>
                <span>${i18n.t('files.loading')}</span>
            </div>
        `);
    }, 100);

    try {
        if (!app.userHomeFolderId) await resolveHomeFolder();

        // Resolve path to home folder when none is set
        if (!app.currentPath || app.currentPath === '') {
            if (app.userHomeFolderId) {
                app.currentPath = app.userHomeFolderId;
                app.breadcrumbPath = [];
                console.log(`Loading user folder: ${app.userHomeFolderName} (${app.userHomeFolderId})`);
            } else {
                console.warn('No home folder id — this should not normally happen');
            }
        }

        await rebuildBreadCrumb();
        ui.updateBreadcrumb();
        updateHistory(options.insertHistory ?? true);

        const { folders, files } = await fetchListing(app.currentPath, {
            forceRefresh: options.forceRefresh ?? false
        });

        clearTimeout(spinnerTimeout);

        // Prepare the container (shows #files-list, hides error panel)
        ui.resetFilesList();

        const component = _ensureComponent();
        if (!component) return;

        batchToolbar.clear();
        batchToolbar.init();
        batchToolbar.setActiveComponent(component);

        if (folders.length === 0 && files.length === 0) {
            ui.showEmptyList();
        } else {
            component.render([...folders, ...files]);
            await component.resolveOwnerCells();
        }

        console.log(`Loaded ${folders.length} folders and ${files.length} files`);

        // Deep-link: open a specific file if requested via app.viewFile
        if (app.viewFile) {
            const fileFound = files.find((f) => f.id === app.viewFile) ?? null;
            if (fileFound) {
                console.log(`file ${app.viewFile} found, calling viewer`);
                await inlineViewer.openFile(fileFound);
            } else {
                console.log(`file ${app.viewFile} not found`);
                app.viewFile = null;
                updateHistory(false);
            }
        }
    } catch (/** @type {any} */ err) {
        clearTimeout(spinnerTimeout);
        if (err?.status === 403) {
            ui.showError(`<p>${i18n.t('errors.forbidden', 'Could not load files')}</p>`);
        } else {
            console.error('Error loading folders:', err);
            uiNotifications.show('Error', 'Could not load files and folders');
        }
    } finally {
        _loading = false;
    }
}

export { addItem, loadFiles };

# Plan: Group-by swimlanes in SharedWithMe

## Context

The SharedWithMe view now uses `ResourceListComponent` which already accepts an optional `groupFn` in `render()` / `append()`. The task is to expose a **Group by** dropdown in the actions-bar that lets users cluster items into swimlane sections by **Owner** or **Share date**. Changing the grouping restarts the cursor-paginated fetch with the matching `sort_by` query param so the server delivers items pre-sorted for the chosen dimension — the frontend only needs to inject dividers when the key changes.

---

## Architecture overview

```
main.js (UI)                    sharedWithMeView.js              grants.js / backend
────────────────────────        ───────────────────              ──────────────────
[Group by] dropdown    ──────→  setGroupBy(key)          ──────→ fetchSharedWithMe({ orderBy })
  shows: None / Owner /           _groupBy state                  ↳ GET …?sort_by=granted_by
         Share date               resets cursor                   ↳ GET …?sort_by=granted_at
                                  _makeGroupFn()          ←──────  items in server sort order
                                  render(f, flds, keyFn, labelFn)
                                  ↓
                             ResourceListComponent
                               injects swimlane dividers
                               when keyFn(item) changes
```

---

## Extensibility contract (`GroupByDef`)

Each view that supports grouping defines a `GroupByDef[]` array locally:

```js
/**
 * @typedef {{ key: string, orderBy: string, keyFn: (item: FileItem|FolderItem) => string|null, labelFn?: (key: string) => string }} GroupByDef
 */
```

- `key` — internal identifier (`''` = none, `'owner'`, `'shareDate'`)
- `orderBy` — value forwarded to the API as `sort_by`
- `keyFn(item)` — returns the grouping key (UUID, bucket name). Same key → same swimlane.
- `labelFn(key)` — converts the raw key to a human-readable header. Optional (identity if omitted).

The separation of `keyFn` / `labelFn` is critical for the Owner case: grouping is keyed by UUID (stable, unique), but the swimlane header shows the resolved display name.

---

## Changes — Frontend

### 1. `static/js/components/resourceList.js`

**A. Persist `_lastGroupKey` across `append()` calls**

Current bug: `_lastGroupKey` is local to `_appendItems`, so loading page 2 always inserts a redundant swimlane header for the first item even if it belongs to the same group as the last item on page 1.

Fix:
```js
// constructor
this._lastGroupKey = /** @type {string|null|undefined} */ (undefined);

// render() — reset before first page
this._lastGroupKey = undefined;

// _appendItems() — read and write instance field
let lastGroupKey = this._lastGroupKey;
// ... existing loop (unchanged) ...
this._lastGroupKey = lastGroupKey;  // persist for next append()
```

**B. Add optional `groupLabelFn` parameter**

```js
/**
 * @param {FolderItem[]} folders
 * @param {FileItem[]}   files
 * @param {((item: FileItem|FolderItem) => string|null)=} groupKeyFn
 * @param {((key: string) => string)=} groupLabelFn  — defaults to identity
 */
render(folders, files, groupKeyFn, groupLabelFn) { … }
append(folders, files, groupKeyFn, groupLabelFn) { … }
```

Pass `groupLabelFn` down to `_appendItems` and use it in `_createGroupHeader`:
```js
_createGroupHeader(key, labelFn) {
    const label = labelFn ? labelFn(key) : key;
    el.textContent = label;
    …
}
```

Store `this._groupLabelFn` on the instance between `render()` and `append()` calls (same pattern as `_lastGroupKey`).

### 2. `static/css/components/resourceList.css`

Add missing swimlane-header styles (block was referenced in JS but had no CSS):

```css
/* ── swimlane group header ─────────────────────────── */
.resource-list__swimlane-header {
    grid-column: 1 / -1;
    padding: 6px 12px 4px;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-faint);
    border-bottom: 1px solid var(--color-border);
    margin-top: 8px;
}
.resource-list__swimlane-header:first-child { margin-top: 0; }
```

### 3. `static/js/core/formatters.js`

Add `normalizeDateBucket(dateStr)` — pure, no imports needed:

```js
/**
 * Normalize an ISO-8601 date string into a human-readable bucket label.
 * Buckets (newest-first): Today | Last 7 days | Last 30 days | <YYYY>
 * @param {string} dateStr
 * @returns {string}
 */
export function normalizeDateBucket(dateStr) {
    const date = new Date(dateStr);
    const diffDays = Math.floor((Date.now() - date.getTime()) / 86_400_000);
    if (diffDays === 0) return i18n.t('dateBucket.today',     'Today');
    if (diffDays <= 7)  return i18n.t('dateBucket.last7days', 'Last 7 days');
    if (diffDays <= 30) return i18n.t('dateBucket.last30days','Last 30 days');
    return String(date.getFullYear());
}
```

(Import `i18n` at top of `formatters.js` if not already present — check first.)

### 4. `static/js/model/systemUsers.js`

Add synchronous best-effort lookup for use in `groupKeyFn` / swimlane labels:

```js
/**
 * Synchronous best-effort display-name lookup from the pre-fetched cache.
 * Returns a shortened UUID prefix when the cache is not yet loaded.
 * @param {string} userId
 * @returns {string}
 */
getDisplayNameSync(userId) {
    if (_index === null) return `${userId.slice(0, 8)}…`;
    return _index.get(userId) ?? `${userId.slice(0, 8)}…`;
},
```

The cache is loaded by `prefetch()` which `sharedWithMeView.init()` already calls at startup. By the time the first items render, the cache is warm in virtually all cases.

### 5. `static/js/model/grants.js`

Add `orderBy` param to `fetchSharedWithMe`:

```js
async fetchSharedWithMe({ resourceTypes = ['file', 'folder'], limit = 50, cursor, orderBy } = {}) {
    const params = new URLSearchParams({ limit: String(limit), resource_types: resourceTypes.join(',') });
    if (cursor)  params.set('cursor', cursor);
    if (orderBy) params.set('sort_by', orderBy);
    …
}
```

### 6. `static/js/views/sharedWithMe/sharedWithMeView.js`

**New state:**
```js
/** @type {string} '' | 'owner' | 'shareDate' */
_groupBy: '',
```

**`GROUP_BY_DEFS` constant (module-level):**
```js
const GROUP_BY_DEFS = [
    {
        key: 'owner',
        orderBy: 'granted_by',
        keyFn: (item) => item.owner_id || null,
        labelFn: (id) => systemUsers.getDisplayNameSync(id)
    },
    {
        key: 'shareDate',
        orderBy: 'granted_at',
        // sort_date is set to item.granted_at in _mapItems()
        keyFn: (item) => {
            const d = /** @type {Record<string,string>} */ (/** @type {unknown} */ (item)).sort_date;
            return d ? normalizeDateBucket(d) : null;
        }
    }
];
```

**`setGroupBy(key)` public method:**
```js
setGroupBy(key) {
    if (this._groupBy === key) return;
    this._groupBy = key;
    this._nextCursor = null;   // restart from page 1
    this._component?.clear();  // clear DOM items
    this._loadPage();
},
```

**`_mapItems()` change:** Set `sort_date: item.granted_at` on both folders and files (replaces `f.modified_at` in files). This is the field the shareDate `keyFn` reads.

**`_loadPage()` change:** Derive active def and pass to API + component:
```js
const def = GROUP_BY_DEFS.find(d => d.key === this._groupBy);
const data = await grants.fetchSharedWithMe({
    …,
    orderBy: def?.orderBy   // undefined when no grouping
});
…
if (isFirstPage) {
    this._component?.render(folders, files, def?.keyFn, def?.labelFn);
} else {
    this._component?.append(folders, files, def?.keyFn, def?.labelFn);
}
```

### 7. `static/js/app/main.js`

**A. New `_toggleButtonsWithGroupBy` template (inside `.view-toggle`):**
```js
const _toggleButtonsWithGroupBy = `
    <div class="view-toggle">
        <div class="group-by-selector" id="group-by-selector">
            <button class="toggle-btn group-by-btn" id="group-by-btn" title="Group by" data-i18n-title="groupby.title">
                <i class="fas fa-layer-group"></i>
            </button>
            <div class="group-by-menu hidden" id="group-by-menu">
                <button class="group-by-option active" data-group-by="" data-i18n="groupby.none">None</button>
                <button class="group-by-option" data-group-by="owner" data-i18n="groupby.owner">Owner</button>
                <button class="group-by-option" data-group-by="shareDate" data-i18n="groupby.shareDate">Share date</button>
            </div>
        </div>
        <span class="view-toggle-separator"></span>
        <button class="toggle-btn active" id="grid-view-btn" title="Grid view">
            <i class="fas fa-th"></i>
        </button>
        <button class="toggle-btn" id="list-view-btn" title="List view">
            <i class="fas fa-list"></i>
        </button>
    </div>
`;
```

**B. Update sharedwithme template:** Also add missing `_batchToolbarButons`:
```js
sharedwithme: `
    <div class="action-buttons" id="default-buttons"></div>
    ${_batchToolbarButons}
    ${_toggleButtonsWithGroupBy}
`
```

**C. `setupActionsBarDelegation()` — add group-by handling before the switch:**
```js
// Group-by option selected
if (btn.classList.contains('group-by-option')) {
    const key = btn.dataset.groupBy ?? '';
    sharedWithMeView.setGroupBy(key);
    document.querySelectorAll('.group-by-option').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    document.getElementById('group-by-menu')?.classList.add('hidden');
    document.getElementById('group-by-btn')?.classList.toggle('active', !!key);
    return;
}
switch (btn.id) {
    case 'group-by-btn':
        document.getElementById('group-by-menu')?.classList.toggle('hidden');
        break;
    …
}
```

**D.** Add a `document.addEventListener('click', …)` (or reuse the existing upload-dropdown pattern) to close the group-by menu on outside clicks.

### 8. CSS for group-by dropdown

Add to `static/css/components/buttons.css` (already contains `.view-toggle` styles):

```css
/* ── Group-by selector (inside .view-toggle) ─────────── */
.view-toggle-separator {
    width: 1px;
    height: 20px;
    background: var(--color-border);
    align-self: center;
    margin: 0 2px;
}

.group-by-selector {
    position: relative;
}

.group-by-btn.active { color: var(--color-accent); }

.group-by-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 200;
    min-width: 140px;
    background: var(--color-bg-elevated);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    box-shadow: 0 4px 16px var(--color-shadow);
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
}

.group-by-menu.hidden { display: none; }

.group-by-option {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: none;
    background: transparent;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.85rem;
    color: var(--color-text);
    text-align: left;
    width: 100%;
}

.group-by-option:hover { background: var(--color-border); }
.group-by-option.active { color: var(--color-accent); font-weight: 600; }
```

---

## Changes — Backend

### 9. `src/domain/services/authorization.rs` — update `GrantCursor`

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GrantCursor {
    /// Sort dimension active when this cursor was issued.
    /// Mis-match with the current `sort_by` param → cursor is discarded.
    #[serde(default = "GrantCursor::default_sort")]
    pub sort_by: String,
    pub granted_at: chrono::DateTime<chrono::Utc>,
    pub resource_id: Uuid,
    /// Present only when `sort_by == "granted_by"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_by: Option<Uuid>,
}
impl GrantCursor {
    fn default_sort() -> String { "granted_at".to_owned() }
}
impl PageCursor for GrantCursor {}
```

Old cursors (which lack `sort_by`) fail serde and are treated as "start from top" — the existing "undecodable cursor → restart" invariant applies.

### 10. `src/application/ports/authorization_ports.rs` — add `sort_by` arg

```rust
async fn list_incoming_resources_paged(
    &self,
    subject: Subject,
    kinds: &[ResourceKind],
    limit: u32,
    cursor: Option<GrantCursor>,
    sort_by: &str,          // "granted_at" | "granted_by"
) -> Result<(Vec<IncomingGrantSummary>, Option<GrantCursor>), DomainError>;
```

### 11. `src/infrastructure/services/pg_acl_engine.rs` — branch SQL on sort_by

Two separate `sqlx::query_as` calls, selected at runtime:

**`sort_by = "granted_by"` SQL:**
```sql
WITH agg AS ( … same aggregation … )
SELECT resource_type, resource_id, permissions, granted_at, granted_by
FROM agg
WHERE (  $4::uuid IS NULL                             -- cursor_by
      OR granted_by > $4::uuid
      OR (granted_by = $4::uuid AND (
              $5::timestamptz IS NULL                  -- cursor_at
           OR granted_at < $5
           OR (granted_at = $5 AND resource_id < $6::uuid))))
ORDER BY granted_by ASC, granted_at DESC, resource_id DESC
LIMIT $7
```
Cursor for next page: `GrantCursor { sort_by: "granted_by", granted_at: r.3, resource_id: r.1, granted_by: Some(r.4) }`

**`sort_by = "granted_at"` SQL (existing, unchanged except cursor struct gains `sort_by` field):**
Cursor for next page: `GrantCursor { sort_by: "granted_at", granted_at: r.3, resource_id: r.1, granted_by: None }`

### 12. `src/interfaces/api/handlers/grant_handler.rs`

```rust
let sort_by = q.sort_by.as_deref().unwrap_or("granted_at");
if !matches!(sort_by, "granted_at" | "granted_by") {
    return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid sort_by"}))).into_response();
}
// Invalidate cursor when sort mode changed (prevents keyset confusion)
let cursor = q.decode_cursor::<GrantCursor>()
    .filter(|c| c.sort_by == sort_by);

let (summaries, next_cursor) = state.authorization
    .list_incoming_resources_paged(subject, &kinds, limit, cursor, sort_by)
    .await …;
```

---

## Files touched

| File | Change |
|---|---|
| `static/js/components/resourceList.js` | Persist `_lastGroupKey`; add `groupLabelFn` param to `render`/`append` |
| `static/css/components/resourceList.css` | Add `.resource-list__swimlane-header` styles |
| `static/js/core/formatters.js` | Add `normalizeDateBucket()` |
| `static/js/model/systemUsers.js` | Add `getDisplayNameSync()` |
| `static/js/model/grants.js` | Add `orderBy` param |
| `static/js/views/sharedWithMe/sharedWithMeView.js` | Add `_groupBy`, `setGroupBy()`, `GROUP_BY_DEFS`; update `_mapItems`, `_loadPage` |
| `static/js/app/main.js` | Add `_toggleButtonsWithGroupBy`; add batch toolbar to sharedwithme; wire group-by delegation |
| `static/css/components/buttons.css` | Add group-by dropdown styles + `.view-toggle-separator` |
| `src/domain/services/authorization.rs` | Update `GrantCursor` struct |
| `src/application/ports/authorization_ports.rs` | Add `sort_by` to trait method signature |
| `src/infrastructure/services/pg_acl_engine.rs` | Branch SQL on `sort_by`; emit new cursor shape |
| `src/interfaces/api/handlers/grant_handler.rs` | Extract + validate `sort_by`; filter cursor on mismatch |

---

## Known limitations (out of scope)

- **Owner sort order is by UUID, not display name.** Items from the same owner are correctly grouped, but the ORDER of groups is UUID-lexicographic, not alphabetical by name. Alphabetical ordering would require a server-side join to the users table and a different cursor — deferred.
- **Batch operations from SharedWithMe navigate to Files** — `batchDelete()` calls `loadFiles()`. Pre-existing bug; separate PR.
- **Group-by is SharedWithMe-only** — the `GroupByDef` contract is extensible but no other view is wired in this PR.

---

## Verification

```bash
# Backend
cargo fmt --all
cargo clippy --all-features --all-targets -- -D warnings
cargo test --workspace

# Frontend
biome lint static/js/
stylelint static/css/
tsc -p jsconfig.json --noEmit
```

Manual smoke tests:
1. SharedWithMe loads with no group-by → items appear, no swimlane headers
2. Select "Owner" → page reloads, swimlane headers show resolved display names grouped by granter
3. "Load more" appends without inserting a redundant header for a continuing group
4. Select "Share date" → swimlane headers: Today / Last 7 days / Last 30 days / year
5. Switch back to "None" → plain list, no headers
6. Cursor cursor changes don't bleed across sort modes (switching group-by resets to page 1)
7. Grid ↔ list toggle still works in all group-by states
8. Group-by menu closes when clicking outside

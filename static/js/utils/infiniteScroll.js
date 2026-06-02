// @ts-check

/**
 * Infinite-scroll for cursor-paginated views.
 *
 * Each section that paginates (Files, Favorites, Recent, Trash,
 * SharedWithMe, MyShares) injects a "Load more" button wrapped in a
 * `<div>` below `.files-container`. The wrapper toggles `.hidden` to
 * reflect whether a next page exists.
 *
 * This utility wires an [`IntersectionObserver`] onto the same wrapper
 * so it auto-fires the load-more action when the user scrolls near it.
 * The visible button stays as a fallback (accessibility, browsers
 * without IntersectionObserver support, the user's reflex from
 * before infinite-scroll landed).
 *
 * Wire it once per wrapper at creation time. The observer is idempotent
 * across re-renders because the wrapper itself is re-used; appended
 * items push it down the document and trigger fresh intersection
 * events when the user scrolls again.
 *
 * Re-entrancy: every existing `_loadPage` implementation has its own
 * in-flight guard (a `_loading` flag returning early on re-entry), so
 * this utility doesn't add another. If a future view doesn't, add a
 * `_loading` field on the view object before adopting infinite-scroll.
 */

/**
 * Auto-fire `onLoadMore()` when `wrapper` scrolls within
 * `rootMargin` of the viewport AND is visible (the `.hidden` class is
 * the cursor-exhausted signal — see `_setLoadMoreVisible` in each view).
 *
 * @param {HTMLElement} wrapper              The `.swm-load-more-wrapper` element.
 * @param {() => void}  onLoadMore           Called on each intersection-enter.
 * @param {{rootMargin?: string}} [opts]
 * @returns {() => void}                     Teardown — disconnects the observer.
 */
export function attachInfiniteScroll(wrapper, onLoadMore, opts = {}) {
    if (!('IntersectionObserver' in window)) {
        // Older browser — degrade to the button-only experience.
        return () => {};
    }

    const observer = new IntersectionObserver(
        (entries) => {
            for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                // No next page → wrapper hidden → skip.
                if (wrapper.classList.contains('hidden')) continue;
                onLoadMore();
            }
        },
        // Trigger a bit before the wrapper actually enters the viewport
        // so the next page is on its way by the time the user reaches
        // the bottom — smoother than a perceptible "click… wait…" beat.
        { rootMargin: opts.rootMargin ?? '200px' }
    );
    observer.observe(wrapper);
    return () => observer.disconnect();
}

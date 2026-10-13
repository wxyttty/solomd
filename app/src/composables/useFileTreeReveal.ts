import { ref } from 'vue';

/**
 * "Reveal in File Tree" plumbing.
 *
 * Its own module for the same reason `useTreeDrag` has one: the request comes
 * from PaneTabBar's tab context menu, while the row lives in FileTree — two
 * components in one file, one of which (`FileTreeNode`) is declared in a plain
 * `<script>` block and can't close over setup's bindings. Both sides import
 * from here instead of prop-drilling through every level of the tree.
 *
 * Why a parked request instead of a one-shot event: the tree only holds the
 * folders that have been expanded, and it lists them lazily. Walking down to a
 * file means one directory read per level, and when the file is outside the
 * open workspace the tree also has to wait for a brand-new root to list. An
 * event fired once would be dropped in those windows — which is exactly what
 * "I clicked Reveal in File Tree and nothing happened" looked like. The tree
 * consumes the request as soon as it is in a position to satisfy it.
 */

/** Absolute path the user asked to see, or null when nobody is asking. */
export const revealRequest = ref<string | null>(null);

/** Path of the row currently highlighted as "here it is", or null. */
export const revealedPath = ref<string | null>(null);

export function requestRevealInTree(path: string) {
  revealRequest.value = path;
}

/** Called by the tree once it has served (or given up on) a request. */
export function clearRevealRequest() {
  revealRequest.value = null;
}

/** Drop the highlight — used when the reveal target stops being meaningful. */
export function clearRevealedPath() {
  revealedPath.value = null;
}

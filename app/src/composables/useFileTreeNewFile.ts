import { ref } from 'vue';

/**
 * #338 — "new file in the selected folder" from the keyboard.
 *
 * The request is parked rather than fired as an event, for the same reason as
 * `useFileTreeReveal`: the tree is `v-if`'d on the sidebar toggle, so the
 * shortcut can arrive while no tree exists to hear it, and the tree serves the
 * request once it has mounted and listed its root.
 */
export const newFileInTreeRequest = ref(0);

export function requestNewFileInTree() {
  newFileInTreeRequest.value++;
}

export function clearNewFileInTreeRequest() {
  newFileInTreeRequest.value = 0;
}

<script setup lang="ts">
/**
 * v2.4 — Public reading mode.
 *
 * Maximally clean single-doc preview: no editor pane, no toolbar, no
 * file tree, no statusbar — just centered prose. A small floating ✕
 * button (top right) restores the user's previous view mode.
 *
 * Reuses `Preview.vue`'s renderer via the `skin: 'reading'` prop, so
 * we don't duplicate the markdown / mermaid / image-overlay pipeline.
 */
import { computed, ref, onMounted, onBeforeUnmount } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import Preview from './Preview.vue';
import Icon from './Icons.vue';
import { useTabsStore } from '../stores/tabs';
import { useSettingsStore } from '../stores/settings';
import { useI18n } from '../i18n';
import { forceWinChromePreview, isWindowsDesktop } from '../lib/platform';

const tabs = useTabsStore();
const settings = useSettingsStore();
const { t } = useI18n();

const tab = computed(() => tabs.activeTab);

function exit() {
  settings.exitReadingMode();
}

// #221(4) — on the frameless Windows build, reading mode hides the toolbar
// and with it the min/✕ caption buttons, leaving no way to minimize or close
// the app without leaving reading mode first. Mirror a minimal pair of window
// controls next to the exit button. macOS keeps its native traffic lights, so
// nothing is rendered there.
const hasTauriShell = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const winControls =
  (isWindowsDesktop() && hasTauriShell) || (import.meta.env.DEV && forceWinChromePreview());
function winMinimize() {
  if (hasTauriShell) void getCurrentWindow().minimize();
}
function winClose() {
  // Same close-requested flow (unsaved-tabs confirm) as the title-bar ✕.
  if (hasTauriShell) void getCurrentWindow().close();
}
// #221 follow-up — the set was min / ✕ only; add maximize/restore so reading
// mode offers the same three caption buttons as the title bar. The toolbar's
// own max button (and its Snap Layouts hit-test rect) is unmounted in reading
// mode, so this is a plain DOM button driving toggleMaximize().
const isMaximized = ref(false);
let unlistenResize: (() => void) | null = null;
async function refreshMaximized() {
  if (!hasTauriShell) return;
  try {
    isMaximized.value = await getCurrentWindow().isMaximized();
  } catch {
    /* not fatal */
  }
}
function winToggleMax() {
  if (hasTauriShell) void getCurrentWindow().toggleMaximize();
}
onMounted(async () => {
  if (!winControls || !hasTauriShell) return;
  await refreshMaximized();
  try {
    unlistenResize = await getCurrentWindow().onResized(() => void refreshMaximized());
  } catch {
    /* not fatal */
  }
});
onBeforeUnmount(() => {
  unlistenResize?.();
  unlistenResize = null;
});
</script>

<template>
  <div class="reading-view" data-reading-view>
    <div class="reading-view__controls">
      <button
        v-if="winControls"
        class="reading-view__winbtn"
        :title="t('menubar.minimize')"
        :aria-label="t('menubar.minimize')"
        @click="winMinimize"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" stroke-width="1" /></svg>
      </button>
      <button
        v-if="winControls"
        class="reading-view__winbtn"
        :title="isMaximized ? t('menubar.restore') : t('menubar.maximize')"
        :aria-label="isMaximized ? t('menubar.restore') : t('menubar.maximize')"
        data-reading-max
        @click="winToggleMax"
      >
        <svg v-if="isMaximized" width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><path d="M2.5 0.5h7v7" /><rect x="0.5" y="2.5" width="7" height="7" /></svg>
        <svg v-else width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><rect x="0.5" y="0.5" width="9" height="9" /></svg>
      </button>
      <button
        v-if="winControls"
        class="reading-view__winbtn reading-view__winbtn--close"
        :title="t('menubar.close')"
        :aria-label="t('menubar.close')"
        @click="winClose"
      >
        <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" /></svg>
      </button>
      <button
        class="reading-view__close"
        :title="t('reading.toEditTooltip')"
        :aria-label="t('reading.toEdit')"
        data-reading-exit
        @click="exit"
      >
        <Icon name="view-edit" :size="18" />
      </button>
    </div>
    <div v-if="tab" class="reading-view__doc">
      <Preview
        :source="tab.content"
        :file-path="tab.filePath"
        :tab-id="tab.id"
        skin="reading"
      />
    </div>
    <div v-else class="reading-view__empty">
      {{ t('reading.empty') }}
    </div>
  </div>
</template>

<style scoped>
.reading-view {
  position: relative;
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  min-width: 0;
  background: var(--bg);
}
.reading-view__doc {
  flex: 1;
  display: flex;
  min-height: 0;
}
.reading-view__doc > :deep(.preview-host) {
  flex: 1;
  min-height: 0;
}
.reading-view__controls {
  position: absolute;
  z-index: 50;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  /* Stay clear of iOS notch / home-indicator areas */
  top: max(14px, env(safe-area-inset-top, 0px));
  right: max(18px, env(safe-area-inset-right, 0px));
}
.reading-view__winbtn {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  background: var(--bg-elev);
  border: 1px solid var(--border);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.06);
  cursor: pointer;
  transition: color 0.15s, background 0.15s, border-color 0.15s;
}
.reading-view__winbtn:hover {
  color: var(--text);
  border-color: var(--accent);
  background: var(--bg-hover);
}
.reading-view__winbtn--close:hover {
  background: #e81123;
  border-color: #e81123;
  color: #fff;
}
.reading-view__close {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: var(--text-muted);
  background: var(--bg-elev);
  border: 1px solid var(--border);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.06);
  cursor: pointer;
  transition: color 0.15s, background 0.15s, border-color 0.15s, transform 0.15s;
}
.reading-view__close:hover {
  color: var(--text);
  border-color: var(--accent);
  background: var(--bg-hover);
}
.reading-view__close:active {
  transform: scale(0.96);
}
.reading-view__empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-faint);
  font-size: 14px;
}
</style>

<script setup lang="ts">
import { computed } from 'vue';
import { isMacOS } from '../lib/platform';
import { shortcutLabel } from '../lib/keybindings';
import { useTabsStore } from '../stores/tabs';
import { useSettingsStore } from '../stores/settings';
import { useWritingSessionStore } from '../stores/writingSession';
import { cjkWordCount } from '../lib/chinese';
import { useInbox } from '../composables/useInbox';
import { useI18n } from '../i18n';
import WritingGoals from './WritingGoals.vue';
import PomodoroPill from './PomodoroPill.vue';
import SyncStatusPill from './SyncStatusPill.vue';
import { usePomodoroStore } from '../stores/pomodoro';

const props = withDefaults(
  defineProps<{ line: number; col: number; selectionText?: string }>(),
  { selectionText: '' },
);
const tabs = useTabsStore();
const settings = useSettingsStore();
const writingSession = useWritingSessionStore();
const inbox = useInbox();
const pomodoro = usePomodoroStore();
const { t } = useI18n();
// #180 — the chord in this sentence comes from the user's bindings, not from
// a literal baked into the translation.
const macChord = isMacOS();
const kbSettings = useSettingsStore();
function withChord(key: string, actionId: string): string {
  return t(key, { key: shortcutLabel(actionId, kbSettings.keybindings, macChord) || '—' });
}

const stats = computed(() => {
  const c = tabs.activeTab?.content ?? '';
  return cjkWordCount(c);
});

const wordCount = computed(() => stats.value.total);
const cjkCount = computed(() => stats.value.cjk);
const charCount = computed(() => stats.value.chars);
const lineCount = computed(() => {
  const c = tabs.activeTab?.content ?? '';
  return c ? c.split('\n').length : 0;
});
// v4.3.0 issue #70: stats for the current editor selection. Shown only when
// non-empty; uses the same cjkWordCount tokenizer as the document totals so
// CJK + Latin counts line up.
const selStats = computed(() => {
  const s = props.selectionText ?? '';
  if (!s) return null;
  return cjkWordCount(s);
});

const lang = computed(() => (tabs.activeTab?.language === 'markdown' ? 'Markdown' : 'Plain Text'));
const enc = computed(() => tabs.activeTab?.encoding ?? 'UTF-8');

const showTodayTotal = computed(
  () =>
    settings.showWritingStats &&
    settings.showWorkspaceDailyTotal &&
    writingSession.todayDocCount > 0,
);

function onPillClick() {
  // Click is the same affordance as ⌘E. v4.6 F6: when the inbox workflow is
  // on, route through organizeAndAdvance so clicking the pill from inside the
  // InboxView / inbox filter marks the note organized and advances; otherwise
  // it's the plain toggle.
  if (settings.inboxWorkflowEnabled) {
    void inbox.organizeAndAdvance();
  } else {
    inbox.toggleActive();
  }
}
</script>

<template>
  <div class="statusbar">
    <span class="seg seg--wide">Ln {{ props.line }}, Col {{ props.col }}</span>
    <span class="sep sep--wide">·</span>
    <span class="seg seg--wide">{{ lineCount }} lines</span>
    <span class="sep sep--wide">·</span>
    <span class="seg">{{ wordCount }} words</span>
    <span v-if="cjkCount > 0" class="seg seg--cjk" :title="`${cjkCount} CJK characters`">
      ({{ cjkCount }} 字)
    </span>
    <span class="sep sep--wide">·</span>
    <span class="seg seg--wide">{{ charCount }} chars</span>
    <span v-if="selStats" class="seg seg--selection" :title="t('statusBar.selectionTooltip')">
      ·
      {{ t('statusBar.selection', { words: String(selStats.total), chars: String(selStats.chars) }) }}
      <span v-if="selStats.cjk > 0" class="seg--cjk">({{ selStats.cjk }} 字)</span>
    </span>
    <WritingGoals v-if="settings.showWritingStats" />
    <span class="spacer"></span>
    <span
      v-if="showTodayTotal"
      class="seg seg--today"
      :title="t('writingStats.todayTooltip')"
    >
      {{
        t('writingStats.todayWorkspaceValue', {
          n: writingSession.todayTotal.toLocaleString(),
          docs: String(writingSession.todayDocCount),
        })
      }}
    </span>
    <PomodoroPill v-if="pomodoro.active" />
    <SyncStatusPill />
    <button
      v-if="inbox.activeIsInbox.value"
      class="seg seg--inbox"
      :title="settings.inboxWorkflowEnabled ? withChord('inbox.pillTooltipOrganize', 'inbox.toggle') : withChord('inbox.pillTooltip', 'inbox.toggle')"
      @click="onPillClick"
    >
      {{ t('inbox.pill') }}
    </button>
    <span class="seg seg--wide">{{ enc }}</span>
    <span class="sep sep--wide">·</span>
    <span class="seg seg--lang seg--wide">{{ lang }}</span>
  </div>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  gap: 8px;
  height: var(--statusbar-h);
  padding: 0 12px;
  background: var(--bg-elev);
  border-top: 1px solid var(--border);
  font-size: 11px;
  color: var(--text-muted);
  user-select: none;
}
.spacer { flex: 1; }
.sep { color: var(--text-faint); }
.seg--lang { color: var(--accent); }
.seg--cjk { color: var(--accent); margin-left: -4px; }
.seg--selection { color: var(--accent); font-weight: 500; }
.seg--inbox {
  background: var(--accent);
  color: var(--bg-elev);
  padding: 1px 8px;
  border-radius: var(--r-full);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  border: none;
  cursor: pointer;
}
.seg--inbox:hover {
  filter: brightness(1.1);
}
.seg--today {
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
</style>

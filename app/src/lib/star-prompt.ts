/**
 * When to ask for a GitHub star from inside the app: once, ever, and only
 * after SoloMD has been opened on several different days — someone who keeps
 * coming back has an opinion worth asking for; someone on day one does not.
 *
 * Pure state in, state out; App.vue owns storage and the toast.
 */

export interface StarPromptState {
  /** First day seen (YYYY-MM-DD). */
  first: string;
  /** Distinct days the app was opened, most recent last (capped). */
  days: string[];
  /** The prompt has been shown — never again. */
  shown: boolean;
}

export const STAR_PROMPT_MIN_DAYS = 5;
export const STAR_PROMPT_MIN_AGE_DAYS = 3;

export function recordDay(state: StarPromptState | null, today: string): StarPromptState {
  const s = state ?? { first: today, days: [], shown: false };
  if (s.days[s.days.length - 1] === today) return s;
  return { ...s, days: [...s.days, today].slice(-STAR_PROMPT_MIN_DAYS * 2) };
}

function daysBetween(a: string, b: string): number {
  return Math.round((Date.parse(b) - Date.parse(a)) / 86_400_000);
}

export function shouldPrompt(state: StarPromptState, today: string): boolean {
  return (
    !state.shown &&
    state.days.length >= STAR_PROMPT_MIN_DAYS &&
    daysBetween(state.first, today) >= STAR_PROMPT_MIN_AGE_DAYS
  );
}

/** One release-notes page, in one language. */
export interface WhatsNewData {
  /** The release the download buttons point at. */
  version: string;
  meta: { title: string; description: string };
  breadcrumb: { home: string; current: string };
  /** The page's standing header — the v4.0 announcement it grew out of. */
  hero: { date: string; h1: string; lead: string };
  /** Three download buttons. `{VERSION}` is substituted at render time. */
  cta: string[];
  /**
   * Newest release first. Each entry is the inner HTML of one <section>,
   * rendered with set:html — which is why WhatsNew.astro's styles are global
   * and scoped under `.whats-new` rather than component-scoped.
   */
  sections: string[];
  thanks: string | null;
}

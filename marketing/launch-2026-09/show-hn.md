# Show HN — draft (post from your own HN account)

HN's guidelines ask that posts and comments be written by people, not generated.
Treat this as notes: rewrite it in your own words before posting, and answer
the comments yourself.

- **When:** a weekday between 07:00 and 10:00 US Pacific (22:00–01:00 Beijing).
- **URL field:** https://github.com/zhitongblog/solomd. HN readers prefer the repo to a landing page.
- **Never posted before:** hn.algolia.com has no SoloMD story, so this is a first Show HN.

## Title (≤ 80 chars)

```
Show HN: SoloMD – an open-source Markdown editor, mostly written by Claude Code
```

Alternative, if you'd rather not lead with the AI angle:

```
Show HN: SoloMD – Typora-style Markdown editor for desktop and mobile (MIT)
```

## Text

```
SoloMD is a Markdown editor I've been building since April: live preview in
the Typora style (markers fade when the cursor leaves the line), KaTeX,
Mermaid, tldraw whiteboards, wiki links, and export to PDF, Word and HTML.
It's Tauri 2 + Vue 3 + CodeMirror 6, MIT-licensed, and runs on Windows,
macOS, Linux, iOS and Android. The Windows installer is 13 MB and the
universal Mac dmg is 26 MB.

The part HN may find more interesting is how it's made. Almost all of the
code, tests, releases, and even the replies on GitHub issues are written by
Claude Code. I set direction, review, and decide what ships. The README says
so up front. Some things that made this workable:

- A CLI and an MCP server ship with the app, so the agent can drive and
  verify the real product rather than trusting that it compiles.
- Every bug report gets reproduced first. On Windows that means a real VM,
  because the Windows build uses native textareas: CodeMirror dropped
  characters under Chinese IMEs in WebView2.
- Releases go to GitHub, the Mac App Store, the Microsoft Store and Google
  Play from scripts, not by hand.

It has also been wrong in instructive ways, and I'm happy to go into those.

Things I'd like feedback on: the editing feel compared with Typora and
Obsidian, and whether the bundled MCP server (read-only unless you pass
--allow-write) is useful to anyone besides me.

Site: https://solomd.app
```

## Likely questions (answer them yourself)

- *Why not Electron?* Tauri uses the system webview, so the installer is
  smaller. The cost is that behaviour differs between WebView2, WKWebView and
  WebKitGTK, which is why the Windows editor is separate.
- *How much is really AI-written?* Be concrete: 660 of the 955 commits since
  2026-04-07 (69%) carry a Co-Authored-By: Claude trailer, and the issue
  replies are Claude's. Recount before posting.
- *Business model?* None. It's free and MIT, and has a Sponsor link.
- *Telemetry?* Be upfront: anonymous usage stats via Aptabase, **on by
  default**, with no file contents or IP address, and off with one switch.
  Expect someone to ask for opt-in instead. Decide your answer before you post.

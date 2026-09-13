#!/usr/bin/env bash
#
# Thin wrapper over the local **Unzoo Browser REST API** for driving store
# consoles (Google Play, Microsoft Partner Center) that have no usable
# submission API.
#
# Unzoo is NOT exposed as `mcp__unzoo__*` MCP tools in this environment — it is
# this HTTP API at 127.0.0.1:9399. Every endpoint lives under /api/v1; there is
# no bare /health. Full surface: curl $BASE/openapi.json
#
# Three things this wrapper exists to stop you getting wrong:
#   1. --noproxy '*'. The local proxy mangles loopback calls.
#   2. tab_id on EVERY call. Without it the API acts on whatever tab the user
#      is looking at — their bank, their mail. `find-tab` resolves one by URL
#      substring; pass it via UNZOO_TAB or -t.
#   3. Reading pages with get-text, not screenshots. Screenshots do not follow
#      scroll, so a full_page shot silently omits what you are looking for.
#
# Usage:
#   unzoo.sh status
#   unzoo.sh up                        # start daemon + browser if needed
#   unzoo.sh tabs
#   unzoo.sh find-tab play.google.com/console
#   UNZOO_TAB=$(unzoo.sh find-tab play.google.com/console)
#   unzoo.sh nav https://play.google.com/console/...
#   unzoo.sh text | head -60
#   unzoo.sh eval 'document.title'
#   unzoo.sh box 'button[aria-label="上传"]'      # center coords of an element
#   unzoo.sh click 640 380
#   unzoo.sh click-el 'button.mdc-button'          # box + click in one step
#   unzoo.sh upload 'input[type=file]' /path/to/app.aab
#   unzoo.sh shot /tmp/console.png
set -uo pipefail

BASE="${UNZOO_BASE:-http://127.0.0.1:9399/api/v1}"
TIMEOUT="${UNZOO_TIMEOUT:-60}"
TAB="${UNZOO_TAB:-}"

# -t <id> may appear anywhere; strip it out before dispatch.
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    -t) TAB="$2"; shift 2 ;;
    *)  args+=("$1"); shift ;;
  esac
done
set -- ${args[@]+"${args[@]}"}

c()    { curl -s --noproxy '*' --max-time "$TIMEOUT" "$@"; }
post() { c -X POST "$BASE/$1" -H "Content-Type: application/json" -d "$2"; }
py()   { python3 -c "$@"; }

# Build a JSON object from key=value pairs, injecting tab_id when known.
# Values starting with @ are read from a file; ~ and & are left alone.
jbody() {
  TAB="$TAB" py '
import json, os, sys
o = {}
for kv in sys.argv[1:]:
    k, _, v = kv.partition("=")
    if v.lstrip("-").isdigit():
        v = int(v)
    elif v in ("true", "false"):
        v = v == "true"
    o[k] = v
tab = os.environ.get("TAB")
if tab and "tab_id" not in o:
    o["tab_id"] = int(tab)
print(json.dumps(o))' "$@"
}

# Build an /evaluate body from JS read on stdin, wrapped as `(SEL)=>{...}`
# called with the selector. JS therefore never passes through shell quoting —
# hand-interpolating it produced expressions that silently matched nothing.
eval_body() {
  SEL="${1-}" TAB="$TAB" py '
import json, os, sys
js = sys.stdin.read()
expr = "((SEL)=>{%s})(%s)" % (js, json.dumps(os.environ.get("SEL", "")))
print(json.dumps({"expression": expr, "tab_id": int(os.environ["TAB"])}))'
}

need_tab() {
  [ -n "$TAB" ] && return 0
  echo "ERROR: no tab. UNZOO_TAB=\$($0 find-tab <url-substring>) or pass -t <id>." >&2
  echo "       Acting without a tab id would drive whatever tab the user has open." >&2
  return 1
}

cmd="${1:-}"; shift || true

case "$cmd" in

  status)
    c --max-time 6 "$BASE/status"; echo ;;

  # Bring the stack up. The daemon listening does NOT imply the browser is
  # connected; when browser_connected is false every navigate/evaluate returns
  # HTTP 500 "Not connected to browser".
  up)
    if ! c --max-time 5 "$BASE/health" >/dev/null 2>&1; then
      echo "daemon down — start it from a Bash call with run_in_background:true:" >&2
      echo '  "/Applications/Unzoo Browser.app/Contents/MacOS/unzoo-service" --daemon' >&2
      echo "  (nohup ... & is not enough; it dies with the tool shell)" >&2
      exit 1
    fi
    for _ in $(seq 1 12); do
      if c --max-time 5 "$BASE/status" | grep -q '"browser_connected":true'; then
        echo "ready"; exit 0
      fi
      open -a "Unzoo Browser" 2>/dev/null
      sleep 2
    done
    echo "browser never connected" >&2; exit 1 ;;

  tabs)
    c "$BASE/tabs" | py '
import json,sys
for t in json.load(sys.stdin).get("data",{}).get("tabs",[]):
    print(t.get("tab_id"), "|", (t.get("title") or "")[:44], "|", (t.get("url") or "")[:90])' ;;

  # Print the id of the first tab whose URL contains the argument. Prefer an
  # already-open, already-signed-in console tab over creating a new one: the
  # session that matters usually lives in the user's own profile.
  find-tab)
    c "$BASE/tabs" | NEEDLE="${1:?usage: find-tab <url-substring>}" py '
import json,os,sys
n=os.environ["NEEDLE"]
for t in json.load(sys.stdin).get("data",{}).get("tabs",[]):
    if n in (t.get("url") or ""): print(t["tab_id"]); break
else: sys.exit("no tab whose URL contains "+n)' ;;

  new)
    url="${1:?usage: new <url>}"
    TAB=$(post tabs/create "$(jbody url="$url")" | py 'import json,sys;print(json.load(sys.stdin)["data"]["tab_id"])')
    [ -n "$TAB" ] || { echo "tabs/create gave no tab_id" >&2; exit 1; }
    # tabs/create returns before the navigation commits and sometimes does not
    # navigate at all — the tab sits on about:blank and every later read looks
    # like a broken page. Drive it explicitly and wait for the document.
    post navigate "$(jbody url="$url")" >/dev/null
    "$0" -t "$TAB" wait-load >/dev/null || { echo "tab $TAB never loaded $url" >&2; exit 1; }
    echo "$TAB" ;;

  # Poll until the document is parsed. Console pages then keep rendering, so
  # follow this with `wait-for <selector>` for anything you intend to click.
  wait-load) need_tab || exit 1
    for _ in $(seq 1 30); do
      r=$(post evaluate "$(jbody expression='document.readyState+"|"+location.href')" \
            | py 'import json,sys
d=json.load(sys.stdin)
print(d.get("data",{}).get("result") or "")')
      case "$r" in
        interactive\|*|complete\|*) echo "${r#*|}"; exit 0 ;;
      esac
      sleep 1
    done
    echo "still not loaded" >&2; exit 1 ;;

  wait-for) need_tab || exit 1
    sel="${1:?usage: wait-for <css-selector> [seconds]}"
    for _ in $(seq 1 "${2:-30}"); do
      if [ "$("$0" -t "$TAB" exists "$sel")" = "true" ]; then echo "found $sel"; exit 0; fi
      sleep 1
    done
    echo "timed out waiting for $sel" >&2; exit 1 ;;

  exists) need_tab || exit 1
    post evaluate "$(jbody expression="!!document.querySelector($(py 'import json,sys;print(json.dumps(sys.argv[1]))' "${1:?usage: exists <css-selector>}"))")" \
      | py 'import json,sys
d=json.load(sys.stdin)
print("true" if d.get("data",{}).get("result") is True else "false")' ;;

  nav)  need_tab || exit 1; post navigate "$(jbody url="${1:?usage: nav <url>}")" >/dev/null; echo "-> $1" ;;

  url)  need_tab || exit 1; post evaluate "$(jbody expression='location.href')" | py '
import json,sys
d=json.load(sys.stdin)
if "data" not in d: sys.exit("unzoo: " + json.dumps(d)[:200])
print(d["data"]["result"])' ;;

  text) need_tab || exit 1; post get-text "$(jbody)" | py '
import json,sys
r=json.load(sys.stdin).get("data",{}).get("result",{})
if isinstance(r,dict) and not r.get("found"): sys.exit("page had no text")
print(r.get("text","") if isinstance(r,dict) else r)' ;;

  html) need_tab || exit 1; post get-html "$(jbody)" | py '
import json,sys
r=json.load(sys.stdin).get("data",{}).get("result",{})
print(r.get("html","") if isinstance(r,dict) else r)' ;;

  # Result must be JSON-serializable — wrap objects in JSON.stringify yourself.
  eval) need_tab || exit 1; post evaluate "$(jbody expression="${1:?usage: eval <js>}")" | py '
import json,sys
d=json.load(sys.stdin)
if "error" in d: sys.exit("evaluate error: "+str(d["error"]))
print(json.dumps(d["data"]["result"], ensure_ascii=False))' ;;

  # Viewport-centre coordinates of a selector. Consoles built on closed shadow
  # DOM (Partner Center's he-button, Play's mdc-button) cannot be clicked by
  # selector — you measure, then click the point.
  # Viewport-centre coordinates of a selector. Consoles built on closed shadow
  # DOM (Partner Center's he-button, Play's mdc-button) cannot be clicked by
  # selector — you measure, then click the point.
  box)  need_tab || exit 1
    sel="${1:?usage: box <css-selector>}"
    post evaluate "$(eval_body "$sel" <<'JS'
const e = document.querySelector(SEL);
if (!e) return null;
const r = e.getBoundingClientRect();
return JSON.stringify({x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2),
                       w: Math.round(r.width), h: Math.round(r.height),
                       visible: r.width > 0 && r.height > 0});
JS
)" | SEL="$sel" py '
import json,os,sys
r = json.load(sys.stdin).get("data",{}).get("result")
if not r or r == "null":
    sys.exit("no element matches " + os.environ["SEL"])
print(r if isinstance(r,str) else json.dumps(r))' ;;

  activate) need_tab || exit 1; post tabs/activate "$(jbody)" >/dev/null; echo "activated $TAB" ;;

  # ★ A coordinate click is injected input: it goes to whatever tab is ACTIVE,
  # not to tab_id, which the endpoint accepts and ignores. Clicking a
  # background tab silently does nothing — no error, no event, and the page
  # looks like it ignored you. So activate first, every time.
  click) need_tab || exit 1
    post tabs/activate "$(jbody)" >/dev/null
    post click "$(jbody x="${1:?usage: click <x> <y>}" y="${2:?}")" >/dev/null
    echo "clicked $1,$2" ;;

  # Find a clickable by its visible text and click its centre. Partner Center
  # and Play both build their controls out of custom elements with closed
  # shadow roots — `he-button`, `mdc-button` — which cannot be clicked through
  # a selector. But the host element is in the light DOM with its text and its
  # geometry, so text is the reliable handle. Beats reading coordinates off a
  # screenshot, which is what the runbooks used to say.
  find-text) need_tab || exit 1
    post evaluate "$(eval_body "${1-}" <<'JS'
const hits = [...document.querySelectorAll("he-button,button,a,[role=button],mdc-button")]
  .map(e => {
    const r = e.getBoundingClientRect();
    return {text: (e.textContent || "").trim(), tag: e.tagName,
            x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2),
            w: Math.round(r.width), h: Math.round(r.height)};
  })
  .filter(o => o.w > 0 && o.h > 0 && o.x > 0 && o.text.includes(SEL));
return JSON.stringify(hits.slice(0, 10));
JS
)" | SEL="${1-}" py '
import json,os,sys
d = json.load(sys.stdin)
if "data" not in d: sys.exit("unzoo: " + json.dumps(d)[:200])
hits = json.loads(d["data"]["result"])
if not hits:
    sys.exit("nothing clickable contains " + repr(os.environ["SEL"]))
for h in hits:
    print(json.dumps(h, ensure_ascii=False))' ;;

  click-text) need_tab || exit 1
    hit=$("$0" -t "$TAB" find-text "${1:?usage: click-text <text>}" | head -1) || exit 1
    xy=$(py 'import json,sys;h=json.loads(sys.argv[1]);print(h["x"],h["y"])' "$hit")
    post tabs/activate "$(jbody)" >/dev/null
    post click "$(jbody x="${xy% *}" y="${xy#* }")" >/dev/null
    echo "clicked $(py 'import json,sys;print(json.loads(sys.argv[1])["text"][:40])' "$hit") at $xy" ;;

  click-el) need_tab || exit 1
    sel="${1:?usage: click-el <css-selector>}"
    b=$("$0" -t "$TAB" box "$sel") || exit 1
    xy=$(py '
import json,sys
b=json.loads(sys.argv[1])
if not b.get("visible"): sys.exit("element has zero size; scroll it into view first")
print(b["x"], b["y"])' "$b") || exit 1
    post tabs/activate "$(jbody)" >/dev/null
    post click "$(jbody x="${xy% *}" y="${xy#* }")" >/dev/null
    echo "clicked $sel at $xy" ;;

  # Two-step on purpose. Setting files on the input does NOT start the upload:
  # Angular/React consoles listen for `change`, which set_input_files does not
  # fire. Without the dispatch the file sits in the input and nothing happens.
  upload) need_tab || exit 1
    sel="${1:?usage: upload <input-selector> <file>}"; f="${2:?}"
    [ -f "$f" ] || { echo "no such file: $f" >&2; exit 1; }
    post set_input_files "$(SEL="$sel" F="$f" TAB="$TAB" py '
import json, os
print(json.dumps({"selector": os.environ["SEL"],
                  "file_paths": [os.path.abspath(os.environ["F"])],
                  "trusted": True,
                  "tab_id": int(os.environ["TAB"])}))')" | py '
import json,sys
d=json.load(sys.stdin)
if not d.get("success"): sys.exit("set_input_files failed: "+json.dumps(d)[:300])
print("files set")'
    post evaluate "$(eval_body "$sel" <<'JS'
const e = document.querySelector(SEL);
if (!e) return "no input";
e.dispatchEvent(new Event("change", {bubbles: true}));
return "change dispatched";
JS
)" | py '
import json,sys
print(json.load(sys.stdin).get("data",{}).get("result"))'
    # Do NOT read back e.files.length to decide whether this worked. Angular
    # and React uploaders take the FileList and clear the input in the same
    # tick, so a successful attach reads back as zero files. Confirm on the
    # page instead — an upload row, a progress bar, a version number. Retrying
    # because the count looked wrong uploads the file twice, which is a
    # duplicate you then have to delete.
    echo "    (verify on the page, not by reading the input back — see above)" ;;

  shot) need_tab || exit 1
    out="${1:-/tmp/unzoo-shot.png}"
    post screenshot "$(jbody)" | OUT="$out" py '
import base64,json,os,sys
r=json.load(sys.stdin)
if "data" not in r: sys.exit("unzoo: " + json.dumps(r)[:200])
d=r["data"]
open(os.environ["OUT"],"wb").write(base64.b64decode(d["image_base64"]))
print(os.environ["OUT"], d.get("width"), "x", d.get("height"))' ;;

  *)
    sed -n '2,30p' "$0" | sed 's/^# \{0,1\}//'
    exit 2 ;;
esac

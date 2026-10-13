#!/usr/bin/env python3
"""Add (or update) a sponsor or promoter, then regenerate the README.

    # A promoter who registered on GitHub (the 📣 Promotion form):
    scripts/add-supporter.py --from-issue 412            # preview
    scripts/add-supporter.py --from-issue 412 --apply    # write + README
    scripts/add-supporter.py --from-issue 412 --apply --close   # + thank & close the issue

    # Registered on Gitee or by email — same fields by hand:
    scripts/add-supporter.py promoter --name 小明 --url https://… --platform 少数派 \\
        [--ref xiaoming] [--github xiaoming] [--quote "…"] [--featured] --apply

    # A sponsor (with their consent to be named):
    scripts/add-supporter.py sponsor --name "tyysoft(太阳雨)" [--github …] [--url …] --apply

Everything lands in web/public/sponsors.json — the one file the website
(homepage, /thanks, /promote), the README and the app's About dialog read.
After --apply: commit and push; the site redeploys itself and the app picks
the names up at runtime (no release needed). Rules for what counts as
promotion: solomd.app/promote. Check the link yourself before applying.
"""
import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DATA = ROOT / "web/public/sponsors.json"
REF_RE = re.compile(r"^[a-z0-9-]{1,40}$")
GH_RE = re.compile(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$")
URL_RE = re.compile(r"^https?://\S+$")

# Labels of the GitHub issue form (.github/ISSUE_TEMPLATE/promotion.yml),
# matched by their English start.
FORM = {
    "name": "Name to show",
    "url": "Link to your piece",
    "platform": "Platform",
    "ref": "Your ref code",
    "github": "GitHub username",
    "quote": "A sentence we may quote",
}


def parse_issue(number: int) -> dict:
    raw = subprocess.run(
        ["gh", "issue", "view", str(number), "--json", "body,labels,state,url"],
        check=True, capture_output=True, text=True, cwd=ROOT,
    ).stdout
    issue = json.loads(raw)
    sections = re.split(r"^### ", issue["body"] or "", flags=re.M)
    fields = {}
    for sec in sections:
        head, _, value = sec.partition("\n")
        value = value.strip()
        if value in ("", "_No response_"):
            value = None
        for key, label in FORM.items():
            if head.strip().startswith(label):
                fields[key] = value
    if "- [X]" not in (issue["body"] or "") and "- [x]" not in (issue["body"] or ""):
        sys.exit(f"#{number}: the consent box is not ticked — ask before listing them.")
    fields["_issue"] = issue
    return fields


def clean(kind: str, f: dict) -> dict:
    name = (f.get("name") or "").strip()
    if not name or len(name) > 40:
        sys.exit("name is required (max 40 characters)")
    entry = {"name": name, "since": f.get("since") or datetime.date.today().strftime("%Y-%m")}
    gh = (f.get("github") or "").strip().lstrip("@")
    if gh:
        if not GH_RE.match(gh):
            sys.exit(f"not a GitHub username: {gh!r}")
        entry["github"] = gh
    url = (f.get("url") or "").strip()
    if kind == "promoter":
        if not URL_RE.match(url):
            sys.exit(f"promoter needs the link to their piece (http/https): {url!r}")
        platform = (f.get("platform") or "").strip()
        if not platform:
            sys.exit("promoter needs a platform (Blog, Bilibili, 少数派, …)")
        entry.update(platform=platform[:40], url=url)
        ref = (f.get("ref") or "").strip().lower()
        if ref:
            if not REF_RE.match(ref):
                sys.exit(f"ref code must be lowercase letters, digits or hyphens: {ref!r}")
            entry["ref"] = ref
        quote = (f.get("quote") or "").strip()
        if quote:
            entry["quote"] = quote[:200]
        if f.get("featured"):
            if not quote:
                sys.exit("--featured needs a --quote to show on the homepage")
            entry["featured"] = True
    elif url:
        if not URL_RE.match(url):
            sys.exit(f"not a link: {url!r}")
        entry["url"] = url
    return entry


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("kind", nargs="?", choices=["promoter", "sponsor"])
    ap.add_argument("--from-issue", type=int, help="import a 📣 Promotion registration from GitHub")
    ap.add_argument("--name")
    ap.add_argument("--url")
    ap.add_argument("--platform")
    ap.add_argument("--ref")
    ap.add_argument("--github")
    ap.add_argument("--quote")
    ap.add_argument("--featured", action="store_true")
    ap.add_argument("--since", help="YYYY-MM (default: this month)")
    ap.add_argument("--apply", action="store_true", help="write the file and regenerate the README")
    ap.add_argument("--close", action="store_true", help="with --from-issue: thank on the issue and close it")
    a = ap.parse_args()

    issue = None
    if a.from_issue:
        fields = parse_issue(a.from_issue)
        issue = fields.pop("_issue")
        kind = "promoter"
        for k in ("name", "url", "platform", "ref", "github", "quote"):
            if getattr(a, k):
                fields[k] = getattr(a, k)  # a flag overrides what the form said
        fields["featured"] = a.featured
        fields["since"] = a.since
    else:
        if not a.kind:
            ap.error("give a kind (promoter / sponsor) or --from-issue")
        kind = a.kind
        fields = {k: getattr(a, k) for k in ("name", "url", "platform", "ref", "github", "quote", "featured", "since")}

    entry = clean(kind, fields)
    data = json.loads(DATA.read_text())
    key = "promoters" if kind == "promoter" else "sponsors"
    lst = data.setdefault(key, [])
    existing = next((i for i, e in enumerate(lst) if e["name"] == entry["name"] and
                     (kind == "sponsor" or e.get("url") == entry.get("url"))), None)
    action = "update" if existing is not None else "add"
    print(f"{action} {kind}:")
    print(json.dumps(entry, ensure_ascii=False, indent=2))
    if not a.apply:
        print("\n(preview only — re-run with --apply to write it)")
        return

    if existing is not None:
        lst[existing] = {**lst[existing], **entry}
    else:
        lst.append(entry)
    DATA.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")
    subprocess.run([sys.executable, str(ROOT / "scripts/update-sponsors-readme.py")], check=True, cwd=ROOT)
    print(f"\nwrote {DATA.relative_to(ROOT)} and the README.")

    if issue and a.close:
        body = (
            f"Thank you, {entry['name']}! You're now on the list — the [thanks page](https://solomd.app/thanks), "
            "the app's About dialog and the README contributors table (live once this is deployed).\n\n"
            f"谢谢你，{entry['name']}！已加入致谢名单：[鸣谢页](https://solomd.app/zh/thanks)、应用「关于」窗口和 README 贡献者表格（部署后可见）。"
        )
        subprocess.run(["gh", "issue", "comment", str(a.from_issue), "--body", body], check=True, cwd=ROOT)
        subprocess.run(["gh", "issue", "close", str(a.from_issue)], check=True, cwd=ROOT)
        print(f"thanked and closed #{a.from_issue}")
    print("next: git add web/public/sponsors.json README*.md && git commit && git push")


if __name__ == "__main__":
    main()

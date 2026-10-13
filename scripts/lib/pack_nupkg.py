#!/usr/bin/env python3
"""Pack a Chocolatey .nupkg without choco.

    pack_nupkg.py <package-dir> <out.nupkg>

<package-dir> holds exactly one .nuspec plus the files it ships (tools/...).

Why not `nuget pack`: NuGet 7 refuses a nuspec that carries Chocolatey's own
metadata (projectSourceUrl, docsUrl, bugTrackerUrl, packageSourceUrl) with
"The schema version of 'solomd' is incompatible", and `choco` itself does not
run on macOS. A .nupkg is an OPC zip: the nuspec at the root, the payload,
[Content_Types].xml, _rels/.rels and a core-properties part. This writes the
same layout `choco pack` produces.
"""
import os
import sys
import uuid
import zipfile
import xml.etree.ElementTree as ET

src, out = sys.argv[1], sys.argv[2]
nuspecs = [f for f in os.listdir(src) if f.endswith(".nuspec")]
assert len(nuspecs) == 1, f"expected one .nuspec in {src}, found {nuspecs}"
nuspec = nuspecs[0]

tree = ET.parse(os.path.join(src, nuspec))
ns = {"n": tree.getroot().tag.split("}")[0].strip("{")}
meta = tree.getroot().find("n:metadata", ns)
pkg_id = meta.find("n:id", ns).text
version = meta.find("n:version", ns).text
authors = meta.find("n:authors", ns).text
desc = (meta.find("n:description", ns).text or "").strip()
tags = meta.find("n:tags", ns)
assert "{{" not in version, "unfilled version placeholder"

payload = []
for root, _, files in os.walk(src):
    for f in files:
        full = os.path.join(root, f)
        rel = os.path.relpath(full, src).replace(os.sep, "/")
        if rel == nuspec or f == ".DS_Store":
            continue
        payload.append((full, rel))
payload.sort(key=lambda p: p[1])

exts = sorted({os.path.splitext(r)[1].lstrip(".").lower() for _, r in payload} | {"nuspec"})
psmdcp = f"package/services/metadata/core-properties/{uuid.uuid4().hex}.psmdcp"

content_types = (
    '<?xml version="1.0" encoding="utf-8"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />'
    '<Default Extension="psmdcp" ContentType="application/vnd.openxmlformats-package.core-properties+xml" />'
    + "".join(f'<Default Extension="{e}" ContentType="application/octet" />' for e in exts if e)
    + "</Types>"
)
rels = (
    '<?xml version="1.0" encoding="utf-8"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    f'<Relationship Type="http://schemas.microsoft.com/packaging/2010/07/manifest" Target="/{nuspec}" Id="R{uuid.uuid4().hex[:16]}" />'
    f'<Relationship Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="/{psmdcp}" Id="R{uuid.uuid4().hex[:16]}" />'
    "</Relationships>"
)


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


core = (
    '<?xml version="1.0" encoding="utf-8"?>'
    '<coreProperties xmlns:dc="http://purl.org/dc/elements/1.1/" '
    'xmlns:dcterms="http://purl.org/dc/terms/" '
    'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" '
    'xmlns="http://schemas.openxmlformats.org/package/2006/metadata/core-properties">'
    f"<dc:creator>{esc(authors)}</dc:creator>"
    f"<dc:description>{esc(desc)}</dc:description>"
    f"<dc:identifier>{esc(pkg_id)}</dc:identifier>"
    f"<version>{esc(version)}</version>"
    f"<keywords>{esc(tags.text if tags is not None else '')}</keywords>"
    "<lastModifiedBy>pack_nupkg.py</lastModifiedBy>"
    "</coreProperties>"
)

with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("_rels/.rels", rels)
    z.write(os.path.join(src, nuspec), nuspec)
    for full, rel in payload:
        z.write(full, rel)
    z.writestr(psmdcp, core)
    z.writestr("[Content_Types].xml", content_types)

print(f"{out}: {pkg_id} {version}, {len(payload)} payload file(s)")

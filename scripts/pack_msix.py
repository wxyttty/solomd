#!/usr/bin/env python3
"""
Build a Microsoft Store MSIX from an MSI payload, on macOS, with no Windows
tooling and no third-party packages.

An MSIX is an OPC zip and the Store re-signs whatever we upload, so none of
signtool, makeappx or a certificate is needed — only the zip laid out the way
the Store's validator expects. The parts it checks are the manifest, the
content-types map, and a block map of SHA-256 hashes over 64 KB blocks of every
payload file.

    python3 scripts/pack_msix.py --msi dist/SoloMD_4.12.0_x64_en-US.msi \\
                                 --version 4.12.0 --arch x64 --out /tmp/SoloMD.msix

This script has now been written twice, because the first copy lived in a temp
directory and was cleaned up between releases. It lives in the repository.
"""

import argparse
import base64
import hashlib
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
from xml.sax.saxutils import escape

# Identity is not ours to choose: the Store assigns it, and a package whose
# identity does not match the product is rejected. These were read back from
# the published package (displaycatalog's PublisherCertificateName and
# PackageFamilyName) and the publisher string is verified against the family
# name hash below — get the case wrong and Windows derives a different hash.
IDENTITY_NAME = "zhitong.SoloMD"
PUBLISHER = "CN=359F6F65-9E32-4216-A4A1-9AF570A7877A"
PUBLISHER_DISPLAY = "zhitong"
EXPECTED_FAMILY_HASH = "3vmxy07xh2v3m"
DISPLAY_NAME = "SoloMD"
DESCRIPTION = "One file. One window. Just write."
BACKGROUND_COLOR = "#121110"

BLOCK = 64 * 1024

ICONS = {
    "Assets/Square44x44Logo.png": "app/src-tauri/icons/Square44x44Logo.png",
    "Assets/Square150x150Logo.png": "app/src-tauri/icons/Square150x150Logo.png",
    "Assets/StoreLogo.png": "app/src-tauri/icons/StoreLogo.png",
}

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="xml" ContentType="application/vnd.ms-appx.manifest+xml" />
  <Default Extension="png" ContentType="image/png" />
  <Default Extension="exe" ContentType="application/octet-stream" />
  <Default Extension="ico" ContentType="application/octet-stream" />
  <Default Extension="dic" ContentType="application/octet-stream" />
  <Default Extension="aff" ContentType="application/octet-stream" />
  <Override PartName="/AppxBlockMap.xml" ContentType="application/vnd.ms-appx.blockmap+xml" />
</Types>
"""


def family_hash(publisher: str) -> str:
    """Windows derives the family-name suffix from the publisher string:
    UTF-16LE, SHA-256, first 8 bytes, base32 over Crockford's alphabet (no
    i, l, o, u), 13 characters."""
    digest = hashlib.sha256(publisher.encode("utf-16-le")).digest()[:8]
    bits = "".join(f"{b:08b}" for b in digest) + "0"
    alphabet = "0123456789abcdefghjkmnpqrstvwxyz"
    return "".join(alphabet[int(bits[i:i + 5], 2)] for i in range(0, 65, 5))


#: COFF machine types, keyed by the MSIX ProcessorArchitecture they belong to.
PE_MACHINE = {"x64": 0x8664, "arm64": 0xAA64, "x86": 0x014C}


def pe_machine(path: str) -> int:
    """Read a PE file's COFF machine type: e_lfanew at 0x3C points at the
    signature, and the machine word follows it."""
    with open(path, "rb") as fh:
        fh.seek(0x3C)
        offset = struct.unpack("<I", fh.read(4))[0]
        fh.seek(offset)
        if fh.read(4) != b"PE\0\0":
            raise ValueError("not a PE image")
        return struct.unpack("<H", fh.read(2))[0]


def check_architecture(payload, arch: str):
    """The manifest declares an architecture and nothing downstream checks it
    against the binaries. Shipping an x64 build labelled arm64 would install
    and then fail to run, on machines we do not have — so it is checked here."""
    want = PE_MACHINE[arch]
    for rel, full in payload:
        if not rel.lower().endswith(".exe"):
            continue
        got = pe_machine(full)
        if got != want:
            names = {v: k for k, v in PE_MACHINE.items()}
            sys.exit(f"ERROR: {rel} is {names.get(got, hex(got))} but the package "
                     f"declares {arch}. Wrong MSI for --arch {arch}.")
        print(f"    {rel}: {arch} confirmed")


def manifest(version: str, arch: str) -> str:
    # The fourth segment must be 0. A revision other than zero is rejected
    # outright ("revision number other than zero").
    return f"""<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">
  <Identity Name="{IDENTITY_NAME}"
            Publisher="{PUBLISHER}"
            Version="{version}.0"
            ProcessorArchitecture="{arch}" />
  <Properties>
    <DisplayName>{escape(DISPLAY_NAME)}</DisplayName>
    <PublisherDisplayName>{escape(PUBLISHER_DISPLAY)}</PublisherDisplayName>
    <Logo>Assets\\StoreLogo.png</Logo>
    <Description>{escape(DESCRIPTION)}</Description>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0"
                        MaxVersionTested="10.0.22621.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="SoloMD" Executable="SoloMD.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="{escape(DISPLAY_NAME)}"
        Description="{escape(DESCRIPTION)}"
        BackgroundColor="{BACKGROUND_COLOR}"
        Square150x150Logo="Assets\\Square150x150Logo.png"
        Square44x44Logo="Assets\\Square44x44Logo.png" />
      <Extensions>
        <uap:Extension Category="windows.fileTypeAssociation">
          <uap:FileTypeAssociation Name="markdown">
            <uap:DisplayName>Markdown Document</uap:DisplayName>
            <uap:SupportedFileTypes>
              <uap:FileType>.md</uap:FileType>
              <uap:FileType>.markdown</uap:FileType>
              <uap:FileType>.txt</uap:FileType>
            </uap:SupportedFileTypes>
          </uap:FileTypeAssociation>
        </uap:Extension>
      </Extensions>
    </Application>
  </Applications>
  <Capabilities>
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"""


def blockmap(files) -> str:
    """One <File> per payload part, each split into 64 KB blocks hashed with
    SHA-256. Because every entry is STORED, a block needs no compressed size —
    which is the only reason this is writable without a zip library that
    reports per-block compressed lengths. LfhSize is the local file header:
    30 bytes plus the UTF-8 name."""
    out = ['<?xml version="1.0" encoding="UTF-8"?>',
           '<BlockMap xmlns="http://schemas.microsoft.com/appx/2010/blockmap" '
           'HashMethod="http://www.w3.org/2001/04/xmlenc#sha256">']
    for name, data in files:
        win = name.replace("/", "\\")
        lfh = 30 + len(name.encode("utf-8"))
        out.append(f'  <File Name="{escape(win)}" Size="{len(data)}" LfhSize="{lfh}">')
        for i in range(0, len(data), BLOCK) or [0]:
            block = data[i:i + BLOCK]
            digest = base64.b64encode(hashlib.sha256(block).digest()).decode()
            out.append(f'    <Block Hash="{digest}" />')
        if not data:
            out.append(f'    <Block Hash="{base64.b64encode(hashlib.sha256(b"").digest()).decode()}" />')
        out.append("  </File>")
    out.append("</BlockMap>")
    return "\n".join(out) + "\n"


def extract_msi(msi: str, into: str):
    if not shutil.which("msiextract"):
        sys.exit("ERROR: msiextract not found — `brew install msitools`.")
    subprocess.run(["msiextract", os.path.abspath(msi)], cwd=into,
                   check=True, capture_output=True)
    root = os.path.join(into, "PFiles", "SoloMD")
    if not os.path.isdir(root):
        sys.exit(f"ERROR: {msi} did not unpack to PFiles/SoloMD — layout changed?")
    payload = []
    for dirpath, _, names in os.walk(root):
        for n in sorted(names):
            full = os.path.join(dirpath, n)
            payload.append((os.path.relpath(full, root).replace(os.sep, "/"), full))
    return sorted(payload)


def main():
    p = argparse.ArgumentParser(description="Build a Store MSIX from an MSI.")
    p.add_argument("--msi", required=True)
    p.add_argument("--version", required=True, help="three-part marketing version, e.g. 4.12.0")
    p.add_argument("--arch", default="x64", choices=("x64", "arm64", "x86"))
    p.add_argument("--out", required=True)
    args = p.parse_args()

    if args.version.count(".") != 2:
        sys.exit("ERROR: --version wants three parts; the fourth is always 0.")

    got = family_hash(PUBLISHER)
    if got != EXPECTED_FAMILY_HASH:
        sys.exit(f"ERROR: the publisher string hashes to {got}, but the published "
                 f"package family name ends in {EXPECTED_FAMILY_HASH}. The identity "
                 f"is wrong and the upload would be rejected.")
    print(f"==> Identity {IDENTITY_NAME}_{got} ({args.arch}) verified against the store")

    with tempfile.TemporaryDirectory() as tmp:
        payload = extract_msi(args.msi, tmp)
        print(f"==> {len(payload)} payload file(s) from {os.path.basename(args.msi)}")
        check_architecture(payload, args.arch)

        parts = []
        for rel, full in payload:
            with open(full, "rb") as fh:
                parts.append((rel, fh.read()))
        for target, src in ICONS.items():
            if not os.path.exists(src):
                sys.exit(f"ERROR: missing icon {src}")
            with open(src, "rb") as fh:
                parts.append((target, fh.read()))

        man = manifest(args.version, args.arch).encode("utf-8")
        ctypes = CONTENT_TYPES.encode("utf-8")
        # The block map covers the payload and the manifest, not itself and not
        # the content-types part.
        bmap = blockmap(parts + [("AppxManifest.xml", man)]).encode("utf-8")

        os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
        # Order matters to the validator: payload, manifest, content types,
        # block map. Everything STORED — a deflated part invalidates the
        # block map, whose hashes are over the uncompressed bytes.
        with zipfile.ZipFile(args.out, "w", zipfile.ZIP_STORED) as z:
            for name, data in parts:
                z.writestr(name, data)
            z.writestr("AppxManifest.xml", man)
            z.writestr("[Content_Types].xml", ctypes)
            z.writestr("AppxBlockMap.xml", bmap)

    size = os.path.getsize(args.out)
    print(f"==> {args.out} ({size / 1024 / 1024:.1f} MB), version {args.version}.0")


if __name__ == "__main__":
    main()

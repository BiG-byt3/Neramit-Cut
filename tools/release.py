"""Build signed GitHub release assets for Neramit Cut.

  python tools/release.py keygen   Create a signing key ONCE (never upload it).
  python tools/release.py pub      Print the public half of the signing key.
  python tools/release.py sign     Package the freshly built app and installer into dist/<version>/
                                   (these are the files to upload to the GitHub release).
  python tools/release.py verify   Verify the packaged hashes and signatures.
  python tools/release.py publish  Upload a draft, then publish when complete.

The same studio key also signs Neramit Planner releases; the product name in each
signature keeps one product from accepting the other's files.

Requires cryptography (python -m pip install cryptography). The private key stays
in %USERPROFILE%/.neramit-release/update-signing-key.pem, or NERAMIT_RELEASE_KEY.
Publish uses GITHUB_TOKEN with Contents: read and write. NERAMIT_RELEASE_TARGET
can pin the release tag to a commit SHA (default: main).

The updater trusts ECDSA P-256/SHA-256 signatures over
"NeramitCut|<version>|<installer-sha256>" and
"NeramitCutPortable|<version>|<portable-sha256>". The private key must match
PUBLIC_KEY in src/update.rs. Never replace that key after shipping the app.
"""

import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import decode_dss_signature, encode_dss_signature

REPO = "BiG-byt3/Neramit-Cut"
ROOT = Path(__file__).resolve().parent.parent
KEY = Path(os.environ.get("NERAMIT_RELEASE_KEY", Path.home() / ".neramit-release" / "update-signing-key.pem"))
API_ROOT = f"https://api.github.com/repos/{REPO}"


def version():
    def read(path):
        match = re.search(r'^version\s*=\s*"([^"]+)"', path.read_text(encoding="utf-8"), re.M)
        if not match or not re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", match[1]) or any(int(part) > 0xFFFFFFFF for part in match[1].split(".")):
            raise ValueError(f"Expected a stable x.y.z version in {path}")
        return match[1]

    value = read(ROOT / "Cargo.toml")
    if read(ROOT / "installer" / "Cargo.toml") != value:
        raise ValueError("Application and installer versions must match.")
    return value


def public_key():
    match = re.search(r'const PUBLIC_KEY: &str = "([0-9a-fA-F]+)";', (ROOT / "src" / "update.rs").read_text(encoding="utf-8"))
    if not match:
        raise ValueError("Could not read PUBLIC_KEY in src/update.rs")
    return ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), b"\x04" + bytes.fromhex(match[1]))


def signing_key():
    key = serialization.load_pem_private_key(KEY.read_bytes(), None)
    if not isinstance(key, ec.EllipticCurvePrivateKey) or not isinstance(key.curve, ec.SECP256R1):
        raise ValueError("The update signing key must be an ECDSA P-256 key.")
    if key.public_key().public_numbers() != public_key().public_numbers():
        raise ValueError("Signing key does not match PUBLIC_KEY in src/update.rs; installed copies would reject this release.")
    return key


def keygen():
    if KEY.exists():
        raise ValueError(f"{KEY} already exists: a new key would lock out installed copies.")
    key = ec.generate_private_key(ec.SECP256R1())
    KEY.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation prevents accidental replacement, including concurrent runs.
    with KEY.open("xb") as output:
        output.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    pub()


def pub():
    key = serialization.load_pem_private_key(KEY.read_bytes(), None)
    raw = key.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)[1:]
    print("PUBLIC_KEY (X || Y):", raw.hex())


def asset_names(v):
    return [f"NeramitCut-Setup-{v}.exe", f"NeramitCut-{v}.exe", "latest.json", "SHA256SUMS.txt", "release-notes.txt"]


def sign():
    v = version()
    key = signing_key()
    sources = [
        ("", "NeramitCut", ROOT / "installer" / "target" / "release" / "NeramitCut-Setup.exe", f"NeramitCut-Setup-{v}.exe"),
        ("portable_", "NeramitCutPortable", ROOT / "target" / "release" / "NeramitCut.exe", f"NeramitCut-{v}.exe"),
    ]
    # Read and validate every required input before touching an existing package.
    payloads = [(prefix, domain, path, name, path.read_bytes()) for prefix, domain, path, name in sources]
    for _, _, path, _, data in payloads:
        if not data.startswith(b"MZ") or len(data) > 512 * 1024 * 1024:
            raise ValueError(f"Expected a Windows executable up to 512 MiB: {path}")
    notes = ROOT / "dist" / f"notes-{v}.txt"
    text = notes.read_text(encoding="utf-8-sig") if notes.exists() else f"Neramit Cut {v}"
    parts = re.split(r"\n\s*---\s*\n", text, maxsplit=1)
    latest = {"version": v, "notes_en": parts[0].strip(), "notes_th": parts[1].strip() if len(parts) > 1 else ""}
    out = ROOT / "dist" / v
    out.mkdir(parents=True, exist_ok=True)
    for prefix, domain, _, name, data in payloads:
        digest = hashlib.sha256(data).hexdigest()
        der = key.sign(f"{domain}|{v}|{digest}".encode(), ec.ECDSA(hashes.SHA256()))
        r, s = decode_dss_signature(der)
        latest.update({f"{prefix}size": len(data), f"{prefix}sha256": digest, f"{prefix}signature": (r.to_bytes(32, "big") + s.to_bytes(32, "big")).hex()})
        (out / name).write_bytes(data)
    (out / "latest.json").write_text(json.dumps(latest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (out / "release-notes.txt").write_text(text, encoding="utf-8")
    sums = "".join(f"{hashlib.sha256((out / name).read_bytes()).hexdigest()}  {name}\n" for name in asset_names(v) if name != "SHA256SUMS.txt")
    (out / "SHA256SUMS.txt").write_text(sums, encoding="utf-8")
    verify()


def verify():
    v = version()
    out = ROOT / "dist" / v
    latest = json.loads((out / "latest.json").read_text(encoding="utf-8"))
    if latest.get("version") != v:
        raise ValueError("Packaged manifest version differs from Cargo.toml.")
    pubkey = public_key()
    for prefix, domain, name in [("", "NeramitCut", f"NeramitCut-Setup-{v}.exe"), ("portable_", "NeramitCutPortable", f"NeramitCut-{v}.exe")]:
        data = (out / name).read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if not data.startswith(b"MZ") or len(data) != latest.get(f"{prefix}size") or digest != latest.get(f"{prefix}sha256"):
            raise ValueError(f"Packaged size or SHA-256 mismatch: {name}")
        sig = bytes.fromhex(latest.get(f"{prefix}signature", ""))
        if len(sig) != 64:
            raise ValueError(f"Invalid signature format: {name}")
        der = encode_dss_signature(int.from_bytes(sig[:32], "big"), int.from_bytes(sig[32:], "big"))
        pubkey.verify(der, f"{domain}|{v}|{digest}".encode(), ec.ECDSA(hashes.SHA256()))
    expected = "".join(f"{hashlib.sha256((out / name).read_bytes()).hexdigest()}  {name}\n" for name in asset_names(v) if name != "SHA256SUMS.txt")
    if (out / "SHA256SUMS.txt").read_text(encoding="utf-8") != expected:
        raise ValueError("SHA256SUMS.txt does not match the packaged assets.")
    print(f"Verified installer, portable executable and signed manifest: {out}")
    return latest


def api(method, url, token, body=None, ctype="application/json"):
    req = urllib.request.Request(url, data=body, method=method, headers={
        "Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json", "Content-Type": ctype,
        "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "neramit-release"})
    with urllib.request.urlopen(req, timeout=60) as response:
        return json.loads(response.read() or b"{}")


def publish():
    v = version()
    token = os.environ.get("GITHUB_TOKEN")
    if not token:
        raise ValueError("Set GITHUB_TOKEN first (repository Contents: read and write).")
    latest = verify()
    out = ROOT / "dist" / v
    body = f"{latest['notes_en']}\n\n{latest['notes_th']}".strip() or f"Neramit Cut {v}"
    # Resume a previous interrupted draft, but never replace a shipped release.
    rel = None
    page = 1
    while True:
        releases = api("GET", f"{API_ROOT}/releases?per_page=100&page={page}", token)
        rel = next((item for item in releases if item["tag_name"] == f"v{v}"), None)
        if rel or len(releases) < 100:
            break
        page += 1
    if rel and not rel["draft"]:
        raise ValueError(f"v{v} is already published. Increase both Cargo versions for a new release.")
    metadata = {"name": f"Neramit Cut {v}", "body": body, "draft": True,
                "target_commitish": os.environ.get("NERAMIT_RELEASE_TARGET", "main")}
    if rel:
        rel = api("PATCH", f"{API_ROOT}/releases/{rel['id']}", token, json.dumps(metadata).encode())
    else:
        metadata["tag_name"] = f"v{v}"
        rel = api("POST", f"{API_ROOT}/releases", token, json.dumps(metadata).encode())
    upload = rel["upload_url"].split("{")[0]
    assets = api("GET", f"{API_ROOT}/releases/{rel['id']}/assets?per_page=100", token)
    for name in asset_names(v):
        previous = next((asset for asset in assets if asset["name"] == name), None)
        if previous:
            api("DELETE", f"{API_ROOT}/releases/assets/{previous['id']}", token)
        ctype = "application/json" if name.endswith(".json") else "application/octet-stream" if name.endswith(".exe") else "text/plain; charset=utf-8"
        asset = api("POST", f"{upload}?{urllib.parse.urlencode({'name': name})}", token, (out / name).read_bytes(), ctype)
        if asset.get("state") != "uploaded" or asset.get("size") != (out / name).stat().st_size:
            raise ValueError(f"GitHub did not confirm the upload: {name}; release remains a draft.")
        print("Uploaded", name)
    # All required files exist before latest/download/latest.json can point here.
    rel = api("PATCH", f"{API_ROOT}/releases/{rel['id']}", token, json.dumps({"draft": False, "make_latest": "true"}).encode())
    print(rel["html_url"])


if __name__ == "__main__":
    commands = {"keygen": keygen, "pub": pub, "sign": sign, "verify": verify, "publish": publish}
    command = sys.argv[1] if len(sys.argv) == 2 else ""
    if command not in commands:
        print(__doc__)
        sys.exit(0 if len(sys.argv) == 1 else 2)
    try:
        commands[command]()
    except InvalidSignature:
        sys.exit("Release failed: manifest signature does not match the public key in src/update.rs.")
    except (OSError, ValueError, KeyError) as error:
        sys.exit(f"Release failed: {error}")

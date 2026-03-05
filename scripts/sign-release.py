#!/usr/bin/env python3
"""Generate a signed release manifest for tempo CLI auto-install.

Usage:
    python3 scripts/sign-release.py \\
        --key-file release.key \\
        --artifacts-dir artifacts/ \\
        --version 0.1.0 \\
        --base-url https://cli.tempo.xyz/extensions/tempo-wallet \\
        --output manifest.json

The key file contains the raw 32-byte Ed25519 private seed.
Generate one with: python3 scripts/sign-release.py --generate-key release.key
"""

import argparse
import base64
import hashlib
import json
import os
import sys


def check_nacl():
    try:
        import nacl.signing

        return nacl.signing
    except ImportError:
        print(
            "error: PyNaCl is required. Install with: pip3 install PyNaCl",
            file=sys.stderr,
        )
        sys.exit(1)


def generate_key(path):
    signing = check_nacl()
    seed = os.urandom(32)
    key = signing.SigningKey(seed)
    with open(path, "wb") as f:
        f.write(seed)
    os.chmod(path, 0o600)

    public_key_b64 = base64.b64encode(bytes(key.verify_key)).decode()
    print(f"Generated Ed25519 keypair")
    print(f"  Private seed: {path}")
    print(f"  Public key (base64): {public_key_b64}")
    print()
    print(
        f"Bake this public key into the tempo CLI launcher (src/launcher.rs PUBLIC_KEY constant)."
    )
    print(f"Keep {path} secret — it signs release binaries.")


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(8192)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def sign_file(path, signing_key):
    with open(path, "rb") as f:
        data = f.read()
    signed = signing_key.sign(data)
    # signed.signature is the raw 64-byte Ed25519 signature
    return base64.b64encode(signed.signature).decode()


def build_manifest(artifacts_dir, version, base_url, signing_key):
    base_url = base_url.rstrip("/")
    version_prefix = f"v{version}" if not version.startswith("v") else version

    binaries = {}
    for filename in sorted(os.listdir(artifacts_dir)):
        filepath = os.path.join(artifacts_dir, filename)
        if not os.path.isfile(filepath):
            continue
        if filename.endswith((".json", ".md", ".sh", ".txt", ".py")):
            continue

        checksum = sha256_file(filepath)
        signature = sign_file(filepath, signing_key)

        binaries[filename] = {
            "url": f"{base_url}/{version_prefix}/{filename}",
            "sha256": checksum,
            "signature": signature,
        }

        print(f"  signed {filename} (sha256: {checksum[:16]}...)")

    return {
        "version": version_prefix,
        "binaries": binaries,
    }


def main():
    parser = argparse.ArgumentParser(
        description="Generate a signed release manifest for tempo CLI"
    )
    parser.add_argument(
        "--generate-key",
        metavar="PATH",
        help="Generate a new Ed25519 keypair and exit",
    )
    parser.add_argument(
        "--key-file",
        metavar="PATH",
        help="Path to raw 32-byte Ed25519 private seed",
    )
    parser.add_argument(
        "--artifacts-dir",
        metavar="PATH",
        help="Directory containing built binaries",
    )
    parser.add_argument("--version", help="Release version (e.g., 0.1.0)")
    parser.add_argument(
        "--base-url",
        default="https://cli.tempo.xyz/extensions/tempo-wallet",
        help="Base URL for download URLs (e.g., https://cli.tempo.xyz/extensions/tempo-wallet)",
    )
    parser.add_argument(
        "--output", default="manifest.json", help="Output manifest path"
    )
    parser.add_argument(
        "--print-public-key",
        metavar="PATH",
        help="Print the public key for a given private key file",
    )

    args = parser.parse_args()
    signing = check_nacl()

    if args.generate_key:
        generate_key(args.generate_key)
        return

    if args.print_public_key:
        seed = open(args.print_public_key, "rb").read()
        key = signing.SigningKey(seed)
        print(base64.b64encode(bytes(key.verify_key)).decode())
        return

    if not args.key_file or not args.artifacts_dir or not args.version:
        parser.error("--key-file, --artifacts-dir, and --version are required")

    seed = open(args.key_file, "rb").read()
    if len(seed) != 32:
        print(
            f"error: key file must be exactly 32 bytes (got {len(seed)})",
            file=sys.stderr,
        )
        sys.exit(1)

    key = signing.SigningKey(seed)
    public_key_b64 = base64.b64encode(bytes(key.verify_key)).decode()

    print(f"Signing release {args.version}")
    print(f"  Public key: {public_key_b64}")
    print(f"  Artifacts: {args.artifacts_dir}")
    print()

    manifest = build_manifest(args.artifacts_dir, args.version, args.base_url, key)

    with open(args.output, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")

    print()
    print(f"Wrote {args.output} ({len(manifest['binaries'])} binaries)")


if __name__ == "__main__":
    main()

#!/usr/bin/env bash
# Add PostgreSQL's signed APT repository for the libpq 17 CI baseline.
#
# Ubuntu 24.04 ships libpq 16, while pkg.db streamed delivery directly links
# PQsetChunkedRowsMode and the nonblocking cancel API introduced in libpq 17.
# CI calls this before ci-apt-llvm.sh resolves its replayable archive set; a
# cache hit already contains the same PGDG-built libpq archives.
set -euo pipefail

readonly KEY_URL=https://www.postgresql.org/media/keys/ACCC4CF8.asc
readonly KEY_FINGERPRINT=B97B0AFCAA1A47F044F244A07FCC7D46ACCC4CF8
readonly KEYRING=/etc/apt/keyrings/align-pgdg.asc
readonly SOURCES=/etc/apt/sources.list.d/align-pgdg.list

codename="$(. /etc/os-release 2>/dev/null && printf '%s' "${VERSION_CODENAME:-}")" \
  || codename=""
if [[ -z "$codename" ]]; then
  echo "/etc/os-release declares no VERSION_CODENAME; cannot pick a PGDG suite" >&2
  exit 1
fi
architecture="$(dpkg --print-architecture)"

for tool in curl gpg; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "$tool is required to add the PostgreSQL APT repository" >&2
    exit 1
  }
done

workdir="$(mktemp -d)"
cleanup() { rm -rf "$workdir"; }
trap cleanup EXIT

curl -fsSL "$KEY_URL" -o "$workdir/pgdg.asc"
fingerprints="$(gpg --show-keys --with-colons "$workdir/pgdg.asc" 2>/dev/null \
  | awk -F: '$1 == "fpr" { print $10 }')"
if ! printf '%s\n' "$fingerprints" | grep -qxF "$KEY_FINGERPRINT"; then
  echo "PostgreSQL repository key does not carry $KEY_FINGERPRINT" >&2
  echo "  fingerprints offered: ${fingerprints:-none}" >&2
  exit 1
fi

sudo install -d -m 0755 /etc/apt/keyrings
sudo install -m 0644 "$workdir/pgdg.asc" "$KEYRING"
printf 'deb [arch=%s signed-by=%s] https://apt.postgresql.org/pub/repos/apt %s-pgdg main\n' \
  "$architecture" "$KEYRING" "$codename" | sudo tee "$SOURCES" >/dev/null
echo "configured the signed PGDG repository for $codename/$architecture"

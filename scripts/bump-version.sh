#!/usr/bin/env bash
#
# Обновляет версию пакета в Cargo.toml и Cargo.lock.
#
# Использование: scripts/bump-version.sh {patch|minor|major|none}
#
# При запуске в GitHub Actions (задан GITHUB_OUTPUT) дополнительно пишет
# outputs: version, tag, previous.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 {patch|minor|major|none}" >&2
  exit 2
fi

bump="$1"
case "$bump" in
  patch | minor | major | none) ;;
  *)
    echo "Неизвестный тип бампа: $bump (ожидается patch, minor, major или none)" >&2
    exit 2
    ;;
esac

manifest="Cargo.toml"
lockfile="Cargo.lock"
package="music-downloader"

current="$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' "$manifest" | head -n 1)"
if [[ -z "$current" ]]; then
  echo "Не удалось прочитать версию из $manifest" >&2
  exit 1
fi

IFS=. read -r major minor patch <<< "$current"

case "$bump" in
  major)
    major=$((major + 1))
    minor=0
    patch=0
    ;;
  minor)
    minor=$((minor + 1))
    patch=0
    ;;
  patch)
    patch=$((patch + 1))
    ;;
  none) ;;
esac

next="$major.$minor.$patch"
tag="v$next"

if [[ "$bump" != "none" ]]; then
  CURRENT="$current" NEXT="$next" perl -0pi -e \
    's/^version = "\Q$ENV{CURRENT}\E"$/version = "$ENV{NEXT}"/m' "$manifest"
fi

if [[ -f "$lockfile" ]]; then
  PACKAGE="$package" CURRENT="$current" NEXT="$next" perl -0pi -e '
    my $package = quotemeta $ENV{PACKAGE};
    my $current = quotemeta $ENV{CURRENT};
    s/(\[\[package\]\]\nname = "$package"\nversion = ")$current(")/$1$ENV{NEXT}$2/;
  ' "$lockfile"
fi

# Проверяем, что манифест и lock-файл согласованы.
if command -v cargo >/dev/null 2>&1; then
  cargo metadata --locked --no-deps --format-version 1 >/dev/null
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "version=$next"
    echo "tag=$tag"
    echo "previous=$current"
  } >> "$GITHUB_OUTPUT"
fi

echo "Версия: $current -> $next ($tag)"

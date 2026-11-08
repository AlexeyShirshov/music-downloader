#!/usr/bin/env bash
#
# Формирует раздел «Что нового» для описания релиза по коммитам между тегами.
#
# Использование: scripts/release-changelog.sh <предыдущий-тег> <тег>
#
# Печатает markdown в stdout. Если изменений нет (или предыдущий тег пуст и
# история ограничена этим же тегом) — ничего не печатает.

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <previous-tag> <tag>" >&2
  exit 2
fi

previous="$1"
tag="$2"

if ! git rev-parse -q --verify "$tag" >/dev/null; then
  echo "Неизвестный тег: $tag" >&2
  exit 1
fi

# Первый релиз (предыдущего тега нет) — раздел не нужен: вся история и так
# доступна по ссылке на коммиты.
if [[ -z "$previous" || "$previous" == "$tag" ]]; then
  exit 0
fi

range="$previous..$tag"

# Служебные коммиты бампа версии в описание не попадают.
changes="$(
  git log --no-merges --reverse --pretty=format:'- %s' "$range" 2>/dev/null \
    | grep -v '^- chore(release):' \
    || true
)"

changes="$(printf '%s\n' "$changes" | sed '/^[[:space:]]*$/d')"

if [[ -z "$changes" ]]; then
  exit 0
fi

printf '### Что нового\n\n%s\n' "$changes"

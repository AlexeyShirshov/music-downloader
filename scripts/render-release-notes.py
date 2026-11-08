#!/usr/bin/env python3
"""Собирает описание GitHub Release из шаблона .github/release-notes.md.

Переменные окружения:
    TAG         тег релиза (обязательно), например v1.0.1
    REPOSITORY  owner/repo (обязательно), например AlexeyShirshov/music-downloader
    PREVIOUS    предыдущий тег; если задан, добавляется ссылка на diff
    HIGHLIGHTS  произвольный markdown с описанием изменений (может быть пустым)

Результат пишется в release-notes.md в текущем каталоге.
"""

from __future__ import annotations

import os
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEMPLATE = ROOT / ".github" / "release-notes.md"
OUTPUT = pathlib.Path("release-notes.md")


def render(tag: str, repository: str, previous: str, highlights: str) -> str:
    body = TEMPLATE.read_text(encoding="utf-8")
    body = body.replace("{{TAG}}", tag)
    body = body.replace("{{REPOSITORY}}", repository)
    body = body.replace("{{HIGHLIGHTS}}", highlights.strip())

    if previous and previous != tag:
        changelog = (
            f"**Full Changelog**: "
            f"https://github.com/{repository}/compare/{previous}...{tag}"
        )
    else:
        changelog = ""
    body = body.replace("{{CHANGELOG}}", changelog)

    # Убираем пустые блоки, которые остаются от незаполненных плейсхолдеров.
    body = re.sub(r"\n{3,}", "\n\n", body)
    return body.strip() + "\n"


def main() -> int:
    tag = os.environ.get("TAG", "").strip()
    repository = os.environ.get("REPOSITORY", "").strip()
    if not tag or not repository:
        print("TAG и REPOSITORY обязательны", file=sys.stderr)
        return 2

    if not TEMPLATE.is_file():
        print(f"Нет шаблона {TEMPLATE}", file=sys.stderr)
        return 1

    body = render(
        tag=tag,
        repository=repository,
        previous=os.environ.get("PREVIOUS", "").strip(),
        highlights=os.environ.get("HIGHLIGHTS", ""),
    )
    OUTPUT.write_text(body, encoding="utf-8")
    print(f"Описание релиза: {OUTPUT} ({len(body)} символов)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

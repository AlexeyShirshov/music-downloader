## music-downloader {{TAG}}

{{HIGHLIGHTS}}

### Скачать

| Платформа | Файл |
| --- | --- |
| Linux x86_64 (статический) | [`music-downloader-linux-x86_64`](https://github.com/{{REPOSITORY}}/releases/download/{{TAG}}/music-downloader-linux-x86_64) |
| macOS Apple Silicon | [`music-downloader-macos-arm64`](https://github.com/{{REPOSITORY}}/releases/download/{{TAG}}/music-downloader-macos-arm64) |
| macOS Intel | [`music-downloader-macos-x86_64`](https://github.com/{{REPOSITORY}}/releases/download/{{TAG}}/music-downloader-macos-x86_64) |
| Windows x86_64 | [`music-downloader-windows-x86_64.exe`](https://github.com/{{REPOSITORY}}/releases/download/{{TAG}}/music-downloader-windows-x86_64.exe) |

Все файлы и `SHA256SUMS` лежат в разделе **Assets**. Проверка целостности:

```bash
sha256sum -c SHA256SUMS --ignore-missing
```

### Установка

```bash
chmod +x music-downloader-linux-x86_64
./music-downloader-linux-x86_64 --version
```

- **macOS:** бинарники не подписаны; если система ругается на неизвестного
  разработчика — `xattr -d com.apple.quarantine <файл>`.
- **Windows:** VC++ Redistributable не нужен, CRT прилинкован статически.
- **Из исходников:** `cargo install --git https://github.com/{{REPOSITORY}}`.

### Проверка подлинности

Артефакты релиза подписаны через GitHub Artifact Attestations:

```bash
gh attestation verify music-downloader-linux-x86_64 --repo {{REPOSITORY}}
```

Документация, параметры и ограничения — в [README](https://github.com/{{REPOSITORY}}#readme).

{{CHANGELOG}}

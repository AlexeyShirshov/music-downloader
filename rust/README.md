# yandex-music-downloader (Rust)

Порт [yandex-music-downloader](../README.md) на Rust. Поведение, аргументы
командной строки, шаблоны путей и теги максимально совместимы с Python-версией,
но для запуска не нужны Python, pip и Visual C++ Redistributable.

## Скачать и запустить

Готовые бинарники публикуются в GitHub Releases (workflow
`.github/workflows/release.yml`):

| Платформа | Файл |
| --- | --- |
| Linux x86_64 (статический) | `ymd-linux-x86_64` |
| macOS Apple Silicon | `ymd-macos-arm64` |
| macOS Intel | `ymd-macos-x86_64` |
| Windows x86_64 | `ymd-windows-x86_64.exe` |

```bash
chmod +x ymd-linux-x86_64
./ymd-linux-x86_64 --help
```

macOS: бинарники не подписаны, поэтому после скачивания может понадобиться
снять карантин:

```bash
xattr -d com.apple.quarantine ymd-macos-arm64
```

Windows: файл не подписан, SmartScreen может показать предупреждение.
VC++ Redistributable не требуется — CRT прилинкован статически.

## Сборка

```bash
cd rust
cargo build --release
./target/release/ymd --help
```

### Статический Linux-бинарник

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools   # Debian/Ubuntu
cargo build --release --target x86_64-unknown-linux-musl
```

### Windows без VC++ Redistributable

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --target x86_64-pc-windows-msvc
```

Токен можно передать через `--token`. Если аргумент не указан, программа
получит его сама через OAuth Device Flow: выведет ссылку и код, их нужно
подтвердить в браузере, после чего напечатает `access_token` (сохрани его для
следующих запусков).

Аргументы и их значения совпадают с Python-версией:

```bash
./ymd --token "<Токен>" --quality 2 --url "https://music.yandex.ru/artist/208167"
./ymd --token "<Токен>" --quality 1 --lyrics-format lrc --url "https://music.yandex.ru/album/294912"
./ymd --token "<Токен>" "https://music.yandex.ru/album/11644078/track/6705392"
# или с флагом:
./ymd --token "<Токен>" --url "https://music.yandex.ru/album/11644078/track/6705392"
```

Поддерживаются:

- исполнитель, альбом, трек, плейлист (`--artist-id`, `--album-id`,
  `--track-id`, `--playlist-id`, `-u/--url`);
- качества `lq`/`nq`/`lossless`;
- теги MP3 (ID3v2.4), FLAC (Vorbis comments), M4A (MP4 ilst);
- обложки: отдельным файлом `cover.jpg/png` или встроенные (`--embed-cover`);
- тексты песен: встроенные (`--lyrics-format text`) или `.lrc`
  (`--lyrics-format lrc`);
- шаблон пути со всеми заполнителями Python-версии, включая многодисковые
  альбомы (`#disc-number`, `#disc-number-padded`);
- `--skip-existing`, `--delay`, `--timeout`, `--tries`, `--retry-delay`,
  `--only-music`, `--stick-to-artist`, `--unsafe-path`,
  `--compatibility-level`.

## Отличия от Python-версии

- Списочные методы API вызываются через `GET` (`/tracks`, `/artists`,
  `/albums`): текущий API больше не принимает `POST /artists`.
- Кадр `WOAF` в MP3 содержит настоящий URL трека. В Python-версии из-за
  особенности mutagen он оставался пустым.
- `--playlist-id` понимает и документированный формат `владелец/тип`, и
  новый формат `/playlist/{uuid}`.
- На Windows перезапись существующего файла выполняется удалением старого
  файла перед переименованием (как и на Unix).
- При ошибке записи тегов временный файл удаляется.

## Проверка

```bash
cd rust
cargo test
```

Тесты сверяют:

- шаблоны путей с эталонными значениями Python-версии
  (`tests/fixtures/paths.json`);
- HMAC-подписи и AES-CTR с векторами из Python;
- теги MP3/FLAC/M4A с файлами, размеченными Python-версией
  (`tests/fixtures/audio`);
- полный цикл скачивания на локальном mock-сервере (`tests/e2e.rs`).

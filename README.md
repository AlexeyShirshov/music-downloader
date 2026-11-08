# music-downloader

[![CI](https://github.com/AlexeyShirshov/music-downloader/actions/workflows/ci.yml/badge.svg)](https://github.com/AlexeyShirshov/music-downloader/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/AlexeyShirshov/music-downloader)](https://github.com/AlexeyShirshov/music-downloader/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Загрузчик музыки с **Яндекс.Музыки** и **VK** в одном самодостаточном
бинарнике. Не нужны Python, pip и Visual C++ Redistributable: Linux, Windows и
macOS поддерживаются из коробки.

## Возможности

- Яндекс.Музыка: исполнители, альбомы, треки и плейлисты, включая lossless.
- VK: отдельные треки по ссылке `vk.com`/`vk.ru`.
- Теги MP3 (ID3v2.4), FLAC (Vorbis comments) и M4A (MP4 ilst).
- Обложки: отдельным файлом `cover.jpg/png` или встроенные в аудиофайл.
- Тексты песен: в тегах или отдельным `.lrc` с временными метками.
- Гибкий шаблон пути сохранения.
- Повторные запросы при сетевых ошибках и задержка между запросами.
- Кроссплатформенные статические сборки.

## Скачать

Готовые бинарники публикуются в
[GitHub Releases](https://github.com/AlexeyShirshov/music-downloader/releases/latest):

| Платформа | Файл |
| --- | --- |
| Linux x86_64 (статический) | [`music-downloader-linux-x86_64`](https://github.com/AlexeyShirshov/music-downloader/releases/latest/download/music-downloader-linux-x86_64) |
| macOS Apple Silicon | [`music-downloader-macos-arm64`](https://github.com/AlexeyShirshov/music-downloader/releases/latest/download/music-downloader-macos-arm64) |
| macOS Intel | [`music-downloader-macos-x86_64`](https://github.com/AlexeyShirshov/music-downloader/releases/latest/download/music-downloader-macos-x86_64) |
| Windows x86_64 | [`music-downloader-windows-x86_64.exe`](https://github.com/AlexeyShirshov/music-downloader/releases/latest/download/music-downloader-windows-x86_64.exe) |

В каждом релизе есть `SHA256SUMS` для проверки целостности.

```bash
chmod +x music-downloader-linux-x86_64
./music-downloader-linux-x86_64 --help
```

- **macOS:** бинарники не подписаны. Если система ругается на неизвестного
  разработчика, снимите карантин:
  `xattr -d com.apple.quarantine music-downloader-macos-arm64`.
- **Windows:** файл не подписан, SmartScreen может показать предупреждение.
  VC++ Redistributable не требуется — CRT прилинкован статически.

## Сборка из исходников

Нужен Rust 1.89 или новее.

```bash
git clone https://github.com/AlexeyShirshov/music-downloader.git
cd music-downloader
cargo build --release
./target/release/music-downloader --help
```

Или установка в `~/.cargo/bin`:

```bash
cargo install --path .
# либо
cargo install --git https://github.com/AlexeyShirshov/music-downloader
```

### Статический Linux-бинарник

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools
cargo build --release --target x86_64-unknown-linux-musl
```

### Windows без VC++ Redistributable

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --target x86_64-pc-windows-msvc
```

## Авторизация

### Яндекс.Музыка

Токен можно передать через `--token`. Если аргумент не указан, программа:

1. использует сохранённый токен из `~/.config/music-downloader/token`
   (на Windows — `%APPDATA%\music-downloader\token`);
2. если токена нет — запускает OAuth Device Flow: печатает ссылку и код,
   которые нужно подтвердить в браузере, и сохраняет полученный токен.

Получить токен вручную можно на <https://ym.marshal.dev/token/#implicit-oauth>.
Путь к файлу токена переопределяется переменной
`MUSIC_DOWNLOADER_TOKEN_FILE`.

### VK

VK не отдаёт музыку без авторизации, а обычный веб-токен не имеет доступа к
методам `audio.*`. Нужен токен приложения **Kate Mobile**:

1. Откройте <https://vkhost.github.io>, выберите **Kate Mobile** и подтвердите
   доступ.
2. Скопируйте `access_token` из адресной строки.

Токен даёт полный доступ к аккаунту — никому его не показывайте. Передать его
можно так:

```bash
music-downloader --vk-token "<Токен VK>" "https://vk.com/audio-2001370911_36370911"
# или
MUSIC_DOWNLOADER_VK_TOKEN="<Токен VK>" music-downloader "https://vk.ru/audio-2001370911_36370911"
```

Флаг `--vk-token` сохраняет токен в `~/.config/music-downloader/vk_token`
(файл создаётся с правами `600`). Переменная
`MUSIC_DOWNLOADER_VK_TOKEN_FILE` задаёт другой путь.

## Использование

```bash
# Все треки исполнителя в lossless
music-downloader --token "<Токен>" --quality 2 \
  --url "https://music.yandex.ru/artist/208167"

# Альбом в высоком качестве с текстами в формате LRC
music-downloader --token "<Токен>" --quality 1 --lyrics-format lrc \
  --url "https://music.yandex.ru/album/294912"

# Отдельный трек
music-downloader --token "<Токен>" \
  "https://music.yandex.ru/album/11644078/track/6705392"

# Плейлист
music-downloader --token "<Токен>" --playlist-id "1/123"

# Трек VK
music-downloader --vk-token "<Токен VK>" \
  "https://vk.ru/audio-2001370911_36370911"
```

## Параметры

### Общие

| Параметр | Описание |
| --- | --- |
| `--quality <0\|1\|2>` | Качество: `0` — AAC 64 kbps, `1` — AAC 192 kbps, `2` — FLAC. По умолчанию `2`. Для VK игнорируется. |
| `--skip-existing` | Пропускать уже скачанные треки. |
| `--lyrics-format <none\|text\|lrc>` | Текст песни: в тегах (`text`) или отдельным `.lrc` (`lrc`). По умолчанию `none`. Только Яндекс.Музыка. |
| `--embed-cover` | Встраивать обложку в аудиофайл вместо отдельного `cover.jpg/png`. |
| `--cover-resolution <px\|original>` | Разрешение обложки. По умолчанию `400`. |
| `--delay <сек>` | Задержка между запросами. По умолчанию `0`. |
| `--stick-to-artist` | При загрузке исполнителя брать только альбомы, созданные им. |
| `--only-music` | Пропускать подкасты и аудиокниги. |
| `--compatibility-level <0\|1>` | Уровень совместимости тегов (см. ниже). По умолчанию `1`. |

### Сеть

| Параметр | Описание |
| --- | --- |
| `--timeout <сек>` | Таймаут запроса. По умолчанию `20`. |
| `--tries <n>` | Число повторов при сетевых ошибках; `0` — бесконечно. По умолчанию `20`. |
| `--retry-delay <сек>` | Пауза между повторами. По умолчанию `5`. |

### Что скачивать

| Параметр | Описание |
| --- | --- |
| `--artist-id <id>` | Все треки исполнителя. |
| `--album-id <id>` | Все треки альбома. |
| `--track-id <id>` | Один трек. |
| `--playlist-id <владелец>/<тип>` | Плейлист. Понимает и `/playlist/{uuid}`. |
| `-u`, `--url <url>` | URL исполнителя, альбома, трека, плейлиста или трека VK. URL можно передать и без `-u`. |

### Путь и авторизация

| Параметр | Описание |
| --- | --- |
| `--dir <папка>` | Куда сохранять музыку. По умолчанию `.`. |
| `--path-pattern <шаблон>` | Шаблон пути (см. ниже). |
| `--unsafe-path` | Не очищать путь от недопустимых символов. |
| `--token <токен>` | Токен Яндекс.Музыки. |
| `--vk-token <токен>` | Токен Kate Mobile для VK. |
| `-h`, `--help` | Справка. |
| `-V`, `--version` | Версия. |

## Шаблон пути

По умолчанию:

- Яндекс.Музыка — `#album-artist/#album/#number - #title`;
- VK — `#track-artist/#title`.

Поддерживаются заполнители:

| Заполнитель | Значение |
| --- | --- |
| `#number` | Номер трека в альбоме. |
| `#number-padded` | Номер трека с ведущими нулями. |
| `#disc-number` | Номер диска. |
| `#disc-number-padded` | Номер диска с ведущими нулями. |
| `#track-artist` | Исполнитель трека. |
| `#album-artist` | Исполнитель альбома. |
| `#title` | Название трека (с версией, если есть). |
| `#album` | Название альбома. |
| `#year` | Год альбома. |
| `#artist-id` | ID исполнителя. |
| `#album-id` | ID альбома. |
| `#track-id` | ID трека. |

Пример:

```bash
music-downloader --token "<Токен>" \
  --path-pattern "#track-artist/#album (#year)/#number-padded - #title" \
  --album-id 294912
```

## Теги и файлы

- **MP3** — ID3v2.4; для FLAC и M4A — Vorbis comments и MP4 ilst
  соответственно.
- Записываются название, исполнители, альбом, номер трека и диска, год,
  жанр и ссылка на источник.
- Обложка по умолчанию кладётся рядом с треком (`cover.jpg` или `cover.png`),
  с `--embed-cover` — встраивается в файл.
- Текст песни с `--lyrics-format text` попадает в тег, с `lrc` — в
  одноимённый `.lrc`.
- Запись идёт через временный файл: при ошибке он удаляется, существующий
  файл не повреждается.

### Уровни совместимости

- `0` — стандартные теги.
- `1` — для M4A несколько исполнителей записываются в один тег через `; `
  (например, `Artist1; Artist2`). Помогает плеерам, которые не понимают
  множественные значения.

## Ограничения VK

- Скачиваются только отдельные треки, не плейлисты и не дискографии.
- VK отдаёт mp3 128–320 kbps, поэтому `--quality` игнорируется.
- У аудиозаписей VK нет альбома, обложки, номера трека и текста песни:
  `--embed-cover`, `--lyrics-format` и заполнители альбома не применяются.
- Шаблон пути по умолчанию — `#track-artist/#title`.

## Переменные окружения

| Переменная | Назначение |
| --- | --- |
| `MUSIC_DOWNLOADER_TOKEN_FILE` | Путь к файлу токена Яндекс.Музыки. |
| `MUSIC_DOWNLOADER_VK_TOKEN` | Токен VK (Kate Mobile). |
| `MUSIC_DOWNLOADER_VK_TOKEN_FILE` | Путь к файлу токена VK. |
| `MUSIC_DOWNLOADER_API_BASE_URL` | Базовый URL API Яндекс.Музыки (для отладки и тестов). |
| `MUSIC_DOWNLOADER_COVER_BASE_URL` | Базовый URL обложек. |
| `MUSIC_DOWNLOADER_VK_API_BASE_URL` | Базовый URL VK API. |
| `MUSIC_DOWNLOADER_VK_API_VERSION` | Версия VK API. По умолчанию `5.95`. |
| `RUST_LOG` | Уровень логирования, например `debug`. |

## Разработка

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Тесты проверяют подписи и расшифровку, шаблоны путей, теги MP3/FLAC/M4A,
разбор ссылок VK и полный цикл скачивания на локальном mock-сервере.

### Релиз

Релизы делает GitHub Actions: **Actions → Release → Run workflow**. Нужно
выбрать тип повышения версии:

- `patch` — `1.0.0` → `1.0.1`;
- `minor` — `1.0.0` → `1.1.0`;
- `major` — `1.0.0` → `2.0.0`;
- `none` — выпустить текущую версию без повышения.

Workflow прогоняет тесты, обновляет версию в `Cargo.toml` и `Cargo.lock`,
коммитит `chore(release): vX.Y.Z`, ставит тег, собирает бинарники для Linux,
Windows и macOS, добавляет `SHA256SUMS` и создаёт GitHub Release со
сгенерированными заметками.

Текущая версия — `1.0.0`; для первого релиза достаточно запустить workflow с
`none`.

## Переход со старого названия

Проект переименован из `yandex-music-downloader`/`ymd`:

- команда `ymd` теперь называется `music-downloader`;
- конфиг переехал из `~/.config/ymd` в `~/.config/music-downloader`;
- переменные окружения `YMD_*` заменены на `MUSIC_DOWNLOADER_*`;
- Python-версия удалена, остался только Rust;
- нумерация версий начата заново с `1.0.0`.

Перенести сохранённые токены можно так:

```bash
mkdir -p ~/.config/music-downloader
mv ~/.config/ymd/token ~/.config/music-downloader/token 2>/dev/null || true
mv ~/.config/ymd/vk_token ~/.config/music-downloader/vk_token 2>/dev/null || true
```

## Благодарности

- [llistochek/yandex-music-downloader](https://github.com/llistochek/yandex-music-downloader)
  — оригинальная Python-версия и большая часть логики работы с API.
- [MarshalX/yandex-music-api](https://github.com/MarshalX/yandex-music-api) —
  документация и примеры работы с API.
- [vodka2/vk-audio-token](https://github.com/vodka2/vk-audio-token) и
  [python273/vk_api](https://github.com/python273/vk_api) — детали Audio API
  VK и преобразование HLS-ссылок.
- @ArtemBay — способ получения lossless-ссылок.
- @keltecc — метод расшифровки файлов.
- @leowerd — корректные имена исполнителей при загрузке сборников.

## Дисклеймер

Проект не связан с Яндексом и VK. Скачивайте только тот контент, который
разрешено скачивать, и соблюдайте условия использования сервисов и
законодательство вашей страны.

## Лицензия

[MIT](LICENSE)

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

С сентября 2026 VK закрыл доступ сторонним приложениям: токены Kate Mobile и
vkhost больше не выдаются, а выданные раньше перестали работать. Единственный
рабочий способ — войти как **веб-клиент VK**: программа обменивает куки вашего
браузера (`remixsid` и остальные) на токен веб-клиента и сохраняет его.

Самый простой способ — `--vk-login`:

```bash
music-downloader --vk-login
# или сразу с загрузкой трека
music-downloader --vk-login "https://vk.com/audio-2001370911_36370911"
```

1. Войдите в VK в браузере — программа откроет <https://vk.ru/>.
2. Нажмите **F12** → вкладка **Network** (Сеть) → обновите страницу (**F5**).
3. Выберите любой запрос к `vk.ru` — обычно самый верхний в списке.
4. Справа откройте **Headers** → **Request Headers** → строку **Cookie**.
5. Скопируйте значение `Cookie` целиком: правый клик по строке →
   **Copy value** (или выделите текст и нажмите `Ctrl+C`).
6. Вернитесь в окно программы, вставьте скопированное (правый клик или
   `Ctrl+V`) и нажмите **Enter**.

В строке `Cookie` есть `remixsid` (или `remixnsid`/`remixdsid`) — именно она
подтверждает вход. Вместо неё можно указать путь к экспортированному
`cookies.txt` (Netscape) или JSON-экспорту расширения — куки других сайтов
отфильтруются.

Если обмен кук не проходит, токен можно получить прямо в браузере: откройте
консоль (Console) на `vk.ru` и выполните

```js
fetch('https://login.vk.ru/?act=web_token', {
  method: 'POST',
  headers: {'Content-Type': 'application/x-www-form-urlencoded'},
  body: 'version=1&app_id=6287487',
  credentials: 'include'
}).then(r => r.json()).then(d => console.log(d.data.access_token))
```

и вставьте полученный `vk1.a...` в `--vk-login` (он принимает и готовый токен)
или в `--vk-token`.

Если токена ещё нет, вход запускается сам при первом скачивании трека VK:
`--vk-login` не обязателен, флаг нужен, чтобы войти заранее или обновить токен.
Без интерактивного терминала (скрипт, CI) программа не ждёт ввода, а печатает
инструкцию и завершается с ошибкой.

Куки можно передать и без интерактива:

```bash
music-downloader --vk-cookies "remixsid=...; remixnsid=..." \
  "https://vk.ru/audio-2001370911_36370911"
# файл cookies.txt (Netscape) или JSON-экспорт
music-downloader --vk-cookies cookies.txt "https://vk.ru/audio-2001370911_36370911"
# из stdin, чтобы не оставлять куки в истории shell
echo "remixsid=..." | music-downloader --vk-cookies - \
  "https://vk.ru/audio-2001370911_36370911"
```

Переменная `MUSIC_DOWNLOADER_VK_COOKIES` задаёт те же куки, что и
`--vk-cookies`.

Веб-токен живёт всего ~15 минут, поэтому сохраняются не он, а куки:
`~/.config/music-downloader/vk_cookies` (на Windows —
`%APPDATA%\music-downloader\vk_cookies`, файл создаётся с правами `600`).
При каждом запуске из них минтится свежий токен, так что повторный вход нужен
только когда истекут сами куки. Путь переопределяет
`MUSIC_DOWNLOADER_VK_COOKIES_FILE`.

Готовый токен всё ещё можно передать через `--vk-token` или
`MUSIC_DOWNLOADER_VK_TOKEN` — но он проживёт всего ~15 минут.

Скачивание: `audio.getById` теперь отдаёт HLS-плейлист. Программа сама
собирает сегменты, расшифровывает AES-128-CBC (ключ и IV из плейлиста) и
пишет mp3, если VK отдаёт mp3. Если поток окажется AAC в MPEG-TS, файл
сохранится как `.ts` без тегов — для mp3 в этом случае нужен `ffmpeg`.

Почему нельзя совсем как у Яндекса: у Яндекс.Музыки есть публичный
OAuth-клиент и Device Flow, а VK закрыл сторонний доступ и device flow не
поддерживает. Куки браузера — это единственный оставшийся вход.

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

# Трек VK (при первом запуске вход по кукам браузера)
music-downloader "https://vk.ru/audio-2001370911_36370911"
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
| `--vk-cookies <куки>` | Куки браузера VK (`p=...; remixsid=...`), файл `cookies.txt`/JSON или `-` для stdin. |
| `--vk-token <токен>` | Готовый токен веб-клиента VK; `-` читает токен из stdin. |
| `--vk-login` | Интерактивно обменять куки браузера на токен VK и сохранить его. |
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
| `MUSIC_DOWNLOADER_VK_COOKIES` | Строка Cookie из браузера целиком или путь к `cookies.txt`. |
| `MUSIC_DOWNLOADER_VK_TOKEN` | Готовый токен веб-клиента VK (живёт ~15 минут). |
| `MUSIC_DOWNLOADER_VK_COOKIES_FILE` | Путь к файлу с куками VK. |
| `MUSIC_DOWNLOADER_API_BASE_URL` | Базовый URL API Яндекс.Музыки (для отладки и тестов). |
| `MUSIC_DOWNLOADER_COVER_BASE_URL` | Базовый URL обложек. |
| `MUSIC_DOWNLOADER_VK_API_BASE_URL` | Базовый URL VK API. |
| `MUSIC_DOWNLOADER_VK_API_VERSION` | Версия VK API. По умолчанию `5.282`. |
| `MUSIC_DOWNLOADER_VK_LOGIN_BASE_URL` | Базовый URL обмена кук на токен (по умолчанию `https://login.vk.ru`). |
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
Windows и macOS, добавляет `SHA256SUMS` и создаёт GitHub Release.

Описание релиза собирается автоматически: `scripts/release-changelog.sh`
берёт коммиты между предыдущим тегом и новым (служебные
`chore(release): ...` отбрасываются) и подставляет их в шаблон
`.github/release-notes.md` вместе с таблицей платформ, инструкциями по
установке и проверке подлинности. Руками ничего вводить не нужно — всё, что
попало в `main` после прошлого релиза, окажется в разделе «Что нового».
Артефакты подписываются через GitHub Artifact Attestations, проверить подпись
можно так:

```bash
gh attestation verify music-downloader-linux-x86_64 \
  --repo AlexeyShirshov/music-downloader
```

Текущая версия — `1.0.0`; для первого релиза достаточно запустить workflow с
`none`.

## Безопасность

Каждый push в `main` и каждый pull request прогоняют
[`.github/workflows/security.yml`](.github/workflows/security.yml):

| Проверка | Что делает |
| --- | --- |
| **RustSec advisories** (`cargo audit`) | Сверяет `Cargo.lock` с базой уязвимостей RustSec. |
| **cargo-deny** | Уязвимости, yanked-версии, недопустимые лицензии, wildcard-зависимости и сторонние источники пакетов (`deny.toml`). |
| **gitleaks** | Ищет случайно закоммиченные токены, ключи и пароли во всей истории git. |
| **zizmor** | Статический анализ самих workflow: инъекции в `run`, слишком широкие `permissions`, незакреплённые action'ы, утечка креденшелов в артефакты. |
| **CodeQL** | Семантический анализ кода на Rust и самих workflow. |
| **Dependency review** | В PR не даёт добавить зависимость с известной уязвимостью (уровень `moderate` и выше). |
| **OpenSSF Scorecard** | Оценка гигиены репозитория с публикацией результата; запускается раз в неделю и на push в `main`. |

Дополнительно:

- все сторонние action'ы в workflow закреплены на конкретный commit SHA, а не
  на плавающий тег;
- раз в неделю Dependabot обновляет cargo- и GitHub Actions-зависимости
  (с недельной задержкой после публикации версии);
- в `Cargo.toml` включён `unsafe_code = "forbid"` — небезопасный код не
  соберётся.

Те же проверки можно запустить локально:

```bash
cargo install cargo-audit cargo-deny
cargo audit
cargo deny check
```

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
mv ~/.config/ymd/vk_token ~/.config/music-downloader/vk_cookies 2>/dev/null || true
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

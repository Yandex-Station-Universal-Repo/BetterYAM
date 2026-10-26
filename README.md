# BetterYAM

**BetterYAM — альтернативный runtime для YAM-плагинов на Яндекс Станциях.**
Он запускает Lua- и binary-пакеты, предоставляет CLI `byam` и HTTP API для
управления устройством. Аппаратные функции подключаются через daemon
конкретного устройства и объявляются в его профиле.

Проект сохраняет совместимость с привычной моделью пакетов и Lua API YAM, но
развивается отдельно: устройства могут иметь собственные аппаратные адаптеры,
а доступность функций определяется capabilities, а не предположениями о модели.

> **Текущий статус:** в репозитории собраны daemons для Yandex Station Mini и Yandex Station Lite (`yandexmicro`).
> Установщик выбирает устройство по модели и не подменяет неизвестную модель
> похожей. Поддержка других устройств требует отдельной реализации и проверки;
> наличие универсальной архитектуры не означает, что любой тип Станции уже
> поддерживается.

## Содержание

- [Возможности](#возможности)
- [Зачем BetterYAM](#зачем-betteryam)
- [Установка](#установка)
- [Первые команды](#первые-команды)
- [Веб-панель и API](#веб-панель-и-api)
- [Пакеты и совместимость](#пакеты-и-совместимость)
- [Поддержка устройств](#поддержка-устройств)
- [Сборка и тестирование](#сборка-и-тестирование)
- [Документация](#документация)
- [Участие в разработке](#участие-в-разработке)
- [Лицензия](#лицензия)

## Возможности

- **Runtime для YAM-пакетов:** запуск Lua-плагинов и поддержка binary-пакетов.
- **CLI `byam`:** установка из файла или registry, управление пакетами,
  сервисами, логами и доступными функциями устройства.
- **HTTP API:** локальное управление daemon-ом и точки подключения для CLI,
  Web Panel и binary-плагинов.
- **Device profiles и capabilities:** устройство сообщает, какие операции
  действительно доступны; плагины могут проверять их до вызова.
- **Раздельная реализация устройств:** аппаратный код и обнаружение функций
  находятся в daemon-е устройства, а не в общем CLI.
- **Разрешения плагинов:** hardware и runtime API доступны в рамках permissions,
  объявленных в пакете.
- **Установка через ADB:** установщик на компьютере подбирает daemon по модели
  подключённой Станции и проверяет контрольные суммы релизных файлов.

В Mini backend сейчас доступны, если обнаружено соответствующее оборудование
или системный интерфейс: RGB-кольцо LED, ALSA Master volume, аудиовоспроизведение,
некоторые input-события и proximity sensor, Bluetooth status/power и Wi-Fi
управление. Для каждой операции актуальным источником служит профиль
`GET /api/device`; полный список и ограничения приведены в
[документации API](./docs/DEPLOYMENT.md#yam-api-и-capability-profile).

## Зачем BetterYAM

BetterYAM предназначен для тех, кому нужен управляемый и расширяемый runtime,
а не прошивка конкретного устройства:

1. **Существующие пакеты.** Сохраняются привычные `.yap`/`.pkg`, Lua-контракт
   `alice`, `device`, `event` и основные вызовы `alice.set`, `alice.off`,
   `alice.play`, `alice.logo.set` и `alice.logo.off`.
2. **Явная поддержка hardware.** Capability включается только при обнаруженном
   и пригодном адаптере. Неподдерживаемая операция не маскируется под успешную.
3. **Безопасное расширение.** Для новой модели можно добавить свой daemon и
   зарегистрировать точные model aliases, не маршрутизируя её в несовместимый
   backend.
4. **Управление с компьютера.** `byam` предоставляет удобные команды, а HTTP
   API остаётся доступен для автоматизации и интеграций.
5. **Permissions пакетов.** Плагин должен явно запросить доступ к аппаратным
   возможностям и системным API.

BetterYAM — самостоятельный форк/runtime, а не официальный продукт Яндекса.
Поддержка функций зависит от модели, прошивки и доступности системных
интерфейсов.

## Установка

### Требования

- macOS Apple Silicon или Linux x86_64 для Unix-установщика; для Windows
  используется PowerShell-установщик.
- Android Platform Tools (`adb`) на компьютере.
- Станция, доступная по ADB. Подключите её по USB или настройте ADB по сети.
- Компьютер и Станция должны иметь сетевое соединение для обращения CLI к
  daemon API.

### macOS и Linux

Установщик загрузит CLI на компьютер и подходящий опубликованный daemon на
Станцию. Перед записью он автоматически перемонтирует `/` в `rw`, установит
startup hook `/etc/init.d/S99zzbetteryam` и запустит BetterYAMd. При включении
станции BusyBox init выполнит hook: он повторно перемонтирует `/` в `rw` и
запустит daemon в фоне. Hook выполняется после остальных `S99`-скриптов;
скрипт установки отказывается перезаписывать чужой файл с таким именем.

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh
```

Для конкретной версии передайте версию первым аргументом:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh -s -- 0.1.18
```

### Windows

Запустите PowerShell от имени пользователя, у которого установлен `adb`:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.ps1 | iex
```

### После установки

Основная команда — `byam`; `yam` оставлен как совместимый alias. На macOS и
Linux CLI устанавливается в `~/.local/bin`. Если команда не найдена, добавьте
этот каталог в `PATH` или вызовите CLI напрямую:

```sh
~/.local/bin/byam --version
~/.local/bin/byam --help
```

Установленный на Станцию daemon находится в
`/data/quazar/betteryam/bin/betteryamd`, его данные — в
`/data/quazar/betteryam/data`. По умолчанию HTTP API доступен на порту `8099`.
Адрес CLI можно переопределить через `--url` или `YAM_URL`.

Модель определяется по hostname Станции. Если модель пока не зарегистрирована
в релизном manifest, установщик остановится, а не установит daemon от другой
модели. Удаление из клона репозитория выполняется командой `sh install.sh
--uninstall`; она также удаляет daemon с выбранной Станции.

## Первые команды

```sh
byam device                    # профиль и возможности устройства
byam list                      # установленные пакеты
byam registry search lights    # поиск в package registry
byam install package.yap       # установка локального пакета
byam install nano              # установка пакета из registry
byam run nano                  # запуск пакета
byam logs nano                 # журнал пакета
byam --version                 # версия CLI
```

Установленные сервисы можно просматривать и обслуживать командами
`byam service list`, `byam service enable <id>`, `byam service restart <id>` и
`byam service logs <id>`. Большинство команд имеют `--help`.

Для скриптов используйте JSON-вывод, например `byam list --json` и
`byam registry list --json`. Цвет включается в интерактивном терминале;
управлять им можно через `--color auto|always|never` или `BYAM_COLOR`.
Подробности и полный список примеров — в [руководстве по пакетам](./docs/PACKAGES.md).

## Веб-панель и API

Daemon предоставляет HTTP API, используемый CLI, Web Panel и binary runtime.
Профиль устройства возвращает `features` для совместимости и boolean-словарь
`capabilities` для проверки отдельных возможностей. Например, интерфейс может
показать Bluetooth status, если доступна `bluetooth.status`, но не показывать
переключатель питания, если `bluetooth.power` недоступна.

Цель архитектуры — общая Web Panel для устройств: она строит доступные разделы
по capabilities профиля, а изображение устройства получает от самого профиля.
В текущей версии встроенная Web Panel находится в daemon-е Mini; поле профиля
с URL изображения ещё не реализовано. Ожидаемый контракт и существующие
маршруты описаны в [руководстве по подключению Web Panel](./docs/WEB_PANEL_INTEGRATION.md).

Основные API-маршруты включают:

- `GET /api/device` — профиль и capabilities;
- `GET /api/plugins` и `/api/plugins/{id}/...` — пакеты, запуск и журналы;
- `POST /api/led`, `POST /api/led/off` — управление LED;
- `/api/wifi/...` — состояние и управление Wi-Fi;
- `/api/volume`, `/api/bluetooth/...`, `/api/sensors...`, `/api/clock` —
  соответствующие API, если они реализованы текущим backend.

Все маршруты, запросы, permissions и доступность функций описаны в
[документации API и развёртывания](./docs/DEPLOYMENT.md#yam-api-и-capability-profile).

## Пакеты и совместимость

Пакет содержит `plugin.toml` в корне и может запускаться как Lua или binary.
Lua-плагины продолжают получать `alice`, `device` и `event`. Используйте
минимальные permissions и указывайте требуемые hardware features в
`[compatibility]`.

После установки binary-пакет с permission `exec` становится shell-командой с
именем своего package ID (например, `byam install nano` → `nano`). BetterYAM
создаёт wrapper в `/data/quazar/betteryam/data/bin` и ссылку в `/usr/bin`,
который входит в исходный `PATH` BusyBox shell, в том числе `adb shell`, если
системная файловая система доступна для записи.
Каталог wrapper-ов также добавляется через `/etc/profile.d/betteryam.sh` для
login shell. Если `/usr/bin/<package-id>` занят, BetterYAM сохраняет
существующую команду. При read-only системной файловой системе пакет всё
равно устанавливается, но shell-команда не экспортируется; BetterYAM сообщает
об этом в журнале. Binary-пакеты запускаются с правами пользователя shell,
поэтому устанавливайте только доверенные пакеты.

Пример фрагмента манифеста:

```toml
id = "ambient-lights"
name = "Ambient lights"
version = "1.0.0"
entry = "main.lua"
permissions = ["led"]

[launch]
kind = "lua"
mode = "service"
path = "main.lua"

[compatibility]
features = ["led.ring.rgb"]
```

Binary-пакеты сохраняют runtime-контракт `YAM_API_URL`, `YAM_API_TOKEN` и
заголовок `X-YAM-Token`. IPC в Lua доступен через `alice.ipc` при наличии
permissions `ipc.read`/`ipc.control` и allowlist сервисов в `[ipc].services`.

Полные сведения:
[пакеты и CLI](./docs/PACKAGES.md) ·
[Lua API](./docs/PLUGIN_API.md) ·
[каталог API Mini](./yandexmini/docs/api/README.md).

## Поддержка устройств

У каждого семейства устройств — собственный daemon, который реализует
совместимый API, обнаруживает hardware и объявляет фактические capabilities.
Общие runtime-поверхности и контракт плагинов должны оставаться совместимыми;
модельно-специфичные драйверы и системные вызовы не следует переносить в CLI
или чужой daemon.

Чтобы добавить модель, нужно исследовать её hostname, архитектуру и системные
интерфейсы, реализовать и проверить daemon, добавить запись в корневой
[`manifest.json`](./manifest.json) и собрать release asset. Не объявляйте
непроверенные функции доступными и не направляйте неизвестную модель на
похожее устройство.

Пошаговая инструкция и готовый запрос для coding-agent находятся в
[руководстве по добавлению устройств](./docs/ADDING_DEVICES.md). Контракт
общей панели — в [WEB_PANEL_INTEGRATION.md](./docs/WEB_PANEL_INTEGRATION.md).

## Сборка и тестирование

Для локальной сборки и установки daemon-а нужны Rust stable, `adb` и
toolchain для target выбранного устройства из корневого `manifest.json`.
Запустите общий [скрипт сборки и деплоя](./tools/build-deploy.sh):

```sh
./tools/build-deploy.sh
```

Без аргументов он предложит выбрать daemon и подключённую по ADB колонку.
Выбор можно передать аргументами — ID daemon-а и serial устройства:

```sh
./tools/build-deploy.sh yandexmini 192.168.0.171:5555
```

Или использовать именованные параметры. Флаг `--docker` собирает daemon
в Docker вместо локального cross toolchain:

```sh
./tools/build-deploy.sh --device yandexmini --serial 192.168.0.171:5555 --docker
```

Скрипт собирает stripped release-бинарник, загружает его во временное имя,
сохраняет предыдущую версию в
`/data/quazar/betteryam/bin/betteryamd.previous`, заменяет daemon и
перезапускает его. Если новая версия не запускается, выполняется попытка
отката. Docker-режиму нужен доступный Docker; без `--docker` настройте
cross-compilation toolchain соответствующей архитектуры. Не запускайте
параллельные деплои на одну колонку. Перед установкой скрипт сверяет hostname
Станции с aliases выбранного daemon-а в manifest и отказывается от несовпадения.

Для разработки и CI доступны стандартные проверки:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
sh -n install.sh
bash -n tools/build-deploy.sh
```

Публикация автоматизирована GitHub Actions при push тега `v*`. Перед выпуском
версия тега должна соответствовать версии CLI в корневом `Cargo.toml`.
Подробности — в [инструкции по развёртыванию и релизам](./docs/DEPLOYMENT.md).

## Документация

| Тема | Документ |
| --- | --- |
| Установка, API, profiles и release-сборка | [DEPLOYMENT.md](./docs/DEPLOYMENT.md) |
| Lua-контракт и permissions | [PLUGIN_API.md](./docs/PLUGIN_API.md) |
| Формат пакетов и команды `byam` | [PACKAGES.md](./docs/PACKAGES.md) |
| Добавление новой модели устройства | [ADDING_DEVICES.md](./docs/ADDING_DEVICES.md) |
| Точки подключения общей Web Panel | [WEB_PANEL_INTEGRATION.md](./docs/WEB_PANEL_INTEGRATION.md) |
| API и ограничения Yandex Mini | [yandexmini/docs/api/](./yandexmini/docs/api/README.md) |

## Участие в разработке

Исправления, предложения по совместимости и новые backend-ы приветствуются.
Перед крупным изменением проверьте документацию и действующий API-контракт.
Для нового устройства приложите подтверждённые model aliases, capabilities,
архитектуру и результаты проверок на реальном устройстве, если оно доступно.
Не включайте в репозиторий credentials, токены, пароли, пользовательские данные
или неочищенные логи.

## Лицензия

BetterYAM распространяется по лицензии [MIT](./LICENSE).

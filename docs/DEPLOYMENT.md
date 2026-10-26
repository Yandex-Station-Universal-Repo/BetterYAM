# Установка, API и развёртывание

## Установка daemon-а

Установщику нужны `adb` и доступная по ADB колонка. При нескольких
поддерживаемых устройствах можно выбрать колонку из списка. Установщик
получает модель через `uname -n`, удаляет конечный сегмент `-XXXX` или
`_XXXX` (четыре заглавные буквы или цифры) и ищет точное совпадение в release
`manifest.json`. Например:

| Hostname | Модель для поиска |
| --- | --- |
| `Yandex-Station-Midi-44QK` | `Yandex-Station-Midi` |
| `Yandex-Mini3-J3MK` | `Yandex-Mini3` |
| `YandexStationMini_729E` | `YandexStationMini` |

Суффикс модели, например `gen2`, не удаляется. Неизвестная модель не
сопоставляется с похожим устройством. Установщик проверяет SHA-256 daemon-а и
размещает его в `/data/quazar/betteryam`.

### macOS и Linux

Установка последнего релиза:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh
```

Для конкретной версии передайте номер релиза первым аргументом:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh -s -- 0.1.18
```

Unix-установщик поддерживает Linux x86_64 и macOS Apple Silicon. CLI
устанавливается в `~/.local/bin/byam`; `yam` создаётся как совместимый alias.
Если каталог не входит в `PATH`, добавьте его или запускайте CLI по полному
пути.

### Windows

Запустите PowerShell от имени пользователя, у которого установлен `adb`:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.ps1 | iex
```

### Запуск при старте и удаление

Установщик создаёт `/etc/init.d/S99zzbetteryam`. BusyBox `rcS` запускает hook
после других `S99`-скриптов; hook перемонтирует `/` в `rw` и запускает
`betteryamd` в фоне с pid-файлом. Повторная установка обновляет управляемый
hook, но не перезаписывает чужой файл с тем же именем.

Чтобы удалить установку из клона репозитория:

```sh
sh install.sh --uninstall
```

При запуске установщика из сети передайте флаг через stdin:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/main/install.sh | sh -s -- --uninstall
```

Удаление останавливает daemon и удаляет hook, если тот содержит маркер
BetterYAM.

## CLI и сборка daemon-ов

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить через
`--url` или `YAM_URL`. По умолчанию daemon выбирает активный LAN IPv4-адрес и
слушает порт `8099`. Параметр `--listen IP:PORT` задаёт адрес и порт вручную.

Корневой `manifest.json` задаёт daemon-ы, модели и build targets. Собрать их
все и создать релизный manifest можно командой:

```sh
sh tools/build-daemons.sh
```

Необязательный третий аргумент ограничивает сборку одним ID устройства:

```sh
sh tools/build-daemons.sh manifest.json dist yandexmini
```

Скрипт создаёт `dist/manifest.json`, бинарные assets в `dist/assets/` и
`dist/SHA256SUMS`. Релизные daemon-бинарники собираются с удалёнными
символами отладки. Для cross-build нужны Rust target и подходящий linker для
каждой архитектуры из manifest. Скрипт `tools/publish-all.sh` дополнительно
требует `gh`, авторизованную GitHub-сессию и сетевой доступ.

## Device API и capabilities

Общий HTTP API и plugin runtime реализованы в `betteryam-runtime/`. Каждый
daemon передаёт runtime adapter и hardware backend. Профиль возвращается
маршрутом `GET /api/device` и содержит `id`, `model`, `architecture`,
`led_count`, `features` и словарь boolean `capabilities`.

`features` сохраняет совместимость с `[compatibility].features` в манифестах
пакетов. `capabilities` показывает доступность конкретных операций;
отсутствующая capability означает `false`. Например, наличие
`bluetooth.status` не гарантирует поддержку `bluetooth.power`.

Для LED используйте `device.led_count`. Порядок цветовых каналов и аппаратный
способ вывода задаёт adapter. Возможности остальных API зависят от устройства,
системных интерфейсов и runtime-состояния. Наличие маршрута само по себе не
означает, что операция доступна. Подробнее:
[каталог Lua-функций](./functions/README.md) и
[матрица поддержки устройств](./DEVICE_SUPPORT.md).

### Основные HTTP-маршруты

Все перечисленные маршруты начинаются с `/api`.

| Область | Маршруты | Назначение |
| --- | --- | --- |
| Профиль | `GET /device` | Device profile, features и capabilities |
| Пакеты | `/plugins`, `/plugins/{id}/...` | Установка, запуск, остановка и журналы |
| LED и логотип | `POST /led`, `/led/off`, `/logo`, `/logo/off` | Вывод кадра или выключение |
| Датчики | `GET /sensors`, `/sensors/{id}` | Список и чтение датчика |
| Часы | `GET/PUT /clock` | Чтение и управление часами |
| Громкость | `GET/PUT /volume` | Чтение и установка `{percent}` |
| Bluetooth | `GET /bluetooth/status`, `PUT /bluetooth/power` | Статус и питание |
| Wi-Fi | `/wifi`, `/wifi/scan`, `/wifi/networks...`, `/wifi/setup` | Состояние и настройка Wi-Fi |
| Настройки | `GET/PUT /settings` | Чтение и сохранение настроек daemon-а |

### Binary runtime API

Binary-плагин получает `YAM_API_URL` и одноразовый `YAM_API_TOKEN`. Передавайте
токен в заголовке `X-YAM-Token`. Маршруты находятся под
`/api/runtime/{id}/...`; права доступа зависят от permissions плагина.

| Операция | Runtime-маршруты |
| --- | --- |
| Устройство и датчики | `GET /device`, `/sensors`, `/sensors/{id}` |
| LED и логотип | `POST /led`, `/led/off`, `/logo`, `/logo/off` |
| Часы и громкость | `POST /clock`, `GET/PUT /volume` |
| Аудио | `POST /audio/play` |
| Bluetooth | `GET /bluetooth/status`, `PUT /bluetooth/power` |
| Input | `GET /input/devices`, `POST /input/next-event` |
| Wi-Fi | `/wifi` и подмаршруты `/scan`, `/networks`, `/setup` |

Runtime-маршруты для аудио принимают PCM WAV до 64 MiB и 600 секунд.
Input-доступ ограничен устройствами, объявленными в манифесте. Lua API и
permissions подробно описаны в [документации плагинов](./PLUGIN_API.md).

## Wi-Fi и точка доступа

Wi-Fi реализуется adapter-ом устройства. На поддерживаемых системах backend
использует `wpa_cli` и wpa_supplicant. Пароль сети преобразуется в ключ
(PBKDF2) и не помещается в командную строку. Сохраняется конфигурация
wpa_supplicant, поэтому её видит системная прошивка.

Основные маршруты:

| Метод и маршрут | Назначение |
| --- | --- |
| `GET /api/wifi` | Текущая сеть, сохранённые сети и состояние setup |
| `GET /api/wifi/networks` | Сохранённые и обнаруженные сети без паролей |
| `POST /api/wifi/scan` | Запуск поиска сетей |
| `POST /api/wifi/networks` | Добавление сети: `{ssid, password}` |
| `DELETE /api/wifi/networks/{ssid}` | Удаление сохранённой сети |
| `GET/POST /api/wifi/setup` | Статус и управление точкой доступа |
| `GET/PUT /api/settings` | Сохранение параметров `wifi_setup` |

Эти операции доступны Lua-плагину с permission `wifi`; методы и результаты
описаны в [каталоге функций](./functions/README.md). Runtime-маршруты требуют
токен и тот же permission.

### Экспериментальная собственная сеть

Режим точки доступа экспериментальный и пока не проверен на устройстве.
При его включении daemon создаёт открытую сеть `BetterYAM-XXXX` по адресу
`192.168.4.1`. Встроенные DHCP и DNS перенаправляют телефон на страницу
настройки; отдельные `dnsmasq` и `udhcpd` не требуются.

После выбора сети daemon пробует подключиться к ней. Если за `try_secs` адрес
не получен, точка доступа запускается снова. Периодически daemon проверяет
сохранённые сети в течение `try_secs`. Пока к точке доступа подключён телефон,
такие попытки откладываются, но не более чем на три цикла.

wpa_supplicant прошивки останавливается через `wpa_cli terminate` и
запускается повторно с исходными аргументами. Если wpa_supplicant запускается
отдельно, BetterYAM снимает свою точку доступа и не конфликтует с ним.

Для точки доступа требуются поддержка режима AP со стороны драйвера и
совместимый `hostapd` в `<data dir>/bin/hostapd` или `PATH`. Без backend
capability `wifi.ap` равна `false`. Режим по умолчанию выключен:

```toml
[wifi_setup]
mode = "off"         # off | manual | auto
ap_after_secs = 180  # ожидание сети перед запуском AP в режиме auto
retry_secs = 300     # интервал повторного поиска сохранённых сетей
try_secs = 60        # длительность одной попытки
channel = 6          # канал 2,4 ГГц
ap_ssid = ""         # пусто — BetterYAM-<конец MAC>
hostapd = ""         # путь; по умолчанию data dir, затем PATH
```

Перед включением точки доступа проверьте драйвер и свободные порты командой
`tools/wifi-probe.sh`. Скрипт только читает состояние:

```sh
adb shell sh < tools/wifi-probe.sh > wifi-probe.txt
```

## Безопасность и обновления

Web UI не имеет аутентификации; используйте его только в доверенной LAN.
Обновляйте daemon через установщик: он выбирает asset по модели Станции и
проверяет контрольную сумму.

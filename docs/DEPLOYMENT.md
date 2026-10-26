# Установка

Нужен `adb`. `uname -a` показывает сведения о ядре; nodename в его выводе —
второе поле после `Linux`, его отдельно возвращает `uname -n`. Установщик
получает hostname через `uname -n`, удаляет конечный
отдельный сегмент из четырёх заглавных ASCII-букв/цифр (ID экземпляра
устройства), затем ищет точную модель в release `manifest.json`. Например,
`Yandex-Station-Midi-44QK`, `Yandex-Mini3-J3MK` и
`YandexStationMini_729E` превращаются соответственно в
`Yandex-Station-Midi`, `Yandex-Mini3` и `YandexStationMini`. Модельный суффикс
`gen2` не удаляется. Установщик проверяет SHA-256 и устанавливает выбранный
BetterYAMd в `/data/quazar/betteryam`.

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.ps1 | iex
```

Unix-установщик поддерживает Linux x86_64 и macOS Apple Silicon. Он загружает
последний релиз по умолчанию; для конкретной версии передайте её первым
аргументом, например `sh -s -- 0.1.18`. CLI устанавливается в
`~/.local/bin/byam`; `yam` создаётся как совместимый alias. Если каталог не
включён в `PATH`, запускайте CLI по полному пути или добавьте `~/.local/bin` в
`PATH`.

Основная команда CLI — `byam`; Windows-установщик добавляет `byam.cmd` и
совместимый `yam.cmd` в пользовательский `PATH`. Для удаления Unix-установки
из клона репозитория выполните `sh install.sh --uninstall`; при запуске из
сети передайте `--uninstall` через stdin:

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/refs/heads/betteryam/install.sh | sh -s -- --uninstall
```

Удаление также удаляет daemon с выбранной колонки. Установщикам нужны Android
Platform Tools (`adb`) и доступная по ADB колонка. Если подключено несколько
поддерживаемых устройств, можно выбрать нужное в интерактивном списке.
Скрипты установки и деплоя перемонтируют корневую файловую систему в `rw`
перед записью файлов. На станции устанавливается
`/etc/init.d/S99zzbetteryam`; BusyBox `rcS` запускает этот hook после других
`S99`-скриптов. Hook сам выполняет `mount -o remount,rw /` и запускает
`betteryamd` в фоне с pid-файлом. Повторные установки обновляют управляемый
hook, но отказываются перезаписывать чужой файл с тем же именем.
`install.sh --uninstall` останавливает BetterYAMd и удаляет hook, только если
он содержит маркер BetterYAM.

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.
По умолчанию daemon выбирает активный LAN IPv4-адрес и слушает порт `8099`.
Задайте `--listen IP:PORT`, чтобы выбрать конкретный интерфейс или порт.
Каталог сборки — root `manifest.json`. `sh tools/build-daemons.sh` собирает все
перечисленные устройства; необязательный третий аргумент ограничивает сборку
одним ID устройства, например
`sh tools/build-daemons.sh manifest.json dist yandexmini`. Релизные daemon
бинарники автоматически собираются с удалёнными symbol/debug данными. Скрипт
создаёт `dist/manifest.json`, assets в `dist/assets/` и `dist/SHA256SUMS`.
`tools/publish-all.sh` собирает daemon-ы локальным Rust toolchain на текущем
компьютере, без Docker. Для cross-build должны быть установлены Rust target и
подходящий linker для каждой архитектуры из manifest. Скрипту публикации также
нужны `gh` с авторизованной GitHub-сессией и доступ к сети. Неизвестная модель
не сопоставляется с похожим устройством: установка завершается с ошибкой.

## YAM API и capability profile

CLI и общая Web Panel подключаются к HTTP API BetterYAMd. HTTP API и plugin
runtime реализованы один раз в `betteryam-runtime/`; daemon каждого устройства
передаёт runtime adapter и hardware backend. Frontend панели общий, а доступные
разделы и действия определяются capabilities профиля, не моделью. Контракт
интеграции панели описан в [WEB_PANEL_INTEGRATION.md](./WEB_PANEL_INTEGRATION.md).

`GET /api/device` возвращает `id`, `model`, `architecture`, `led_count`, `features`
и словарь `capabilities` с boolean значениями. `features` сохраняется для
совместимости с manifest compatibility; `capabilities` отвечает за доступность
каждой отдельной операции/возможности. Отсутствующий capability считается
недоступным (`false`). Например:

```json
{
  "features": ["led.ring.rgb", "led.ring.12", "clock"],
  "capabilities": {
    "led.ring.rgb": true,
    "led.ring.12": true,
    "clock": true,
    "clock.segments": false
  }
}
```

Полный контракт API сохраняется:

| Маршрут | Контракт |
| --- | --- |
| `GET /api/device` | Профиль, feature list и boolean capabilities |
| `POST /api/led`, `POST /api/led/off` | `{colors: ["#RRGGBB", ...]}` / выключить LED |
| `GET /api/sensors`, `GET /api/sensors/{id}` | Список `{id, kind, unit, value}` и чтение сенсора |
| `GET /api/clock`, `PUT /api/clock` | Состояние часов; запрос принимает `time`, `brightness`, `segments` или `off` |
| `GET /api/volume`, `PUT /api/volume` | Получение уровня; установка `{percent}` |
| `POST /api/runtime/{id}/led`, `POST /api/runtime/{id}/led/off` | LED frame / выключить LED; runtime token и permission `led` |
| `POST /api/runtime/{id}/logo`, `POST /api/runtime/{id}/logo/off` | Logo frame / выключить; runtime token и permission `led` |
| `POST /api/runtime/{id}/audio/play` | PCM WAV bytes, максимум 64 MiB и 600 секунд; runtime token и permission `audio` |
| `POST /api/runtime/{id}/clock` | Управление часами; runtime token и permission `clock` |
| `GET/PUT /api/runtime/{id}/volume` | Получение/установка `{percent}`; runtime token и permission `volume` |
| `GET /api/bluetooth/status` | JSON-статус |
| `PUT /api/bluetooth/power` | `{on: boolean}`; доступно только когда Quasar остановлен |
| `GET /api/runtime/:id/input/devices` | Разрешённые package input device names; runtime token и permission `input` обязательны |
| `POST /api/runtime/:id/input/next-event` | `{devices, timeout_ms}`; только объявленные package устройства |
| `/api/runtime/{id}/bluetooth/status`, `/api/runtime/{id}/bluetooth/power` | Bluetooth status/power; runtime token и permission `bluetooth` |
| `/api/runtime/{id}/wifi` и его subroutes `/scan`, `/networks`, `/networks/{ssid}`, `/setup` | Wi-Fi API; runtime token и permission `wifi` |

Общий runtime сохраняет прежний Lua API и permissions. LED frame должен
содержать ровно `device.led_count` цветов; порядок RGB/BGR и способ записи
определяет hardware adapter. Профили Mini и Lite имеют разные LED counts и
backend-ы. Остальные аппаратные возможности сообщаются динамически и зависят
от устройства, обнаруженных системных интерфейсов и безопасных условий
выполнения. Наличие маршрута или Lua-метода не означает, что операция
поддерживается на конкретном устройстве. Проверяйте точные capabilities
профиля; сигнатуры и ограничения Lua API собраны в
[каталоге функций](./functions/README.md), сведения об адаптерах — в
[матрице поддержки устройств](./DEVICE_SUPPORT.md).

## Wi-Fi и своя сеть для настройки с телефона

Раздел «Wi-Fi» в Web UI и API используют Wi-Fi adapter. На устройствах с
поддерживаемым `wpa_cli` он работает с тем же wpa_supplicant, которым управляет
прошивка колонки. Сеть добавляется ключом,
выведенным из пароля (PBKDF2, как `wpa_passphrase`): пароль не хранится и не
попадает в командную строку. Сохраняется `save_config` в конфигурацию самого
wpa_supplicant, поэтому прошивка видит её как свою. Capabilities `wifi`,
`wifi.status`, `wifi.scan`, `wifi.networks` включены, когда есть `wpa_cli`
и интерфейс `wlan0`.

| Маршрут | Контракт |
| --- | --- |
| `GET /api/wifi` | `{available, current, saved, around, setup}`: текущая сеть, сохранённые, сети вокруг, состояние своей сети |
| `GET /api/wifi/networks` | `{saved, around}` без паролей |
| `POST /api/wifi/scan` | `{started}`; начать поиск сетей (`false` в режиме собственной точки доступа) |
| `POST /api/wifi/networks` | `{ssid, password}`; пустой пароль — открытая сеть. Ответ `{leaving}`: `true`, если колонка уходит со своей сети на эту |
| `DELETE /api/wifi/networks/{ssid}` | Забыть сеть |
| `GET /api/wifi/setup` | Состояние точки доступа, её доступность и последняя ошибка |
| `POST /api/wifi/setup` | `{on}`: поднять или убрать свою сеть вручную |
| `GET/PUT /api/settings` | Прочитать/сохранить `wifi_setup` (режим, имя, канал и интервалы); значения валидируются перед сохранением |

Те же операции доступны package runtime по `/api/runtime/{id}/wifi...` с
обязательными runtime token и permission `wifi`, а из Lua — если manifest
явно запросил это permission. Lua методы: `alice.wifi.status()`, `scan()`,
`add(ssid, password)`, `forget(ssid)` и `setup(on)`. `status()` включает
`current`, `saved`, `around` и `setup`; `scan()` возвращает `true`, если поиск
запущен, и `false`, когда устройство уже находится в режиме собственной точки
доступа. Ошибки backend возвращаются вызывающему коду, а не заменяются пустым
результатом.

**Своя сеть (экспериментально, на железе ещё не проверялась).** Без сети
колонка поднимает открытую сеть `BetterYAM-XXXX` (`192.168.4.1`). Телефон,
подключившийся к ней, получает адрес и DNS, который отвечает на любое имя
адресом колонки. Поэтому телефон сам предлагает «войти в сеть» и открывает
страницу выбора Wi-Fi (также `http://192.168.4.1`). DHCP и DNS встроены в
BetterYAMd, dnsmasq и udhcpd не нужны. Выбранная сеть добавляется, как только
вернётся wpa_supplicant колонки, и колонка сразу её пробует. Если за
`try_secs` она не получила адрес, своя сеть поднимается снова, а страница
сообщает, что подключиться не вышло.

Обратный путь — всегда прежняя сеть. Раз в `retry_secs` (кроме времени, когда
к своей сети подключён телефон, но не дольше трёх таких кругов) колонка на
`try_secs` уходит искать знакомые сети, поэтому после отключения света она
сама вернётся, когда поднимется роутер. wpa_supplicant прошивки
останавливается через `wpa_cli terminate` и запускается обратно ровно той
командой, какой был запущен (она запоминается из `/proc`). Если кто-то
запустит wpa_supplicant, пока поднята своя сеть, она сразу снимается: с
Wi-Fi прошивки BetterYAMd не спорит.

Точку доступа поднимает `hostapd`, если это поддерживают драйвер и прошивка
устройства. Укажите путь к совместимому `hostapd` в `<data dir>/bin/hostapd`
или `PATH`. Без рабочего backend capability `wifi.ap` равна `false`. По
умолчанию своя сеть выключена (`settings.toml`):

```toml
[wifi_setup]
mode = "auto"        # off (по умолчанию) | manual — только кнопкой | auto — сама без сети
ap_after_secs = 180  # сколько ждать без сети в режиме auto
retry_secs = 300     # как часто со своей сети искать знакомые
try_secs = 60        # сколько их искать
channel = 6          # канал 2,4 ГГц своей сети
ap_ssid = ""         # пусто — BetterYAM-<конец MAC>
hostapd = ""         # путь к hostapd; пусто — <data dir>/bin/hostapd, затем PATH
```

Прежде чем включать: снимите с колонки `tools/wifi-probe.sh`. Он только
читает: умеет ли драйвер Wi-Fi режим точки доступа, кто управляет
wpa_supplicant, свободны ли порты 53, 67 и 80.

```sh
adb shell sh < tools/wifi-probe.sh > wifi-probe.txt
```

## Безопасность и обновления

Web UI не имеет аутентификации: используйте только доверенную LAN. Обновление BetterYAMd выполняется установщиком: он выбирает asset по текущей
модели станции.

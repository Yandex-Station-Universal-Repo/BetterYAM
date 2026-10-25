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

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.
По умолчанию daemon выбирает активный LAN IPv4-адрес и слушает порт `8099`.
Задайте `--listen IP:PORT`, чтобы выбрать конкретный интерфейс или порт.
Каталог сборки — root `manifest.json`. `sh tools/build-daemons.sh` собирает все
перечисленные устройства; необязательный третий аргумент ограничивает сборку
одним ID устройства, например
`sh tools/build-daemons.sh manifest.json dist yandexmini`. Релизные daemon
бинарники автоматически собираются с удалёнными symbol/debug данными. Скрипт
создаёт `dist/manifest.json`, assets в `dist/assets/` и `dist/SHA256SUMS`.
Для полного GitHub Release добавьте CLI assets, которые собираются отдельно,
и обновите общий `SHA256SUMS`. Неизвестная модель не сопоставляется с похожим
устройством: установка завершается с ошибкой.

## YAM API и capability profile

CLI и общая Web Panel подключаются к HTTP API BetterYAMd. API и hardware backend
реализуются для конкретного устройства; frontend панели должен быть общим, а
доступные разделы и действия определяются capabilities профиля, не моделью.
Yandex Mini реализован в `yandexmini/`: `main.rs` содержит API и plugin runtime,
`led.rs` — аппаратную функцию LED. Контракт интеграции панели описан в
[WEB_PANEL_INTEGRATION.md](./WEB_PANEL_INTEGRATION.md).

`GET /device` возвращает `id`, `model`, `architecture`, `led_count`, `features`
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
| `GET /device` | Профиль, feature list и булевы capabilities |
| `POST /led`, `POST /led/off` | `{colors: ["#RRGGBB", ...]}` / выключить LED |
| `GET /sensors`, `GET /sensors/{id}` | Список `{id, kind, unit, value}` и чтение сенсора |
| `POST /sensors/ambient-light/configure` | `{name, gain, integration_ms}` |
| `GET /clock`, `PUT /clock` | `ClockInfo`; запрос принимает `time`, `brightness`, `segments` или `off` |
| `GET /api/volume`, `PUT /api/volume` | Получение уровня; установка `{percent}` |
| `POST /audio/play` | Тело запроса — PCM WAV bytes; максимум 64 MiB и 600 секунд |
| `POST /api/runtime/{id}/audio/play` | То же для binary plugins; требуются runtime token и permission `audio` |
| `GET /api/bluetooth/status` | JSON-статус |
| `PUT /api/bluetooth/power` | `{on: boolean}`; доступно только когда Quasar остановлен |
| `GET /display/backlights` | Список доступных backlight IDs |
| `PUT /display/backlight` | `{name, value}` |
| `GET /api/runtime/:id/input/devices` | Разрешённые package input device names; runtime token и permission `input` обязательны |
| `POST /api/runtime/:id/input/next-event` | `{devices, timeout_ms}`; только объявленные package устройства |
| `POST /ipc/alarm/next`, `/ipc/speech/next` | `{timeout_ms}`, ответ — event или `null` |

В текущем Mini backend реализованы LED ring (`led.ring.rgb` и `led.ring.6`),
чтение доступных Linux input event devices (`input.devices`,
`input.next_event`) и громкость через ALSA control `Master` на card 0.
Громкость 0..100 отображается на ALSA integer range; capability включается,
если `amixer` может прочитать control. Input capability включается только если
event-устройства доступны daemon-процессу; пакет может читать только имена из
`[input].devices`. Остальные перечисленные операции/API оставлены в runtime,
но Mini сообщает для неподдерживаемых операций `capabilities: false` и
возвращает понятную ошибку до появления hardware-реализации. Audio playback
доступен через `aplay`, если команда успешно перечисляет ALSA playback devices;
поддерживается только mono/stereo PCM WAV с sample rate 8..192 kHz и длительностью
не более 600 секунд. Операция передаёт данные через временный файл, удаляет его
после завершения и отменяется при остановке plugin. Proximity sensor
обнаруживается через Linux `ABS_DISTANCE`; API отдаёт значение в mm, если оно
лежит в допустимом диапазоне 0..8192, иначе `value: null`. На проверенном Mini
live API вернул `819 mm`; физическое воспроизведение аудио не проверялось.
Bluetooth status читается из `rfkill` и
`bsa_server`; capability status сохраняется доступной. Включение/выключение вызывает системный
`bsa_server_run.sh`. Power capability и каждая команда power проверяют
`pidof quasar_launcher2` и `pidof maind`; при работающем Quasar операция
отклоняется с причиной `quasar running, not verified`. Проверка выполняется
при каждом вызове, не только при старте daemon.
Сканирование, pair/connect/disconnect/remove и управление discoverable
недоступны: проверенный интерфейс для них требует долгоживущий `bt_demo`,
который перезапускает `bsa_server`. BetterYAM его не запускает. Из
`bt_devices.xml` данные и секреты не читаются и не выдаются. Lua сохраняет
прежние методы, а
`alice.<api>.is_available()` и `device.capabilities["<capability>"]` позволяют
проверить доступность без вызова операции. Для сегментов часов сохраняется
вызов `alice.clock.segments({...})`; теперь этот callable API также предоставляет
`alice.clock.segments.is_available()`.

LED frames для Mini должны содержать ровно шесть цветов. Аппаратный драйвер
останавливает запуск, если `/dev/i2c-0` или LED controller недоступны.

## Wi-Fi и своя сеть для настройки с телефона

Раздел «Wi-Fi» в Web UI и API работают через `wpa_cli` с тем же
wpa_supplicant, которым управляет прошивка колонки. Сеть добавляется ключом,
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

Точку доступа поднимает `hostapd`. В прошивке Mini его нет, а
wpa_supplicant собран без режима точки доступа, поэтому нужен статический
`hostapd` (nl80211) для armv7 в `<data dir>/bin/hostapd` или в `PATH`. Без
него capability `wifi.ap` равна `false`, а раздел Wi-Fi говорит, что своя
сеть недоступна. По умолчанию своя сеть выключена (`settings.toml`):

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

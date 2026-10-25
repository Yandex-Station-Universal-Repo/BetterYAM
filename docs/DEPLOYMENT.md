# Установка

Нужен `adb`. Установщик определяет имя модели через `uname -n`, ищет точную
запись в release `manifest.json`, проверяет SHA-256 и устанавливает выбранный
BetterYAMd в `/data/quazar/betteryam`.

```sh
curl -fsSL https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.sh | sh
```

Windows:

```powershell
irm https://raw.githubusercontent.com/Yandex-Station-Universal-Repo/BetterYAM/main/install.ps1 | iex
```

Для конкретной версии передайте её первым аргументом (`sh -s -- 0.1.13`). Удаление на Unix: `sh -s -- --uninstall`.

CLI обращается к `http://<колонка>:8099`; адрес можно переопределить `YAM_URL`.
По умолчанию daemon выбирает активный LAN IPv4-адрес и слушает порт `8099`.
Задайте `--listen IP:PORT`, чтобы выбрать конкретный интерфейс или порт.
Каталог сборки — root `manifest.json`. Запустите
`sh tools/build-daemons.sh`; скрипт собирает все перечисленные устройства и
создаёт `dist/manifest.json`, daemon assets в `dist/assets/` и `dist/SHA256SUMS`
для всех файлов в `dist/assets/`. CLI release assets, которые собираются
отдельно, также нужно положить в `dist/assets/` перед сборкой, чтобы их
checksum вошёл в общий файл. Загрузите manifest, `SHA256SUMS` и все assets как
release assets. Неизвестная модель не сопоставляется с похожим устройством:
установка завершается с ошибкой.

## YAM API и capability profile

CLI подключается к HTTP API BetterYAMd. Сервер API и Web Panel принадлежат
папке соответствующей модели; модельный backend управляет своим hardware
напрямую. Yandex Mini реализован в `yandexmini/`: `main.rs` содержит API и
plugin runtime, `led.rs` — аппаратную функцию LED.

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
| `POST /audio/play` | Тело запроса — audio bytes из файла пакета |
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
но Mini сообщает для них `capabilities: false` и возвращает понятную ошибку
до появления hardware-реализации. Bluetooth status читается из `rfkill` и
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
| `POST /api/wifi/scan` | Начать поиск сетей (на своей сети — ничего: точка доступа не сканирует) |
| `POST /api/wifi/networks` | `{ssid, password}`; пустой пароль — открытая сеть. Ответ `{leaving}`: `true`, если колонка уходит со своей сети на эту |
| `DELETE /api/wifi/networks/{ssid}` | Забыть сеть |
| `POST /api/wifi/setup` | `{on}`: поднять или убрать свою сеть вручную |

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

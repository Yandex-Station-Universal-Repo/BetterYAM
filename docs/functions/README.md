# Lua API: функции по темам

Этот каталог описывает общий Lua API BetterYAM, совместимый с YAM-плагинами:
сигнатуры, permissions, результаты и примеры вызовов. Аппаратная доступность
операций зависит от backend и профиля устройства; перед вызовом проверяйте
`device.capabilities` или соответствующий `alice.<api>.is_available()`.
Количество LED берите из `device.led_count`, а идентификаторы сенсоров —
из `alice.sensors.list()`. Не привязывайте плагины к модели устройства.

Все функции требуют соответствующее permission из `plugin.toml`, кроме
`alice.sleep`. Общие правила совместимости и разрешений описаны в
[контракте Lua API](../PLUGIN_API.md); профили текущих адаптеров и известные
ограничения — в [матрице поддержки устройств](../DEVICE_SUPPORT.md).

## Светодиоды

- [`alice.set`](led-set.md)
- [`alice.off`](led-off.md)
- [`alice.play`](led-play.md)
- [`alice.logo.set`](logo-set.md)
- [`alice.logo.off`](logo-off.md)

## Громкость и звук

- [`alice.volume.get`](volume-get.md)
- [`alice.volume.set`](volume-set.md)
- [`alice.audio.play`](audio-play.md)

## Bluetooth

- [`alice.bluetooth.status`](bluetooth-status.md)
- [`alice.bluetooth.power`](bluetooth-power.md)
- [`alice.bluetooth.scan`](bluetooth-scan.md)
- [`alice.bluetooth.pair`](bluetooth-pair.md)
- [`alice.bluetooth.connect`](bluetooth-connect.md)
- [`alice.bluetooth.disconnect`](bluetooth-disconnect.md)
- [`alice.bluetooth.remove`](bluetooth-remove.md)

## Дисплей, датчики и часы

- [`alice.display.backlights`](display-backlights.md)
- [`alice.display.set_backlight`](display-set-backlight.md)
- [`alice.sensors.list`](sensors-list.md)
- [`alice.sensors.read`](sensors-read.md)
- [`alice.sensors.configure_ambient_light`](sensors-configure-ambient-light.md)
- [`alice.clock.available`](clock-available.md)
- [`alice.clock.time`](clock-time.md)
- [`alice.clock.segments`](clock-segments.md)
- [`alice.clock.off`](clock-off.md)

## Ввод и системные события

- [`alice.input.devices`](input-devices.md)
- [`alice.input.next_event`](input-next-event.md)
- [`alice.ipc.next_alarm`](ipc-next-alarm.md)
- [`alice.ipc.next_speech`](ipc-next-speech.md)

## Wi-Fi, сеть и система

- [`alice.wifi.status`](wifi-status.md)
- [`alice.wifi.scan`](wifi-scan.md)
- [`alice.wifi.add`](wifi-add.md)
- [`alice.wifi.forget`](wifi-forget.md)
- [`alice.wifi.setup`](wifi-setup.md)
- [`alice.http.get`](http-get.md)
- [`alice.http.post`](http-post.md)
- [`alice.system.hostname`](system-hostname.md)
- [`alice.system.uptime`](system-uptime.md)
- [`alice.system.meminfo`](system-meminfo.md)
- [`alice.exec`](exec.md)
- [`alice.sleep`](sleep.md)

## Проверка доступности

- [`alice.led.is_available`](available-led.md)
- [`alice.led.ring.is_available`](available-led-ring.md)
- [`alice.led.ring.rgb.is_available`](available-led-rgb.md)
- [`alice.led.ring.segments.is_available`](available-led-segments.md)
- [`alice.logo.is_available`](available-logo.md)
- [`alice.volume.is_available`](available-volume.md)
- [`alice.audio.is_available`](available-audio.md)
- [`alice.bluetooth.is_available`](available-bluetooth.md)
- [`alice.display.is_available`](available-display.md)
- [`alice.sensors.is_available`](available-sensors.md)
- [`alice.sensors.control.is_available`](available-sensor-control.md)
- [`alice.clock.is_available`](available-clock.md)
- [`alice.clock.segments.is_available`](available-clock-segments.md)
- [`alice.input.is_available`](available-input.md)
- [`alice.wifi.is_available`](available-wifi.md)
- [`alice.ipc.alarm.is_available`](available-ipc-alarm.md)
- [`alice.ipc.speech.is_available`](available-ipc-speech.md)

`is_available()` возвращает boolean и не выполняет аппаратную операцию.
Доступность конкретного действия уточняйте по его capability; отсутствующая
capability означает `false`.

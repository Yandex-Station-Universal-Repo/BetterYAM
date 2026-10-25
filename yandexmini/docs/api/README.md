# Yandex Mini: Lua API по функциям

Каждый файл описывает один метод Lua API: сигнатуру, permission, результат,
пример и ограничения реализации Yandex Mini. Все функции доступны только при
указанном permission в `plugin.toml`, кроме `alice.sleep`.

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

Проверки возвращают boolean и не выполняют аппаратную операцию. Доступность
отдельных действий уточняйте по `device.capabilities["<capability>"]`;
отсутствующая capability означает `false`.

## Статус на проверенном Yandex Mini

- LED ring RGB: доступен, 6 элементов.
- Громкость: доступна через ALSA mixer.
- Proximity sensor: обнаружен; API возвращает показание в миллиметрах.
- Bluetooth status: доступен; при последней проверке радио было включено,
  `bsa_server` работал, а Quasar был остановлен.
- Bluetooth power: доступен только при наличии vendor script и остановленных
  `quasar_launcher2`/`maind`. Цикл выключения/включения радио проверен; он
  останавливает и перезапускает `bsa_server` и не должен запускаться при
  работающем Quasar.
- Bluetooth scan и входящее pairing вручную проверены через интерактивную
  vendor-утилиту `bt_demo`, но не через BetterYAM API. Утилита временно
  перезапускает BSA и не является backend-ом daemon. Повторное подключение
  после перезапуска BSA не подтвердилось. Тестовая пара удалена, ранее
  сохранённые пары не затронуты.
- Bluetooth scan/pair/connect/disconnect/remove через BetterYAM API на Mini
  не реализованы; соответствующие capabilities выключены. Для их надёжной
  реализации нужен поддерживаемый BSA SDK или API device daemon.
- Display, ambient-light control, clock и IPC на Mini не реализованы;
  соответствующие capabilities выключены.
- Audio capability пока означает, что `aplay` перечисляет playback-устройства.
  На протестированной прошивке короткий playback завершился ALSA ошибкой
  настройки, поэтому успешный физический вывод звука не подтверждён.
- `adc_keypad` заявляет поддержку клавиши MUTE (Linux key code 113), однако
  событие физического нажатия пока не удалось поймать через input API.

Смотрите также общий [контракт Lua API](../../../docs/PLUGIN_API.md) и
[деплоймент/API daemon](../../../docs/DEPLOYMENT.md).

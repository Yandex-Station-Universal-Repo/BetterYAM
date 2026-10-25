# `alice.input.next_event(timeout_ms)`

Ожидает одно событие с разрешённого input-устройства.

- **Permission:** `input`
- **Аргумент:** timeout в миллисекундах, `0..60000`.
- **Результат:** `nil` при timeout либо таблица
  `{device, type, code, value, time_ms}`.
- **Отмена:** ожидание периодически проверяет отмену plugin.
- **Доступность Mini:** Linux input events доступны для устройств, указанных в
  `[input].devices`.

```lua
local event = alice.input.next_event(5000)
if event and event.type == 1 then
    print(event.device, event.code, event.value)
end
```

Linux `KEY_MUTE` имеет code `113`. На проверенной Mini `adc_keypad` заявляет
поддержку key event, но фактическое событие нажатия MUTE в live-захвате не
наблюдалось. Не считайте кнопку подтверждённо доступной только по bitmap
capabilities.

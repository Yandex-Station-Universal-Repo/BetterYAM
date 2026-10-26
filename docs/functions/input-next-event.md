# `alice.input.next_event(timeout_ms)`

Ожидает одно событие с разрешённого input-устройства.

- **Permission:** `input`
- **Аргумент:** timeout в миллисекундах, `0..60000`.
- **Результат:** `nil` при timeout либо таблица
  `{device, type, code, value, time_ms}`.
- **Отмена:** ожидание периодически проверяет отмену plugin.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local event = alice.input.next_event(5000)
if event and event.type == 1 then
    print(event.device, event.code, event.value)
end
```

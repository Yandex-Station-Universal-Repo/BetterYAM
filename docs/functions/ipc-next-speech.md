# `alice.ipc.next_speech(timeout_ms)`

Ожидает событие распознанной речи через стандартный Device API.

- **Permission:** `ipc.speech`
- **Аргумент:** timeout в миллисекундах, `0..60000`.
- **Результат:** `nil` при timeout либо таблица события.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local speech = alice.ipc.next_speech(1000)
```

Распознанный текст может быть персональными данными. Не пересылайте его без
необходимости.
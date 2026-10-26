# `alice.ipc.next_alarm(timeout_ms)`

Ожидает событие будильника через стандартный Device API.

- **Permission:** `ipc.alarm`
- **Аргумент:** timeout в миллисекундах, `0..60000`.
- **Результат:** `nil` при timeout либо таблица события.
- **Доступность Mini:** нет (`ipc.alarm=false`).

```lua
local alarm = alice.ipc.next_alarm(1000)
```

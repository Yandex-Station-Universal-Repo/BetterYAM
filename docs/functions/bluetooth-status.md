# `alice.bluetooth.status()`

Возвращает состояние Bluetooth-радио и управляющего сервиса.

- **Permission:** `bluetooth`
- **Аргументы:** нет.
- **Результат:** таблица с `powered`, `server_running`,
  `power_control_available`; при ограничении также `power_control_reason`.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
local state = alice.bluetooth.status()
print(state.powered, state.server_running)
```

При последней проверке радио было включено, `bsa_server` работал, Quasar был
остановлен. Status показывает состояние радио и процесса, но не подтверждает
доступность scan/pair/connect или других Bluetooth-операций.
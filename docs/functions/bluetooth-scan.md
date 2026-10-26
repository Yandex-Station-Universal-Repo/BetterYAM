# `alice.bluetooth.scan(on)`

Запрашивает запуск или остановку поиска Bluetooth-устройств.

- **Permission:** `bluetooth`
- **Аргумент:** boolean.
- **Результат:** на поддерживающем backend — результат стандартного Device API.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.

```lua
alice.bluetooth.scan(true)
```
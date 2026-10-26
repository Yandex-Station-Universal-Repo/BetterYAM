# `alice.wifi.is_available()`

Проверяет доступность Wi-Fi adapter.

- **Permission:** `wifi`
- **Результат:** boolean без запуска scan или смены сети.
- **Доступность:** проверяйте capability этой операции в
  `device.capabilities`; отсутствие capability означает, что операция
  недоступна.
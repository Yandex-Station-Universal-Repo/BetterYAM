# `alice.bluetooth.is_available()`

Проверяет общую Bluetooth capability.

- **Permission:** `bluetooth`
- **Результат:** boolean без включения/выключения радио.
- **Mini:** status capability доступна при найденном Bluetooth rfkill device.
  Это не означает, что scan/pair/connect поддерживаются.

# `alice.input.is_available()`

Проверяет общую capability чтения input events.

- **Permission:** `input`
- **Результат:** boolean без чтения очереди событий.
- **Mini:** `true`, если daemon обнаружил доступные input devices. Проверяйте
  нужное имя через [`alice.input.devices`](input-devices.md).

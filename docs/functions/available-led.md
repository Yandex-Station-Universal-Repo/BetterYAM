# `alice.led.is_available()`

Проверяет capability `led` в активном device profile.

- **Permission:** `led`
- **Результат:** boolean; метод не отправляет frame.

```lua
if alice.led.is_available() then alice.off() end
```
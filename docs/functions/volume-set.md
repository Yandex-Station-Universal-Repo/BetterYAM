# `alice.volume.set(percent)`

Устанавливает уровень громкости.

- **Permission:** `volume`
- **Аргумент:** целое число `0..100`.
- **Результат:** `nil` при успехе; ошибка ALSA передаётся Lua.

```lua
alice.volume.set(35)
```
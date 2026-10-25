# `alice.audio.is_available()`

Проверяет capability `audio`.

- **Permission:** `audio`
- **Результат:** boolean без запуска playback.
- **Mini:** отражает обнаружение `aplay` и ALSA playback-устройств. Это
  предварительная проверка, а не гарантия, что ALSA маршрут успешно проиграет
  звук; на проверенной прошивке playback завершился ошибкой настройки.

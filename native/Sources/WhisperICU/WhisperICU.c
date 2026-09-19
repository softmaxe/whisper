#include "WhisperICU.h"
#include <stddef.h>

// ICU's stable C break-iterator ABI is exported by macOS libicucore. The SDK
// exports these symbols but omits ubrk.h; declare only the four functions used.
// https://unicode-org.github.io/icu-docs/apidoc/released/icu4c/ubrk_8h.html
struct UBreakIterator;
extern struct UBreakIterator *ubrk_open(int32_t type, const char *locale,
    const uint16_t *text, int32_t length, int32_t *status);
extern int32_t ubrk_next(struct UBreakIterator *iterator);
extern int32_t ubrk_getRuleStatus(struct UBreakIterator *iterator);
extern void ubrk_close(struct UBreakIterator *iterator);

int32_t whisper_icu_word_count(const uint16_t *text, int32_t length) {
    if (length <= 0) return 0;
    int32_t status = 0;
    // UBRK_WORD = 1. Negative ICU statuses are successful fallback warnings.
    struct UBreakIterator *iterator = ubrk_open(1, "und", text, length, &status);
    if (status > 0 || iterator == NULL) {
        if (iterator != NULL) ubrk_close(iterator);
        return -1;
    }
    int32_t count = 0;
    while (ubrk_next(iterator) != -1) {
        // Word-like categories start at UBRK_WORD_NUMBER (100); whitespace,
        // punctuation and emoji-only segments have UBRK_WORD_NONE (<100).
        if (ubrk_getRuleStatus(iterator) >= 100) count += 1;
    }
    ubrk_close(iterator);
    return count;
}

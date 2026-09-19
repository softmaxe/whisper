#ifndef WHISPER_ICU_H
#define WHISPER_ICU_H
#include <stdint.h>

// Returns word-like ICU segments, or -1 when ICU cannot initialize the iterator.
int32_t whisper_icu_word_count(const uint16_t *text, int32_t length);
#endif

import assert from "node:assert/strict"
import test from "node:test"
import {
  CONDITIONAL_SUBTITLES_ERROR,
  LANGUAGE_LIST_ERROR,
  parseLanguageList,
  validateLanguageList,
  validateViewingDraft,
} from "../src/routes/settings/viewing-validation.ts"

function viewing(overrides = {}) {
  return {
    spoilerProtection: false,
    nextEpisode: "auto",
    countdownSeconds: 10,
    episodeLimit: 0,
    audioLanguages: [],
    subtitleLanguages: [],
    preferOriginalAudio: false,
    subtitleMode: "server",
    resumeRewindSeconds: 0,
    textScale: 100,
    posterSize: 168,
    previewDelayMs: 550,
    startupDestination: "home",
    rememberFilters: false,
    hideWatched: false,
    ...overrides,
  }
}

test("parseLanguageList trims, lowercases, and drops empty entries", () => {
  assert.deepEqual(parseLanguageList(" EN , pt-BR, , de "), ["en", "pt-br", "de"])
  assert.deepEqual(parseLanguageList(" ,  "), [])
})

test("validateLanguageList matches native length and character rules", () => {
  assert.equal(validateLanguageList([]), null)
  assert.equal(validateLanguageList(["en", "eng", "pt-br", "zh-hans-cn"]), null)
  assert.equal(validateLanguageList(["a", "b", "c", "d", "e", "f", "g", "h"]), LANGUAGE_LIST_ERROR)
  assert.equal(validateLanguageList(["en", "de", "fr", "es", "it", "ja", "ko", "zh"]), null)
  assert.equal(validateLanguageList(["en", "de", "fr", "es", "it", "ja", "ko", "zh", "pt"]), LANGUAGE_LIST_ERROR)
  for (const invalid of ["e", "abcdefghijklm", "en1", "en_us", "en us", "de!"]) {
    assert.equal(validateLanguageList([invalid]), LANGUAGE_LIST_ERROR, invalid)
  }
})

test("validateViewingDraft enforces conditional subtitles and numeric bounds", () => {
  const missingAudio = validateViewingDraft(viewing({ subtitleMode: "foreignAudio" }), "", "")
  assert.equal(missingAudio.valid, false)
  assert.equal(missingAudio.subtitleModeError, CONDITIONAL_SUBTITLES_ERROR)

  const withAudio = validateViewingDraft(viewing({ subtitleMode: "foreignAudio" }), "en, de", "")
  assert.equal(withAudio.valid, true)
  assert.equal(withAudio.subtitleModeError, null)

  const badSubtitles = validateViewingDraft(viewing(), "en", "en_US")
  assert.equal(badSubtitles.valid, false)
  assert.equal(badSubtitles.subtitleLanguagesError, LANGUAGE_LIST_ERROR)

  const badRewind = validateViewingDraft(viewing({ resumeRewindSeconds: 15 }), "", "")
  assert.equal(badRewind.valid, false)
  assert.equal(badRewind.numbersValid, false)
})

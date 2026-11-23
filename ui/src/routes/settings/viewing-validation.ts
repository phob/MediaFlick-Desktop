import type { ViewingSettings } from "../../lib/api/types/settings.ts"
import { isSettingsNumberValid } from "../../lib/settings-numbers.ts"

const LANGUAGE_CODE_PATTERN = /^[a-z-]{2,12}$/
const RESUME_REWIND_VALUES = new Set([0, 5, 10, 30])

export const LANGUAGE_LIST_ERROR = "Use up to eight language codes, such as en, eng, or de."
export const CONDITIONAL_SUBTITLES_ERROR = "Choose a preferred audio language for conditional subtitles."

export type ViewingValidation = {
  audioLanguagesError: string | null
  subtitleLanguagesError: string | null
  subtitleModeError: string | null
  numbersValid: boolean
  valid: boolean
}

export function parseLanguageList(text: string | undefined): string[] {
  return (text ?? "")
    .split(",")
    .map((value) => value.trim().toLowerCase())
    .filter(Boolean)
}

export function validateLanguageList(languages: readonly string[]): string | null {
  if (languages.length > 8 || languages.some((language) => !LANGUAGE_CODE_PATTERN.test(language))) {
    return LANGUAGE_LIST_ERROR
  }
  return null
}

export function validateViewingDraft(
  draft: ViewingSettings,
  audioText: string | undefined,
  subtitleText: string | undefined,
): ViewingValidation {
  const audioLanguages = parseLanguageList(audioText)
  const subtitleLanguages = parseLanguageList(subtitleText)
  const audioLanguagesError = validateLanguageList(audioLanguages)
  const subtitleLanguagesError = validateLanguageList(subtitleLanguages)
  const subtitleModeError = draft.subtitleMode === "foreignAudio" && audioLanguages.length === 0
    ? CONDITIONAL_SUBTITLES_ERROR
    : null
  const numbersValid = isSettingsNumberValid(draft.countdownSeconds, 3, 60)
    && isSettingsNumberValid(draft.episodeLimit, 0, 20)
    && RESUME_REWIND_VALUES.has(draft.resumeRewindSeconds)
    && isSettingsNumberValid(draft.textScale, 80, 150)
    && isSettingsNumberValid(draft.posterSize, 120, 240)
    && isSettingsNumberValid(draft.previewDelayMs, 200, 2000)

  return {
    audioLanguagesError,
    subtitleLanguagesError,
    subtitleModeError,
    numbersValid,
    valid: numbersValid && !audioLanguagesError && !subtitleLanguagesError && !subtitleModeError,
  }
}

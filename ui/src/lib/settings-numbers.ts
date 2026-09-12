export function isSettingsNumberValid(value: number, min: number, max: number) {
  return Number.isInteger(value) && value >= min && value <= max
}

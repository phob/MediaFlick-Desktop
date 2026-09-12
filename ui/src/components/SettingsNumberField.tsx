import { useId } from "react"
import { Input } from "@/components/ui/input"
import { Slider } from "@/components/ui/slider"
import { isSettingsNumberValid } from "@/lib/settings-numbers"

export default function SettingsNumberField({
  id,
  label,
  value,
  min,
  max,
  sliderStep = 1,
  disabled = false,
  validate = true,
  unit,
  onValueChange,
  "aria-describedby": descriptionId,
}: {
  id?: string
  label: string
  value: number
  min: number
  max: number
  sliderStep?: number
  disabled?: boolean
  validate?: boolean
  unit?: string
  onValueChange: (value: number) => void
  "aria-describedby"?: string
}) {
  const generatedId = useId()
  const inputId = id ?? generatedId
  const invalid = validate && !isSettingsNumberValid(value, min, max)
  const describedBy = [descriptionId, invalid ? `${inputId}-error` : undefined].filter(Boolean).join(" ") || undefined
  return <div className="w-80 max-w-full space-y-2">
    <div className="flex items-center gap-4">
      <Slider
        aria-label={`${label} slider`}
        aria-describedby={describedBy}
        aria-invalid={invalid}
        aria-valuetext={unit && Number.isFinite(value) ? `${value} ${unit}` : undefined}
        min={min}
        max={max}
        step={sliderStep}
        disabled={disabled}
        value={[Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : min]}
        onValueChange={([next]) => onValueChange(next)}
      />
      <Input
        id={inputId}
        aria-label={label}
        aria-describedby={describedBy}
        aria-invalid={invalid}
        className="w-20 shrink-0 appearance-textfield text-right tabular-nums [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={1}
        disabled={disabled}
        value={Number.isFinite(value) ? value : ""}
        onChange={(event) => onValueChange(event.target.valueAsNumber)}
      />
    </div>
    {invalid && <p id={`${inputId}-error`} className="text-sm text-destructive" role="alert">Enter a whole number from {min} to {max}.</p>}
  </div>
}

import { RefreshCw } from "lucide-react"
import type { ReactNode } from "react"
import { Link as RouterLink } from "react-router-dom"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"

/** One labelled setting: title and help on the left, its control on the right. */
export function SettingsRow({
  title,
  description,
  controlId,
  children,
}: {
  title: string
  description?: string
  controlId?: string
  children: ReactNode
}) {
  return (
    <div className="settings-row">
      <div className="min-w-0">
        <h3 className="font-medium">{controlId ? <Label htmlFor={controlId}>{title}</Label> : title}</h3>
        {description && (
          <p id={controlId ? `${controlId}-help` : undefined} className="mt-1 text-sm text-muted-foreground">
            {description}
          </p>
        )}
      </div>
      <div className="settings-control">{children}</div>
    </div>
  )
}

export type SelectOption<Value extends string> = {
  value: Value
  label: string
  disabled?: boolean
}

export function SelectField<const Value extends string>({
  value,
  onValueChange,
  options,
  label,
  id,
  "aria-describedby": descriptionId,
}: {
  id?: string
  "aria-describedby"?: string
  value: Value
  onValueChange: (value: Value) => void
  options: readonly SelectOption<Value>[]
  label: string
}) {
  const selectOption = (candidate: string) => {
    const selected = options.find((option) => option.value === candidate)?.value
    if (selected !== undefined) onValueChange(selected)
  }
  return (
    <Select value={value} onValueChange={selectOption}>
      <SelectTrigger
        id={id}
        aria-label={label}
        aria-describedby={descriptionId}
        className="w-64 max-w-full h-auto min-h-9 whitespace-normal [&_[data-slot=select-value]]:line-clamp-none"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value} disabled={option.disabled}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export function Section({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">{children}</CardContent>
    </Card>
  )
}

export function PageTitle({ title }: { title: string }) {
  return (
    <header className="settings-page-title">
      <h1>{title}</h1>
    </header>
  )
}

export function SettingsLoading() {
  return (
    <div className="settings-page text-sm text-muted-foreground" role="status">
      Loading settings…
    </div>
  )
}

export function SettingsError({
  title = "Settings unavailable",
  error,
  onRetry,
}: {
  title?: string
  error: Error
  onRetry: () => void
}) {
  return (
    <div className="settings-page">
      <PageTitle title={title} />
      <Section title="Could not load settings" description={error.message}>
        <Button variant="outline" onClick={onRetry}>
          <RefreshCw /> Try again
        </Button>
      </Section>
    </div>
  )
}

export function SignInRequired({ name }: { name: string }) {
  return (
    <div className="settings-page">
      <PageTitle title={name} />
      <Section title="Sign in required" description={`Sign in to your Jellyfin server to view or configure ${name}.`}>
        <Button asChild>
          <RouterLink to="/">Go to sign in</RouterLink>
        </Button>
      </Section>
    </div>
  )
}

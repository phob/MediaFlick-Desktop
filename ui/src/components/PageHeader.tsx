import type { ReactNode } from "react"
import { cn } from "@/lib/utils"

export function PageHeader({
  eyebrow,
  title,
  description,
  actions,
  contentClassName,
}: {
  eyebrow: string
  title: string
  description?: string
  actions?: ReactNode
  /** Keeps page actions on the same content canvas as a constrained body. */
  contentClassName?: string
}) {
  return (
    <header className="page-header shrink-0 px-6 pt-10 pb-6 sm:px-10 lg:px-14">
      <div className={cn("flex flex-wrap items-end justify-between gap-6", contentClassName)}>
        <div className="flex max-w-3xl flex-col gap-3">
          <p className="flex items-center gap-2 text-xs font-semibold tracking-[0.18em] text-foreground/65 uppercase">
            <span className="size-1.5 rounded-full bg-primary" aria-hidden />
            {eyebrow}
          </p>
          <div className="space-y-2">
            <h1 className="text-3xl leading-none font-black tracking-[-0.035em] text-balance sm:text-4xl">
              {title}
            </h1>
            {description && (
              <p className="max-w-2xl text-sm leading-relaxed text-muted-foreground sm:text-base">
                {description}
              </p>
            )}
          </div>
        </div>
        {actions}
      </div>
    </header>
  )
}

export function PageEmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode
  title: string
  description: string
  action?: ReactNode
}) {
  return (
    <div className="cinematic-empty-state flex min-h-64 flex-col items-center justify-center gap-4 rounded-2xl border border-white/5 bg-card/35 px-6 py-12 text-center">
      <div className="grid size-14 place-items-center rounded-full bg-white/5 text-muted-foreground">
        {icon}
      </div>
      <div className="max-w-md space-y-1.5">
        <h2 className="text-lg font-semibold">{title}</h2>
        <p className="text-sm leading-relaxed text-muted-foreground">{description}</p>
      </div>
      {action}
    </div>
  )
}

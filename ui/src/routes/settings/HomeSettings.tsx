import { useMutation, useQueryClient } from "@tanstack/react-query"
import { ArrowDown, ArrowUp, GripVertical } from "lucide-react"
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react"
import { createPortal } from "react-dom"
import { toast } from "sonner"
import SaveBar from "@/components/SettingsSaveBar"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Label } from "@/components/ui/label"
import { useSourceDraft } from "@/hooks/use-source-draft"
import { api, homeSettingsWrite, type HomeConfiguration } from "@/lib/api"
import { accountKey, useHomeSettings, useStatus } from "@/lib/queries"
import { queryKeys } from "@/lib/query-client"
import { same } from "./helpers"
import { PageTitle, Section, SettingsError, SettingsLoading, SettingsRow, SignInRequired } from "./shared"

type HomeElement = HomeConfiguration["elements"][number]

type HomeDrag = {
  key: string
  pointerId: number
  x: number
  y: number
  offsetX: number
  offsetY: number
  width: number
  height: number
  dropIndex: number
}

type WatchingOption = keyof HomeConfiguration["watching"]

const WATCHING_OPTIONS: ReadonlyArray<[WatchingOption, string]> = [
  ["continueWatching", "Continue Watching"],
  ["nextUp", "Next Up"],
  ["combine", "Combine shelves"],
]

const homeElementKey = (element: HomeElement) => `${element.kind}:${element.id}`

const isWatching = (element: HomeElement) => element.kind === "builtIn" && element.id === "watching"

function dropHomeElement(configuration: HomeConfiguration, key: string, dropIndex: number) {
  const visible = configuration.elements.filter((element) => element.available)
  const from = visible.findIndex((element) => homeElementKey(element) === key)
  if (from < 0) return configuration
  const [dragged] = visible.splice(from, 1)
  visible.splice(Math.max(0, Math.min(dropIndex, visible.length)), 0, dragged)
  let visibleIndex = 0
  return {
    ...configuration,
    elements: configuration.elements.map((element) => element.available ? visible[visibleIndex++] : element),
  }
}

function DragPreview({ element, dragging }: { element: HomeElement; dragging: HomeDrag }) {
  return createPortal(
    <div
      aria-hidden
      data-testid="home-drag-preview"
      className="pointer-events-none fixed z-[100] rotate-[0.35deg] scale-[1.015] rounded-lg border border-primary/50 bg-card/95 p-3 opacity-95 shadow-2xl ring-1 ring-primary/30"
      style={{
        left: dragging.x - dragging.offsetX,
        top: dragging.y - dragging.offsetY,
        width: dragging.width,
        minHeight: dragging.height,
      }}
    >
      <div className="flex items-center gap-3">
        <span className="flex size-8 shrink-0 items-center justify-center text-primary">
          <GripVertical className="size-4" />
        </span>
        <Checkbox checked={element.enabled} disabled tabIndex={-1} />
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">{element.label}</div>
          <div className="text-xs text-muted-foreground">{element.category}</div>
        </div>
      </div>
      {isWatching(element) && (
        <div className="mt-3 ml-7 border-t pt-3 text-sm text-muted-foreground">
          {WATCHING_OPTIONS.map(([, label]) => label).join(" · ")}
        </div>
      )}
    </div>,
    document.body,
  )
}

export default function HomeSettings() {
  const cache = useQueryClient()
  const status = useStatus()
  const query = useHomeSettings(Boolean(status.data?.authenticated))
  const [draft, setDraft, updateDraft, acceptSaved] = useSourceDraft(query.data?.settings, accountKey(status.data))
  const [dragging, setDragging] = useState<HomeDrag | null>(null)
  const dragRef = useRef<HomeDrag | null>(null)
  const visible = useMemo(() => draft?.elements.filter((element) => element.available) ?? [], [draft])
  useEffect(() => {
    if (!dragging?.key) return
    const movePointer = (event: PointerEvent) => {
      const current = dragRef.current
      if (!current || event.pointerId !== current.pointerId) return
      const remainingKeys = visible
        .map(homeElementKey)
        .filter((key) => key !== current.key)
      let dropIndex = remainingKeys.length
      for (const row of document.querySelectorAll<HTMLElement>("[data-home-element-key]")) {
        const index = remainingKeys.indexOf(row.dataset.homeElementKey ?? "")
        if (index < 0) continue
        const bounds = row.getBoundingClientRect()
        if (event.clientY < bounds.top + bounds.height / 2) {
          dropIndex = index
          break
        }
      }
      const next = { ...current, x: event.clientX, y: event.clientY, dropIndex }
      dragRef.current = next
      setDragging(next)
    }
    const dropPointer = (event: PointerEvent) => {
      const current = dragRef.current
      if (!current || event.pointerId !== current.pointerId) return
      updateDraft((configuration) =>
        configuration ? dropHomeElement(configuration, current.key, current.dropIndex) : configuration)
      dragRef.current = null
      setDragging(null)
    }
    const cancelDrag = () => {
      dragRef.current = null
      setDragging(null)
    }
    const cancelWithEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") cancelDrag()
    }
    window.addEventListener("pointermove", movePointer)
    window.addEventListener("pointerup", dropPointer)
    window.addEventListener("pointercancel", cancelDrag)
    window.addEventListener("keydown", cancelWithEscape)
    window.addEventListener("blur", cancelDrag)
    return () => {
      window.removeEventListener("pointermove", movePointer)
      window.removeEventListener("pointerup", dropPointer)
      window.removeEventListener("pointercancel", cancelDrag)
      window.removeEventListener("keydown", cancelWithEscape)
      window.removeEventListener("blur", cancelDrag)
    }
  }, [dragging?.key, updateDraft, visible])
  const mutation = useMutation({
    mutationFn: (value: HomeConfiguration) => api.saveHomeSettings(homeSettingsWrite(value)),
    onSuccess: (saved, submitted) => {
      acceptSaved(saved.settings, submitted)
      cache.setQueryData(queryKeys.homeSettings, saved)
      void cache.invalidateQueries({ queryKey: queryKeys.home })
      void cache.invalidateQueries({ queryKey: queryKeys.homeResume })
      cache.removeQueries({ queryKey: queryKeys.billboard })
      toast.success("Home settings saved")
    },
    onError: (error: Error) => toast.error(error.message),
  })
  if (status.isPending) return <SettingsLoading />
  if (!status.data?.authenticated) return <SignInRequired name="Home" />
  if (query.error && !query.data) {
    return (
      <SettingsError title="Home settings unavailable" error={query.error} onRetry={() => void query.refetch()} />
    )
  }
  if (!query.data || !draft) return <SettingsLoading />

  const move = (fromKey: string, toKey: string) => {
    if (fromKey === toKey) return
    updateDraft((current) => {
      if (!current) return current
      const from = current.elements.findIndex((element) => homeElementKey(element) === fromKey)
      const to = current.elements.findIndex((element) => homeElementKey(element) === toKey)
      if (from < 0 || to < 0) return current
      const elements = [...current.elements]
      const [element] = elements.splice(from, 1)
      elements.splice(to, 0, element)
      return { ...current, elements }
    })
  }
  const moveVisible = (index: number, offset: number) => {
    const target = visible[index + offset]
    if (target) move(homeElementKey(visible[index]), homeElementKey(target))
  }
  const setElementEnabled = (key: string, enabled: boolean) => updateDraft((current) => current ? {
    ...current,
    elements: current.elements.map((element) => homeElementKey(element) === key ? { ...element, enabled } : element),
  } : current)
  const setWatching = (option: WatchingOption, enabled: boolean) =>
    setDraft({ ...draft, watching: { ...draft.watching, [option]: enabled } })
  const startDrag = (key: string, visibleIndex: number) => (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return
    const row = event.currentTarget.closest<HTMLElement>("[data-home-element-key]")
    if (!row) return
    event.preventDefault()
    const bounds = row.getBoundingClientRect()
    const next = {
      key,
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      offsetX: event.clientX - bounds.left,
      offsetY: event.clientY - bounds.top,
      width: bounds.width,
      height: bounds.height,
      dropIndex: visibleIndex,
    }
    dragRef.current = next
    setDragging(next)
  }
  const draggedElement = dragging ? visible.find((element) => homeElementKey(element) === dragging.key) : null
  const remaining = dragging ? visible.filter((element) => homeElementKey(element) !== dragging.key) : visible
  const slots = remaining.length + (dragging ? 1 : 0)

  return (
    <div className="settings-page">
      <PageTitle title="Home" />
      <Section title="Billboard" description="The billboard stays fixed above every shelf.">
        <SettingsRow
          controlId="settings-show-billboard"
          title="Show billboard"
          description="Rotate a small selection of titles with landscape artwork."
        >
          <Checkbox
            id="settings-show-billboard"
            aria-describedby="settings-show-billboard-help"
            checked={draft.billboard}
            onCheckedChange={(checked) => setDraft({ ...draft, billboard: checked === true })}
            aria-label="Show billboard"
          />
        </SettingsRow>
      </Section>
      <Section
        title="Shelves"
        description="Disabled shelves keep their positions. Drag a handle or use the arrow buttons to reorder."
      >
        <div className="space-y-2">
          {Array.from({ length: slots }, (_, slot) => {
            if (dragging && slot === dragging.dropIndex) {
              return (
                <div
                  key="home-drop-placeholder"
                  data-testid="home-drop-placeholder"
                  aria-hidden
                  className="rounded-lg border-2 border-dashed border-primary/60 bg-primary/10 shadow-inner"
                  style={{ height: dragging.height }}
                />
              )
            }
            const index = dragging && slot > dragging.dropIndex ? slot - 1 : slot
            const element = remaining[index]
            if (!element) return null
            const key = homeElementKey(element)
            const visibleIndex = visible.findIndex((candidate) => homeElementKey(candidate) === key)
            const checkboxId = `home-element-${encodeURIComponent(key)}`
            return (
              <div key={key} data-home-element-key={key} className="rounded-lg border bg-card p-3">
                <div className="flex items-center gap-3">
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="ghost"
                    aria-label={`Drag ${element.label}`}
                    className="shrink-0 touch-none select-none cursor-grab text-muted-foreground active:cursor-grabbing"
                    onPointerDown={startDrag(key, visibleIndex)}
                  >
                    <GripVertical className="size-4" aria-hidden />
                  </Button>
                  <Checkbox
                    id={checkboxId}
                    checked={element.enabled}
                    onCheckedChange={(checked) => setElementEnabled(key, checked === true)}
                    aria-label={`Show ${element.label}`}
                  />
                  <div className="min-w-0 flex-1">
                    <Label htmlFor={checkboxId} className="truncate leading-normal">{element.label}</Label>
                    <div className="text-xs text-muted-foreground">{element.category}</div>
                  </div>
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="ghost"
                    disabled={visibleIndex === 0}
                    aria-label={`Move ${element.label} up`}
                    onClick={() => moveVisible(visibleIndex, -1)}
                  >
                    <ArrowUp />
                  </Button>
                  <Button
                    type="button"
                    size="icon-sm"
                    variant="ghost"
                    disabled={visibleIndex === visible.length - 1}
                    aria-label={`Move ${element.label} down`}
                    onClick={() => moveVisible(visibleIndex, 1)}
                  >
                    <ArrowDown />
                  </Button>
                </div>
                {isWatching(element) && (
                  <div className="mt-3 ml-7 grid gap-3 border-t pt-3 sm:grid-cols-3">
                    {WATCHING_OPTIONS.map(([option, label]) => (
                      <Label key={option} className="font-normal leading-normal">
                        <Checkbox
                          checked={draft.watching[option]}
                          onCheckedChange={(checked) => setWatching(option, checked === true)}
                        />
                        {label}
                      </Label>
                    ))}
                  </div>
                )}
              </div>
            )
          })}
        </div>
        {query.data.collectionMode === "jellyfin" && (
          <p className="text-xs text-muted-foreground">
            My Collection shelves are hidden while Jellyfin collection mode is active.
          </p>
        )}
      </Section>
      <SaveBar
        dirty={!same(draft, query.data.settings)}
        saving={mutation.isPending}
        onSave={() => mutation.mutate(draft)}
        onDiscard={() => setDraft(query.data.settings)}
        onReset={() => setDraft(query.data.defaults)}
      />
      {dragging && draggedElement && <DragPreview element={draggedElement} dragging={dragging} />}
    </div>
  )
}

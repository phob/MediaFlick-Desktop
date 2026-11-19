import { CoverCard } from "@/components/CoverCard"

/**
 * Shown instead of the app, sign-in included, when the services failed to
 * open at startup. The reason says what to do; nothing here touches the file
 * it names.
 */
export function StartupFailure({ reason }: { reason: string }) {
  return <CoverCard title="MediaFlick could not start" description={<span className="break-words select-text">{reason}</span>} />
}

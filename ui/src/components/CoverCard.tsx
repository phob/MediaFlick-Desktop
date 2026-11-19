import type { ReactNode } from "react"
import appIcon from "../../../distribution/app-icon.png?inline"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"

/**
 * The app icon over one card on the sign-in backdrop, for a screen shown in
 * place of the app when it cannot continue.
 */
export function CoverCard({ title, description, children }: { title: string; description: ReactNode; children?: ReactNode }) {
  return (
    <div className="signin-page relative grid h-full place-items-center overflow-x-hidden overflow-y-auto p-6">
      <div className="relative z-10 flex w-full max-w-md flex-col gap-7">
        <img
          src={appIcon}
          alt="MediaFlick"
          className="mx-auto size-28 drop-shadow-[0_1.25rem_2rem_rgb(0_0_0/45%)]"
        />
        <Card className="cinematic-panel w-full gap-5 rounded-2xl py-7">
          <CardHeader className="gap-2">
            <CardTitle role="heading" aria-level={1} className="text-xl">
              {title}
            </CardTitle>
            <CardDescription>{description}</CardDescription>
          </CardHeader>
          {children ? <CardContent className="flex flex-col gap-4">{children}</CardContent> : null}
        </Card>
      </div>
    </div>
  )
}

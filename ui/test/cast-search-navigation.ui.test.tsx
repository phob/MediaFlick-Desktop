import { fireEvent, render, screen } from "@testing-library/react"
import type { ReactNode } from "react"
import { MemoryRouter, useLocation } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { CastRow } from "../src/components/detail/CastRow"
import type { Person, SeerrMediaDetail } from "../src/lib/api"
import { Cast } from "../src/routes/DiscoverDetail"

function LocationProbe() {
  const location = useLocation()
  return <output data-location>{location.pathname + location.search}</output>
}

function currentParams() {
  const value = document.querySelector("[data-location]")?.textContent ?? ""
  return new URL(value, "https://app.test").searchParams
}

const jellyfinPerson: Person = {
  id: "jf-keanu",
  name: "Keanu Reeves",
  role: "Neo",
  type: "Actor",
  imageTag: null,
}

function wrapper({ children }: { children: ReactNode }) {
  return (
    <MemoryRouter initialEntries={["/item/matrix"]}>
      {children}
      <LocationProbe />
    </MemoryRouter>
  )
}

describe("cast click navigation", () => {
  test("Jellyfin cast links enter exact person mode with keyboard-usable semantics", () => {
    render(<CastRow people={[jellyfinPerson]} />, { wrapper })

    const link = screen.getByRole("link", { name: "Find titles featuring Keanu Reeves" })
    expect(link.getAttribute("href")).toContain("mode=person")
    fireEvent.click(link)

    expect(currentParams().get("personId")).toBe("jf-keanu")
    expect(currentParams().get("personName")).toBe("Keanu Reeves")
    expect(currentParams().get("search")).toBeNull()
  })

  test("Seerr cast links preserve the exact TMDB person namespace", () => {
    const detail = {
      cast: [{ id: 6384, name: "Keanu Reeves", character: "Neo", profilePath: null }],
    } as SeerrMediaDetail
    render(<Cast detail={detail} />, { wrapper })

    fireEvent.click(screen.getByRole("link", { name: "Find titles featuring Keanu Reeves" }))

    expect(currentParams().get("tmdbPersonId")).toBe("6384")
    expect(currentParams().get("personName")).toBe("Keanu Reeves")
  })
})

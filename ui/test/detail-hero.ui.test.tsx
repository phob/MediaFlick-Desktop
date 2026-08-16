import { render, screen } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { describe, expect, test } from "vitest"
import { DetailHeroLayout } from "../src/components/detail/DetailHeroLayout"

describe("shared detail hero", () => {
  test("always renders the supplied return action as part of the shared shell", () => {
    render(
      <MemoryRouter>
        <DetailHeroLayout
          back={{ to: "/library?kind=Movie", label: "Back to library" }}
          title="Example title"
          facts={["Movie", "2026"]}
          genres={[{ label: "Drama" }]}
          overview={<p>Example overview</p>}
        />
      </MemoryRouter>,
    )

    expect(screen.getByTestId("detail-hero-layout")).toBeTruthy()
    expect(screen.getByRole("link", { name: "Back to library" }).getAttribute("href"))
      .toBe("/library?kind=Movie")
    expect(screen.getByRole("heading", { name: "Example title" })).toBeTruthy()
    expect(screen.getByText("Example overview")).toBeTruthy()
  })
})

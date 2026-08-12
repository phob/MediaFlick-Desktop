import { render, screen } from "@testing-library/react"
import { describe, expect, test } from "vitest"
import { SeerrRequestStatusBadge } from "../src/components/seerr/SeerrStatusBadge"

describe("Seerr request status continuity", () => {
  test("suppresses an unknown request state when a concrete media state is shown", () => {
    const { rerender } = render(<SeerrRequestStatusBadge status="unknown" suppressUnknown />)
    expect(screen.queryByText("Unknown")).toBeNull()

    rerender(<SeerrRequestStatusBadge status="unknown" />)
    expect(screen.getByText("Unknown")).toBeTruthy()
  })

  test("never suppresses actionable request states", () => {
    render(<SeerrRequestStatusBadge status="pending" suppressUnknown />)
    expect(screen.getByText("Awaiting approval")).toBeTruthy()
  })
})

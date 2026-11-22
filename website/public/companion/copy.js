const button = document.querySelector("[data-copy]")
const source = document.querySelector("[data-copy-source]")
const label = document.querySelector("[data-copy-label]")

if (button && source && label && navigator.clipboard) {
  button.hidden = false
  let reset = 0
  button.addEventListener("click", () => {
    navigator.clipboard.writeText(source.textContent.trim()).then(
      () => {
        label.textContent = "Copied"
        button.setAttribute("data-copied", "")
      },
      () => {
        label.textContent = "Select and copy"
      },
    )
    clearTimeout(reset)
    reset = setTimeout(() => {
      label.textContent = "Copy URL"
      button.removeAttribute("data-copied")
    }, 2000)
  })
}

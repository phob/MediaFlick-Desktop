// flick.media behaviour. The page is complete without it; this adds the
// accent switch, scroll reveals, in-view video playback, the hero's readouts,
// the scroll walkthrough, the player and Companion tabs, the gallery lightbox
// and release-aware download links.

const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)")
const root = document.documentElement

// Replaces an element's text with a short entrance, so a changing title reads
// as a change rather than a flicker.
function swapText(element, text) {
  if (!element || typeof text !== "string" || element.textContent === text) return
  element.textContent = text
  element.removeAttribute("data-swapped")
  void element.offsetWidth
  element.setAttribute("data-swapped", "")
}

/* ------------------------------------------------------------ accent */

const ACCENTS = ["signal", "cobalt", "amber", "violet"]
const accentButtons = document.querySelectorAll("[data-accent-choice]")

function applyAccent(value) {
  if (!ACCENTS.includes(value)) return
  if (value === "signal") delete root.dataset.accent
  else root.dataset.accent = value
  for (const button of accentButtons) button.setAttribute("aria-pressed", String(button.dataset.accentChoice === value))
}

try {
  const saved = localStorage.getItem("mf-accent")
  if (saved) applyAccent(saved)
} catch {
  // Storage can be unavailable (private windows); the default accent stays.
}
for (const button of accentButtons) {
  button.addEventListener("click", () => {
    applyAccent(button.dataset.accentChoice)
    try {
      localStorage.setItem("mf-accent", button.dataset.accentChoice)
    } catch {
      // Not persisted; the choice still applies to this visit.
    }
  })
}

/* ------------------------------------------------------------ chrome */

const chrome = document.querySelector("[data-chrome]")
const onScroll = () => chrome?.toggleAttribute("data-scrolled", scrollY > 8)
addEventListener("scroll", onScroll, { passive: true })
onScroll()

// The section crossing the middle of the viewport is current in the nav and
// the spine; the nav's underline slides to it.
const chromeNav = document.querySelector(".chrome-nav")
const sectionLinks = [...document.querySelectorAll(".chrome-nav a[href^='#'], .spine a[href^='#']")]
function setCurrentSection(id) {
  for (const link of sectionLinks) link.toggleAttribute("aria-current", link.getAttribute("href") === `#${id}`)
  const active = chromeNav?.querySelector(`a[href="#${id}"]`)
  if (!chromeNav) return
  if (active) {
    chromeNav.style.setProperty("--ink-x", `${active.offsetLeft}px`)
    chromeNav.style.setProperty("--ink-w", `${active.offsetWidth}px`)
    chromeNav.setAttribute("data-ink", "")
  } else {
    chromeNav.removeAttribute("data-ink")
  }
}
const sectionObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) if (entry.isIntersecting) setCurrentSection(entry.target.id)
  },
  { rootMargin: "-45% 0px -50% 0px" },
)
for (const section of document.querySelectorAll("main section[id]")) sectionObserver.observe(section)

/* ------------------------------------------------------------ ticker */

// The ticker runs at reading pace and speeds up with the scroll, easing back
// once the page settles.
const tickerTrack = document.querySelector(".ticker-track")
if (tickerTrack && !reducedMotion.matches) {
  let lastY = scrollY
  let lastTime = performance.now()
  let boost = 0
  let running = false
  const settle = () => {
    const animation = tickerTrack.getAnimations()[0]
    boost *= 0.9
    if (animation) animation.playbackRate = 1 + boost
    if (boost > 0.02) requestAnimationFrame(settle)
    else running = false
  }
  addEventListener(
    "scroll",
    () => {
      const now = performance.now()
      boost = Math.min(6, (Math.abs(scrollY - lastY) / Math.max(1, now - lastTime)) * 1.6)
      lastY = scrollY
      lastTime = now
      if (!running) {
        running = true
        requestAnimationFrame(settle)
      }
    },
    { passive: true },
  )
}

/* ------------------------------------------------------------ reveal */

// Siblings that reveal together are staggered by their order in the parent.
for (const element of document.querySelectorAll(".reveal")) {
  const siblings = [...(element.parentElement?.children ?? [])].filter((child) => child.classList.contains("reveal"))
  const index = siblings.indexOf(element)
  if (index > 0) element.style.setProperty("--reveal-delay", `${Math.min(index, 6) * 80}ms`)
}
const revealObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue
      entry.target.classList.add("is-visible")
      revealObserver.unobserve(entry.target)
    }
  },
  { rootMargin: "0px 0px -8% 0px", threshold: 0.08 },
)
for (const element of document.querySelectorAll(".reveal")) revealObserver.observe(element)

/* ------------------------------------------------------------ videos */

// Loops play only while visible, and not at all under reduced motion, where
// they keep their poster and gain controls instead.
const videos = [...document.querySelectorAll("video[data-autoplay]")]
function playIfAllowed(video) {
  if (reducedMotion.matches || video.closest("[hidden]")) return
  video.play().catch(() => {
    // Autoplay can be refused (data saver, battery); the poster remains.
  })
}
const videoObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      const video = entry.target
      if (entry.isIntersecting) playIfAllowed(video)
      else video.pause()
    }
  },
  { threshold: 0.3 },
)
function applyMotionPreference() {
  for (const video of videos) {
    if (reducedMotion.matches) {
      video.pause()
      video.controls = true
    } else {
      video.controls = false
    }
  }
}
applyMotionPreference()
reducedMotion.addEventListener("change", applyMotionPreference)
for (const video of videos) videoObserver.observe(video)

/* ------------------------------------------------------------ hero readouts */

// The recording tours the library and then starts playback. The readouts
// around the window follow it: library ones first, playback ones once the
// film starts, and the window's title says which is which.
const heroStage = document.querySelector("[data-hero-stage]")
const heroVideo = document.querySelector("[data-hero]")
if (heroStage && heroVideo) {
  const heroTitle = heroStage.querySelector("[data-hero-title]")
  const playAt = Number(heroVideo.dataset.playAt) || 0
  const setPhase = (phase) => {
    if (heroStage.dataset.phase === phase) return
    heroStage.dataset.phase = phase
    swapText(heroTitle, phase === "play" ? "MediaFlick · Now playing" : "MediaFlick · Home")
  }
  const follow = () => setPhase(heroVideo.currentTime >= playAt ? "play" : "browse")
  if (reducedMotion.matches) {
    setPhase("play")
  } else {
    setPhase("browse")
    heroVideo.addEventListener("timeupdate", follow)
    heroVideo.addEventListener("seeked", follow)
    // If autoplay is refused, the poster stays on the tour; show the
    // playback readouts anyway rather than an empty stage.
    setTimeout(() => {
      if (heroVideo.paused) setPhase("play")
    }, 2500)
  }
  reducedMotion.addEventListener("change", () => {
    if (reducedMotion.matches) setPhase("play")
  })
}

/* ------------------------------------------------------------ walkthrough */

// The Browse walkthrough: the window pins while the steps scroll past. The
// step in the middle of the viewport is the frame the window shows, and the
// window's title names it. The hover recording plays only while it is shown.
const walk = document.querySelector("[data-walk]")
if (walk) {
  const steps = [...walk.querySelectorAll("[data-walk-step]")]
  const frames = [...walk.querySelectorAll("[data-walk-frame]")]
  const title = walk.querySelector("[data-walk-title]")
  let inView = false

  function syncVideos() {
    for (const frame of frames) {
      if (!(frame instanceof HTMLVideoElement)) continue
      const show = frame.dataset.walkFrame === walk.dataset.walkActive
      if (show && inView && !reducedMotion.matches) {
        frame.play().catch(() => {
          // Autoplay can be refused; the poster remains.
        })
      } else {
        frame.pause()
      }
    }
  }

  function activate(name) {
    if (walk.dataset.walkActive === name) return
    walk.dataset.walkActive = name
    for (const step of steps) step.classList.toggle("is-active", step.dataset.walkStep === name)
    for (const frame of frames) frame.classList.toggle("is-active", frame.dataset.walkFrame === name)
    const step = steps.find((candidate) => candidate.dataset.walkStep === name)
    swapText(title, step?.dataset.walkTitle)
    syncVideos()
  }

  const stepObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) if (entry.isIntersecting) activate(entry.target.dataset.walkStep)
    },
    { rootMargin: "-45% 0px -42% 0px" },
  )
  for (const step of steps) stepObserver.observe(step)
  new IntersectionObserver(
    ([entry]) => {
      inView = entry.isIntersecting
      syncVideos()
    },
    { threshold: 0.15 },
  ).observe(walk)
  reducedMotion.addEventListener("change", syncVideos)
}

/* ------------------------------------------------------------ tiles */

// A light follows the pointer across the tiles in Highlights and Details.
for (const bento of document.querySelectorAll(".bento")) {
  bento.addEventListener("pointermove", (event) => {
    const tile = event.target instanceof Element ? event.target.closest(".tile") : null
    if (!tile) return
    const rect = tile.getBoundingClientRect()
    tile.style.setProperty("--mx", `${event.clientX - rect.left}px`)
    tile.style.setProperty("--my", `${event.clientY - rect.top}px`)
  })
}

/* ------------------------------------------------------------ tabs */

// Shared roving-tabindex behaviour for the two tab strips.
function tabs(tabList, select) {
  const buttons = [...tabList.querySelectorAll("[role=tab]")]
  tabList.addEventListener("keydown", (event) => {
    const current = buttons.indexOf(document.activeElement)
    if (current < 0) return
    let next = current
    if (event.key === "ArrowRight") next = (current + 1) % buttons.length
    else if (event.key === "ArrowLeft") next = (current - 1 + buttons.length) % buttons.length
    else if (event.key === "Home") next = 0
    else if (event.key === "End") next = buttons.length - 1
    else return
    event.preventDefault()
    buttons[next].focus()
    select(buttons[next], true)
  })
  for (const button of buttons) button.addEventListener("click", () => select(button, true))
  return buttons
}

function markSelected(buttons, selected) {
  for (const button of buttons) {
    const active = button === selected
    button.setAttribute("aria-selected", String(active))
    button.tabIndex = active ? 0 : -1
  }
}

/* ------------------------------------------------------------ player switch */

const playerSwitch = document.querySelector("[data-switch]")
if (playerSwitch) {
  const tabList = playerSwitch.querySelector("[role=tablist]")
  const panels = [...playerSwitch.querySelectorAll("[data-switch-panel]")]
  const buttons = tabs(tabList, (button) => {
    const name = button.dataset.switchTab
    markSelected(buttons, button)
    playerSwitch.dataset.active = name
    for (const panel of panels) {
      const show = panel.dataset.switchPanel === name
      panel.hidden = !show
      for (const video of panel.querySelectorAll("video")) {
        if (show) playIfAllowed(video)
        else video.pause()
      }
    }
    if (name === "external") typeConfig()
  })
  playerSwitch.dataset.active = "builtin"
}

// Types the unchanged mpv.conf once, the first time it is shown.
const typer = document.querySelector("[data-typer]")
const typerText = typer?.textContent ?? ""
let typed = false
function typeConfig() {
  if (!typer || typed) return
  typed = true
  if (reducedMotion.matches) return
  const code = typer.querySelector("code")
  code.textContent = ""
  typer.setAttribute("data-typing", "")
  let index = 0
  const step = () => {
    index = Math.min(typerText.length, index + (typerText[index] === "\n" ? 1 : 2))
    code.textContent = typerText.slice(0, index)
    if (index < typerText.length) setTimeout(step, typerText[index - 1] === "\n" ? 140 : 18)
    else setTimeout(() => typer.removeAttribute("data-typing"), 1600)
  }
  step()
}

/* ------------------------------------------------------------ companion showcase */

const showcase = document.querySelector("[data-showcase]")
if (showcase) {
  const SHOWCASE_MS = 6000
  const progress = showcase.querySelector("[data-showcase-progress]")
  const panels = [...showcase.querySelectorAll("[data-show-panel]")]
  let auto = !reducedMotion.matches
  let inView = false
  let paused = false
  let timer = 0

  const buttons = tabs(showcase.querySelector("[role=tablist]"), (button, byUser) => {
    if (byUser) auto = false
    show(button)
  })

  function show(button) {
    markSelected(buttons, button)
    for (const panel of panels) panel.hidden = panel.dataset.showPanel !== button.dataset.show
    schedule()
  }

  function schedule() {
    clearTimeout(timer)
    progress.removeAttribute("data-running")
    if (!auto || !inView || paused) return
    // Restart the bar's animation for the new slide.
    void progress.offsetWidth
    progress.style.setProperty("--showcase-ms", `${SHOWCASE_MS}ms`)
    progress.setAttribute("data-running", "")
    timer = setTimeout(() => {
      const current = buttons.findIndex((button) => button.getAttribute("aria-selected") === "true")
      show(buttons[(current + 1) % buttons.length])
    }, SHOWCASE_MS)
  }

  new IntersectionObserver(
    ([entry]) => {
      inView = entry.isIntersecting
      schedule()
    },
    { threshold: 0.35 },
  ).observe(showcase)
  for (const [on, off] of [["pointerenter", "pointerleave"], ["focusin", "focusout"]]) {
    showcase.addEventListener(on, () => {
      paused = true
      schedule()
    })
    showcase.addEventListener(off, () => {
      paused = false
      schedule()
    })
  }
}

/* ------------------------------------------------------------ playstate readout */

// Follows the CSS progress animation so the time matches the bar.
const syncTime = document.querySelector("[data-sync-time]")
const syncFill = document.querySelector(".sync-demo .bar-fill:not(.bar-fill-lag)")
if (syncTime && syncFill && !reducedMotion.matches) {
  const TOTAL_SECONDS = 42 * 60 + 17
  let visible = false
  new IntersectionObserver(([entry]) => (visible = entry.isIntersecting)).observe(syncTime)
  const tick = () => {
    const animation = syncFill.getAnimations()[0]
    if (visible && animation && typeof animation.currentTime === "number") {
      const duration = animation.effect.getTiming().duration
      const seconds = Math.floor(((animation.currentTime % duration) / duration) * TOTAL_SECONDS)
      syncTime.textContent = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`
    }
    requestAnimationFrame(tick)
  }
  requestAnimationFrame(tick)
}

/* ------------------------------------------------------------ gallery */

const gallery = document.querySelector("[data-gallery]")
if (gallery) {
  const scrollByItem = (direction) => {
    const item = gallery.querySelector("li")
    const gap = parseFloat(getComputedStyle(gallery).columnGap) || 0
    gallery.scrollBy({ left: direction * ((item?.offsetWidth ?? 400) + gap), behavior: reducedMotion.matches ? "auto" : "smooth" })
  }
  document.querySelector("[data-gallery-prev]")?.addEventListener("click", () => scrollByItem(-1))
  document.querySelector("[data-gallery-next]")?.addEventListener("click", () => scrollByItem(1))

  const lightbox = document.querySelector("[data-lightbox]")
  const lightboxImage = lightbox?.querySelector("[data-lightbox-image]")
  // Where the browser supports it, the thumbnail grows into the lightbox and
  // shrinks back into place on close. Elsewhere the dialog simply opens.
  const morph = (thumbnail, change) => {
    if (typeof document.startViewTransition !== "function" || reducedMotion.matches) {
      change()
      return
    }
    thumbnail.style.viewTransitionName = "shot"
    lightboxImage.style.viewTransitionName = ""
    const transition = document.startViewTransition(() => {
      change()
      thumbnail.style.viewTransitionName = ""
      lightboxImage.style.viewTransitionName = lightbox.open ? "shot" : ""
    })
    transition.finished.finally(() => {
      thumbnail.style.viewTransitionName = ""
      lightboxImage.style.viewTransitionName = ""
    })
  }
  let openedFrom = null
  for (const shot of gallery.querySelectorAll("[data-full]")) {
    shot.addEventListener("click", async () => {
      if (!lightbox || !lightboxImage) return
      const thumbnail = shot.querySelector("img")
      lightboxImage.src = shot.dataset.full
      lightboxImage.alt = thumbnail?.alt ?? ""
      await lightboxImage.decode().catch(() => {
        // A failed decode still opens the dialog; the image shows its alt text.
      })
      openedFrom = thumbnail
      morph(thumbnail, () => lightbox.showModal())
    })
  }
  const closeLightbox = () => {
    if (!lightbox?.open) return
    if (openedFrom) morph(openedFrom, () => lightbox.close())
    else lightbox.close()
  }
  // Escape, the close button and a click on the backdrop all morph back.
  lightbox?.addEventListener("cancel", (event) => {
    event.preventDefault()
    closeLightbox()
  })
  lightbox?.querySelector("form")?.addEventListener("submit", (event) => {
    event.preventDefault()
    closeLightbox()
  })
  lightbox?.addEventListener("click", (event) => {
    if (event.target === lightbox) closeLightbox()
  })
}

/* ------------------------------------------------------------ downloads */

const PLATFORM_LABELS = { windows: "Windows", linux: "Linux", macos: "macOS" }

function detectPlatform() {
  const hint = (navigator.userAgentData?.platform || navigator.platform || navigator.userAgent).toLowerCase()
  if (hint.includes("win")) return "windows"
  if (hint.includes("mac")) return "macos"
  if (hint.includes("linux") && !/android/.test(navigator.userAgent.toLowerCase())) return "linux"
  return null
}

const platform = detectPlatform()
const primaryButton = document.querySelector("[data-download-primary]")
const primaryLabel = document.querySelector("[data-download-label]")
if (platform) {
  document.querySelector(`[data-platform="${platform}"]`)?.setAttribute("data-recommended", "")
  if (primaryLabel) primaryLabel.textContent = `Download for ${PLATFORM_LABELS[platform]}`
}

// The newest published release fills in the version and direct asset links.
// Every link already points at the GitHub releases page if this fails.
fetch("https://api.github.com/repos/phob/MediaFlick-Desktop/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
  .then((response) => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
  .then((release) => {
    if (typeof release?.tag_name !== "string" || !Array.isArray(release.assets)) return
    for (const element of document.querySelectorAll("[data-release-version]")) element.textContent = release.tag_name
    for (const card of document.querySelectorAll("[data-asset-suffix]")) {
      const asset = release.assets.find((candidate) => typeof candidate?.name === "string" && candidate.name.endsWith(card.dataset.assetSuffix))
      if (typeof asset?.browser_download_url === "string" && asset.browser_download_url.startsWith("https://github.com/")) {
        card.href = asset.browser_download_url
      }
    }
    const recommended = platform && document.querySelector(`[data-platform="${platform}"][data-asset-suffix]`)
    if (recommended && primaryButton && recommended.href.startsWith("https://github.com/") && !recommended.href.endsWith("/latest")) {
      primaryButton.href = recommended.href
    }
  })
  .catch(() => {
    // Offline or rate-limited: the static links stay.
  })

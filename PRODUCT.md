# Product

## Register

product

## Users

MediaFlick Desktop is for Jellyfin users who want the familiar Jellyfin Web library experience while keeping full control of an external mpv setup. They are often desktop power users, home-theater enthusiasts, and video-quality tinkerers who care about mpv configuration, scripts, shaders, HDR profiles, SVP workflows, input bindings, and reliable playback state reporting.

Users are usually launching the app to browse a personal Jellyfin server, start media quickly, and trust that watched state, resume points, and playback progress remain synchronized while mpv handles playback.

## Product Purpose

MediaFlick Desktop embeds Jellyfin Web in a desktop CEF shell and hands direct-play media streams to an external mpv process. It exists to combine the convenience and look of Jellyfin Web with the flexibility of a user-owned mpv installation.

Success means the app feels like a native desktop companion to Jellyfin: quick to configure, visually compatible with Jellyfin, transparent about mpv handoff, and dependable enough that playback state never feels fragile or mysterious.

## Brand Personality

Quiet, technical, media-native.

The product should feel like a focused desktop media tool rather than a branded entertainment portal. The interface borrows Jellyfin's dark, poster-first, cyan-accented media-center language, while keeping MediaFlick's own gradient mark and app identity separate from Jellyfin branding.

Tone should be direct and practical. Prefer clear labels, restrained help text, and confidence-building status messages over promotional copy. App dialogs, especially About, should be terse: users do not want paragraphs of descriptive brand or product explanation where a name, version, creator, status, and one useful action will do.

## Anti-references

Do not make MediaFlick look like an unrelated SaaS dashboard, a generic Electron settings app, or a neon gamer skin. Avoid decorative glassmorphism, heavy purple gradients across surfaces, marketing-style hero sections, oversized metric cards, and visual treatments that compete with Jellyfin Web content.

Do not copy Jellyfin logo assets or imply that MediaFlick is an official Jellyfin product. Jellyfin is the visual baseline for product compatibility, not the brand owner for this app.

## Design Principles

1. **Respect the host experience.** MediaFlick surrounds and extends Jellyfin Web; it should not visually fight the embedded app.
2. **Playback trust comes first.** Status, settings, and errors should make mpv handoff and playstate reporting feel observable and reliable.
3. **Keep power-user control visible but calm.** Advanced mpv options should be accessible without turning first launch into a configuration wall.
4. **Keep copy short enough to be used.** Dialogs should answer the immediate user question, not explain the whole product. About is identity and version information, not a brochure.
5. **Use media as the atmosphere.** Let posters, backdrops, and playback state carry visual interest; chrome and dialogs should stay quiet.
6. **Own the bridge, not the library.** MediaFlick's identity should appear in setup, settings, installer, updates, and app-level controls, while Jellyfin content remains visually primary.

## Accessibility & Inclusion

Target WCAG AA for app-owned surfaces. Preserve keyboard navigation, visible focus states, readable contrast on dark backgrounds, reduced-motion support, and clear error recovery. Avoid relying on color alone for playback, update, or validation states.

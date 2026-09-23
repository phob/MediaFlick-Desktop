import { AlertTriangle, CheckCircle2 } from "lucide-react"
import { ApiError, type CompanionService } from "@/lib/api"
import { useCompanion, useSeerrStatus, useStatus } from "@/lib/queries"
import { PageTitle, Section, SettingsError, SettingsLoading, SettingsRow, SignInRequired } from "./shared"

const COMPANION_SERVICES: ReadonlyArray<{
  id: CompanionService
  name: string
  description: string
}> = [
  { id: "seerr", name: "Seerr", description: "Discovery and requests for mapped Jellyfin users." },
  { id: "sonarr", name: "Sonarr", description: "Upcoming episodes and download status." },
  { id: "radarr", name: "Radarr", description: "Upcoming films and download status." },
  { id: "mdblist", name: "MDBList", description: "Shared ratings and public list sources." },
  { id: "tmdb", name: "TMDB", description: "Movie franchises and collection metadata." },
]

const COMPANION_FEATURES: ReadonlyArray<{
  capability: string
  requires: string
  name: string
  description: string
}> = [
  {
    capability: "franchise-memberships-v1",
    requires: "collection-experience-v1",
    name: "Movie franchises",
    description: "The server plugin cannot supply the franchise membership data this Desktop build needs.",
  },
  {
    capability: "seerr-person-discovery",
    requires: "seerr",
    name: "Cast discovery",
    description: "Cast pages cannot load a person's Seerr credits with this server plugin.",
  },
  {
    capability: "seerr-discovery-v4",
    requires: "seerr",
    name: "Release-decade filters",
    description: "Discover cannot send release-decade filters to this server plugin.",
  },
  {
    capability: "seerr-request-profiles",
    requires: "seerr",
    name: "Request profile selection",
    description: "Requests cannot select a Sonarr or Radarr quality profile with this server plugin.",
  },
]

function StatusBadge({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span className="settings-status" data-status={ok ? "verified" : "unverified"}>
      {ok ? <CheckCircle2 /> : <AlertTriangle />}
      {label}
    </span>
  )
}

function Availability({ available }: { available: boolean }) {
  return <StatusBadge ok={available} label={available ? "available" : "unavailable"} />
}

function FeatureCompatibility({ compatible }: { compatible: boolean }) {
  return <StatusBadge ok={compatible} label={compatible ? "compatible" : "missing"} />
}

export function CompanionIntegration() {
  const statusQuery = useStatus()
  const companionQuery = useCompanion()
  const { data: status } = statusQuery
  const { data: companion } = companionQuery
  const companionSeerr = Boolean(companion?.compatible && companion.info?.services.seerr)
  const seerrQuery = useSeerrStatus(companionSeerr)
  if (statusQuery.error && !status) {
    return (
      <SettingsError
        title="Companion status unavailable"
        error={statusQuery.error}
        onRetry={() => void statusQuery.refetch()}
      />
    )
  }
  if (statusQuery.isPending) return <SettingsLoading />
  if (!status?.authenticated) return <SignInRequired name="MediaFlick Companion" />
  if (companionQuery.error && !companion) {
    return (
      <SettingsError
        title="Companion status unavailable"
        error={companionQuery.error}
        onRetry={() => void companionQuery.refetch()}
      />
    )
  }
  if (!companion) return <SettingsLoading />

  const pluginDescription = companion.available
    ? "Companion answered this Desktop build's authenticated status check."
    : companion.error ?? "MediaFlick Companion was not found on this Jellyfin server."
  const pluginAvailable = companion.available && companion.compatible
  const capabilities = companion.info?.capabilities ?? []
  const missingFeatures = pluginAvailable
    ? COMPANION_FEATURES.filter((feature) =>
      capabilities.includes(feature.requires) && !capabilities.includes(feature.capability))
    : []
  const seerrMapping = () => {
    if (seerrQuery.data?.mapped) {
      return ` This account is mapped${seerrQuery.data.user?.name ? ` as ${seerrQuery.data.user.name}` : ""}.`
    }
    if (seerrQuery.error instanceof ApiError && seerrQuery.error.status === 409) {
      return " This Jellyfin account has not been imported into Seerr."
    }
    if (seerrQuery.error) return " The account mapping could not be checked."
    return " Checking this account's mapping."
  }

  return (
    <div className="settings-page">
      <PageTitle title="MediaFlick Companion" />
      <Section
        title="Plugin"
        description="Administrators configure these services in Jellyfin's MediaFlick Companion dashboard."
      >
        <SettingsRow title="Connection" description={pluginDescription}>
          <Availability available={companion.available} />
        </SettingsRow>
      </Section>
      {companion.available && (
        <Section
          title="Feature compatibility"
          description="Desktop checks the features advertised by Companion. Plugin version numbers are not used for this check."
        >
          {!companion.compatible && (
            <SettingsRow
              title="Companion protocol"
              description="This Desktop build cannot read the plugin's status contract."
            >
              <FeatureCompatibility compatible={false} />
            </SettingsRow>
          )}
          {missingFeatures.map((feature) => (
            <SettingsRow key={feature.capability} title={feature.name} description={feature.description}>
              <FeatureCompatibility compatible={false} />
            </SettingsRow>
          ))}
          {companion.compatible && missingFeatures.length === 0 && (
            <SettingsRow
              title="Desktop features"
              description="This Companion provides every server feature used by this Desktop build."
            >
              <FeatureCompatibility compatible />
            </SettingsRow>
          )}
        </Section>
      )}
      <Section
        title="Services"
        description="Desktop reads these connections through Companion and never receives their addresses or credentials."
      >
        {COMPANION_SERVICES.map((service) => {
          const available = Boolean(pluginAvailable && companion.info?.services[service.id])
          const description = service.id === "seerr" && available
            ? service.description + seerrMapping()
            : service.description
          return (
            <SettingsRow key={service.id} title={service.name} description={description}>
              <Availability available={available} />
            </SettingsRow>
          )
        })}
      </Section>
    </div>
  )
}

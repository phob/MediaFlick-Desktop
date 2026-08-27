import {
  Award,
  Binary,
  Blocks,
  Bone,
  BookOpen,
  Briefcase,
  Bug,
  CalendarClock,
  CalendarDays,
  Circle,
  Compass,
  Crosshair,
  Drama,
  Film,
  Flame,
  Ghost,
  Heart,
  Landmark,
  Languages,
  Laugh,
  ListVideo,
  type LucideIcon,
  MonitorPlay,
  Mountain,
  Music,
  Orbit,
  Palette,
  PawPrint,
  Popcorn,
  Rocket,
  Search,
  SlidersHorizontal,
  Sparkles,
  Star,
  Swords,
  Telescope,
  TrendingUp,
  Trophy,
  Tv,
  UsersRound,
  WandSparkles,
  Zap,
} from "lucide-react"
import type {
  CollectionCategory,
  CollectionTemplatePictogram as PictogramName,
} from "@/lib/api"
import { cn } from "@/lib/utils"

const PICTOGRAMS = {
  award: Award,
  binary: Binary,
  blocks: Blocks,
  bone: Bone,
  bookOpen: BookOpen,
  briefcase: Briefcase,
  bug: Bug,
  calendarClock: CalendarClock,
  calendarDays: CalendarDays,
  circle: Circle,
  compass: Compass,
  crosshair: Crosshair,
  drama: Drama,
  film: Film,
  flame: Flame,
  ghost: Ghost,
  heart: Heart,
  landmark: Landmark,
  languages: Languages,
  laugh: Laugh,
  listVideo: ListVideo,
  monitorPlay: MonitorPlay,
  mountain: Mountain,
  music: Music,
  orbit: Orbit,
  palette: Palette,
  pawPrint: PawPrint,
  popcorn: Popcorn,
  rocket: Rocket,
  search: Search,
  slidersHorizontal: SlidersHorizontal,
  sparkles: Sparkles,
  star: Star,
  swords: Swords,
  telescope: Telescope,
  trendingUp: TrendingUp,
  trophy: Trophy,
  tv: Tv,
  usersRound: UsersRound,
  wandSparkles: WandSparkles,
  zap: Zap,
} satisfies Record<PictogramName, LucideIcon>

const CATEGORY_STYLES = {
  trending: "bg-orange-500/15 text-orange-700 dark:text-orange-300",
  popular: "bg-amber-500/15 text-amber-700 dark:text-amber-300",
  streamingServices: "bg-violet-500/15 text-violet-700 dark:text-violet-300",
  topRated: "bg-yellow-500/15 text-yellow-700 dark:text-yellow-300",
  inTheaters: "bg-rose-500/15 text-rose-700 dark:text-rose-300",
  upcoming: "bg-sky-500/15 text-sky-700 dark:text-sky-300",
  onAir: "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300",
  editorial: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
  custom: "bg-slate-500/15 text-slate-700 dark:text-slate-300",
} satisfies Record<CollectionCategory, string>

export default function CollectionTemplatePictogram({
  category,
  pictogram,
}: {
  category: CollectionCategory
  pictogram: PictogramName
}) {
  const Icon = PICTOGRAMS[pictogram]

  return (
    <span
      aria-hidden="true"
      className={cn(
        "flex size-16 shrink-0 items-center justify-center rounded-lg",
        CATEGORY_STYLES[category],
      )}
    >
      <Icon className="size-8" strokeWidth={1.8} />
    </span>
  )
}

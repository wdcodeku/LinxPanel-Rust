// The panel's navigation, shared by the sidebar, the breadcrumb and quick search.

export interface NavItem {
  path: string
  titleKey: string
  icon: string
  /** Also active on sub-paths (e.g. /sites/3). */
  match?: string
}

export interface NavGroup {
  key: string
  titleKey: string
  items: NavItem[]
}

export const navGroups: NavGroup[] = [
  {
    key: 'monitor',
    titleKey: 'nav.groupMonitor',
    items: [
      { path: '/dashboard', titleKey: 'dashboard', icon: 'Odometer' },
      { path: '/processes', titleKey: 'nav.processes', icon: 'Histogram' },
      { path: '/services', titleKey: 'nav.services', icon: 'SetUp' },
    ],
  },
  {
    key: 'server',
    titleKey: 'nav.groupServer',
    items: [
      { path: '/sites', titleKey: 'sites', icon: 'Connection', match: '/sites' },
      { path: '/appstore', titleKey: 'nav.appstore', icon: 'Shop' },
      { path: '/databases', titleKey: 'nav.databases', icon: 'Coin' },
      { path: '/runtime', titleKey: 'nav.runtime', icon: 'Box' },
      { path: '/files', titleKey: 'fileManager', icon: 'FolderOpened' },
      { path: '/terminal', titleKey: 'terminal', icon: 'Monitor' },
      { path: '/cron', titleKey: 'nav.cron', icon: 'AlarmClock' },
      { path: '/security', titleKey: 'nav.security', icon: 'Lock' },
    ],
  },
  {
    key: 'system',
    titleKey: 'nav.groupSystem',
    items: [{ path: '/settings', titleKey: 'settings', icon: 'Setting' }],
  },
]

/** Places inside pages that quick search can jump to directly. */
export const quickLinks: Array<{ path: string; titleKey: string; parentKey: string; icon: string }> = [
  { path: '/settings?tab=security', titleKey: 'st.tabSecurity', parentKey: 'settings', icon: 'Lock' },
  { path: '/settings?tab=update', titleKey: 'upd.tab', parentKey: 'settings', icon: 'Upload' },
  { path: '/settings?tab=alerts', titleKey: 'st.tabAlerts', parentKey: 'settings', icon: 'Bell' },
  { path: '/settings?tab=about', titleKey: 'st.tabAbout', parentKey: 'settings', icon: 'InfoFilled' },
]

export function findNav(path: string): { group: NavGroup; item: NavItem } | null {
  for (const group of navGroups) {
    for (const item of group.items) {
      if (item.path === path || (item.match && path.startsWith(`${item.match}/`))) return { group, item }
    }
  }
  return null
}

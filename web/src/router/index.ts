import { createRouter, createWebHistory } from 'vue-router'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'Login',
      component: () => import('@/views/login/LoginView.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      component: () => import('@/components/Layout.vue'),
      redirect: '/dashboard',
      children: [
        {
          path: 'dashboard',
          name: 'Dashboard',
          component: () => import('@/views/dashboard/DashboardView.vue'),
          meta: { title: 'Dashboard', icon: 'Odometer' },
        },
        {
          path: 'processes',
          name: 'Processes',
          component: () => import('@/views/processes/ProcessesView.vue'),
          meta: { title: 'Processes', icon: 'Histogram' },
        },
        {
          path: 'services',
          name: 'Services',
          component: () => import('@/views/services/ServicesView.vue'),
          meta: { title: 'Services', icon: 'SetUp' },
        },
        {
          path: 'cron',
          name: 'Cron',
          component: () => import('@/views/cron/CronView.vue'),
          meta: { title: 'Cron', icon: 'AlarmClock' },
        },
        {
          path: 'security',
          name: 'Security',
          component: () => import('@/views/security/SecurityView.vue'),
          meta: { title: 'Security', icon: 'Lock' },
        },
        {
          path: 'appstore',
          name: 'AppStore',
          component: () => import('@/views/appstore/AppStoreView.vue'),
          meta: { title: 'App Store', icon: 'Shop' },
        },
        {
          path: 'databases',
          name: 'Databases',
          component: () => import('@/views/databases/DatabaseView.vue'),
          meta: { title: 'Databases', icon: 'Coin' },
        },
        {
          path: 'runtime',
          name: 'Runtime',
          component: () => import('@/views/runtime/RuntimeView.vue'),
          meta: { title: 'Runtime', icon: 'Box' },
        },
        {
          path: 'terminal',
          name: 'Terminal',
          component: () => import('@/views/terminal/TerminalView.vue'),
          meta: { title: 'Terminal', icon: 'Monitor' },
        },
        {
          path: 'sites',
          name: 'Sites',
          component: () => import('@/views/sites/SitesView.vue'),
          meta: { title: 'Sites', icon: 'Monitor' },
        },
        {
          path: 'sites/:id',
          name: 'SiteDetail',
          component: () => import('@/views/sites/SiteDetail.vue'),
          meta: { title: 'Site Detail', hidden: true },
        },
        {
          path: 'settings',
          name: 'Settings',
          component: () => import('@/views/settings/SettingsView.vue'),
          meta: { title: 'Settings', icon: 'Setting' },
        },
        {
          path: 'files',
          name: 'Files',
          component: () => import('@/views/files/FileManager.vue'),
          meta: { title: 'File Manager', icon: 'FolderOpened' },
        },
      ],
    },
  ],
})

router.beforeEach((to, _from, next) => {
  const token = localStorage.getItem('token')
  const normalizedPath = to.path.replace(/\/+$/, '') || '/'
  const isLoginRoute = to.name === 'Login' || normalizedPath === '/login'

  if (!token && !to.meta.public) {
    next('/login')
    return
  }

  if (token && isLoginRoute) {
    next('/dashboard')
    return
  }

  next()
})

export default router

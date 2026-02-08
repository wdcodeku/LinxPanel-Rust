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
  if (!to.meta.public && !token) {
    next('/login')
  } else {
    next()
  }
})

export default router
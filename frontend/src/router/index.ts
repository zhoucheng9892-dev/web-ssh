import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from '@/stores/auth'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('@/views/LoginView.vue'),
      meta: { public: true },
    },
    {
      path: '/setup',
      name: 'setup',
      component: () => import('@/views/SetupView.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      component: () => import('@/components/AppLayout.vue'),
      children: [
        {
          path: '',
          name: 'terminals',
          component: () => import('@/views/TerminalView.vue'),
        },
        {
          path: 'connections',
          name: 'connections',
          component: () => import('@/views/ConnectionsView.vue'),
        },
        {
          path: 'files',
          name: 'files',
          component: () => import('@/views/FilesView.vue'),
        },
        {
          path: 'password',
          name: 'password',
          component: () => import('@/views/PasswordView.vue'),
        },
        {
          path: 'users',
          name: 'users',
          component: () => import('@/views/UsersView.vue'),
          // Admin-only; AppLayout hides the nav entry for non-admins.
        },
      ],
    },
  ],
})

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  if (auth.loading) await auth.init()

  if (auth.needsSetup && to.name !== 'setup') {
    return { name: 'setup' }
  }
  if (!auth.needsSetup && !auth.user && !to.meta.public) {
    return { name: 'login' }
  }
  // Admin-only routes.
  if (to.name === 'users' && !auth.user?.is_admin) {
    return { name: 'terminals' }
  }
  // Don't let logged-in users sit on login/setup pages.
  if (auth.user && to.meta.public) {
    return { name: 'terminals' }
  }
})

export default router

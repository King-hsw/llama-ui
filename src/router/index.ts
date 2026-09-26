import { createRouter, createWebHashHistory } from "vue-router";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/server" },
    { path: "/server", component: () => import("../views/ServerView.vue") },
    { path: "/models", component: () => import("../views/ModelsView.vue") },
    { path: "/download", component: () => import("../views/DownloadView.vue") },
    { path: "/settings", component: () => import("../views/SettingsView.vue") },
  ],
});

export default router;

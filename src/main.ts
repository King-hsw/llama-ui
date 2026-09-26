import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
// TDesign 组件改为按需引入（见 vite.config.ts 中的 unplugin-vue-components 配置），
// 此处仅保留全库公共基础样式（设计 token / reset，约 19kB，必须引入），
// 以及 JS 中按需调用的插件函数（MessagePlugin / DialogPlugin）所需的组件样式。
import "tdesign-vue-next/es/style/index.css";
import "tdesign-vue-next/es/message/style/index.css";
import "tdesign-vue-next/es/dialog/style/index.css";
import "./styles.css";

createApp(App).use(router).mount("#app");

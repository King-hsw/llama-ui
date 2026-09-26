import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import Components from "unplugin-vue-components/vite";
import { TDesignResolver } from "unplugin-vue-components/resolvers";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

/**
 * TDesign 子组件（模板标签）与其样式所在的父组件目录映射。
 * 例：<t-form-item> 的样式在 es/form/style/index.css 中。
 */
const SUB_COMPONENT_STYLE_DIR: Record<string, string> = {
  Aside: "layout",
  Content: "layout",
  MenuItem: "menu",
  MenuItemGroup: "menu",
  Option: "select",
  OptionGroup: "select",
  CollapsePanel: "collapse",
  FormItem: "form",
  RadioGroup: "radio",
  CheckboxGroup: "checkbox",
  TabPanel: "tabs",
  StepItem: "steps",
  TimelineItem: "timeline",
  BreadcrumbItem: "breadcrumb",
  DropdownItem: "dropdown",
  DropdownMenu: "dropdown",
};

/** 计算组件的按需样式路径；样式文件不存在（如 Icon 无独立样式）时返回 null。 */
function tdesignStyleSideEffect(componentName: string): string | null {
  const dir = (SUB_COMPONENT_STYLE_DIR[componentName] ?? componentName)
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/([a-zA-Z])(\d)/g, "$1-$2")
    .toLowerCase();
  if (existsSync(resolve(process.cwd(), `node_modules/tdesign-vue-next/es/${dir}/style/index.css`))) {
    return `tdesign-vue-next/es/${dir}/style/index.css`;
  }
  return null;
}

// TDesignResolver 只负责 JS 的按需引入，不携带样式；此处包一层补上组件级 CSS。
const tdesignBaseResolver = TDesignResolver({ library: "vue-next" });
function tdesignResolverWithStyle(name: string) {
  const resolved = tdesignBaseResolver.resolve?.(name);
  if (!resolved) return;
  return { ...resolved, sideEffects: tdesignStyleSideEffect(name.slice(1)) ?? undefined };
}

export default defineConfig({
  plugins: [
    vue(),
    Components({
      dts: "src/components.d.ts",
      resolvers: [{ type: "component", resolve: tdesignResolverWithStyle }],
    }),
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // 忽略 Rust 编译产物，避免 cargo 编译时 Windows 文件锁导致 EBUSY 崩溃
      ignored: ["**/src-tauri/target/**", "**/src-tauri/gen/**"],
    },
  },
  build: {
    target: "chrome105",
    outDir: "dist",
  },
});
